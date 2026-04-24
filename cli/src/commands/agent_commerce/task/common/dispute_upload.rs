//! 上传仲裁证据（纯链下，multipart）— 买卖双方共用
//!
//! 对应后端 `POST /priapi/v1/aieco/task/{jobId}/evidence/upload`，
//! Content-Type: multipart/form-data，字段 `text` 和/或 `images[]`。
//! 仅在 1 小时准备期内可提交，不上链。

use anyhow::{bail, Result};
use reqwest::multipart::{Form, Part};
use std::path::Path;
use tokio::fs;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// 允许的图片扩展名（与后端校验对齐）
const ALLOWED_IMG_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

/// 上传链下证据（买卖双方共用入口）
///
/// 调用方须先完成钱包解析，把 `agent_id` + `address` 传进来。
/// - 买家：`signing::resolve_wallet_and_agent_for_task`
/// - 卖家：`signing::resolve_wallet_and_agent_for_provider`
pub async fn handle_upload_evidence(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    address: &str,
    text: Option<&str>,
    image_paths: &[String],
) -> Result<()> {
    // 至少一个字段必须提供
    if text.is_none_or(str::is_empty) && image_paths.is_empty() {
        bail!("必须提供 --text 或 --image 之一");
    }

    let mut form = Form::new();
    if let Some(t) = text {
        if !t.is_empty() {
            form = form.text("text", t.to_string());
        }
    }

    for p in image_paths {
        let path = Path::new(p);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        if !ALLOWED_IMG_EXTS.contains(&ext.as_str()) {
            bail!("不支持的图片格式: {p}（仅支持 {ALLOWED_IMG_EXTS:?}）");
        }
        let bytes = fs::read(path)
            .await
            .map_err(|e| anyhow::anyhow!("读取文件 {p} 失败: {e}"))?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image")
            .to_string();
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => "application/octet-stream",
        };
        let part = Part::bytes(bytes).file_name(filename).mime_str(mime)?;
        form = form.part("images", part);
    }

    let path = client.endpoint(job_id, "evidence/upload");
    client.multipart_post_with_identity(&path, form, agent_id, address).await?;

    println!("✓ 证据已上传（链下，1h 准备期内生效）");
    println!("  jobId:  {job_id}");
    if let Some(t) = text {
        if !t.is_empty() {
            println!("  文本:   {t}");
        }
    }
    if !image_paths.is_empty() {
        println!("  图片数: {}", image_paths.len());
    }
    Ok(())
}
