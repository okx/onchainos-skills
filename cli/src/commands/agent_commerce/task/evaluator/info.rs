use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Map, Value};

use super::helpers::evidence_dir;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// 双方证据袋的顶层 key（后端返回扁平结构，provider/client 直接在顶层）。
const EVIDENCE_SIDES: [&str; 2] = ["provider", "client"];

pub async fn handle_info(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    let (_account_id, _address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let path = client.endpoint(job_id, "evidence");
    let mut data = client.get_with_identity(&path, &agent_id).await?;

    let tmp_dir = evidence_dir(job_id, &agent_id)?;
    fs::create_dir_all(&tmp_dir)?;

    // 后端扁平结构：provider/client 直接在顶层；images 为 `<jobId>/<idx>/<uuid>` 字符串数组。
    for side in EVIDENCE_SIDES {
        let Some(bucket) = data.get_mut(side).and_then(Value::as_object_mut) else { continue };
        let Some(images) = bucket.get_mut("images").and_then(Value::as_array_mut) else { continue };
        for item in images.iter_mut() {
            let Some(file_key) = item.as_str().map(str::to_string) else { continue };
            let mut merged = Map::new();
            merged.insert("fileKey".into(), json!(&file_key));
            match download_image(client, job_id, &file_key, &tmp_dir, &agent_id).await {
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
/// fileKey 形态 `<jobId>/<idx>/<uuid>` —— 去掉 jobId 前缀后用 `_` 拼成 `<idx>_<uuid>`，
/// 再按 magic bytes 嗅扩展名（png/jpg/gif/webp），让本地文件能直接被图片预览器打开。
async fn download_image(
    client: &TaskApiClient,
    job_id: &str,
    file_key: &str,
    tmp_dir: &Path,
    agent_id: &str,
) -> Result<PathBuf> {
    let bytes = fetch_evidence_bytes(client, job_id, file_key, agent_id).await?;
    let stem = file_key
        .split_once('/')
        .map(|(_, rest)| rest.replace('/', "_"))
        .unwrap_or_else(|| file_key.to_string());
    let filename = match sniff_image_ext(&bytes) {
        Some(ext) => format!("{stem}.{ext}"),
        None => stem,
    };
    let path = tmp_dir.join(filename);
    fs::write(&path, &bytes)?;
    Ok(path)
}

/// 按 magic bytes 嗅常见图片格式，返回不带点的扩展名。未识别返回 None。
fn sniff_image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}
