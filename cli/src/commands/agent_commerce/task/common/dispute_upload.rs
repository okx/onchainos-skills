//! Upload dispute evidence (off-chain only, multipart) — shared by buyer and seller.
//!
//! Maps to backend `POST /priapi/v1/aieco/task/{jobId}/evidence/upload`,
//! Content-Type: multipart/form-data, fields `text` and/or `files[]`.
//! Can only be submitted within the 1-hour preparation window; never goes on-chain.
//!
//! In addition to the explicit `--text` / `--file` inputs, the CLI auto-attaches every
//! entry recorded in `~/.onchainos/deliverables/<role>/<jobId>/manifest.json`
//! (buyer side: the downloaded deliverable + any later attachments; provider side:
//! the submitted deliverable copy). This guarantees the arbitrator always sees the
//! actual deliverable artefacts even if the user only writes a textual summary.
//!
//! Implementation note: the multipart body is assembled manually per RFC 7578 (curl-compatible format)
//! instead of using reqwest's `multipart::Form` builder. Reasons:
//! 1) The reqwest builder sends the body with chunked transfer by default, and some Spring/Tomcat
//!    configurations reject multipart requests that lack a Content-Length.
//! 2) Hand-rolled format lets us control part-header quoting / boundary / ordering so the wire format
//!    matches curl output, making backend integration debugging straightforward.

use anyhow::{bail, Result};
use std::path::PathBuf;
use tokio::fs;

use crate::commands::agent_commerce::task::common::deliverables;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// Max byte length of the text field (rough pre-escape upper bound to prevent oversized pastes from bloating the multipart body).
const MAX_TEXT_BYTES: usize = 16 * 1024;

/// Soft cap per attachment file (matches `deliverables.rs::MAX_FILE_SIZE`, so
/// anything successfully saved via `task-deliverable-save` is also uploadable).
/// Larger files are skipped (manifest entries) or bail (explicit `--file`) —
/// the current transport buffers the entire body in memory, so unbounded
/// uploads would risk OOM on agent hosts.
const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Upload off-chain evidence (shared entrypoint for buyer and seller).
///
/// `role` selects which local deliverables manifest to auto-attach
/// (`buyer` → `~/.onchainos/deliverables/buyer/<jobId>/`,
/// `provider` → `~/.onchainos/deliverables/provider/<jobId>/`).
pub async fn handle_upload_evidence(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    role: &str,
    text: Option<&str>,
    explicit_file_paths: &[String],
) -> Result<()> {
    if role != "buyer" && role != "provider" {
        bail!("--role must be 'buyer' or 'provider', got '{role}'");
    }

    // Align with backend `StringUtils.isBlank`: trim then check empty.
    let text_clean: Option<String> = text
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // Length precheck before escaping — more intuitive than post-escape, and short-circuits large pastes.
    if let Some(t) = text_clean.as_deref() {
        if t.len() > MAX_TEXT_BYTES {
            bail!(
                "--text too long: {} bytes, limit is {} bytes",
                t.len(),
                MAX_TEXT_BYTES
            );
        }
    }

    // Collect every file path that will be uploaded as a multipart attachment,
    // tagged with its source so missing-file handling can diverge: explicit
    // user-supplied paths must exist (bail on miss); manifest entries are
    // best-effort (skip + warn on miss, so a stale manifest doesn't kill the
    // auto-upload triggered by `job_disputed`).
    let mut file_paths: Vec<(PathBuf, bool /* is_explicit */)> = explicit_file_paths
        .iter()
        .map(|p| (PathBuf::from(p), true))
        .collect();

    // Best-effort manifest read: a corrupted manifest must NOT kill the upload
    // (the chat history `--text` is still valuable on its own).
    let manifest = match deliverables::read_manifest(role, job_id) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "[dispute_upload] WARN: manifest at ~/.onchainos/deliverables/{role}/{job_id}/manifest.json \
                 unreadable ({e}); proceeding without auto-attached deliverables"
            );
            None
        }
    };
    let mut manifest_filenames: Vec<String> = Vec::new();
    if let Some(m) = manifest.as_ref() {
        let dir = deliverables::deliverables_dir(role, job_id)?;
        for entry in &m.entries {
            file_paths.push((dir.join(&entry.filename), false));
            manifest_filenames.push(entry.filename.clone());
        }
    }

    eprintln!(
        "[dispute_upload] input: role={role} text_raw_len={} text_clean_len={} \
         explicit_files={} manifest_entries={}",
        text.map_or(0, str::len),
        text_clean.as_deref().map_or(0, str::len),
        explicit_file_paths.len(),
        manifest_filenames.len(),
    );

    if text_clean.is_none() && file_paths.is_empty() {
        bail!(
            "no evidence to upload: --text is blank, no --file was given, and no local \
             deliverables were found at ~/.onchainos/deliverables/{role}/{job_id}/"
        );
    }

    // Read every attachment into memory so Content-Length can be computed up-front.
    struct FilePart {
        filename: String,
        mime: &'static str,
        bytes: Vec<u8>,
    }
    let mut parts: Vec<FilePart> = Vec::with_capacity(file_paths.len());
    let mut skipped_manifest_missing = 0usize;
    for (idx, (p, is_explicit)) in file_paths.iter().enumerate() {
        let meta = match fs::metadata(p).await {
            Ok(m) => m,
            Err(e) => {
                if *is_explicit {
                    bail!("evidence file not found / unreadable: {} ({e})", p.display());
                }
                eprintln!(
                    "[dispute_upload] WARN: manifest entry missing or unreadable: {} ({e}); \
                     skipping (remaining evidence will still upload)",
                    p.display()
                );
                skipped_manifest_missing += 1;
                continue;
            }
        };
        if meta.len() > MAX_FILE_BYTES {
            let size_mb = meta.len() as f64 / 1_048_576.0;
            if *is_explicit {
                bail!(
                    "evidence file too large: {} ({size_mb:.1} MB, limit 100 MB). \
                     Compress / split before uploading.",
                    p.display()
                );
            }
            eprintln!(
                "[dispute_upload] WARN: skipping oversized manifest entry {} ({size_mb:.1} MB > 100 MB)",
                p.display()
            );
            skipped_manifest_missing += 1;
            continue;
        }
        let bytes = match fs::read(p).await {
            Ok(b) => b,
            Err(e) => {
                if *is_explicit {
                    bail!("failed to read {}: {e}", p.display());
                }
                eprintln!(
                    "[dispute_upload] WARN: failed to read manifest entry {} ({e}); \
                     skipping (remaining evidence will still upload)",
                    p.display()
                );
                skipped_manifest_missing += 1;
                continue;
            }
        };
        let original_name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("evidence");
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        let filename = if original_name.is_ascii() {
            original_name.to_string()
        } else if ext.is_empty() {
            format!("evidence_{idx}")
        } else {
            format!("evidence_{idx}.{ext}")
        };
        let mime = mime_for_ext(&ext);
        eprintln!(
            "[dispute_upload] file part: name=files filename={filename} mime={mime} bytes={}",
            bytes.len(),
        );
        parts.push(FilePart { filename, mime, bytes });
    }

    // Post-loop safety net: if every manifest entry was unreadable and the
    // caller passed neither text nor explicit files, we have nothing to send.
    // Bail BEFORE POSTing an empty body (backend would reject with code=1001).
    if text_clean.is_none() && parts.is_empty() {
        bail!(
            "no evidence to upload: --text is blank, no --file was given, and every \
             manifest entry under ~/.onchainos/deliverables/{role}/{job_id}/ was \
             missing or unreadable ({skipped_manifest_missing} skipped)"
        );
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

    for part in &parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"files\"; filename=\"{}\"\r\n",
                part.filename
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", part.mime).as_bytes());
        body.extend_from_slice(&part.bytes);
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

    println!("✓ Evidence uploaded (off-chain, effective within 1h preparation window)");
    println!("  jobId:    {job_id}");
    println!("  role:     {role}");
    if let Some(t) = text_clean.as_deref() {
        println!("  text:     {} bytes ({} chars)", t.len(), t.chars().count());
    }
    if !explicit_file_paths.is_empty() {
        println!("  --file:   {} explicit attachment(s)", explicit_file_paths.len());
    }
    let manifest_attached = manifest_filenames.len().saturating_sub(skipped_manifest_missing);
    if manifest_attached > 0 {
        println!("  manifest: {manifest_attached} local deliverable(s) auto-attached");
    }
    if skipped_manifest_missing > 0 {
        println!(
            "  skipped:  {skipped_manifest_missing} manifest entry/entries missing or unreadable on disk"
        );
    }
    Ok(())
}

/// Best-effort extension → MIME mapping. Unknown extensions fall back to
/// `application/octet-stream`; the backend stores arbitrary file types and
/// the evaluator probes the content without trusting the declared MIME.
fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
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
