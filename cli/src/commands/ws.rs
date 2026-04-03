use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use crate::output;
use crate::watch::store::{self, now_ms};
use crate::watch::types::{
    channel_pattern, is_tracker_channel, ChannelPattern, DaemonState, TokenPair, TradeEvent,
    WatchConfig, WatchEnv, ALL_CHANNELS, DEFAULT_CHANNELS,
};

/// Resolve trade type alias to API integer string.
fn resolve_trade_type(s: &str) -> &str {
    match s.to_lowercase().as_str() {
        "all" | "0" => "0",
        "buy" | "1" => "1",
        "sell" | "2" => "2",
        _ => s,
    }
}

/// Parse a human-friendly duration string (e.g. "30m", "1h", "0") into milliseconds.
fn parse_duration_ms(s: &str) -> Result<u64> {
    let s = s.trim();
    if s == "0" {
        return Ok(0);
    }
    let (num, suffix) = if let Some(n) = s.strip_suffix('m') {
        (n, 60_000u64)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3_600_000u64)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1_000u64)
    } else {
        bail!("invalid --idle-timeout '{}'; use e.g. 30m, 1h, 0", s);
    };
    let n: u64 = num
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --idle-timeout '{}'; use e.g. 30m, 1h, 0", s))?;
    Ok(n * suffix)
}

#[derive(Subcommand)]
pub enum WsCommand {
    /// List all supported WebSocket channels
    Channels,

    /// Show detailed info for a specific channel
    ChannelInfo {
        /// Channel name (e.g. price, trades, kol_smartmoney-tracker-activity)
        #[arg(long)]
        channel: String,
    },

    /// Start a background WebSocket session and return its ID
    Start {
        /// Channel(s) to subscribe, e.g. --channel kol_smartmoney-tracker-activity.
        /// Can be specified multiple times. Defaults to all known channels.
        #[arg(long)]
        channel: Vec<String>,
        /// Wallet addresses for address-tracker-activity, comma-separated (max 200).
        #[arg(long)]
        wallet_addresses: Option<String>,
        /// Chain index(es) for per-chain channels, comma-separated (e.g. 1,501,56).
        #[arg(long)]
        chain_index: Option<String>,
        /// Token pair(s) for per-token channels: chainIndex:tokenAddress, comma-separated.
        /// e.g. 1:0xdac17f958d2ee523a2206206994597c13d831ec7
        #[arg(long)]
        token_pair: Option<String>,
        /// Environment: prod (default) or pre
        #[arg(long, default_value = "prod")]
        env: String,
        /// Auto-stop if no poll within this duration (default: 30m, 0 to disable).
        /// Formats: 30m, 1h, 2h, 0
        #[arg(long, default_value = "30m")]
        idle_timeout: String,
    },

    /// Poll incremental events from a running session
    Poll {
        /// Session ID returned by ws start
        #[arg(long)]
        id: String,
        /// Channel to poll. Defaults to the session's first channel.
        #[arg(long)]
        channel: Option<String>,
        /// Maximum number of events to return (default: 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Filter (tracker only): min quoteTokenAmount
        #[arg(long)]
        min_quote_amount: Option<f64>,
        /// Filter (tracker only): min marketCap (USD)
        #[arg(long)]
        min_market_cap: Option<f64>,
        /// Filter (tracker only): min realizedPnlUsd (set 0 for profit-only)
        #[arg(long)]
        min_pnl: Option<f64>,
        /// Filter (tracker only): walletAddress prefix match
        #[arg(long)]
        trader: Option<String>,
        /// Filter (tracker only): smart_money (1) or kol (2)
        #[arg(long)]
        tag: Option<String>,
        /// Filter (tracker only): tradeTime >= this ms timestamp
        #[arg(long)]
        since: Option<u64>,
        /// Filter (tracker only): buy or sell
        #[arg(long)]
        trade_type: Option<String>,
    },

    /// Stop a running session. If --id is omitted, all sessions are stopped.
    Stop {
        /// Session ID to stop. Omit to stop all.
        #[arg(long)]
        id: Option<String>,
        /// Return any unread events before stopping
        #[arg(long)]
        flush: bool,
    },

    /// List all sessions
    List,

    /// Internal: run daemon event loop (not for direct use)
    #[command(hide = true)]
    RunDaemon {
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(cmd: WsCommand) -> Result<()> {
    match cmd {
        WsCommand::Channels => ws_channels(),
        WsCommand::ChannelInfo { channel } => ws_channel_info(&channel),
        WsCommand::Start {
            channel,
            wallet_addresses,
            chain_index,
            token_pair,
            env,
            idle_timeout,
        } => {
            let addrs: Vec<String> = wallet_addresses
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let chain_indexes: Vec<String> = chain_index
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let token_pairs: Vec<TokenPair> = token_pair
                .unwrap_or_default()
                .split(',')
                .filter_map(|s| {
                    let s = s.trim();
                    let (ci, addr) = s.split_once(':')?;
                    if ci.is_empty() || addr.is_empty() {
                        return None;
                    }
                    Some(TokenPair {
                        chain_index: ci.to_string(),
                        token_contract_address: addr.to_string(),
                    })
                })
                .collect();
            let idle_timeout_ms = parse_duration_ms(&idle_timeout)?;
            ws_start(
                channel,
                addrs,
                chain_indexes,
                token_pairs,
                &env,
                idle_timeout_ms,
            )
            .await
        }
        WsCommand::Poll {
            id,
            channel,
            limit,
            min_quote_amount,
            min_market_cap,
            min_pnl,
            trader,
            tag,
            since,
            trade_type,
        } => ws_poll(
            &id,
            channel,
            limit,
            min_quote_amount,
            min_market_cap,
            min_pnl,
            trader,
            tag,
            since,
            trade_type,
        ),
        WsCommand::Stop { id, flush } => match id {
            Some(id) => ws_stop(&id, flush),
            None => ws_stop_all(flush),
        },
        WsCommand::List => ws_list(),
        WsCommand::RunDaemon { id } => run_daemon_entry(&id).await,
    }
}

// ── channels / channel-info ──────────────────────────────────────────────────

fn ws_channels() -> Result<()> {
    let entries: Vec<Value> = ALL_CHANNELS
        .iter()
        .map(|ch| {
            json!({
                "channel": ch.name,
                "group": ch.group,
                "pattern": format!("{:?}", ch.pattern).to_lowercase(),
                "description": ch.description,
            })
        })
        .collect();
    output::success(json!(entries));
    Ok(())
}

fn ws_channel_info(name: &str) -> Result<()> {
    // Match exact name or candle pattern
    let info = ALL_CHANNELS.iter().find(|ch| {
        ch.name == name
            || (ch.name == "dex-token-candle{period}" && name.starts_with("dex-token-candle"))
    });
    match info {
        Some(ch) => {
            output::success(json!({
                "channel": if ch.name == "dex-token-candle{period}" { name } else { ch.name },
                "group": ch.group,
                "pattern": format!("{:?}", ch.pattern).to_lowercase(),
                "description": ch.description,
                "params": ch.params_hint,
                "example": ch.example,
            }));
        }
        None => {
            bail!(
                "unknown channel '{}'; use 'onchainos ws channels' to list all",
                name
            );
        }
    }
    Ok(())
}

// ── start ────────────────────────────────────────────────────────────────────

async fn ws_start(
    channels: Vec<String>,
    wallet_addresses: Vec<String>,
    chain_indexes: Vec<String>,
    token_pairs: Vec<TokenPair>,
    env: &str,
    idle_timeout_ms: u64,
) -> Result<()> {
    let watch_env = match env {
        "pre" => WatchEnv::Pre,
        "prod" => WatchEnv::Prod,
        other => bail!("unknown --env '{}'; use pre or prod", other),
    };

    let mut channels = channels;
    if channels.is_empty() {
        channels = DEFAULT_CHANNELS.iter().map(|c| c.to_string()).collect();
    }
    channels.sort();
    channels.dedup();

    // Validate required params per channel pattern
    for ch in &channels {
        match channel_pattern(ch) {
            ChannelPattern::PerWallet => {
                if wallet_addresses.is_empty() {
                    bail!("--wallet-addresses is required for channel '{}'", ch);
                }
                if wallet_addresses.len() > 200 {
                    bail!(
                        "--wallet-addresses exceeds maximum of 200 (got {})",
                        wallet_addresses.len()
                    );
                }
            }
            ChannelPattern::PerToken => {
                if token_pairs.is_empty() {
                    bail!("--token-pair is required for channel '{}' (format: chainIndex:tokenAddress)", ch);
                }
            }
            ChannelPattern::PerChain => {
                if chain_indexes.is_empty() {
                    bail!("--chain-index is required for channel '{}'", ch);
                }
            }
            ChannelPattern::Global => {}
        }
    }

    // Deduplicate params
    let mut wallet_addresses_sorted = wallet_addresses;
    wallet_addresses_sorted.sort();
    wallet_addresses_sorted.dedup();
    let wallet_addresses = wallet_addresses_sorted;

    let mut token_pairs_sorted = token_pairs;
    token_pairs_sorted.sort();
    token_pairs_sorted.dedup();
    let token_pairs = token_pairs_sorted;

    let mut chain_indexes_sorted = chain_indexes;
    chain_indexes_sorted.sort();
    chain_indexes_sorted.dedup();
    let chain_indexes = chain_indexes_sorted;

    // Return existing session if same config is already running
    let existing = store::list_watches()?;
    for w in &existing {
        if let Some(cfg) = &w.config {
            let mut ec = cfg.channels.clone();
            ec.sort();
            let mut ew = cfg.wallet_addresses.clone();
            ew.sort();
            let mut etp = cfg.token_pairs.clone();
            etp.sort();
            let mut eci = cfg.chain_indexes.clone();
            eci.sort();
            if ec == channels
                && ew == wallet_addresses
                && etp == token_pairs
                && eci == chain_indexes
                && cfg.env == watch_env
                && matches!(w.state, DaemonState::Running | DaemonState::Reconnecting)
            {
                output::success(json!({
                    "id": w.id,
                    "status": "already_running",
                    "channels": channels,
                    "env": env
                }));
                return Ok(());
            }
        }
    }

    let id = format!("ws_{}", &uuid::Uuid::new_v4().to_string()[..6]);

    let config = WatchConfig {
        channels: channels.clone(),
        wallet_addresses: wallet_addresses.clone(),
        token_pairs: token_pairs.clone(),
        chain_indexes: chain_indexes.clone(),
        env: watch_env,
        created_at: now_ms(),
        idle_timeout_ms,
    };
    // Pre-flight: verify credentials before spawning daemon
    crate::watch::daemon::Credentials::from_watch_env(&config.env)?;

    let dir = store::init_watch_dir(&id, &config)?;

    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["ws", "run-daemon", "--id", &id]);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    let log_file = std::fs::File::create(dir.join("daemon.log"))?;
    cmd.stderr(std::process::Stdio::from(log_file));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x00000008); // DETACHED_PROCESS
    }

    let child = cmd.spawn()?;
    let pid = child.id();
    store::write_pid(&dir, pid)?;
    drop(child);

    output::success(json!({
        "id": id,
        "status": "starting",
        "pid": pid,
        "channels": channels,
        "env": env,
        "dir": dir.to_string_lossy()
    }));
    Ok(())
}

// ── poll ─────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn ws_poll(
    id: &str,
    channel: Option<String>,
    limit: usize,
    min_quote_amount: Option<f64>,
    min_market_cap: Option<f64>,
    min_pnl: Option<f64>,
    trader: Option<String>,
    tag: Option<String>,
    since: Option<u64>,
    trade_type: Option<String>,
) -> Result<()> {
    let dir = store::watch_dir(id)?;
    if !dir.exists() {
        bail!("session '{}' not found", id);
    }

    let daemon_state = store::read_daemon_state(id)?;

    let poll_channel = match channel {
        Some(c) => c,
        None => {
            let config = store::read_config(id)?;
            config
                .channels
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("session has no channels configured"))?
        }
    };

    let is_tracker = is_tracker_channel(&poll_channel);
    let has_filters = is_tracker
        && (min_quote_amount.is_some()
            || min_market_cap.is_some()
            || min_pnl.is_some()
            || trader.is_some()
            || tag.is_some()
            || since.is_some()
            || trade_type.is_some());

    let fetch_limit = if has_filters { limit * 4 } else { limit };
    let result = store::read_events_from_cursor(&dir, &poll_channel, fetch_limit)?;

    let status_str = match &daemon_state {
        DaemonState::Disconnected(reason) => format!("disconnected:{}", reason),
        other => other.as_str().to_string(),
    };

    if is_tracker && has_filters {
        // Tracker channel with filters: deserialize into TradeEvent for filtering
        let tag_filter: Option<u8> = match tag.as_deref() {
            Some("smart_money") | Some("sm") | Some("1") => Some(1),
            Some("kol") | Some("2") => Some(2),
            Some(other) => bail!("unknown --tag value '{}'; use smart_money or kol", other),
            None => None,
        };
        let trade_type_filter = trade_type
            .as_deref()
            .map(resolve_trade_type)
            .map(str::to_string);

        // Filter with original index tracking for correct cursor commit
        let filtered: Vec<(usize, Value)> = result
            .events
            .iter()
            .enumerate()
            .filter(|&(_, v)| {
                let Ok(e) = serde_json::from_value::<TradeEvent>(v.clone()) else {
                    return false;
                };
                if let Some(min) = min_quote_amount {
                    if e.quote_token_amount.parse::<f64>().unwrap_or(0.0) < min {
                        return false;
                    }
                }
                if let Some(min) = min_market_cap {
                    if e.market_cap.parse::<f64>().unwrap_or(0.0) < min {
                        return false;
                    }
                }
                if let Some(min) = min_pnl {
                    if e.realized_pnl_usd
                        .parse::<f64>()
                        .unwrap_or(f64::NEG_INFINITY)
                        < min
                    {
                        return false;
                    }
                }
                if let Some(ref t) = trader {
                    if !e.wallet_address.starts_with(t.as_str()) {
                        return false;
                    }
                }
                if let Some(tag) = tag_filter {
                    let has_tag = e
                        .tracker_type
                        .as_ref()
                        .map(|list| list.contains(&tag))
                        .unwrap_or(false);
                    if !has_tag {
                        return false;
                    }
                }
                if let Some(ts) = since {
                    if e.trade_time.parse::<u64>().unwrap_or(0) < ts {
                        return false;
                    }
                }
                if let Some(ref tt) = trade_type_filter {
                    if !tt.is_empty() && tt != "0" && e.trade_type != tt.as_str() {
                        return false;
                    }
                }
                true
            })
            .take(limit)
            .map(|(i, v)| (i, v.clone()))
            .collect();

        let new_count = filtered.len();
        // Only advance cursor to the last *returned* event's position.
        // If no events matched, do NOT advance — those events may match
        // different filters on the next poll.
        if let Some((last_idx, _)) = filtered.last() {
            let commit_cursor = result
                .per_event_cursors
                .get(*last_idx)
                .copied()
                .unwrap_or(result.new_cursor);
            store::write_cursor(
                &dir,
                &poll_channel,
                commit_cursor.file_no,
                commit_cursor.offset,
            )?;
        }
        let filtered: Vec<Value> = filtered.into_iter().map(|(_, v)| v).collect();

        output::success(json!({
            "daemon_status": status_str,
            "new_count": new_count,
            "trades": filtered
        }));
    } else {
        // Non-tracker channel or no filters: return raw JSON events.
        // Cursor advances to the end of the read window (fetch_limit == limit here,
        // so all read events are returned — no events are skipped).
        let events: Vec<Value> = result.events.into_iter().take(limit).collect();
        let new_count = events.len();
        store::write_cursor(
            &dir,
            &poll_channel,
            result.new_cursor.file_no,
            result.new_cursor.offset,
        )?;

        let data_key = if is_tracker { "trades" } else { "events" };
        output::success(json!({
            "daemon_status": status_str,
            "new_count": new_count,
            data_key: events
        }));
    }
    Ok(())
}

// ── stop ─────────────────────────────────────────────────────────────────────

struct StopResult {
    flushed_events: Vec<Value>,
}

fn stop_one(id: &str, flush: bool) -> Result<StopResult> {
    let dir = store::watch_dir(id)?;
    if !dir.exists() {
        bail!("session '{}' not found", id);
    }

    let mut flushed_events = Vec::new();
    if flush {
        let config = store::read_config(id)?;
        for ch in &config.channels {
            let result = store::read_events_from_cursor(&dir, ch, 1000)?;
            store::write_cursor(
                &dir,
                ch,
                result.new_cursor.file_no,
                result.new_cursor.offset,
            )?;
            flushed_events.extend(result.events);
        }
    }

    let _ = kill_daemon(id);
    let _ = store::write_status(&dir, "stopped", None);
    store::remove_watch_dir(id)?;

    Ok(StopResult { flushed_events })
}

fn ws_stop(id: &str, flush: bool) -> Result<()> {
    let result = stop_one(id, flush)?;
    output::success(json!({
        "id": id,
        "status": "stopped",
        "flushed_count": result.flushed_events.len(),
        "flushed_events": result.flushed_events,
    }));
    Ok(())
}

fn ws_stop_all(flush: bool) -> Result<()> {
    let watches = store::list_watches()?;
    if watches.is_empty() {
        output::success(json!({ "stopped": [], "message": "no active sessions" }));
        return Ok(());
    }
    let mut stopped = Vec::new();
    for w in watches {
        match stop_one(&w.id, flush) {
            Ok(_) => stopped.push(w.id),
            Err(e) => eprintln!("[warn] failed to stop {}: {}", w.id, e),
        }
    }
    output::success(json!({ "stopped": stopped }));
    Ok(())
}

fn kill_daemon(id: &str) -> Result<()> {
    let pid = store::read_pid(id)?;

    #[cfg(unix)]
    {
        use std::time::Duration;
        let pid_i32 = i32::try_from(pid)
            .ok()
            .filter(|&p| p > 0)
            .ok_or_else(|| anyhow::anyhow!("invalid PID {} — refusing to send signal", pid))?;
        unsafe { libc_kill(pid_i32, 15) }; // SIGTERM
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(100));
            if unsafe { libc_kill(pid_i32, 0) } != 0 {
                return Ok(());
            }
        }
        unsafe { libc_kill(pid_i32, 9) }; // SIGKILL
    }

    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
    }

    Ok(())
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, sig)
}

// ── list ─────────────────────────────────────────────────────────────────────

fn ws_list() -> Result<()> {
    let watches = store::list_watches()?;
    let entries: Vec<_> = watches
        .iter()
        .map(|w| {
            let channels = w
                .config
                .as_ref()
                .map(|c| c.channels.clone())
                .unwrap_or_default();
            let env = w
                .config
                .as_ref()
                .map(|c| format!("{:?}", c.env).to_lowercase())
                .unwrap_or_default();
            let created_at = w
                .config
                .as_ref()
                .map(|c| c.created_at.to_string())
                .unwrap_or_default();
            let status_str = match &w.state {
                DaemonState::Disconnected(r) => format!("disconnected:{}", r),
                other => other.as_str().to_string(),
            };
            json!({
                "id": w.id,
                "status": status_str,
                "pid": w.pid,
                "channels": channels,
                "env": env,
                "created_at": created_at
            })
        })
        .collect();
    output::success(json!(entries));
    Ok(())
}

// ── daemon entry ─────────────────────────────────────────────────────────────

async fn run_daemon_entry(id: &str) -> Result<()> {
    let dir = store::watch_dir(id)?;
    if !dir.exists() {
        bail!("session dir for '{}' does not exist", id);
    }
    crate::watch::daemon::run_daemon(id, &dir).await
}
