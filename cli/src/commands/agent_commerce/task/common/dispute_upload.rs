//! Upload dispute evidence (off-chain only, multipart) — shared by buyer and seller.
//!
//! Maps to backend `POST /priapi/v1/aieco/task/{jobId}/evidence/upload`,
//! Content-Type: multipart/form-data, fields `text` and/or `images[]`.
//! Can only be submitted within the 1-hour preparation window; never goes on-chain.
//!
//! Implementation note: the multipart body is assembled manually per RFC 7578 (curl-compatible format)
//! instead of using reqwest's `multipart::Form` builder. Reasons:
//! 1) The reqwest builder sends the body with chunked transfer by default, and some Spring/Tomcat
//!    configurations reject multipart requests that lack a Content-Length.
//! 2) Hand-rolled format lets us control part-header quoting / boundary / ordering so the wire format
//!    matches curl output, making backend integration debugging straightforward.

use anyhow::{bail, Result};
use std::path::Path;
use tokio::fs;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// Allowed image extensions (aligned with backend validation).
const ALLOWED_IMG_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

/// Max byte length of the text field (rough pre-escape upper bound to prevent oversized pastes from bloating the multipart body).
const MAX_TEXT_BYTES: usize = 16 * 1024;

/// Max byte size per image (aligned with backend multipart.maxFileSize; client-side fail-fast to avoid wasting bandwidth on oversized files).
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// Upload off-chain evidence (shared entrypoint for buyer and seller).
pub async fn handle_upload_evidence(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    text: Option<&str>,
    image_paths: &[String],
) -> Result<()> {
    // Align with backend `StringUtils.isBlank`: trim then check empty.
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
    // Length precheck before escaping — more intuitive than post-escape, and short-circuits large pastes.
    if let Some(t) = text_clean.as_deref() {
        if t.len() > MAX_TEXT_BYTES {
            bail!(
                "--text 过长：{} 字节，上限 {} 字节",
                t.len(),
                MAX_TEXT_BYTES
            );
        }
    }

    // Collect image part metadata (read file contents into memory first so Content-Length can be computed uniformly).
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
        if bytes.len() > MAX_IMAGE_BYTES {
            bail!(
                "图片 {p} 过大：{} 字节 ({:.1} MB)，单文件上限 20 MB",
                bytes.len(),
                bytes.len() as f64 / 1_048_576.0
            );
        }
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

    // Manually assemble the multipart body.
    let boundary = format!("----onchainos-{:016x}", rand_u64());
    let mut body: Vec<u8> = Vec::new();

    if let Some(t) = text_clean.as_deref() {
        // Align with backend validation: the text field value must be wrapped in literal double quotes
        // (curl's `--form 'text="..."'` forwards the double quotes into the multipart body, and the backend
        // parses it in that format; missing quotes will be rejected with code=1001).
        // Inner quotes must be escaped to avoid prematurely ending the part body.
        let escaped = t.replace('\\', "\\\\").replace('"', "\\\"");
        let wrapped = format!("\"{escaped}\"");
        eprintln!(
            "[dispute_upload] text part: raw_bytes={} wrapped_bytes={}\n\
             [dispute_upload] text content (raw):\n{t}\n\
             [dispute_upload] text content (wrapped, sent on wire):\n{wrapped}",
            t.len(),
            wrapped.len(),
        );
        // Align with curl `--form 'text=...'` behavior: the plain-text part has no Content-Type header,
        // otherwise Spring would treat it as a multipart-file and the @RequestParam String text binding would fail.
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

    // Closing boundary.
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

/// Simple non-cryptographic random number (used only for boundary generation; no cryptographic strength required).
fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    nanos.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(pid)
}
