//! Durable FIFO serialization for deliveries that need a user decision.
//!
//! A later delivery is never declared terminal merely because an earlier A/B/C
//! card is still open. It waits here until the active delivery reaches a durable
//! execution/report outcome, then resumes in its exact originating Job Session.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::consent::{self, DeliveryContext};
use super::grants::job_id_is_safe;
use super::super::okx_a2a;

const QUEUE_VERSION: u32 = 1;
const RESUME_ENVELOPE_VERSION: u32 = 2;
const RETRY_DELAY_SEC: u64 = 30;
const RESUME_ACK_TIMEOUT_SEC: u64 = 30;
const PROCESSING_WATCHDOG_SEC: u64 = 15 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EntryState {
    Processing,
    AwaitingDecision,
    Waiting,
    ResumePending,
    ResumeSent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueEntry {
    delivery_id: String,
    state: EntryState,
    enqueued_at_ms: u64,
    #[serde(default)]
    next_resume_attempt_at: u64,
    #[serde(default)]
    resume_attempts: u32,
    #[serde(default)]
    resume_sent_at: u64,
    #[serde(default)]
    processing_started_at: u64,
    #[serde(default)]
    processing_attempt: u32,
    #[serde(default)]
    resume_protocol_version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueFile {
    version: u32,
    job_id: String,
    entries: Vec<QueueEntry>,
}

pub enum EnqueueResult {
    Active {
        context: DeliveryContext,
        already_present: bool,
    },
    Queued {
        active_delivery_id: String,
        position: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeAck {
    Accepted,
    DuplicateOrStale,
    NotQueueHead,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn root() -> Result<PathBuf> {
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("delivery-queue"))
}

fn queue_path(job_id: &str) -> Result<PathBuf> {
    if !job_id_is_safe(job_id) {
        bail!("invalid job id");
    }
    Ok(root()?.join(format!("{job_id}.json")))
}

fn lock_path(job_id: &str) -> Result<PathBuf> {
    Ok(root()?.join(format!("{job_id}.lock")))
}

fn acquire_lock(job_id: &str) -> Result<std::fs::File> {
    let path = lock_path(job_id)?;
    if let Some(parent) = path.parent() {
        crate::home::ensure_dir_0700(parent)?;
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    file.lock_exclusive()?;
    Ok(file)
}

fn read_queue(job_id: &str) -> Result<QueueFile> {
    let path = queue_path(job_id)?;
    if !path.exists() {
        return Ok(QueueFile {
            version: QUEUE_VERSION,
            job_id: job_id.to_string(),
            entries: Vec::new(),
        });
    }
    let queue: QueueFile = serde_json::from_slice(&std::fs::read(path)?)?;
    if queue.version != QUEUE_VERSION || queue.job_id != job_id {
        bail!("delivery queue mismatch");
    }
    Ok(queue)
}

fn write_queue(queue: &QueueFile) -> Result<()> {
    let path = queue_path(&queue.job_id)?;
    if queue.entries.is_empty() {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    crate::home::write_secure(&path, &serde_json::to_vec_pretty(queue)?)?;
    Ok(())
}

/// Admit a decision-requiring delivery without overwriting the active pointer.
pub fn enqueue(job_id: &str, delivery_id: &str) -> Result<EnqueueResult> {
    let context = consent::load_delivery_context(job_id, delivery_id)?;
    let _lock = acquire_lock(job_id)?;
    let mut queue = read_queue(job_id)?;

    // Migrate an in-flight card from a pre-queue build before accepting a new
    // delivery. This preserves the exact reply binding across upgrades.
    if queue.entries.is_empty() {
        if let Some(pending) = consent::load_pending_delivery_context(job_id)? {
            if pending.delivery_id != delivery_id {
                queue.entries.push(QueueEntry {
                    delivery_id: pending.delivery_id,
                    state: EntryState::AwaitingDecision,
                    enqueued_at_ms: pending.received_at_ms,
                    next_resume_attempt_at: 0,
                    resume_attempts: 0,
                    resume_sent_at: 0,
                    processing_started_at: 0,
                    processing_attempt: 0,
                    resume_protocol_version: 0,
                });
            }
        }
    }

    if let Some(index) = queue
        .entries
        .iter()
        .position(|entry| entry.delivery_id == delivery_id)
    {
        if index == 0 {
            let front = &mut queue.entries[index];
            // A replay of the same consent request must not push a second
            // A/B/C card or turn user think-time back into processing. A
            // queued-resume ACK leaves its exact attempt here; consume that
            // marker once so only the resumed worker may present the card.
            let claimed_resume = front.state == EntryState::Processing
                && front.processing_attempt > 0;
            if claimed_resume {
                front.processing_attempt = 0;
                front.processing_started_at = now_secs();
                write_queue(&queue)?;
            }
            return Ok(EnqueueResult::Active {
                context,
                already_present: !claimed_resume,
            });
        }
        return Ok(EnqueueResult::Queued {
            active_delivery_id: queue.entries[0].delivery_id.clone(),
            position: index + 1,
        });
    }

    let active = queue.entries.is_empty();
    queue.entries.push(QueueEntry {
        delivery_id: delivery_id.to_string(),
        state: if active {
            EntryState::Processing
        } else {
            EntryState::Waiting
        },
        enqueued_at_ms: now_ms(),
        next_resume_attempt_at: 0,
        resume_attempts: 0,
        resume_sent_at: 0,
        processing_started_at: if active { now_secs() } else { 0 },
        processing_attempt: 0,
        resume_protocol_version: 0,
    });
    let position = queue.entries.len();
    let active_delivery_id = queue.entries[0].delivery_id.clone();
    write_queue(&queue)?;
    if active {
        Ok(EnqueueResult::Active {
            context,
            already_present: false,
        })
    } else {
        Ok(EnqueueResult::Queued {
            active_delivery_id,
            position,
        })
    }
}

pub fn contains_delivery(job_id: &str, delivery_id: &str) -> Result<bool> {
    let _lock = acquire_lock(job_id)?;
    Ok(read_queue(job_id)?
        .entries
        .iter()
        .any(|entry| entry.delivery_id == delivery_id))
}

/// Record that the visible A/B/C or manual decision is now the intentional
/// blocker. A watchdog must never treat user think-time as a crashed worker.
pub fn mark_awaiting_decision(job_id: &str, delivery_id: &str) -> Result<()> {
    let _lock = acquire_lock(job_id)?;
    let mut queue = read_queue(job_id)?;
    let Some(front) = queue.entries.first_mut() else {
        bail!("delivery queue is empty");
    };
    if front.delivery_id != delivery_id {
        bail!("delivery is not the queue head");
    }
    front.state = EntryState::AwaitingDecision;
    front.next_resume_attempt_at = 0;
    front.resume_sent_at = 0;
    front.processing_started_at = 0;
    front.processing_attempt = 0;
    write_queue(&queue)?;
    Ok(())
}

/// Acknowledge one exact queued-resume transport before any async lookup or
/// model work. Replayed/stale envelopes are absorbed so they cannot create a
/// second model execution path for the same delivery.
pub fn acknowledge_resume(
    job_id: &str,
    delivery_id: &str,
    envelope_version: Option<u32>,
    attempt: Option<u32>,
) -> Result<ResumeAck> {
    let _lock = acquire_lock(job_id)?;
    let mut queue = read_queue(job_id)?;
    let Some(front) = queue.entries.first_mut() else {
        return Ok(ResumeAck::NotQueueHead);
    };
    if front.delivery_id != delivery_id {
        return Ok(ResumeAck::NotQueueHead);
    }
    match front.state {
        EntryState::ResumePending | EntryState::ResumeSent => {
            // New envelopes must match both the protocol version and exact
            // persisted attempt. Missing version/attempt is accepted only for
            // a queue entry that predates the versioned protocol marker.
            let accepted_attempt = match (envelope_version, attempt) {
                (Some(RESUME_ENVELOPE_VERSION), Some(attempt))
                    if front.resume_protocol_version == RESUME_ENVELOPE_VERSION
                        && attempt > 0
                        && attempt == front.resume_attempts =>
                {
                    attempt
                }
                (None, Some(attempt))
                    if front.resume_protocol_version == 0
                        && attempt > 0
                        && attempt == front.resume_attempts =>
                {
                    attempt
                }
                (None, None) if front.resume_protocol_version == 0 => front.resume_attempts,
                _ => return Ok(ResumeAck::DuplicateOrStale),
            };
            front.state = EntryState::Processing;
            front.processing_started_at = now_secs();
            front.processing_attempt = accepted_attempt;
            front.resume_sent_at = 0;
            front.next_resume_attempt_at = 0;
            write_queue(&queue)?;
            Ok(ResumeAck::Accepted)
        }
        EntryState::Processing | EntryState::AwaitingDecision | EntryState::Waiting => {
            Ok(ResumeAck::DuplicateOrStale)
        }
    }
}

fn resume_envelope(context: &DeliveryContext, attempt: u32) -> String {
    serde_json::json!({
        "agentId": context.agent_id,
        "message": {
            "event": "autotrade_queued_resume",
            "data": "resume_queued_delivery",
            "code": 0,
            "description": "A previously queued Active-subscription delivery is now at the head of its FIFO. Call onchainos agent next-action with this envelope and follow the returned playbook. Re-read and re-validate the saved artifact; do not reuse prior dynamic trade fields.",
            "source": "system",
            "jobId": context.job_id,
            "deliveryId": context.delivery_id,
            "resumeEnvelopeVersion": RESUME_ENVELOPE_VERSION,
            "resumeAttempt": attempt,
            "role": "user",
            "timestamp": now_secs(),
        }
    })
    .to_string()
}

fn send_resume(context: &DeliveryContext, attempt: u32, timeout: Duration) -> Result<()> {
    let content = resume_envelope(context, attempt);
    let message_id = format!(
        "autotrade-queue-resume:{}",
        hex::encode(Sha256::digest(format!(
            "{}\0{}\0{}",
            context.job_id, context.delivery_id, attempt
        )))
    );
    if let Some(session_key) = context
        .origin_session_key
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return okx_a2a::session_send_exact_with_timeout(
            session_key,
            &content,
            &message_id,
            timeout,
        );
    }
    okx_a2a::session_send_with_timeout(
        &context.job_id,
        Some(&context.provider_agent_id),
        &content,
        timeout,
    )
}

fn dispatch_front(job_id: &str, timeout: Duration) -> Result<bool> {
    let (delivery_id, attempt) = {
        let _lock = acquire_lock(job_id)?;
        let mut queue = read_queue(job_id)?;
        let Some(front) = queue.entries.first_mut() else {
            return Ok(false);
        };
        let now = now_secs();
        let pending_due = front.state == EntryState::ResumePending
            && front.next_resume_attempt_at <= now;
        let acknowledgement_timed_out = front.state == EntryState::ResumeSent
            && (front.resume_sent_at == 0
                || front
                    .resume_sent_at
                    .saturating_add(RESUME_ACK_TIMEOUT_SEC)
                    <= now);
        if !pending_due && !acknowledgement_timed_out {
            return Ok(false);
        }
        front.state = EntryState::ResumePending;
        front.next_resume_attempt_at = now.saturating_add(RETRY_DELAY_SEC);
        front.resume_attempts = front.resume_attempts.saturating_add(1);
        front.resume_protocol_version = RESUME_ENVELOPE_VERSION;
        front.resume_sent_at = 0;
        front.processing_started_at = 0;
        front.processing_attempt = 0;
        let delivery_id = front.delivery_id.clone();
        let attempt = front.resume_attempts;
        write_queue(&queue)?;
        (delivery_id, attempt)
    };
    let context = consent::load_delivery_context(job_id, &delivery_id)?;
    send_resume(&context, attempt, timeout)?;
    let _lock = acquire_lock(job_id)?;
    let mut queue = read_queue(job_id)?;
    if let Some(front) = queue.entries.first_mut() {
        if front.delivery_id == delivery_id && front.state == EntryState::ResumePending {
            front.state = EntryState::ResumeSent;
            front.resume_sent_at = now_secs();
            front.next_resume_attempt_at = front
                .resume_sent_at
                .saturating_add(RESUME_ACK_TIMEOUT_SEC);
            write_queue(&queue)?;
        }
    }
    Ok(true)
}

/// Return a transported queue head to retryable state after a transient
/// revalidation failure, without declaring the delivery terminal.
pub fn schedule_retry(job_id: &str, delivery_id: &str) -> Result<()> {
    let _lock = acquire_lock(job_id)?;
    let mut queue = read_queue(job_id)?;
    if let Some(front) = queue.entries.first_mut() {
        if front.delivery_id == delivery_id {
            front.state = EntryState::ResumePending;
            front.next_resume_attempt_at = now_secs().saturating_add(RETRY_DELAY_SEC);
            front.resume_sent_at = 0;
            front.processing_started_at = 0;
            front.processing_attempt = 0;
            write_queue(&queue)?;
        }
    }
    Ok(())
}

fn remove_terminal_and_promote(job_id: &str, delivery_id: &str) -> Result<bool> {
    let _lock = acquire_lock(job_id)?;
    let mut queue = read_queue(job_id)?;
    let Some(index) = queue
        .entries
        .iter()
        .position(|entry| entry.delivery_id == delivery_id)
    else {
        return Ok(false);
    };
    let was_front = index == 0;
    queue.entries.remove(index);
    if was_front {
        if let Some(next) = queue.entries.first_mut() {
            next.state = EntryState::ResumePending;
            next.next_resume_attempt_at = 0;
            next.resume_sent_at = 0;
            next.processing_started_at = 0;
            next.processing_attempt = 0;
        }
    }
    let should_dispatch = was_front && !queue.entries.is_empty();
    write_queue(&queue)?;
    Ok(should_dispatch)
}

/// Idempotently repair only the durable FIFO transition. The caller owns any
/// transport attempt, so startup reconciliation remains bounded and never
/// turns a state repair into a transaction retry.
pub fn reconcile_terminal(job_id: &str, delivery_id: &str) -> Result<bool> {
    remove_terminal_and_promote(job_id, delivery_id)
}

/// Remove one terminal delivery and wake exactly the next FIFO entry.
pub fn complete_and_advance(job_id: &str, delivery_id: &str) -> Result<bool> {
    let should_dispatch = remove_terminal_and_promote(job_id, delivery_id)?;
    if should_dispatch {
        dispatch_front(job_id, Duration::from_secs(1))
    } else {
        Ok(false)
    }
}

/// Repair an outcome->queue transition even when the terminal journal could
/// not be created. The durable outcome is sufficient proof that this delivery
/// must never execute again; recovery only repairs notice/pending/FIFO state.
pub(crate) fn reconcile_terminal_head(job_id: &str) -> Result<bool> {
    let delivery_id = {
        let _lock = acquire_lock(job_id)?;
        let queue = read_queue(job_id)?;
        let Some(front) = queue.entries.first() else {
            return Ok(false);
        };
        front.delivery_id.clone()
    };
    if super::executor::recovery_state(job_id, &delivery_id)?
        != super::executor::RecoveryState::TerminalOutcome
    {
        return Ok(false);
    }
    super::executor::recover_incomplete(job_id, &delivery_id)
}

/// Migrate queue entries written before Processing had a durable timestamp.
/// A matching pending pointer proves that the user is intentionally deciding;
/// otherwise the existing latch/outcome facts determine safe recovery.
fn migrate_legacy_processing(job_id: &str) -> Result<bool> {
    let delivery_id = {
        let _lock = acquire_lock(job_id)?;
        let queue = read_queue(job_id)?;
        let Some(front) = queue.entries.first() else {
            return Ok(false);
        };
        if front.state != EntryState::Processing || front.processing_started_at != 0 {
            return Ok(false);
        }
        front.delivery_id.clone()
    };

    let recovery = super::executor::recovery_state(job_id, &delivery_id)?;
    if recovery != super::executor::RecoveryState::NoExecution {
        return super::executor::recover_incomplete(job_id, &delivery_id);
    }

    let awaiting_user = consent::load_pending_delivery_context(job_id)?
        .is_some_and(|context| context.delivery_id == delivery_id);
    if awaiting_user {
        let _lock = acquire_lock(job_id)?;
        let mut queue = read_queue(job_id)?;
        if let Some(front) = queue.entries.first_mut() {
            if front.delivery_id == delivery_id
                && front.state == EntryState::Processing
                && front.processing_started_at == 0
            {
                front.state = EntryState::AwaitingDecision;
                front.processing_attempt = 0;
                write_queue(&queue)?;
                return Ok(true);
            }
        }
        return Ok(false);
    }

    let _lock = acquire_lock(job_id)?;
    let mut queue = read_queue(job_id)?;
    if let Some(front) = queue.entries.first_mut() {
        if front.delivery_id == delivery_id
            && front.state == EntryState::Processing
            && front.processing_started_at == 0
        {
            front.state = EntryState::ResumePending;
            front.next_resume_attempt_at = 0;
            front.resume_sent_at = 0;
            front.processing_attempt = 0;
            front.resume_protocol_version = 0;
            write_queue(&queue)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn recover_stalled_processing(job_id: &str) -> Result<bool> {
    let delivery_id = {
        let _lock = acquire_lock(job_id)?;
        let queue = read_queue(job_id)?;
        let Some(front) = queue.entries.first() else {
            return Ok(false);
        };
        if front.state != EntryState::Processing
            || front.processing_started_at == 0
            || front
                .processing_started_at
                .saturating_add(PROCESSING_WATCHDOG_SEC)
                > now_secs()
        {
            return Ok(false);
        }
        front.delivery_id.clone()
    };

    match super::executor::recovery_state(job_id, &delivery_id)? {
        super::executor::RecoveryState::NoExecution => {
            let _lock = acquire_lock(job_id)?;
            let mut queue = read_queue(job_id)?;
            if let Some(front) = queue.entries.first_mut() {
                if front.delivery_id == delivery_id && front.state == EntryState::Processing {
                    front.state = EntryState::ResumePending;
                    front.next_resume_attempt_at = 0;
                    front.processing_started_at = 0;
                    front.processing_attempt = 0;
                    write_queue(&queue)?;
                    return Ok(true);
                }
            }
            Ok(false)
        }
        super::executor::RecoveryState::PreSubmitInterrupted
        | super::executor::RecoveryState::SubmissionUnknown
        | super::executor::RecoveryState::TerminalOutcome => {
            super::executor::recover_incomplete(job_id, &delivery_id)
        }
    }
}

/// Retry only active queue heads and stop within the caller's hot-path budget.
pub fn flush_due(limit: usize, budget: Duration) -> Result<usize> {
    let deadline = Instant::now() + budget;
    let root = root()?;
    if !root.is_dir() {
        return Ok(0);
    }
    let mut dispatched = 0;
    for entry in std::fs::read_dir(root)? {
        if dispatched >= limit || Instant::now() >= deadline {
            break;
        }
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(job_id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let _ = reconcile_terminal_head(job_id);
        let _ = migrate_legacy_processing(job_id);
        let _ = recover_stalled_processing(job_id);
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining < Duration::from_millis(25) {
            break;
        }
        dispatched += usize::from(dispatch_front(job_id, remaining).unwrap_or(false));
    }
    Ok(dispatched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(job: &str, delivery: &str) {
        consent::register_delivery_context(
            job,
            "7",
            "8",
            Some("session:test"),
            delivery,
            "/tmp/signal.txt",
            "text",
            now_ms(),
        )
        .unwrap();
    }

    #[test]
    fn concurrent_deliveries_are_fifo_and_not_terminally_skipped() {
        let _guard = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        register("job1", "d1");
        register("job1", "d2");
        register("job1", "d3");
        assert!(matches!(
            enqueue("job1", "d1").unwrap(),
            EnqueueResult::Active { .. }
        ));
        assert!(matches!(
            enqueue("job1", "d2").unwrap(),
            EnqueueResult::Queued { position: 2, .. }
        ));
        assert!(matches!(
            enqueue("job1", "d3").unwrap(),
            EnqueueResult::Queued { position: 3, .. }
        ));
        let queue = read_queue("job1").unwrap();
        assert_eq!(
            queue
                .entries
                .iter()
                .map(|entry| entry.delivery_id.as_str())
                .collect::<Vec<_>>(),
            vec!["d1", "d2", "d3"]
        );
        assert!(remove_terminal_and_promote("job1", "d1").unwrap());
        let queue = read_queue("job1").unwrap();
        assert_eq!(queue.entries[0].delivery_id, "d2");
        assert_eq!(queue.entries[0].state, EntryState::ResumePending);
        assert_eq!(queue.entries[1].delivery_id, "d3");
        assert!(remove_terminal_and_promote("job1", "d2").unwrap());
        let queue = read_queue("job1").unwrap();
        assert_eq!(queue.entries[0].delivery_id, "d3");
        assert_eq!(queue.entries[0].state, EntryState::ResumePending);
        assert!(!remove_terminal_and_promote("job1", "d3").unwrap());
        assert!(read_queue("job1").unwrap().entries.is_empty());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn resume_ack_is_exactly_once_and_user_decision_has_no_watchdog() {
        let _guard = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        register("job2", "d1");
        assert!(matches!(
            enqueue("job2", "d1").unwrap(),
            EnqueueResult::Active { .. }
        ));
        mark_awaiting_decision("job2", "d1").unwrap();
        assert!(!recover_stalled_processing("job2").unwrap());

        {
            let _lock = acquire_lock("job2").unwrap();
            let mut queue = read_queue("job2").unwrap();
            let front = queue.entries.first_mut().unwrap();
            front.state = EntryState::ResumeSent;
            front.resume_attempts = 2;
            front.resume_sent_at = now_secs();
            front.resume_protocol_version = RESUME_ENVELOPE_VERSION;
            write_queue(&queue).unwrap();
        }
        assert_eq!(
            acknowledge_resume(
                "job2",
                "d1",
                Some(RESUME_ENVELOPE_VERSION),
                Some(2),
            )
            .unwrap(),
            ResumeAck::Accepted
        );
        assert_eq!(
            acknowledge_resume(
                "job2",
                "d1",
                Some(RESUME_ENVELOPE_VERSION),
                Some(2),
            )
            .unwrap(),
            ResumeAck::DuplicateOrStale
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn resume_ack_rejects_future_attempt_and_decision_replay_stays_pending() {
        let _guard = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        register("job4", "d1");
        assert!(matches!(
            enqueue("job4", "d1").unwrap(),
            EnqueueResult::Active {
                already_present: false,
                ..
            }
        ));
        mark_awaiting_decision("job4", "d1").unwrap();
        assert!(matches!(
            enqueue("job4", "d1").unwrap(),
            EnqueueResult::Active {
                already_present: true,
                ..
            }
        ));
        assert_eq!(
            read_queue("job4").unwrap().entries[0].state,
            EntryState::AwaitingDecision
        );

        {
            let _lock = acquire_lock("job4").unwrap();
            let mut queue = read_queue("job4").unwrap();
            let front = queue.entries.first_mut().unwrap();
            front.state = EntryState::ResumeSent;
            front.resume_attempts = 3;
            front.resume_sent_at = now_secs();
            front.resume_protocol_version = RESUME_ENVELOPE_VERSION;
            write_queue(&queue).unwrap();
        }
        assert_eq!(
            acknowledge_resume(
                "job4",
                "d1",
                Some(RESUME_ENVELOPE_VERSION),
                Some(4),
            )
            .unwrap(),
            ResumeAck::DuplicateOrStale
        );
        assert_eq!(
            acknowledge_resume("job4", "d1", None, None).unwrap(),
            ResumeAck::DuplicateOrStale
        );
        assert_eq!(
            acknowledge_resume(
                "job4",
                "d1",
                Some(RESUME_ENVELOPE_VERSION),
                Some(0),
            )
            .unwrap(),
            ResumeAck::DuplicateOrStale
        );
        assert_eq!(
            acknowledge_resume(
                "job4",
                "d1",
                Some(RESUME_ENVELOPE_VERSION),
                Some(3),
            )
            .unwrap(),
            ResumeAck::Accepted
        );
        assert!(matches!(
            enqueue("job4", "d1").unwrap(),
            EnqueueResult::Active {
                already_present: false,
                ..
            }
        ));
        assert!(matches!(
            enqueue("job4", "d1").unwrap(),
            EnqueueResult::Active {
                already_present: true,
                ..
            }
        ));
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn unversioned_resume_is_accepted_only_for_a_legacy_queue_entry() {
        let _guard = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        register("job5", "d1");
        enqueue("job5", "d1").unwrap();
        {
            let _lock = acquire_lock("job5").unwrap();
            let mut queue = read_queue("job5").unwrap();
            let front = queue.entries.first_mut().unwrap();
            front.state = EntryState::ResumeSent;
            front.resume_attempts = 1;
            front.resume_sent_at = 0;
            front.resume_protocol_version = 0;
            write_queue(&queue).unwrap();
        }
        assert_eq!(
            acknowledge_resume("job5", "d1", None, None).unwrap(),
            ResumeAck::Accepted
        );
        assert_eq!(
            acknowledge_resume("job5", "d1", None, None).unwrap(),
            ResumeAck::DuplicateOrStale
        );

        register("job8", "d1");
        enqueue("job8", "d1").unwrap();
        {
            let _lock = acquire_lock("job8").unwrap();
            let mut queue = read_queue("job8").unwrap();
            let front = queue.entries.first_mut().unwrap();
            front.state = EntryState::ResumeSent;
            front.resume_attempts = 2;
            front.resume_sent_at = now_secs();
            front.resume_protocol_version = 0;
            write_queue(&queue).unwrap();
        }
        assert_eq!(
            acknowledge_resume("job8", "d1", None, Some(3)).unwrap(),
            ResumeAck::DuplicateOrStale
        );
        assert_eq!(
            acknowledge_resume("job8", "d1", None, Some(2)).unwrap(),
            ResumeAck::Accepted
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn legacy_processing_migrates_from_persisted_facts() {
        let _guard = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", temp.path());

        register("job6", "d1");
        enqueue("job6", "d1").unwrap();
        consent::activate_delivery_context_exclusive("job6", "d1").unwrap();
        {
            let _lock = acquire_lock("job6").unwrap();
            let mut queue = read_queue("job6").unwrap();
            queue.entries[0].processing_started_at = 0;
            write_queue(&queue).unwrap();
        }
        assert!(migrate_legacy_processing("job6").unwrap());
        assert_eq!(
            read_queue("job6").unwrap().entries[0].state,
            EntryState::AwaitingDecision
        );

        register("job7", "d1");
        enqueue("job7", "d1").unwrap();
        {
            let _lock = acquire_lock("job7").unwrap();
            let mut queue = read_queue("job7").unwrap();
            queue.entries[0].processing_started_at = 0;
            write_queue(&queue).unwrap();
        }
        assert!(migrate_legacy_processing("job7").unwrap());
        assert_eq!(
            read_queue("job7").unwrap().entries[0].state,
            EntryState::ResumePending
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn stalled_processing_without_execution_is_requeued_not_terminal() {
        let _guard = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        register("job3", "d1");
        enqueue("job3", "d1").unwrap();
        {
            let _lock = acquire_lock("job3").unwrap();
            let mut queue = read_queue("job3").unwrap();
            queue.entries[0].processing_started_at = now_secs()
                .saturating_sub(PROCESSING_WATCHDOG_SEC + 1);
            write_queue(&queue).unwrap();
        }
        assert!(recover_stalled_processing("job3").unwrap());
        let queue = read_queue("job3").unwrap();
        assert_eq!(queue.entries[0].state, EntryState::ResumePending);
        assert!(!temp
            .path()
            .join("autotrade/outcomes/job3/d1.json")
            .exists());
        std::env::remove_var("ONCHAINOS_HOME");
    }
}
