use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Map, Value};

use super::dispute_status;
use super::helpers::evidence_dir;
use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// Top-level keys of the two evidence buckets (backend returns a flat
/// structure: `provider` / `client` sit directly at the top).
const EVIDENCE_SIDES: [&str; 2] = ["provider", "client"];

pub async fn handle_info(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    round_num: &str,
) -> Result<()> {
    // Pre-gate: the 4 AND hard gates before commit (taskStatus not terminal /
    // round aligned / disputeStatus=CommitPhase / selectedVoter non-null).
    // Any failure → precheck already printed `reason: ...` + `selected: no`;
    // this function returns early without downloading evidence.
    if !dispute_status::precheck_round_gate(client, job_id, agent_id, round_num).await? {
        return Ok(());
    }

    let path = client.endpoint(job_id, "evidence");
    let mut data = client.get_with_identity(&path, agent_id).await?;

    let tmp_dir = evidence_dir(job_id, agent_id)?;
    fs::create_dir_all(&tmp_dir)?;

    // Backend flat structure: top level holds task metadata (title/description)
    // plus the two evidence buckets `provider` / `client`, each carrying:
    //   - `reason`: provider.reason = dispute-raise reason; client.reason = reject-delivery reason
    //   - `texts[]`: free-text evidence
    //   - `files[]`: file evidence (any type — not limited to images)
    // `reason` / `texts[]` are JSON passthroughs; only `files[]` items need download.
    for side in EVIDENCE_SIDES {
        let Some(bucket) = data.get_mut(side).and_then(Value::as_object_mut) else { continue };
        let Some(files) = bucket.get_mut("files").and_then(Value::as_array_mut) else { continue };
        for item in files.iter_mut() {
            let Some(file_key) = item.as_str().map(str::to_string) else { continue };
            let mut merged = Map::new();
            merged.insert("fileKey".into(), json!(&file_key));
            match download_file(client, job_id, &file_key, &tmp_dir, agent_id).await {
                Ok(p) => {
                    merged.insert(
                        "localPath".into(),
                        Value::String(p.to_string_lossy().into()),
                    );
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    audit::log(
                        "cli",
                        "evaluator/evidence_download_failed",
                        false,
                        Duration::default(),
                        Some(vec![
                            format!("jobId={job_id}"),
                            format!("agentId={agent_id}"),
                            format!("side={side}"),
                            format!("fileKey={file_key}"),
                        ]),
                        Some(&err_msg),
                    );
                    merged.insert("downloadError".into(), Value::String(err_msg));
                }
            }
            *item = Value::Object(merged);
        }
    }

    println!("{}", serde_json::to_string_pretty(&data)?);

    println!();
    println!("---");
    println!();
    print!("{}", super::flow::evaluator_selected_post_evidence_steps(job_id, agent_id));
    Ok(())
}

pub(super) async fn fetch_evidence_bytes(
    client: &TaskApiClient,
    job_id: &str,
    file_key: &str,
    agent_id: &str,
) -> Result<Vec<u8>> {
    let path = format!("{}/evidence/download", client.task_path(job_id));
    client
        .get_bytes_with_identity(&path, &[("fileKey", file_key)], agent_id)
        .await
}

/// Download a single evidence file into `tmp_dir`; returns the local path.
///
/// fileKey shape is `<jobId>/<idx>/<uuid>` — strip the jobId prefix, join the
/// rest with `_` into `<idx>_<uuid>` and write the file with **no extension**.
/// The CLI deliberately does NOT magic-byte-sniff: the evaluator agent
/// inspects the file content itself (see the evaluator playbook Step 3 +
/// `references/evaluator-decision-rubric.md` Pass 4 for the probe procedure).
async fn download_file(
    client: &TaskApiClient,
    job_id: &str,
    file_key: &str,
    tmp_dir: &Path,
    agent_id: &str,
) -> Result<PathBuf> {
    let bytes = fetch_evidence_bytes(client, job_id, file_key, agent_id).await?;
    let filename = file_key
        .split_once('/')
        .map(|(_, rest)| rest.replace('/', "_"))
        .unwrap_or_else(|| file_key.to_string());
    let path = tmp_dir.join(filename);
    fs::write(&path, &bytes)?;
    Ok(path)
}
