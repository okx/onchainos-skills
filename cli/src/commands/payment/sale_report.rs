//! Best-effort sale reports run in a detached worker, never in the service path.

use std::{
    future::Future,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const REPORT_PATH: &str = "/priapi/v5/wallet/agentic/agent/service-sale-report";
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaleContext {
    agent_id: String,
    service_id: String,
}

impl SaleContext {
    pub fn new(agent_id: &str, service_id: &str) -> Result<Self> {
        if agent_id.trim().is_empty() || service_id.trim().is_empty() {
            bail!("sale_report_invalid_context: agentId and serviceId are required");
        }
        Ok(Self {
            agent_id: agent_id.trim().into(),
            service_id: service_id.trim().into(),
        })
    }

    pub fn validate(&self) -> Result<()> {
        Self::new(&self.agent_id, &self.service_id)?;
        Ok(())
    }

    fn body(&self, sell_time: u64) -> Value {
        json!({
            "agentId": self.agent_id, "serviceId": self.service_id,
            "serviceType": "A2MCP", "sellTime": sell_time,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct Work {
    context: SaleContext,
    owner: String,
    sell_time: u64,
}

fn check_owner(owner: &str) -> Result<()> {
    if super::state::current_owner_id().as_deref() != Some(owner) {
        bail!("sale_report_account_changed");
    }
    Ok(())
}

async fn send(owner: &str, body: Value) -> Result<Value> {
    check_owner(owner)?;
    let token = crate::commands::agentic_wallet::auth::ensure_tokens_refreshed().await?;
    check_owner(owner)?;
    crate::wallet_api::WalletApiClient::new()?
        .post_authed_mutation_no_retry(REPORT_PATH, &token, &body)
        .await
}

async fn report_with<F, Fut>(body: Value, timeout: Duration, mut send: F) -> Result<()>
where
    F: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let mut last_error = anyhow!("sale_report_not_acknowledged");
    for _ in 0..2 {
        match tokio::time::timeout(timeout, send(body.clone())).await {
            Ok(Ok(data))
                if data.as_array().is_some_and(|rows| rows.len() == 1)
                    && data[0]["success"] == true =>
            {
                return Ok(())
            }
            Ok(Ok(_)) => last_error = anyhow!("sale_report_not_acknowledged"),
            Ok(Err(error)) => last_error = error,
            Err(_) => last_error = anyhow!("sale_report_timeout"),
        }
    }
    Err(last_error)
}

fn log_result(started: Instant, result: &Result<()>) {
    let error = result.as_ref().err().map(|error| format!("{error:#}"));
    crate::audit::log(
        "cli",
        "a2mcp/sale-report",
        result.is_ok(),
        started.elapsed(),
        None,
        error.as_deref(),
    );
}

fn spawn_worker(executable: &Path, work: &Work) -> Result<Child> {
    let mut command = Command::new(executable);
    if crate::endpoints::base_url() == crate::endpoints::DEV_BASE_URL {
        command.arg("--dev");
    }
    command
        .args(["agent", "a2mcp", "report-sale"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Do not inherit the foreground tool's process group or output pipes.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000200); // no window, new process group
    }
    let mut child = command.spawn()?;
    let result = (|| -> Result<()> {
        let mut stdin = child
            .stdin
            .take()
            .context("sale_report_worker_stdin_missing")?;
        stdin.write_all(&serde_json::to_vec(work)?)?;
        Ok(()) // closing stdin releases the worker to perform the report
    })();
    if let Err(error) = result {
        let _ = child.kill();
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Err(error);
    }
    Ok(child)
}

/// Launch and return without waiting for auth, network I/O, or retries. No queue
/// or new state files. A separate process survives the foreground CLI's exit.
pub fn record_success(context: &SaleContext, owner: &str) {
    let started = Instant::now();
    let result = (|| -> Result<()> {
        let work = Work {
            context: context.clone(),
            owner: owner.into(),
            sell_time: u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?,
        };
        let mut child = spawn_worker(&std::env::current_exe()?, &work)?;
        // Reap while the parent lives; never join this thread in the service path.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    })();
    if result.is_err() {
        log_result(started, &result);
    }
}

/// Internal worker input travels over stdin, not shell arguments or state files.
pub async fn run_worker() -> Result<()> {
    let started = Instant::now();
    let work: Work = serde_json::from_reader(std::io::stdin().take(16 * 1024))?;
    work.context.validate()?;
    if work.owner.is_empty() || work.sell_time == 0 {
        bail!("sale_report_invalid_work");
    }
    let result = report_with(work.context.body(work.sell_time), ATTEMPT_TIMEOUT, |body| {
        send(&work.owner, body)
    })
    .await;
    log_result(started, &result);
    // Reporting failures are logged, never returned as service failures.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retry_once_reuses_identical_body_and_sell_time() {
        let body = SaleContext::new("7514", "2130").unwrap().body(1000);
        assert_eq!(
            body,
            json!({"agentId":"7514", "serviceId":"2130", "serviceType":"A2MCP", "sellTime":1000})
        );
        let mut calls = Vec::new();
        report_with(body.clone(), ATTEMPT_TIMEOUT, |body| {
            calls.push(body);
            let attempt = calls.len();
            async move {
                if attempt == 1 {
                    bail!("response lost after remote insertion");
                }
                Ok(json!([{"success":true,"duplicated":true}]))
            }
        })
        .await
        .unwrap();
        assert_eq!(calls, vec![body.clone(), body]);
    }

    #[tokio::test]
    async fn stop_after_first_success_or_second_failure() {
        for (success, expected) in [(true, 1), (false, 2)] {
            let mut calls = 0;
            let result = report_with(json!({}), ATTEMPT_TIMEOUT, |_| {
                calls += 1;
                async move { Ok(json!([{"success":success,"duplicated":false}])) }
            })
            .await;
            assert_eq!(calls, expected);
            assert_eq!(result.is_ok(), success);
        }
    }

    #[tokio::test]
    async fn timeout_also_retries_only_once() {
        let mut calls = 0;
        let result = report_with(json!({}), Duration::from_millis(1), |_| {
            calls += 1;
            std::future::pending::<Result<Value>>()
        })
        .await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("sale_report_timeout"));
        assert_eq!(calls, 2);
    }

    #[cfg(unix)]
    #[test]
    fn launching_worker_does_not_wait_for_it_to_finish() {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let script = dir.path().join("worker");
        std::fs::write(&script, "#!/bin/sh\ncat >/dev/null\nexec sleep 10\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut child = spawn_worker(
            &script,
            &Work {
                context: SaleContext::new("1", "svc").unwrap(),
                owner: "owner".into(),
                sell_time: 1000,
            },
        )
        .unwrap();
        let running = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            running,
            "launch must return while the worker is still running"
        );
    }
}
