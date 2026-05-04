//! Evaluator 证据查询
//!
//! disputeId 格式: `d-<jobId>-r<round>` — 解析 jobId，GET /evidence，下载双方图片到本地，
//! 再把 localPath 塞回响应对象，供多模态 agent 直接 open-image 阅读。
//!
//! 后端响应结构（扁平：provider/client 在顶层，无 `evidences` 包装层）：
//! ```json
//! {
//!   "jobId":"...", "title":"...", "description":"...", "descriptionSummary":"...",
//!   "provider": { "texts":["..."], "images":[ {"fileKey":"..."} | "..." , ...] },
//!   "client":   { "texts":["..."], "images":[ ... ] }
//! }
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Map, Value};

use super::helpers::parse_job_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// 双方证据袋的顶层 key（后端返回扁平结构，provider/client 直接在顶层）。
const EVIDENCE_SIDES: [&str; 2] = ["provider", "client"];

/// E1: fetch dispute evidence (text + images) for the evaluator. Downloads every referenced image
/// and adds `localPath` to each image entry so downstream multimodal agents can open the file.
pub async fn handle_info(
    client: &mut TaskApiClient,
    dispute_id: &str,
    agent_id: &str,
) -> Result<()> {
    let job_id = parse_job_id(dispute_id)?;

    let (_account_id, _address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let path = client.endpoint(&job_id, "evidence");
    let mut data = client.get_with_identity(&path, &agent_id).await?;

    let tmp_dir = std::env::temp_dir()
        .join("onchainos-dispute")
        .join(dispute_id);
    fs::create_dir_all(&tmp_dir)?;

    // 后端扁平结构：provider/client 直接在顶层（不嵌套在 evidences 下）
    for side in EVIDENCE_SIDES {
        let Some(bucket) = data.get_mut(side).and_then(Value::as_object_mut) else { continue };
        let Some(images) = bucket.get_mut("images").and_then(Value::as_array_mut) else { continue };
        for item in images.iter_mut() {
            let Some(file_key) = extract_file_key(item) else { continue };
            let mut merged = normalize_image_item(item, &file_key);
            match download_image(client, &job_id, &file_key, &tmp_dir, &agent_id).await {
                Ok(p) => {
                    merged.insert(
                        "localPath".into(),
                        Value::String(p.to_string_lossy().into()),
                    );
                }
                Err(e) => {
                    merged.insert("downloadError".into(), Value::String(e.to_string()));
                }
            }
            *item = Value::Object(merged);
        }
    }

    println!("{}", serde_json::to_string_pretty(&data)?);
    Ok(())
}

/// 从 image item 里挖 fileKey —— item 可能是裸字符串，也可能是 object with {fileKey|key|name|url}。
fn extract_file_key(item: &Value) -> Option<String> {
    if let Some(s) = item.as_str() {
        return Some(s.to_string());
    }
    for k in ["fileKey", "key", "name", "url"] {
        if let Some(s) = item.get(k).and_then(Value::as_str) {
            return Some(s.to_string());
        }
    }
    None
}

/// 规范化 image item 成 object；若原本是裸字符串则升级为 `{fileKey}`。
fn normalize_image_item(item: &Value, file_key: &str) -> Map<String, Value> {
    match item {
        Value::Object(m) => {
            let mut out = m.clone();
            out.entry("fileKey".to_string())
                .or_insert_with(|| Value::String(file_key.to_string()));
            out
        }
        _ => {
            let mut out = Map::new();
            out.insert("fileKey".into(), json!(file_key));
            out
        }
    }
}

/// 拉取证据二进制：`GET /priapi/v1/aieco/task/{jobId}/evidence/download?fileKey=<...>`。
/// 后端对该端点强制 JWT + agenticId 鉴权，所以走 TaskApiClient 的
/// `get_bytes_with_identity`（裸 reqwest + 注入 token / agenticId header）。
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

/// 把单张证据图下载到 `tmp_dir`，返回本地路径。
async fn download_image(
    client: &TaskApiClient,
    job_id: &str,
    file_key: &str,
    tmp_dir: &Path,
    agent_id: &str,
) -> Result<PathBuf> {
    let bytes = fetch_evidence_bytes(client, job_id, file_key, agent_id).await?;
    // fileKey 可能含 `/` 或 query-safe 字符；取最后一段做本地文件名
    let filename = file_key.rsplit('/').next().unwrap_or(file_key);
    let path = tmp_dir.join(filename);
    fs::write(&path, &bytes)?;
    Ok(path)
}
