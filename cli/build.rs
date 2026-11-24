use std::collections::HashMap;
use std::path::PathBuf;

const DEFAULT_BASE_URL: &str = "https://web3.okx.com";
const DEFAULT_AGENT_IDENTITY_WS_URL: &str = "wss://wsdex.okx.com:8443/ws/v5/private";
const DEFAULT_WS_URL: &str = "wss://wsdex.okx.com/ws/v6/dex";

fn parse_dotenv(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((
                key.trim().to_string(),
                value.trim().trim_matches(['"', '\'']).to_string(),
            ))
        })
        .collect()
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let env_path = manifest_dir.join(".env");
    println!("cargo:rerun-if-changed={}", env_path.display());

    // Only endpoint keys are accepted from cli/.env. In particular, credentials
    // are never copied into the compiled binary and process environment variables
    // cannot override these values at runtime or build time.
    let values = std::fs::read_to_string(&env_path)
        .map(|content| parse_dotenv(&content))
        .unwrap_or_default();
    let configured_base_url = values
        .get("OKX_BASE_URL")
        .filter(|value| !value.is_empty())
        .map(String::as_str);
    let base_url = configured_base_url.unwrap_or(DEFAULT_BASE_URL);
    let agent_identity_ws_url = values
        .get("OKX_AGENTIC_WS_URL")
        .filter(|value| !value.is_empty())
        .map(String::as_str)
        .unwrap_or(DEFAULT_AGENT_IDENTITY_WS_URL);
    let ws_url = values
        .get("ONCHAINOS_WS_URL")
        .filter(|value| !value.is_empty())
        .map(String::as_str)
        .unwrap_or(DEFAULT_WS_URL);

    println!("cargo:rustc-env=ONCHAINOS_COMPILED_BASE_URL={base_url}");
    println!(
        "cargo:rustc-env=ONCHAINOS_COMPILED_BASE_URL_CUSTOM={}",
        u8::from(configured_base_url.is_some())
    );
    println!("cargo:rustc-env=ONCHAINOS_COMPILED_AGENT_IDENTITY_WS_URL={agent_identity_ws_url}");
    println!("cargo:rustc-env=ONCHAINOS_COMPILED_WS_URL={ws_url}");
}
