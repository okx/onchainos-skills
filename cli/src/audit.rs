//! Audit log — append-only JSONL file at `~/.onchainos/audit.jsonl`.
//!
//! Every CLI command and MCP tool call is recorded with:
//! - timestamp, source (cli / mcp), command path, success flag, duration, optional error.
//!
//! The log is automatically rotated when it exceeds `MAX_LINES` (keeps the most recent half).

use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::Duration;

const AUDIT_FILE: &str = "audit.jsonl";
const MAX_LINES: usize = 10_000;
const KEEP_LINES: usize = 5_000;

#[derive(Serialize)]
struct Entry<'a> {
    ts: String,
    source: &'a str,
    command: &'a str,
    ok: bool,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// First line of the log file — written once when the file is created.
#[derive(Serialize)]
struct DeviceHeader {
    #[serde(rename = "type")]
    kind: &'static str,
    os: &'static str,
    arch: &'static str,
    version: &'static str,
}

const DEVICE_HEADER_TYPE: &str = "device";

fn device_header_line() -> String {
    let h = DeviceHeader {
        kind: DEVICE_HEADER_TYPE,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        version: env!("CARGO_PKG_VERSION"),
    };
    serde_json::to_string(&h).unwrap_or_default()
}

/// Returns true if `line` is a device-header line.
/// NOTE: relies on `type` being the first field in `DeviceHeader` (serde serializes
/// struct fields in declaration order). Keep `kind` as the first field in the struct.
fn is_device_header(line: &str) -> bool {
    line.starts_with("{\"type\":\"device\"")
}

/// Append one audit entry. Never panics — failures are silently ignored.
pub fn log(
    source: &str,
    command: &str,
    ok: bool,
    duration: Duration,
    args: Option<Vec<String>>,
    error: Option<&str>,
) {
    let _ = try_log(source, command, ok, duration, args, error);
}

fn try_log(
    source: &str,
    command: &str,
    ok: bool,
    duration: Duration,
    args: Option<Vec<String>>,
    error: Option<&str>,
) -> Option<()> {
    let home = crate::home::onchainos_home().ok()?;
    if !home.exists() {
        fs::create_dir_all(&home).ok()?;
    }
    let path = home.join(AUDIT_FILE);

    // Rotate if needed (best-effort, ignore errors)
    rotate_if_needed(&path);

    // Write device header as first line if the file is new or empty.
    let needs_header = !path.exists() || fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true);

    let entry = Entry {
        ts: {
            let local = chrono::Local::now();
            let offset_secs = local.offset().local_minus_utc();
            let offset_hours = offset_secs as f32 / 3600.0;
            format!(
                "{} {:+.1} {}",
                local.format("%Y-%m-%d"),
                offset_hours,
                local.format("%H:%M:%S%.3f")
            )
        },
        source,
        command,
        ok,
        duration_ms: duration.as_millis() as u64,
        args,
        error: error.map(truncate_error),
    };
    let line = serde_json::to_string(&entry).ok()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    if needs_header {
        writeln!(file, "{}", device_header_line()).ok()?;
    }
    writeln!(file, "{}", line).ok()
}

/// Truncate error messages to avoid bloating the log.
/// Safe for multi-byte UTF-8: never splits in the middle of a character.
fn truncate_error(msg: &str) -> String {
    const MAX_LEN: usize = 512;
    if msg.len() <= MAX_LEN {
        msg.to_string()
    } else {
        // Find the last char boundary at or before MAX_LEN.
        let mut end = MAX_LEN;
        while !msg.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &msg[..end])
    }
}

/// If the file exceeds MAX_LINES, keep the device-header line (first line)
/// plus the most recent KEEP_LINES entry lines.
fn rotate_if_needed(path: &std::path::Path) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let line_count = content.lines().count();
    if line_count <= MAX_LINES {
        return;
    }
    let lines: Vec<&str> = content.lines().collect();

    // Separate the device-header (if present) from entry lines.
    let (header, entries): (Option<&str>, &[&str]) =
        if !lines.is_empty() && is_device_header(lines[0]) {
            (Some(lines[0]), &lines[1..])
        } else {
            (None, &lines[..])
        };

    let keep = &entries[entries.len().saturating_sub(KEEP_LINES)..];
    let mut out = String::new();
    if let Some(h) = header {
        out.push_str(h);
        out.push('\n');
    }
    for line in keep {
        out.push_str(line);
        out.push('\n');
    }
    let _ = fs::write(path, out);
}

// ── Argument redaction ───────────────────────────────────────────────

/// Flags whose next positional value must be fully replaced with `[REDACTED]`.
const REDACT_FULL: &[&str] = &[
    "--otp",
    "--signed-tx",
    "--unsigned-tx",
    "--jito-unsigned-tx",
    "--input-data",
    "--data",
    "--message",
];

/// Flags whose next positional value is an address / email — keep prefix + suffix.
const REDACT_ADDR: &[&str] = &["--from", "--wallet", "--email", "--address"];

/// Subcommand sequences whose next positional argument is sensitive.
/// e.g. `onchainos wallet verify <OTP>` — the OTP is a positional arg, not a flag.
const REDACT_POSITIONAL: &[&[&str]] = &[&["wallet", "verify"]];

/// Redact sensitive values from a CLI argv list.
///
/// Handles:
/// - `--flag value` and `--flag=value` forms (flag-based redaction)
/// - Positional args after known subcommand sequences (e.g. `wallet verify <otp>`)
pub fn redact_args(raw: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(raw.len());
    let mut redact_next: Option<RedactKind> = None;

    // Check if any REDACT_POSITIONAL pattern matches and find the index of the
    // positional arg to redact (the arg right after the last subcommand word).
    let positional_redact_indices = positional_indices_to_redact(raw);

    for (i, arg) in raw.iter().enumerate() {
        if let Some(kind) = redact_next.take() {
            out.push(apply_redact(arg, kind));
            continue;
        }

        // Positional arg redaction (e.g. `wallet verify <otp>`)
        if positional_redact_indices.contains(&i) {
            out.push("[REDACTED]".to_string());
            continue;
        }

        // --flag=value form
        if let Some((flag, value)) = arg.split_once('=') {
            if let Some(kind) = classify_flag(flag) {
                out.push(format!("{}={}", flag, apply_redact(value, kind)));
                continue;
            }
        }

        // --flag value form (value is the next arg)
        if let Some(kind) = classify_flag(arg) {
            redact_next = Some(kind);
            out.push(arg.clone());
            continue;
        }

        out.push(arg.clone());
    }
    out
}

/// Find indices of positional args that should be redacted based on subcommand patterns.
fn positional_indices_to_redact(raw: &[String]) -> Vec<usize> {
    let lower: Vec<String> = raw.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut indices = Vec::new();
    for pattern in REDACT_POSITIONAL {
        // Find the last original index of the matched subcommand sequence.
        if let Some(last_pattern_idx) = find_subcommand_sequence_last_idx(&lower, pattern) {
            // The positional arg to redact is the next non-flag arg after the pattern.
            let redact_idx = last_pattern_idx + 1;
            if redact_idx < raw.len() && !raw[redact_idx].starts_with('-') {
                indices.push(redact_idx);
            }
        }
    }
    indices
}

/// Find a subcommand sequence in args (skipping flags) and return the original index
/// of the **last** matched element. This ensures the redact target is always the arg
/// immediately following the last subcommand word, even when flags appear in between.
fn find_subcommand_sequence_last_idx(args: &[String], pattern: &[&str]) -> Option<usize> {
    // Collect only the non-flag args with their original indices.
    let subcmds: Vec<(usize, &str)> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.starts_with('-'))
        .map(|(i, a)| (i, a.as_str()))
        .collect();

    // Slide a window over the subcommand positions.
    for window in subcmds.windows(pattern.len()) {
        if window.iter().zip(pattern.iter()).all(|((_, a), p)| a == p) {
            return Some(window[pattern.len() - 1].0);
        }
    }
    None
}

#[derive(Clone, Copy)]
enum RedactKind {
    Full,
    Addr,
}

fn classify_flag(flag: &str) -> Option<RedactKind> {
    if REDACT_FULL.iter().any(|f| f.eq_ignore_ascii_case(flag)) {
        return Some(RedactKind::Full);
    }
    if REDACT_ADDR.iter().any(|f| f.eq_ignore_ascii_case(flag)) {
        return Some(RedactKind::Addr);
    }
    None
}

fn apply_redact(value: &str, kind: RedactKind) -> String {
    match kind {
        RedactKind::Full => "[REDACTED]".to_string(),
        RedactKind::Addr => mask_addr(value),
    }
}

/// Keep first 6 and last 4 characters, mask the rest with "***".
/// For short values (≤ 10 chars), fully redact.
fn mask_addr(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 10 {
        return "[REDACTED]".to_string();
    }
    let prefix: String = chars[..6].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{}***{}", prefix, suffix)
}

// ── CLI command name resolution ──────────────────────────────────────

/// Extract a human-readable command path from the parsed CLI command.
pub fn cli_command_name(cmd: &crate::Commands) -> String {
    use crate::Commands;
    match cmd {
        Commands::Market { command } => format!("market {}", market_sub(command)),
        Commands::Signal { command } => format!("signal {}", signal_sub(command)),
        Commands::Memepump { command } => format!("memepump {}", memepump_sub(command)),
        Commands::Token { command } => format!("token {}", token_sub(command)),
        Commands::Swap { command } => format!("swap {}", swap_sub(command)),
        Commands::Gateway { command } => format!("gateway {}", gateway_sub(command)),
        Commands::Portfolio { command } => format!("portfolio {}", portfolio_sub(command)),
        Commands::Mcp { .. } => "mcp".to_string(),
        Commands::Wallet { command } => format!("wallet {}", wallet_sub(command)),
        Commands::Security { command } => format!("security {}", security_sub(command)),
        Commands::Leaderboard { command } => format!("leaderboard {}", leaderboard_sub(command)),
        Commands::Tracker { command } => format!("tracker {}", tracker_sub(command)),
        Commands::Payment { command } => format!("payment {}", payment_sub(command)),
        Commands::Defi { command } => format!("defi {}", defi_sub(command)),
        Commands::Upgrade(_) => "upgrade".to_string(),
    }
}

use crate::commands::agentic_wallet::payment::PaymentCommand;
use crate::commands::agentic_wallet::wallet::WalletCommand;
use crate::commands::{
    defi::DefiCommand, gateway::GatewayCommand, leaderboard::LeaderboardCommand,
    market::MarketCommand, memepump::MemepumpCommand, portfolio::PortfolioCommand,
    security::SecurityCommand, signal::SignalCommand, swap::SwapCommand, token::TokenCommand,
    tracker::TrackerCommand,
};

fn market_sub(c: &MarketCommand) -> &'static str {
    match c {
        MarketCommand::Price { .. } => "price",
        MarketCommand::Prices { .. } => "prices",
        MarketCommand::Kline { .. } => "kline",
        MarketCommand::Index { .. } => "index",
        MarketCommand::PortfolioSupportedChains => "portfolio-supported-chains",
        MarketCommand::PortfolioOverview { .. } => "portfolio-overview",
        MarketCommand::PortfolioDexHistory { .. } => "portfolio-dex-history",
        MarketCommand::PortfolioRecentPnl { .. } => "portfolio-recent-pnl",
        MarketCommand::PortfolioTokenPnl { .. } => "portfolio-token-pnl",
    }
}

fn signal_sub(c: &SignalCommand) -> &'static str {
    match c {
        SignalCommand::Chains => "chains",
        SignalCommand::List { .. } => "list",
    }
}

fn tracker_sub(c: &TrackerCommand) -> &'static str {
    match c {
        TrackerCommand::Activities { .. } => "activities",
    }
}

fn leaderboard_sub(c: &LeaderboardCommand) -> &'static str {
    match c {
        LeaderboardCommand::SupportedChains => "supported-chains",
        LeaderboardCommand::List { .. } => "list",
    }
}

fn memepump_sub(c: &MemepumpCommand) -> &'static str {
    match c {
        MemepumpCommand::Chains => "chains",
        MemepumpCommand::Tokens { .. } => "tokens",
        MemepumpCommand::TokenDetails { .. } => "token-details",
        MemepumpCommand::TokenDevInfo { .. } => "token-dev-info",
        MemepumpCommand::SimilarTokens { .. } => "similar-tokens",
        MemepumpCommand::TokenBundleInfo { .. } => "token-bundle-info",
        MemepumpCommand::ApedWallet { .. } => "aped-wallet",
    }
}

fn token_sub(c: &TokenCommand) -> &'static str {
    match c {
        TokenCommand::Search { .. } => "search",
        TokenCommand::Info { .. } => "info",
        TokenCommand::Holders { .. } => "holders",
        TokenCommand::PriceInfo { .. } => "price-info",
        TokenCommand::Liquidity { .. } => "liquidity",
        TokenCommand::HotTokens { .. } => "hot-tokens",
        TokenCommand::AdvancedInfo { .. } => "advanced-info",
        TokenCommand::TopTrader { .. } => "top-trader",
        TokenCommand::Trades { .. } => "trades",
        TokenCommand::ClusterOverview { .. } => "cluster-overview",
        TokenCommand::ClusterTopHolders { .. } => "cluster-top-holders",
        TokenCommand::ClusterList { .. } => "cluster-list",
        TokenCommand::ClusterSupportedChains => "cluster-supported-chains",
    }
}

fn swap_sub(c: &SwapCommand) -> &'static str {
    match c {
        SwapCommand::Quote { .. } => "quote",
        SwapCommand::Swap { .. } => "swap",
        SwapCommand::Approve { .. } => "approve",
        SwapCommand::CheckApprovals { .. } => "check-approvals",
        SwapCommand::Chains => "chains",
        SwapCommand::Liquidity { .. } => "liquidity",
        SwapCommand::Execute { .. } => "execute",
    }
}

fn gateway_sub(c: &GatewayCommand) -> &'static str {
    match c {
        GatewayCommand::Gas { .. } => "gas",
        GatewayCommand::GasLimit { .. } => "gas-limit",
        GatewayCommand::Simulate { .. } => "simulate",
        GatewayCommand::Broadcast { .. } => "broadcast",
        GatewayCommand::Orders { .. } => "orders",
        GatewayCommand::Chains => "chains",
    }
}

fn portfolio_sub(c: &PortfolioCommand) -> &'static str {
    match c {
        PortfolioCommand::Chains => "chains",
        PortfolioCommand::TotalValue { .. } => "total-value",
        PortfolioCommand::AllBalances { .. } => "all-balances",
        PortfolioCommand::TokenBalances { .. } => "token-balances",
    }
}

fn wallet_sub(c: &WalletCommand) -> &'static str {
    match c {
        WalletCommand::Login { .. } => "login",
        WalletCommand::Verify { .. } => "verify",
        WalletCommand::Add => "add",
        WalletCommand::Switch { .. } => "switch",
        WalletCommand::Status => "status",
        WalletCommand::Addresses { .. } => "addresses",
        WalletCommand::Logout => "logout",
        WalletCommand::Chains => "chains",
        WalletCommand::Balance { .. } => "balance",
        WalletCommand::Send { .. } => "send",
        WalletCommand::History { .. } => "history",
        WalletCommand::ContractCall { .. } => "contract-call",
        WalletCommand::SignMessage { .. } => "sign-message",
    }
}

fn security_sub(c: &SecurityCommand) -> &'static str {
    match c {
        SecurityCommand::TokenScan { .. } => "token-scan",
        SecurityCommand::DappScan { .. } => "dapp-scan",
        SecurityCommand::TxScan { .. } => "tx-scan",
        SecurityCommand::Approvals { .. } => "approvals",
        SecurityCommand::SigScan { .. } => "sig-scan",
    }
}

fn payment_sub(c: &PaymentCommand) -> &'static str {
    match c {
        PaymentCommand::X402Pay { .. } => "x402-pay",
    }
}

fn defi_sub(c: &DefiCommand) -> &'static str {
    match c {
        DefiCommand::SupportChains => "support-chains",
        DefiCommand::SupportPlatforms => "support-platforms",
        DefiCommand::List { .. } => "list",
        DefiCommand::Search { .. } => "search",
        DefiCommand::Detail { .. } => "detail",
        DefiCommand::Prepare { .. } => "prepare",
        DefiCommand::Deposit { .. } => "deposit",
        DefiCommand::Redeem { .. } => "redeem",
        DefiCommand::Claim { .. } => "claim",
        DefiCommand::CalculateEntry { .. } => "calculate-entry",
        DefiCommand::Invest { .. } => "invest",
        DefiCommand::Withdraw { .. } => "withdraw",
        DefiCommand::Collect { .. } => "collect",
        DefiCommand::Positions { .. } => "positions",
        DefiCommand::PositionDetail { .. } => "position-detail",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn with_temp_dir<F: FnOnce(&Path)>(name: &str, f: F) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        f(&dir);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn truncate_error_short_message_unchanged() {
        let msg = "short error";
        assert_eq!(truncate_error(msg), msg);
    }

    #[test]
    fn truncate_error_long_message_truncated() {
        let msg = "x".repeat(1000);
        let result = truncate_error(&msg);
        assert!(result.len() < 600);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn rotate_if_needed_does_nothing_under_limit() {
        with_temp_dir("rotate_noop", |dir| {
            let path = dir.join("test.jsonl");
            let header = device_header_line();
            let mut content = format!("{header}\n");
            for i in 0..100 {
                content.push_str(&format!("line {i}\n"));
            }
            fs::write(&path, &content).unwrap();
            rotate_if_needed(&path);
            let after = fs::read_to_string(&path).unwrap();
            assert_eq!(after.lines().count(), 101); // header + 100
        });
    }

    #[test]
    fn rotate_if_needed_truncates_over_limit() {
        with_temp_dir("rotate_truncate", |dir| {
            let path = dir.join("test.jsonl");
            let header = device_header_line();
            let mut content = format!("{header}\n");
            for i in 0..MAX_LINES + 500 {
                content.push_str(&format!("line {i}\n"));
            }
            fs::write(&path, &content).unwrap();
            rotate_if_needed(&path);
            let after = fs::read_to_string(&path).unwrap();
            // header + KEEP_LINES entries
            assert_eq!(after.lines().count(), KEEP_LINES + 1);
            // First line is still the device header
            assert!(is_device_header(after.lines().next().unwrap()));
            // Should keep the LAST entry lines
            assert!(after.contains(&format!("line {}", MAX_LINES + 499)));
        });
    }

    #[test]
    fn rotate_without_header_still_works() {
        with_temp_dir("rotate_no_header", |dir| {
            let path = dir.join("test.jsonl");
            let content = (0..MAX_LINES + 100)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            fs::write(&path, &content).unwrap();
            rotate_if_needed(&path);
            let after = fs::read_to_string(&path).unwrap();
            assert_eq!(after.lines().count(), KEEP_LINES);
            assert!(after.contains(&format!("line {}", MAX_LINES + 99)));
        });
    }

    #[test]
    fn entry_serializes_without_error_when_none() {
        let entry = Entry {
            ts: "2026-03-17 +0.0 00:00:00.000".to_string(),
            source: "cli",
            command: "wallet login",
            ok: true,
            duration_ms: 42,
            args: None,
            error: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("error"));
        assert!(!json.contains("args"));
        assert!(json.contains("\"ok\":true"));
    }

    #[test]
    fn entry_serializes_with_args_and_error() {
        let entry = Entry {
            ts: "2026-03-17 +0.0 00:00:00.000".to_string(),
            source: "cli",
            command: "wallet login",
            ok: false,
            duration_ms: 100,
            args: Some(vec![
                "onchainos".to_string(),
                "wallet".to_string(),
                "login".to_string(),
                "--otp".to_string(),
                "[REDACTED]".to_string(),
            ]),
            error: Some("not found".to_string()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"error\":\"not found\""));
        assert!(json.contains("\"args\""));
        assert!(json.contains("[REDACTED]"));
    }

    #[test]
    fn entry_serializes_mcp_without_args() {
        let entry = Entry {
            ts: "2026-03-17 +0.0 00:00:00.000".to_string(),
            source: "mcp",
            command: "token_search",
            ok: false,
            duration_ms: 100,
            args: None,
            error: Some("not found".to_string()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"error\":\"not found\""));
        assert!(json.contains("\"source\":\"mcp\""));
        assert!(!json.contains("\"args\""));
    }

    #[test]
    fn device_header_line_contains_expected_fields() {
        let header = device_header_line();
        assert!(header.contains("\"type\":\"device\""));
        assert!(header.contains("\"os\":"));
        assert!(header.contains("\"arch\":"));
        assert!(header.contains("\"version\":"));
        assert!(is_device_header(&header));
    }

    // ── redact_args tests ────────────────────────────────────────────

    #[test]
    fn redact_otp_two_arg_form() {
        let args = vec_s(&["onchainos", "wallet", "login", "--otp", "123456"]);
        let out = redact_args(&args);
        assert_eq!(
            out,
            vec_s(&["onchainos", "wallet", "login", "--otp", "[REDACTED]"])
        );
    }

    #[test]
    fn redact_otp_equals_form() {
        let args = vec_s(&["onchainos", "wallet", "login", "--otp=123456"]);
        let out = redact_args(&args);
        assert_eq!(
            out,
            vec_s(&["onchainos", "wallet", "login", "--otp=[REDACTED]"])
        );
    }

    #[test]
    fn redact_signed_tx() {
        let args = vec_s(&[
            "onchainos",
            "gateway",
            "broadcast",
            "--chain",
            "ethereum",
            "--signed-tx",
            "0xdeadbeef1234",
        ]);
        let out = redact_args(&args);
        assert_eq!(out[5], "--signed-tx");
        assert_eq!(out[6], "[REDACTED]");
    }

    #[test]
    fn redact_wallet_addr() {
        let args = vec_s(&[
            "onchainos",
            "swap",
            "swap",
            "--wallet",
            "0x1234567890abcdef1234567890abcdef12345678",
        ]);
        let out = redact_args(&args);
        assert_eq!(out[4], "0x1234***5678");
    }

    #[test]
    fn redact_email() {
        let args = vec_s(&[
            "onchainos",
            "wallet",
            "login",
            "--email",
            "alice@example.com",
        ]);
        let out = redact_args(&args);
        assert_eq!(out[4], "alice@***.com");
    }

    #[test]
    fn redact_short_addr_fully() {
        let args = vec_s(&["onchainos", "swap", "swap", "--wallet", "short"]);
        let out = redact_args(&args);
        assert_eq!(out[4], "[REDACTED]");
    }

    #[test]
    fn no_redaction_for_safe_args() {
        let args = vec_s(&["onchainos", "token", "search", "--chain", "ethereum", "ETH"]);
        let out = redact_args(&args);
        assert_eq!(out, args);
    }

    // ── positional redaction tests ──────────────────────────────────

    #[test]
    fn redact_wallet_verify_positional_otp() {
        let args = vec_s(&["onchainos", "wallet", "verify", "123456"]);
        let out = redact_args(&args);
        assert_eq!(out, vec_s(&["onchainos", "wallet", "verify", "[REDACTED]"]));
    }

    #[test]
    fn redact_wallet_verify_with_flag_interleaved() {
        // Flag between subcommands: onchainos wallet --force verify 123456
        let args = vec_s(&["onchainos", "wallet", "--force", "verify", "123456"]);
        let out = redact_args(&args);
        assert_eq!(
            out,
            vec_s(&["onchainos", "wallet", "--force", "verify", "[REDACTED]"])
        );
    }

    #[test]
    fn no_redact_wallet_other_subcommand() {
        let args = vec_s(&["onchainos", "wallet", "status"]);
        let out = redact_args(&args);
        assert_eq!(out, args);
    }

    fn vec_s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }
}
