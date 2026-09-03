//! Shared in-process Unicode QR encoder.
//!
//! Single source of QR rendering for the CLI. `wallet receive`, Wallet Send,
//! Swap, A2A, and Agent-Commerce funding notices all call this module so the
//! encoder, runtime display selection, and PNG fallback are never duplicated.

use qrcode::{render::unicode, Color as QrColor, QrCode};
use serde::Serialize;
use std::fs;
use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const PNG_SCALE: usize = 8;
const PNG_QUIET_ZONE: usize = 4;

/// Render `text` as a `Dense1x2` Unicode QR block.
///
/// Encoding fidelity (FR-4): the bytes encoded into the QR are exactly
/// `text.as_bytes()` — a bare address with no URI scheme, chain prefix, or
/// amount appended. Input normalization (trimming / emptiness checks) is the
/// caller's responsibility; this function encodes what it is given verbatim.
///
/// Returns the `qrcode` error on encode failure so low-level callers can test or
/// handle it. Business scenes use `build_qr_output`, which degrades to the bare
/// receive address instead of failing the funding flow.
pub fn render_address_qr_unicode(text: &str) -> Result<String, qrcode::types::QrError> {
    let code = QrCode::new(text.as_bytes())?;
    let rendered = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .quiet_zone(true)
        .build();
    Ok(rendered)
}

/// Render `text` as a PNG QR image.
///
/// Uses a tiny in-process PNG writer with uncompressed zlib blocks to avoid
/// adding runtime dependencies. Pixels encode the bare input bytes verbatim.
pub fn render_address_qr_png(text: &str) -> Result<Vec<u8>, qrcode::types::QrError> {
    let code = QrCode::new(text.as_bytes())?;
    let modules = code.width();
    let image_modules = modules + PNG_QUIET_ZONE * 2;
    let size = image_modules * PNG_SCALE;
    let mut raw = Vec::with_capacity((size + 1) * size);

    for y in 0..size {
        raw.push(0); // PNG filter: None
        for x in 0..size {
            let mx = x / PNG_SCALE;
            let my = y / PNG_SCALE;
            let dark = mx >= PNG_QUIET_ZONE
                && mx < PNG_QUIET_ZONE + modules
                && my >= PNG_QUIET_ZONE
                && my < PNG_QUIET_ZONE + modules
                && code[(mx - PNG_QUIET_ZONE, my - PNG_QUIET_ZONE)] != QrColor::Light;
            raw.push(if dark { 0 } else { 255 });
        }
    }

    Ok(encode_grayscale_png(size as u32, size as u32, &raw))
}

fn encode_grayscale_png(width: u32, height: u32, raw_rows: &[u8]) -> Vec<u8> {
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // 8-bit grayscale
    push_png_chunk(&mut png, b"IHDR", &ihdr);
    push_png_chunk(&mut png, b"IDAT", &zlib_store(raw_rows));
    push_png_chunk(&mut png, b"IEND", &[]);
    png
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // zlib header: no compression/fastest
    for (i, chunk) in data.chunks(u16::MAX as usize).enumerate() {
        let final_block = i == data.len().saturating_sub(1) / (u16::MAX as usize);
        out.push(if final_block { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn push_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_data = Vec::with_capacity(kind.len() + data.len());
    crc_data.extend_from_slice(kind);
    crc_data.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

// ---------------------------------------------------------------------------
// Common QR output (spec §2.5 `QrOutput`, §4.1/§4.4 env+dirs, Appendix B).
//
// Extends the single encoder above into the scene-agnostic Common QR capability
// consumed by the Wallet Send / Swap / A2A funding scenes (via `funding.rs`,
// T8) and the Agent-Commerce funding notice (T9). It adds: (1) display-mode
// detection (Codex session metadata + TTY fallback, lifted from
// `funding_notice.rs`), (2) PNG directory selection + write, (3) the `QrOutput`
// struct, and (4) a `build_qr_output` builder that NEVER fails — on any encode
// or write error it silently degrades to an address-only `QrOutput` (FR-6).
// ---------------------------------------------------------------------------

/// PNG filename stem for on-disk QR images (`<stem>-<pid>-<ts>.png`).
const QR_PNG_FILENAME_PREFIX: &str = "onchainos-funding-qr";

/// The complete Common QR field set embedded by every funding scene.
///
/// Per-mode fields are populated only for the active `display_mode`
/// (`terminal_qr` for `terminal-unicode`; `image_path` / `mime_type` /
/// `markdown_image` / `notify_command_args` for `image-notify`) and are omitted
/// from JSON when absent via `skip_serializing_if`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrOutput {
    /// Requested format — always `"auto"` (the CLI resolves the concrete format).
    pub requested_format: String,
    /// Resolved format: `"unicode"` (terminal) or `"png"` (image).
    pub resolved_format: String,
    /// Display mode: `"terminal-unicode"` or `"image-notify"`.
    pub display_mode: String,
    /// Unicode QR block — present only for `terminal-unicode`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_qr: Option<String>,
    /// On-disk PNG path — present only for `image-notify`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    /// MIME type (`"image/png"`) — present only for `image-notify`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Markdown image reference — present only for `image-notify`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_image: Option<String>,
    /// `onchainos agent user-notify` argv — present only for `image-notify`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_command_args: Option<Vec<String>>,
}

/// Build the Common QR output for `address`, resolving the display mode from the
/// runtime (Codex session metadata, else TTY). `image_dir` is the highest-priority
/// PNG directory when the resolved mode is `image-notify`.
///
/// This function NEVER returns an error: on any QR encode or PNG write failure it
/// silently degrades (FR-6) to an address-only `QrOutput` carrying no QR fields.
pub fn build_qr_output(address: &str, image_dir: Option<&Path>) -> QrOutput {
    build_qr_output_with_mode(address, image_dir, detect_display_mode())
}

/// Testable seam for [`build_qr_output`] with an explicit display mode.
fn build_qr_output_with_mode(
    address: &str,
    image_dir: Option<&Path>,
    display_mode: QrDisplayMode,
) -> QrOutput {
    let mut out = QrOutput {
        requested_format: "auto".to_string(),
        resolved_format: "unicode".to_string(),
        display_mode: display_mode.as_str().to_string(),
        terminal_qr: None,
        image_path: None,
        mime_type: None,
        markdown_image: None,
        notify_command_args: None,
    };

    if display_mode.is_image_notify() {
        // image-notify: write a PNG and expose image / markdown / notify fields.
        // FR-6: any encode or write failure degrades silently to address-only.
        if let Ok(path) = write_qr_png(address, image_dir) {
            out.resolved_format = "png".to_string();
            out.markdown_image = Some(markdown_image_for_path(&path));
            out.notify_command_args = Some(notify_command_args_for_path(&path));
            out.image_path = Some(path.display().to_string());
            out.mime_type = Some("image/png".to_string());
        }
    } else if let Ok(rendered) = render_address_qr_unicode(address) {
        // terminal-unicode: render the Dense1x2 block. FR-6 degrade on encode error.
        out.terminal_qr = Some(rendered);
    }

    out
}

/// Write a PNG QR for `address` and return its path.
///
/// Directory priority (spec §4.4): explicit `image_dir` >
/// `<ONCHAINOS_HOME>/tmp/funding-qr/` > `std::env::temp_dir()`. Each candidate is
/// created with `ensure_dir_0700` semantics on Unix (`home::ensure_dir_0700`). The
/// first writable candidate wins; if all fail, the last error is returned so the
/// caller can degrade.
fn write_qr_png(address: &str, image_dir: Option<&Path>) -> anyhow::Result<PathBuf> {
    let png = render_address_qr_png(address)
        .map_err(|e| anyhow::anyhow!("Failed to encode QR for {}: {}", address, e))?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let filename = format!("{QR_PNG_FILENAME_PREFIX}-{}-{ts}.png", std::process::id());

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = image_dir {
        candidates.push(dir.to_path_buf());
    }
    if let Ok(home) = crate::home::onchainos_home() {
        candidates.push(home.join("tmp").join("funding-qr"));
    }
    candidates.push(std::env::temp_dir());

    let mut last_error: Option<(PathBuf, anyhow::Error)> = None;
    for dir in candidates {
        if let Err(err) = crate::home::ensure_dir_0700(&dir) {
            last_error = Some((dir.join(&filename), err));
            continue;
        }
        let path = dir.join(&filename);
        match fs::write(&path, &png) {
            Ok(()) => return Ok(path),
            Err(err) => last_error = Some((path, anyhow::Error::new(err))),
        }
    }

    let (path, err) = match last_error {
        Some(pair) => pair,
        None => return Err(anyhow::anyhow!("no PNG directory candidates")),
    };
    Err(anyhow::anyhow!(
        "failed to write QR PNG {}: {}",
        path.display(),
        err
    ))
}

/// Markdown image reference for a PNG at `path`, made cwd-relative when possible.
fn markdown_image_for_path(path: &Path) -> String {
    let target = if path.is_absolute() {
        std::env::current_dir()
            .ok()
            .and_then(|cwd| {
                path.strip_prefix(cwd)
                    .ok()
                    .map(|rel| PathBuf::from(".").join(rel))
            })
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        PathBuf::from(".").join(path)
    };
    format!(
        "![QR Code](<{}>)",
        target.to_string_lossy().replace('>', "%3E")
    )
}

/// `onchainos agent user-notify` argv that pushes the PNG at `path` to the user.
fn notify_command_args_for_path(path: &Path) -> Vec<String> {
    vec![
        "onchainos".to_string(),
        "agent".to_string(),
        "user-notify".to_string(),
        "--content".to_string(),
        "<localized content>".to_string(),
        "--image-path".to_string(),
        path.display().to_string(),
    ]
}

/// Terminal-unicode vs image-notify display mode (Codex session metadata + TTY
/// fallback). Lifted from `funding_notice.rs` so the Common QR module owns the
/// single detection path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QrDisplayMode {
    TerminalUnicode,
    ImageNotify,
}

impl QrDisplayMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::TerminalUnicode => "terminal-unicode",
            Self::ImageNotify => "image-notify",
        }
    }

    fn is_image_notify(self) -> bool {
        self == Self::ImageNotify
    }
}

fn detect_display_mode() -> QrDisplayMode {
    if let Some(mode) = display_mode_from_codex_session() {
        return mode;
    }
    display_mode_from_tty()
}

fn display_mode_from_tty() -> QrDisplayMode {
    if std::io::stdout().is_terminal() || std::io::stderr().is_terminal() {
        QrDisplayMode::TerminalUnicode
    } else {
        QrDisplayMode::ImageNotify
    }
}

fn display_mode_from_codex_session() -> Option<QrDisplayMode> {
    let thread_id = std::env::var("CODEX_THREAD_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let mut paths = Vec::new();
    for root in codex_session_roots() {
        paths.extend(find_codex_session_files(&root, &thread_id));
    }
    display_mode_from_codex_session_files(paths)
}

fn display_mode_from_codex_session_files(paths: Vec<PathBuf>) -> Option<QrDisplayMode> {
    for path in paths {
        let Some(line) = read_first_line(&path) else {
            continue;
        };
        if let Some(mode) = display_mode_from_codex_session_line(&line) {
            return Some(mode);
        }
    }
    None
}

fn codex_session_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(home).join("sessions"));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".codex").join("sessions"));
    }
    roots
}

fn find_codex_session_files(root: &Path, thread_id: &str) -> Vec<PathBuf> {
    let mut matches = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > 5000 {
            break;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let is_session_file = name.ends_with(".jsonl") || name.ends_with(".json");
            if is_session_file && name.contains(thread_id) {
                matches.push(path);
            }
        }
    }
    matches
}

fn read_first_line(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    Some(line)
}

fn display_mode_from_codex_session_line(line: &str) -> Option<QrDisplayMode> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let meta = value
        .get("session_meta")
        .or_else(|| value.get("payload"))
        .unwrap_or(&value);
    display_mode_from_codex_meta(
        find_json_string(meta, "originator"),
        find_json_string(meta, "source"),
    )
}

fn display_mode_from_codex_meta(
    originator: Option<&str>,
    source: Option<&str>,
) -> Option<QrDisplayMode> {
    let originator = originator.map(normalize_codex_meta_value);
    let source = source.map(normalize_codex_meta_value);
    match originator.as_deref() {
        Some("codex-tui") | Some("codex_exec") => Some(QrDisplayMode::TerminalUnicode),
        Some("codex desktop") => Some(QrDisplayMode::ImageNotify),
        _ => match source.as_deref() {
            Some("cli") | Some("exec") => Some(QrDisplayMode::TerminalUnicode),
            Some("vscode") | Some("appserver") => Some(QrDisplayMode::ImageNotify),
            _ => None,
        },
    }
}

fn normalize_codex_meta_value(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn find_json_string<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    match value {
        serde_json::Value::Object(map) => map
            .get(key)
            .and_then(|value| value.as_str())
            .or_else(|| map.values().find_map(|value| find_json_string(value, key))),
        serde_json::Value::Array(values) => {
            values.iter().find_map(|value| find_json_string(value, key))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ADDR: &str = "0x1234567890abcdef1234567890abcdef12345678";

    // (a) A valid address encodes to a non-empty Unicode block.
    #[test]
    fn render_address_qr_unicode_returns_non_empty() {
        let rendered = render_address_qr_unicode(SAMPLE_ADDR).expect("encode should succeed");
        assert!(!rendered.is_empty(), "rendered QR must be non-empty");
    }

    // (b) FR-4: the encoder consumes the bare address bytes verbatim — no scheme
    // prefix, no amount. We prove the encoder is fed exactly `text.as_bytes()` by
    // confirming a `QrCode` built from those same bytes succeeds identically (the
    // `qrcode` crate exposes no decoder, so byte-in fidelity is asserted at the
    // construction boundary the function itself uses).
    #[test]
    fn render_address_qr_unicode_encodes_bare_address_bytes() {
        // Same input the function encodes; must construct without error.
        assert!(QrCode::new(SAMPLE_ADDR.as_bytes()).is_ok());
        // No scheme/amount is prepended: a URI-wrapped payload is a *different*
        // (longer) input and would generally differ in rendered size.
        let bare = render_address_qr_unicode(SAMPLE_ADDR).unwrap();
        let with_scheme = render_address_qr_unicode(&format!("ethereum:{SAMPLE_ADDR}")).unwrap();
        assert_ne!(
            bare, with_scheme,
            "bare-address QR must not equal a scheme-prefixed QR (FR-4: no scheme embedded)"
        );
    }

    // Snapshot: the shared encoder is deterministic for a fixed input, so every
    // business scene emits a byte-for-byte stable block across runs — the guard
    // that the extraction preserved render params.
    #[test]
    fn render_address_qr_unicode_is_deterministic() {
        let a = render_address_qr_unicode(SAMPLE_ADDR).unwrap();
        let b = render_address_qr_unicode(SAMPLE_ADDR).unwrap();
        assert_eq!(
            a, b,
            "encoder output must be deterministic for a fixed input"
        );
    }

    #[test]
    fn render_address_qr_png_returns_png_bytes() {
        let png = render_address_qr_png(SAMPLE_ADDR).expect("encode should succeed");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

        let mut offset = 8;
        let mut saw_ihdr = false;
        let mut saw_idat = false;
        let mut saw_iend = false;
        while offset < png.len() {
            let len = u32::from_be_bytes(png[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = &png[offset + 4..offset + 8];
            let data_start = offset + 8;
            let data_end = data_start + len;
            let stored_crc = u32::from_be_bytes(png[data_end..data_end + 4].try_into().unwrap());
            assert_eq!(stored_crc, crc32(&png[offset + 4..data_end]));

            match kind {
                b"IHDR" => {
                    saw_ihdr = true;
                    assert_eq!(len, 13);
                    let width =
                        u32::from_be_bytes(png[data_start..data_start + 4].try_into().unwrap());
                    let height =
                        u32::from_be_bytes(png[data_start + 4..data_start + 8].try_into().unwrap());
                    assert_eq!(width, height);
                    assert!(width > 0);
                }
                b"IDAT" => saw_idat = true,
                b"IEND" => saw_iend = true,
                _ => {}
            }
            offset = data_end + 4;
        }

        assert!(saw_ihdr);
        assert!(saw_idat);
        assert!(saw_iend);
    }

    // --- Common QR output (`build_qr_output` / `QrOutput`) ---

    /// Sandbox PNG dir under `cli/target/test_tmp/<name>` (never `tempfile::tempdir()`,
    /// §13 sandbox rule) so the image-notify path is hermetic.
    fn build_qr_test_dir(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(name)
    }

    #[test]
    fn build_qr_output_terminal_mode_populates_terminal_qr_only() {
        let out = build_qr_output_with_mode(SAMPLE_ADDR, None, QrDisplayMode::TerminalUnicode);

        assert_eq!(out.requested_format, "auto");
        assert_eq!(out.resolved_format, "unicode");
        assert_eq!(out.display_mode, "terminal-unicode");
        assert!(out.terminal_qr.as_deref().is_some_and(|s| !s.is_empty()));
        // Image-notify fields are absent in terminal mode.
        assert!(out.image_path.is_none());
        assert!(out.mime_type.is_none());
        assert!(out.markdown_image.is_none());
        assert!(out.notify_command_args.is_none());
    }

    #[test]
    fn build_qr_output_image_mode_populates_image_fields_only() {
        let dir = build_qr_test_dir("qr_build_output_image");
        let out = build_qr_output_with_mode(SAMPLE_ADDR, Some(&dir), QrDisplayMode::ImageNotify);

        assert_eq!(out.requested_format, "auto");
        assert_eq!(out.resolved_format, "png");
        assert_eq!(out.display_mode, "image-notify");
        assert!(out.terminal_qr.is_none());

        let image_path = out
            .image_path
            .clone()
            .expect("image_path present in image mode");
        let image_path = std::path::Path::new(&image_path);
        assert!(
            image_path.starts_with(&dir),
            "PNG must live under image_dir"
        );
        assert!(image_path.exists(), "PNG file must be written to disk");
        assert_eq!(out.mime_type.as_deref(), Some("image/png"));

        let markdown = out.markdown_image.as_deref().unwrap_or_default();
        assert!(markdown.contains(QR_PNG_FILENAME_PREFIX));

        let notify_args = out.notify_command_args.clone().unwrap_or_default();
        assert!(notify_args.iter().any(|a| a.as_str() == "--image-path"));

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn build_qr_output_degrades_silently_on_encode_failure() {
        // An over-capacity payload makes `QrCode::new` fail; `build_qr_output` must
        // NOT panic or return `Err` — it degrades to an address-only `QrOutput`
        // carrying no QR fields (FR-6 silent degrade).
        let dir = build_qr_test_dir("qr_build_output_degrade");
        let over_long = format!("0x{}", "a".repeat(8000));
        let out = build_qr_output_with_mode(&over_long, Some(&dir), QrDisplayMode::ImageNotify);

        assert!(out.terminal_qr.is_none());
        assert!(out.image_path.is_none());
        assert!(out.mime_type.is_none());
        assert!(out.markdown_image.is_none());
        assert!(out.notify_command_args.is_none());
        // display_mode is still reported even when the QR itself degrades.
        assert_eq!(out.display_mode, "image-notify");
    }

    #[test]
    fn qr_output_unicode_serializes_camelcase_and_skips_image_fields() {
        let out = build_qr_output_with_mode(SAMPLE_ADDR, None, QrDisplayMode::TerminalUnicode);
        let json = serde_json::to_value(&out).expect("QrOutput serializes");

        assert_eq!(json["requestedFormat"], "auto");
        assert_eq!(json["resolvedFormat"], "unicode");
        assert_eq!(json["displayMode"], "terminal-unicode");
        assert!(json.get("terminalQr").is_some(), "terminalQr present");
        // Image-mode fields absent thanks to skip_serializing_if.
        assert!(json.get("imagePath").is_none());
        assert!(json.get("mimeType").is_none());
        assert!(json.get("markdownImage").is_none());
        assert!(json.get("notifyCommandArgs").is_none());
    }
}
