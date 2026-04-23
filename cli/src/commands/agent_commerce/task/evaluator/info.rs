use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use super::helpers::{parse_job_id, task_api_url};
use crate::commands::Context;

/// E1: fetch dispute evidence (text + images) for the evaluator.
/// disputeId format: `d-<jobId>-r<round>` — parse jobId, call /evidence;
/// download image bytes locally so multimodal agents can view them.
pub async fn run_info(dispute_id: String, _ctx: &Context) -> Result<()> {
    let job_id = parse_job_id(&dispute_id)?;
    let api = task_api_url();
    let client = reqwest::Client::new();

    let resp: serde_json::Value = client
        .get(format!("{api}/api/v1/task/{job_id}/evidence"))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach task backend: {e}"))?
        .json()
        .await?;
    if resp["code"] != 0 {
        bail!("{}", resp["msg"].as_str().unwrap_or("error"));
    }

    let mut data = resp["data"].clone();
    let tmp_dir = std::env::temp_dir()
        .join("onchainos-dispute")
        .join(&dispute_id);
    fs::create_dir_all(&tmp_dir)?;

    if let Some(evs) = data["evidences"].as_array().cloned() {
        let mut enriched = Vec::with_capacity(evs.len());
        for ev in evs {
            let mut ev2 = ev.clone();
            if ev["kind"].as_str() == Some("image") {
                if let Some(name) = ev["name"].as_str() {
                    match download_image(&client, &api, &job_id, name, &tmp_dir).await {
                        Ok(path) => {
                            ev2["localPath"] = serde_json::Value::String(path.to_string_lossy().into());
                        }
                        Err(e) => {
                            ev2["downloadError"] = serde_json::Value::String(e.to_string());
                        }
                    }
                }
            }
            enriched.push(ev2);
        }
        data["evidences"] = serde_json::Value::Array(enriched);
    }

    println!("{}", serde_json::to_string_pretty(&data)?);
    Ok(())
}

async fn download_image(
    client: &reqwest::Client,
    api: &str,
    job_id: &str,
    name: &str,
    tmp_dir: &Path,
) -> Result<PathBuf> {
    let bytes = client
        .get(format!("{api}/api/v1/task/{job_id}/evidence/download"))
        .query(&[("name", name)])
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let path = tmp_dir.join(name);
    fs::write(&path, &bytes)?;
    Ok(path)
}

