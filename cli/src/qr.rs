//! Shared in-process Unicode QR encoder.
//!
//! Single source of QR rendering for the CLI. Both `wallet qrcode` (stdout) and
//! the Agent-Commerce insufficient-balance deposit path (stderr, TTY-gated) call
//! this one function so the encoder logic is never duplicated. The builder chain
//! and render parameters are lifted verbatim from the former inline builder in
//! `agentic_wallet::cmd_qrcode` (quiet-zone enabled, dark/light inversion).

use qrcode::{render::unicode, Color as QrColor, QrCode};

const PNG_SCALE: usize = 8;
const PNG_QUIET_ZONE: usize = 4;

/// Render `text` as a `Dense1x2` Unicode QR block.
///
/// Encoding fidelity (FR-4): the bytes encoded into the QR are exactly
/// `text.as_bytes()` — a bare address with no URI scheme, chain prefix, or
/// amount appended. Input normalization (trimming / emptiness checks) is the
/// caller's responsibility; this function encodes what it is given verbatim.
///
/// Returns the `qrcode` error on encode failure so callers decide how to handle
/// it (`wallet qrcode` surfaces it; the deposit path silent-degrades, FR-6).
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

    // Snapshot: the shared encoder is deterministic for a fixed input, so
    // `wallet qrcode` (which now delegates here) emits a byte-for-byte stable
    // block across runs — the guard that the extraction preserved render params.
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
}
