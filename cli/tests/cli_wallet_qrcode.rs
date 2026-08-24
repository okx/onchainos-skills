mod common;

use common::{assert_error_contains, onchainos};
use std::fs;
use std::path::Path;

#[test]
fn wallet_qrcode_writes_png_output() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("wallet_qrcode_png");
    fs::create_dir_all(&dir).expect("create qrcode test dir");
    let path = dir.join(format!("qr-{}.png", std::process::id()));
    let _ = fs::remove_file(&path);

    onchainos()
        .args([
            "wallet",
            "qrcode",
            "--address",
            "0x1234567890abcdef1234567890abcdef12345678",
            "--format",
            "png",
            "--output",
            path.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();

    let bytes = fs::read(&path).expect("read generated png");
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    let _ = fs::remove_file(path);
}

#[test]
fn wallet_qrcode_rejects_output_without_png_format() {
    let output = onchainos()
        .args([
            "wallet",
            "qrcode",
            "--address",
            "0x1234567890abcdef1234567890abcdef12345678",
            "--output",
            "qr.png",
        ])
        .output()
        .expect("run qrcode command");

    assert_error_contains(&output, &["--output requires --format png"]);
}
