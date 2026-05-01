//! 上传仲裁证据（纯链下，multipart）— 买卖双方共用
//!
//! 对应后端 `POST /priapi/v1/aieco/task/{jobId}/evidence/upload`，
//! Content-Type: multipart/form-data，字段 `text` 和/或 `images[]`。
//! 仅在 1 小时准备期内可提交，不上链。
//!
//! 实现说明：手动按 RFC 7578 拼 multipart body（curl 兼容格式），
//! 而不是用 reqwest 的 `multipart::Form` builder。原因：
//! 1) reqwest builder 默认把 body 用 chunked transfer 发送，部分 Spring/Tomcat
//!    配置不接受没有 Content-Length 的 multipart；
//! 2) 手写格式可控制 part header 的引号 / boundary / 顺序，与 curl 输出对得上，
//!    便于和后端联调对照。

use anyhow::{bail, Result};
use std::path::Path;
use tokio::fs;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// 允许的图片扩展名（与后端校验对齐）
const ALLOWED_IMG_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

/// 上传链下证据（买卖双方共用入口）
pub async fn handle_upload_evidence(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    text: Option<&str>,
    image_paths: &[String],
) -> Result<()> {
    // 与 backend `StringUtils.isBlank` 对齐：trim 后再判空
    let text_clean: Option<String> = text
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    eprintln!(
        "[dispute_upload] input: text_raw_len={} text_clean_len={} image_count={}",
        text.map_or(0, str::len),
        text_clean.as_deref().map_or(0, str::len),
        image_paths.len(),
    );

    if text_clean.is_none() && image_paths.is_empty() {
        bail!("必须提供 --text（非空白）或 --image 之一");
    }

    // 收集 image part 元信息（先把文件内容读到内存，方便统一计算 Content-Length）
    struct ImagePart {
        filename: String,
        mime: &'static str,
        bytes: Vec<u8>,
    }
    let mut images: Vec<ImagePart> = Vec::with_capacity(image_paths.len());
    for (idx, p) in image_paths.iter().enumerate() {
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
        let original_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image");
        let filename = if original_name.is_ascii() {
            original_name.to_string()
        } else {
            format!("image_{idx}.{ext}")
        };
        let mime: &'static str = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => "application/octet-stream",
        };
        eprintln!(
            "[dispute_upload] image part: name=images filename={filename} mime={mime} bytes={}",
            bytes.len(),
        );
        images.push(ImagePart { filename, mime, bytes });
    }

    // 手动拼 multipart body
    let boundary = format!("----onchainos-{:016x}", rand_u64());
    let mut body: Vec<u8> = Vec::new();

    if let Some(t) = text_clean.as_deref() {
        // 对齐 backend 校验：text 字段值必须带字面双引号包裹（curl 的 `--form 'text="..."'`
        // 透传双引号到 multipart body，backend 按这种格式 parse；不带引号会被拒 code=1001）
        // 内层引号需转义防止结束 part body
        let escaped = t.replace('\\', "\\\\").replace('"', "\\\"");
        let wrapped = format!("\"{escaped}\"");
        eprintln!(
            "[dispute_upload] text part: raw_bytes={} wrapped_bytes={}",
            t.len(),
            wrapped.len(),
        );
        // 对齐 curl `--form 'text=...'` 行为：纯文本 part 不带 Content-Type header，
        // 否则 Spring 会把它当成 multipart-file，导致 @RequestParam String text 绑定失败。
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"text\"\r\n\r\n");
        body.extend_from_slice(wrapped.as_bytes());
        body.extend_from_slice(b"\r\n");
    }

    for img in &images {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"images\"; filename=\"{}\"\r\n",
                img.filename
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", img.mime).as_bytes());
        body.extend_from_slice(&img.bytes);
        body.extend_from_slice(b"\r\n");
    }

    // 结束 boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    eprintln!(
        "[dispute_upload] body: total_bytes={} boundary={boundary}",
        body.len(),
    );

    let path = client.endpoint(job_id, "evidence/upload");
    let content_type = format!("multipart/form-data; boundary={boundary}");
    client
        .raw_post_with_identity(&path, body, &content_type, agent_id)
        .await?;

    println!("✓ 证据已上传（链下，1h 准备期内生效）");
    println!("  jobId:  {job_id}");
    if let Some(t) = text_clean.as_deref() {
        println!("  文本:   {t}");
    }
    if !image_paths.is_empty() {
        println!("  图片数: {}", image_paths.len());
    }
    Ok(())
}

/// 简单的非加密随机数（仅用于生成 boundary，不需要密码学强度）
fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    nanos.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(pid)
}
