use anyhow::{bail, Result};
use clap::{Args, ValueEnum};
use serde::Serialize;
use std::fs;
use std::io::BufRead;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, ValueEnum)]
pub enum FundingNoticeFormat {
    Json,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum FundingNoticeReason {
    #[value(name = "task-payment")]
    TaskPayment,
    #[value(name = "payment-402", alias = "payment402")]
    Payment402,
    #[value(name = "dispute-bond")]
    DisputeBond,
    #[value(name = "subscription")]
    Subscription,
}

#[derive(Args, Debug)]
pub struct FundingNoticeArgs {
    #[arg(long)]
    pub chain: String,
    #[arg(long)]
    pub currency: String,
    #[arg(long)]
    pub shortfall: String,
    #[arg(long = "deposit-address")]
    pub deposit_address: String,
    #[arg(long)]
    pub required: Option<String>,
    #[arg(long)]
    pub available: Option<String>,
    #[arg(long = "deposit-chain")]
    pub deposit_chain: Option<String>,
    #[arg(long, value_enum, default_value_t = FundingNoticeReason::TaskPayment)]
    pub reason: FundingNoticeReason,
    #[arg(long, value_enum, default_value_t = FundingNoticeFormat::Json)]
    pub format: FundingNoticeFormat,
    #[arg(long = "notify-user")]
    pub notify_user: bool,
    /// Already-localized content to send with --notify-user.
    #[arg(long)]
    pub content: Option<String>,
    #[arg(long = "image-dir", hide = true)]
    pub image_dir: Option<PathBuf>,
}

struct FundingNoticeInput {
    chain: String,
    currency: String,
    shortfall: String,
    deposit_address: String,
    required: Option<String>,
    available: Option<String>,
    deposit_chain: String,
    reason: FundingNoticeReason,
    image_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FundingNoticeOutput {
    content_canonical: String,
    fallback_content_canonical: String,
    image_path: Option<String>,
    terminal_qr: Option<String>,
    display_mode: String,
    deposit_address: String,
    chain: String,
    deposit_chain: String,
    currency: String,
    shortfall: String,
    required: Option<String>,
    available: Option<String>,
    reason: String,
    must_localize: bool,
    must_notify_with_image_path: bool,
    must_run_notify_command: bool,
    must_repeat_in_final_response: bool,
    forbid_funding_summary: bool,
    display_policy: String,
    end_turn: bool,
    notify_command: Option<String>,
    notify_command_args: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FundingDisplayMode {
    TerminalUnicode,
    ImageNotify,
}

impl FundingDisplayMode {
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

pub fn funding_display_mode() -> &'static str {
    detect_funding_display_mode().as_str()
}

pub fn funding_notice_command(warning: &serde_json::Value, reason: &str) -> Option<String> {
    let chain = warning["chain"].as_str().unwrap_or("XLayer");
    let currency = warning["currency"].as_str()?;
    let shortfall = warning["shortfall"].as_str()?;
    let deposit_address = warning["depositAddress"].as_str()?;
    let mut parts = vec![
        "onchainos".to_string(),
        "agent".to_string(),
        "funding-notice".to_string(),
        "--chain".to_string(),
        chain.to_string(),
        "--currency".to_string(),
        currency.to_string(),
        "--shortfall".to_string(),
        shortfall.to_string(),
        "--deposit-address".to_string(),
        deposit_address.to_string(),
    ];
    for (flag, key) in [
        ("--available", "available"),
        ("--required", "required"),
        ("--deposit-chain", "depositChain"),
    ] {
        if let Some(value) = warning[key].as_str().filter(|value| !value.is_empty()) {
            parts.push(flag.to_string());
            parts.push(value.to_string());
        }
    }
    parts.extend([
        "--reason".to_string(),
        reason.to_string(),
        "--format".to_string(),
        "json".to_string(),
    ]);
    Some(parts.join(" "))
}

pub fn funding_blocked_envelope(
    warning: &serde_json::Value,
    reason: &str,
    action: &str,
) -> serde_json::Value {
    let funding_notice_command = funding_notice_command(warning, reason);
    let must_run_funding_notice = funding_notice_command.is_some();
    let funding_display_mode = funding_display_mode();
    let image_notify = must_run_funding_notice && funding_display_mode == "image-notify";
    serde_json::json!({
        "blocked": true,
        "blockedReason": "insufficient-balance",
        "submitted": false,
        "balanceWarning": warning.clone(),
        "mustRunFundingNotice": must_run_funding_notice,
        "fundingNoticeCommand": funding_notice_command,
        "fundingDisplayMode": funding_display_mode,
        "mustRunNotifyCommand": image_notify,
        "mustRepeatInFinalResponse": true,
        "forbidFundingSummary": true,
        "finalResponsePolicy": "Final response must repeat the full localized funding notice with all four funding options; never summarize.",
        "platformPolicy": if !must_run_funding_notice {
            "Funding notice unavailable: show balanceWarning, explain deposit address is missing, then end turn."
        } else if image_notify {
            "Non-TTY: run fundingNoticeCommand, then notifyCommandArgs for PNG QR, then full final notice."
        } else {
            "TTY: run fundingNoticeCommand, show terminalQr and full notice; do not claim PNG was sent."
        },
        "resumeAction": "After the user says topped up, rerun the saved original command.",
        "guidance": if must_run_funding_notice {
            format!("{action} was blocked by insufficient balance. Save the command. Run fundingNoticeCommand, then follow its displayMode. End turn.")
        } else {
            format!("{action} was blocked by insufficient balance. Save the command. Show balanceWarning and missing deposit address. End turn.")
        },
    })
}

fn detect_funding_display_mode() -> FundingDisplayMode {
    if let Some(mode) = display_mode_from_codex_session() {
        return mode;
    }
    display_mode_from_tty()
}

fn display_mode_from_tty() -> FundingDisplayMode {
    if std::io::stdout().is_terminal() || std::io::stderr().is_terminal() {
        FundingDisplayMode::TerminalUnicode
    } else {
        FundingDisplayMode::ImageNotify
    }
}

fn display_mode_from_codex_session() -> Option<FundingDisplayMode> {
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

fn display_mode_from_codex_session_files(paths: Vec<PathBuf>) -> Option<FundingDisplayMode> {
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

fn find_codex_session_files(root: &std::path::Path, thread_id: &str) -> Vec<PathBuf> {
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

fn read_first_line(path: &std::path::Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    Some(line)
}

fn display_mode_from_codex_session_line(line: &str) -> Option<FundingDisplayMode> {
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
) -> Option<FundingDisplayMode> {
    let originator = originator.map(normalize_codex_meta_value);
    let source = source.map(normalize_codex_meta_value);
    match originator.as_deref() {
        Some("codex-tui") | Some("codex_exec") => Some(FundingDisplayMode::TerminalUnicode),
        Some("codex desktop") => Some(FundingDisplayMode::ImageNotify),
        _ => match source.as_deref() {
            Some("cli") | Some("exec") => Some(FundingDisplayMode::TerminalUnicode),
            Some("vscode") | Some("appserver") => Some(FundingDisplayMode::ImageNotify),
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

pub fn execute(args: FundingNoticeArgs) -> Result<()> {
    let format = args.format.clone();
    let notify_user = args.notify_user;
    let content = args.content.clone();
    let input = FundingNoticeInput::try_from(args)?;
    let (notice, image_path) = build_funding_notice(input)?;

    if notify_user {
        let content = content.as_deref().expect("checked by input conversion");
        let image_path = image_path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("--notify-user requires an image-notify display path")
        })?;
        super::okx_a2a::user_notify(content, Some(image_path), true)?;
        return Ok(());
    }

    match format {
        FundingNoticeFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true,
                    "data": notice,
                }))?
            );
        }
    }
    Ok(())
}

fn build_funding_notice(
    input: FundingNoticeInput,
) -> Result<(FundingNoticeOutput, Option<PathBuf>)> {
    build_funding_notice_with_mode(input, detect_funding_display_mode())
}

fn build_funding_notice_with_mode(
    input: FundingNoticeInput,
    display_mode: FundingDisplayMode,
) -> Result<(FundingNoticeOutput, Option<PathBuf>)> {
    let image_path = if display_mode.is_image_notify() || input.image_dir.is_some() {
        Some(write_qr_png(
            &input.deposit_address,
            input.image_dir.as_deref(),
        )?)
    } else {
        None
    };
    let terminal_qr = if display_mode == FundingDisplayMode::TerminalUnicode {
        Some(
            crate::qr::render_address_qr_unicode(&input.deposit_address).map_err(|e| {
                anyhow::anyhow!("Failed to encode QR for {}: {}", input.deposit_address, e)
            })?,
        )
    } else {
        None
    };
    let content_canonical = render_content(&input);
    let fallback_content_canonical = render_fallback_content(&input);
    let notify_command_args = image_path.as_ref().map(|path| {
        vec![
            "onchainos".to_string(),
            "agent".to_string(),
            "user-notify".to_string(),
            "--content".to_string(),
            "<localized contentCanonical>".to_string(),
            "--image-path".to_string(),
            path.display().to_string(),
        ]
    });
    let notify_command = image_path.as_ref().map(|path| {
        format!(
            "onchainos agent user-notify --content \"$ONCHAINOS_FUNDING_NOTICE_CONTENT\" --image-path {}",
            shell_quote(&path.display().to_string())
        )
    });
    let image_notify = display_mode.is_image_notify();

    let notice = FundingNoticeOutput {
        content_canonical,
        fallback_content_canonical,
        image_path: image_path.as_ref().map(|path| path.display().to_string()),
        terminal_qr,
        display_mode: display_mode.as_str().to_string(),
        deposit_address: input.deposit_address,
        chain: input.chain,
        deposit_chain: input.deposit_chain,
        currency: input.currency,
        shortfall: input.shortfall,
        required: input.required,
        available: input.available,
        reason: reason_name(&input.reason).to_string(),
        must_localize: true,
        must_notify_with_image_path: image_notify,
        must_run_notify_command: image_notify,
        must_repeat_in_final_response: true,
        forbid_funding_summary: true,
        display_policy: if image_notify {
            "Non-TTY: run notifyCommandArgs for PNG QR, then repeat the full localized notice in final; never summarize."
        } else {
            "TTY: show terminalQr and the full localized notice; never summarize or claim PNG was sent."
        }
        .to_string(),
        end_turn: true,
        notify_command,
        notify_command_args,
    };
    Ok((notice, image_path))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl TryFrom<FundingNoticeArgs> for FundingNoticeInput {
    type Error = anyhow::Error;

    fn try_from(args: FundingNoticeArgs) -> Result<Self> {
        let chain = required_arg("--chain", args.chain)?;
        let currency = required_arg("--currency", args.currency)?;
        let shortfall = required_arg("--shortfall", args.shortfall)?;
        let deposit_address = required_arg("--deposit-address", args.deposit_address)?;
        if args.notify_user
            && args
                .content
                .as_ref()
                .map_or(true, |value| value.trim().is_empty())
        {
            bail!("--notify-user requires --content with already-localized text");
        }
        Ok(Self {
            deposit_chain: args
                .deposit_chain
                .map(|value| required_arg("--deposit-chain", value))
                .transpose()?
                .unwrap_or_else(|| chain.clone()),
            chain,
            currency,
            shortfall,
            deposit_address,
            required: args.required,
            available: args.available,
            reason: args.reason,
            image_dir: args.image_dir,
        })
    }
}

fn required_arg(name: &str, value: String) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{name} must not be empty");
    }
    Ok(trimmed.to_string())
}

fn render_content(input: &FundingNoticeInput) -> String {
    let mut lines = vec![format!(
        "Insufficient {} balance on {}: shortfall {} {}.",
        input.currency, input.chain, input.shortfall, input.currency
    )];
    if let Some(available) = &input.available {
        lines.push(format!("Available: {available} {}.", input.currency));
    }
    if let Some(required) = &input.required {
        lines.push(format!("Required: {required} {}.", input.currency));
    }
    lines.extend([
        String::new(),
        format!("Deposit address: {}", input.deposit_address),
        format!("Deposit network: {}", input.deposit_chain),
        String::new(),
        "Funding options:".to_string(),
        format!(
            "1. Scan and deposit — send {} directly to the address above on {}.",
            input.currency, input.deposit_chain
        ),
        format!(
            "2. Swap — swap <token> to {} {} on {}.",
            input.shortfall, input.currency, input.chain
        ),
        format!(
            "3. Bridge — bridge {} {} from <chain> to {}.",
            input.shortfall, input.currency, input.chain
        ),
        format!(
            "4. Withdraw from OKX — withdraw {} to the address above using the {} network.",
            input.currency, input.deposit_chain
        ),
        String::new(),
        gas_line(input),
        String::new(),
        "After topping up, tell me \"I topped up\".".to_string(),
    ]);
    lines.join("\n")
}

fn render_fallback_content(input: &FundingNoticeInput) -> String {
    format!(
        "QR image could not be attached. Deposit {} to {} on {}. After topping up, tell me \"I topped up\".",
        input.currency, input.deposit_address, input.deposit_chain
    )
}

fn gas_line(input: &FundingNoticeInput) -> String {
    if is_x_layer(&input.chain) || is_x_layer(&input.deposit_chain) {
        "Gas is paid by the platform; no OKB or other native token is required.".to_string()
    } else {
        "Ensure the wallet meets the network gas requirements.".to_string()
    }
}

/// Chain labels come from multiple surfaces (`XLayer`, `X Layer`, `x-layer`).
/// Normalize separators before applying the X Layer gas-sponsorship rule.
fn is_x_layer(chain: &str) -> bool {
    chain
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .eq("xlayer".chars())
}

fn reason_name(reason: &FundingNoticeReason) -> &'static str {
    match reason {
        FundingNoticeReason::TaskPayment => "task-payment",
        FundingNoticeReason::Payment402 => "payment-402",
        FundingNoticeReason::DisputeBond => "dispute-bond",
        FundingNoticeReason::Subscription => "subscription",
    }
}

fn write_qr_png(address: &str, image_dir: Option<&std::path::Path>) -> Result<PathBuf> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let filename = format!("onchainos-funding-qr-{}-{ts}.png", std::process::id());
    let png = crate::qr::render_address_qr_png(address)
        .map_err(|e| anyhow::anyhow!("Failed to encode QR for {}: {}", address, e))?;
    let mut last_error = None;
    let mut dirs = Vec::new();
    if let Some(dir) = image_dir {
        dirs.push(dir.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("ONCHAINOS_FUNDING_IMAGE_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join(".onchainos").join("tmp"));
    }
    dirs.extend([std::env::temp_dir(), PathBuf::from("/tmp")]);
    for dir in dirs {
        if let Err(err) = std::fs::create_dir_all(&dir) {
            last_error = Some((dir.join(&filename), err));
            continue;
        }
        let path = dir.join(&filename);
        match std::fs::write(&path, &png) {
            Ok(()) => return Ok(path),
            Err(err) => last_error = Some((path, err)),
        }
    }
    let (path, err) = last_error.expect("at least one candidate path");
    Err(anyhow::anyhow!(
        "failed to write QR PNG {}: {}",
        path.display(),
        err
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_input() -> FundingNoticeInput {
        FundingNoticeInput {
            chain: "XLayer".to_string(),
            currency: "USDT".to_string(),
            shortfall: "0.01".to_string(),
            deposit_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
            required: Some("0.01".to_string()),
            available: Some("0".to_string()),
            deposit_chain: "XLayer".to_string(),
            reason: FundingNoticeReason::TaskPayment,
            image_dir: None,
        }
    }

    #[test]
    fn x_layer_gas_sponsorship_accepts_display_name_variants() {
        for chain in ["XLayer", "X Layer", "x-layer", "x_layer"] {
            let mut input = test_input();
            input.chain = chain.to_string();
            input.deposit_chain = chain.to_string();
            assert_eq!(
                gas_line(&input),
                "Gas is paid by the platform; no OKB or other native token is required."
            );
        }
    }

    #[test]
    fn non_x_layer_keeps_generic_gas_requirement() {
        let mut input = test_input();
        input.chain = "Ethereum".to_string();
        input.deposit_chain = "Ethereum".to_string();
        assert_eq!(
            gas_line(&input),
            "Ensure the wallet meets the network gas requirements."
        );
    }

    #[test]
    fn terminal_mode_returns_unicode_qr_without_notify_command() {
        let (notice, image_path) =
            build_funding_notice_with_mode(test_input(), FundingDisplayMode::TerminalUnicode)
                .expect("funding notice");

        assert!(image_path.is_none());
        assert_eq!(notice.display_mode, "terminal-unicode");
        assert!(notice.image_path.is_none());
        assert!(
            notice
                .terminal_qr
                .as_deref()
                .unwrap_or_default()
                .contains('█')
        );
        assert!(!notice.must_notify_with_image_path);
        assert!(!notice.must_run_notify_command);
        assert!(notice.notify_command.is_none());
    }

    #[test]
    fn image_mode_requires_notify_and_full_final_notice() {
        let mut input = test_input();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("funding_notice_image");
        input.image_dir = Some(dir);

        let (notice, image_path) =
            build_funding_notice_with_mode(input, FundingDisplayMode::ImageNotify)
                .expect("funding notice");

        assert_eq!(notice.display_mode, "image-notify");
        assert!(image_path.as_ref().is_some_and(|path| path.exists()));
        assert!(notice.terminal_qr.is_none());
        assert!(notice.must_run_notify_command);
        assert!(notice.must_repeat_in_final_response);
        assert!(notice.forbid_funding_summary);
        assert!(
            notice
                .display_policy
                .contains("repeat the full localized notice")
        );
        assert!(
            notice
                .notify_command
                .as_deref()
                .unwrap_or_default()
                .contains("--image-path")
        );
        if let Some(path) = image_path {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn blocked_envelope_carries_requested_reason() {
        let warning = serde_json::json!({
            "sufficient": false,
            "chain": "XLayer",
            "currency": "USDT",
            "shortfall": "0.5",
            "available": "0",
            "required": "0.5",
            "depositAddress": "0x1234567890abcdef1234567890abcdef12345678",
            "depositChain": "XLayer"
        });

        let envelope = funding_blocked_envelope(&warning, "dispute-bond", "Dispute bond");
        assert_eq!(envelope["submitted"], serde_json::json!(false));
        assert!(
            envelope["fundingNoticeCommand"]
                .as_str()
                .unwrap()
                .contains("--reason dispute-bond")
        );
        assert_eq!(
            envelope["mustRepeatInFinalResponse"],
            serde_json::json!(true)
        );
        assert_eq!(envelope["forbidFundingSummary"], serde_json::json!(true));
        assert!(
            envelope["finalResponsePolicy"]
                .as_str()
                .unwrap()
                .contains("all four funding options")
        );
    }

    #[test]
    fn codex_tui_session_meta_uses_terminal_unicode() {
        let line = r#"{"type":"session_meta","payload":{"originator":"codex-tui","source":"cli"}}"#;

        assert_eq!(
            display_mode_from_codex_session_line(line),
            Some(FundingDisplayMode::TerminalUnicode)
        );
    }

    #[test]
    fn codex_desktop_session_meta_uses_image_notify() {
        for source in ["vscode", "appServer"] {
            let line = format!(
                r#"{{"type":"session_meta","payload":{{"originator":"Codex Desktop","source":"{source}"}}}}"#
            );

            assert_eq!(
                display_mode_from_codex_session_line(&line),
                Some(FundingDisplayMode::ImageNotify)
            );
        }
    }

    #[test]
    fn codex_session_meta_matching_is_case_insensitive() {
        assert_eq!(
            display_mode_from_codex_meta(Some(" Codex Desktop "), Some("AppServer")),
            Some(FundingDisplayMode::ImageNotify)
        );
        assert_eq!(
            display_mode_from_codex_meta(Some("CODEX-TUI"), Some("CLI")),
            Some(FundingDisplayMode::TerminalUnicode)
        );
    }

    #[test]
    fn codex_exec_session_meta_uses_terminal_unicode() {
        let line =
            r#"{"type":"session_meta","payload":{"originator":"codex_exec","source":"exec"}}"#;

        assert_eq!(
            display_mode_from_codex_session_line(line),
            Some(FundingDisplayMode::TerminalUnicode)
        );
    }

    #[test]
    fn malformed_codex_session_meta_falls_back() {
        assert_eq!(display_mode_from_codex_session_line("not json"), None);
        assert_eq!(
            display_mode_from_codex_meta(Some("unknown"), Some("unknown")),
            None
        );
    }

    #[test]
    fn invalid_codex_session_candidate_does_not_stop_search() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("funding_notice_sessions");
        std::fs::create_dir_all(&dir).expect("create session test dir");
        let bad = dir.join("bad.jsonl");
        let good = dir.join("good.jsonl");
        std::fs::write(&bad, "not json\n").expect("write bad session");
        std::fs::write(
            &good,
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","source":"cli"}}"#,
        )
        .expect("write good session");

        assert_eq!(
            display_mode_from_codex_session_files(vec![bad.clone(), good.clone()]),
            Some(FundingDisplayMode::TerminalUnicode)
        );
        let _ = std::fs::remove_file(bad);
        let _ = std::fs::remove_file(good);
    }
}
