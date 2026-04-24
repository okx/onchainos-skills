//! Evaluator 证据查询
//!
//! disputeId 格式: `d-<jobId>-r<round>` — 解析 jobId，GET /evidence，下载双方图片到本地，
//! 再把 localPath 塞回响应对象，供多模态 agent 直接 open-image 阅读。
//!
//! 后端真响应结构（Lark wiki §7）：
//! ```json
//! {
//!   "jobId":"...", "title":"...", "description":"...", "description_summary":"...",
//!   "evidences": {
//!     "provider": { "texts":["..."], "images":[ {"fileKey":"..."} | "..." , ...] },
//!     "client":   { "texts":["..."], "images":[ ... ] }
//!   }
//! }
//! ```
//! mock-api 可能仍返回平铺数组（`evidences: [{kind:"image"|"text", name|text}]`），本模块对两种
//! 形状都兼容处理。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde_json::{json, Map, Value};

use super::helpers::parse_job_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// E1: fetch dispute evidence (text + images) for the evaluator. Downloads every referenced image
/// and adds `localPath` to each image entry so downstream multimodal agents can open the file.
pub async fn handle_info(client: &mut TaskApiClient, dispute_id: &str) -> Result<()> {
    let job_id = parse_job_id(dispute_id)?;

    let path = client.endpoint(&job_id, "evidence");
    let mut data = client.get(&path).await?;

    let tmp_dir = std::env::temp_dir()
        .join("onchainos-dispute")
        .join(dispute_id);
    fs::create_dir_all(&tmp_dir)?;

    match data["evidences"].clone() {
        // 真后端结构：{provider: {texts,images}, client: {texts,images}}
        Value::Object(mut by_side) => {
            for side in ["provider", "client"] {
                if let Some(bucket) = by_side.get_mut(side).and_then(Value::as_object_mut) {
                    if let Some(images) = bucket.get_mut("images").and_then(Value::as_array_mut) {
                        for item in images.iter_mut() {
                            if let Some(file_key) = extract_file_key(item) {
                                let mut merged = normalize_image_item(item, &file_key);
                                match download_image(client, &job_id, &file_key, &tmp_dir).await {
                                    Ok(p) => {
                                        merged.insert(
                                            "localPath".into(),
                                            Value::String(p.to_string_lossy().into()),
                                        );
                                    }
                                    Err(e) => {
                                        merged.insert(
                                            "downloadError".into(),
                                            Value::String(e.to_string()),
                                        );
                                    }
                                }
                                *item = Value::Object(merged);
                            }
                        }
                    }
                }
            }
            data["evidences"] = Value::Object(by_side);
        }

        // mock-api 兼容：evidences 是平铺数组，kind=image 的项带 name/fileKey
        Value::Array(evs) => {
            let mut enriched = Vec::with_capacity(evs.len());
            for ev in evs {
                let mut ev2 = ev.clone();
                if ev["kind"].as_str() == Some("image") {
                    if let Some(file_key) = extract_file_key(&ev) {
                        match download_image(client, &job_id, &file_key, &tmp_dir).await {
                            Ok(p) => {
                                ev2["localPath"] =
                                    Value::String(p.to_string_lossy().into());
                            }
                            Err(e) => {
                                ev2["downloadError"] = Value::String(e.to_string());
                            }
                        }
                    }
                }
                enriched.push(ev2);
            }
            data["evidences"] = Value::Array(enriched);
        }

        _ => { /* null / 其他：原样透传 */ }
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

/// 图片下载走裸 reqwest（二进制流，不能经 handle_response 的 JSON 解析）。
/// 路径对齐真后端：`GET /priapi/v1/aieco/task/{jobId}/evidence/download?fileKey=<...>`。
async fn download_image(
    client: &TaskApiClient,
    job_id: &str,
    file_key: &str,
    tmp_dir: &Path,
) -> Result<PathBuf> {
    let url = format!(
        "{}{}/evidence/download",
        client.base_url().trim_end_matches('/'),
        client.endpoint(job_id, "").trim_end_matches('/'),
    );
    let resp = client
        .http()
        .get(&url)
        .query(&[("fileKey", file_key)])
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("evidence download failed ({}): {}", resp.status(), url);
    }
    let bytes = resp.bytes().await?;
    // fileKey 可能含 `/` 或 query-safe 字符；取最后一段做本地文件名
    let filename = file_key.rsplit('/').next().unwrap_or(file_key);
    let path = tmp_dir.join(filename);
    fs::write(&path, &bytes)?;
    Ok(path)
}
