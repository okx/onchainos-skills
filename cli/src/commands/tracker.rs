use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;
use crate::watch::store::{self, now_ms};
use crate::watch::types::{DaemonState, WatchConfig, WatchEnv, DEFAULT_CHANNELS};

/// Resolve trade type alias to API integer string.
fn resolve_trade_type(s: &str) -> &str {
    match s.to_lowercase().as_str() {
        "all" | "0" => "0",
        "buy" | "1" => "1",
        "sell" | "2" => "2",
        _ => s,
    }
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum TrackerCommand {
    /// Get latest DEX activities for tracked addresses (smart money, KOL, or custom multi-address)
    Activities {
        /// Tracker type: smart_money (or 1), kol (or 2), multi_address (or 3)
        #[arg(long)]
        tracker_type: String,
        /// Wallet addresses (required for multi_address), comma-separated, max 20
        #[arg(long)]
        wallet_address: Option<String>,
        /// Trade type: 0=all (default), 1=buy, 2=sell
        #[arg(long)]
        trade_type: Option<String>,
        /// Chain filter (e.g. ethereum, solana). Omit for all chains
        #[arg(long)]
        chain: Option<String>,
        /// Minimum trade volume (USD)
        #[arg(long)]
        min_volume: Option<String>,
        /// Maximum trade volume (USD)
        #[arg(long)]
        max_volume: Option<String>,
        /// Minimum number of holding addresses
        #[arg(long)]
        min_holders: Option<String>,
        /// Minimum market cap (USD)
        #[arg(long)]
        min_market_cap: Option<String>,
        /// Maximum market cap (USD)
        #[arg(long)]
        max_market_cap: Option<String>,
        /// Minimum liquidity (USD)
        #[arg(long)]
        min_liquidity: Option<String>,
        /// Maximum liquidity (USD)
        #[arg(long)]
        max_liquidity: Option<String>,
    },
    /// Real-time WebSocket watch for tracker events
    Watch {
        #[command(subcommand)]
        command: WatchCommand,
    },
}

#[derive(Subcommand)]
pub enum WatchCommand {
    /// Start a background WebSocket watch session and return its ID
    Start {
        /// Channel(s) to subscribe, e.g. --channel kol_smartmoney-tracker-activity.
        /// Can be specified multiple times. Defaults to all known channels.
        #[arg(long)]
        channel: Vec<String>,
        /// Wallet addresses for the address-tracker-activity channel, comma-separated.
        /// e.g. --wallet-addresses 0xAAA,0xBBB,0xCCC (max 20)
        /// Required when --channel address-tracker-activity is used.
        #[arg(long)]
        wallet_addresses: Option<String>,
        /// Environment: prod (default) or pre
        #[arg(long, default_value = "prod")]
        env: String,
    },

    /// Poll incremental events from a running watch session
    Poll {
        /// Watch session ID returned by watch start
        #[arg(long)]
        id: String,
        /// Channel to poll (e.g. kol_smartmoney-tracker-activity). Defaults to the session's first channel.
        #[arg(long)]
        channel: Option<String>,
        /// Maximum number of events to return (default: 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Filter: only return events where quoteTokenAmount >= this value
        #[arg(long)]
        min_quote_amount: Option<f64>,
        /// Filter: only return events where marketCap >= this value (USD)
        #[arg(long)]
        min_market_cap: Option<f64>,
        /// Filter: only return events where realizedPnlUsd >= this value (set 0 for profit-only)
        #[arg(long)]
        min_pnl: Option<f64>,
        /// Filter: only return events from this walletAddress (exact or prefix match)
        #[arg(long)]
        trader: Option<String>,
        /// Filter: only return events matching tag type — smart_money (1) or kol (2)
        #[arg(long)]
        tag: Option<String>,
        /// Filter: only return events with tradeTime >= this ms timestamp
        #[arg(long)]
        since: Option<u64>,
        /// Filter: buy or sell
        #[arg(long)]
        trade_type: Option<String>,
    },

    /// Stop a running watch session and clean up its resources.
    /// If --id is omitted, all running sessions are stopped.
    Stop {
        /// Watch session ID to stop. Omit to stop all sessions.
        #[arg(long)]
        id: Option<String>,
        /// Return any unread events before stopping
        #[arg(long)]
        flush: bool,
    },

    /// List all watch sessions
    List,

    /// Internal: run daemon event loop (not for direct use)
    #[command(hide = true)]
    RunDaemon {
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(ctx: &Context, cmd: TrackerCommand) -> Result<()> {
    match cmd {
        TrackerCommand::Activities {
            tracker_type,
            wallet_address,
            trade_type,
            chain,
            min_volume,
            max_volume,
            min_holders,
            min_market_cap,
            max_market_cap,
            min_liquidity,
            max_liquidity,
        } => {
            tracker_activities(
                ctx,
                &tracker_type,
                wallet_address.as_deref(),
                trade_type.as_deref(),
                chain.as_deref(),
                min_volume.as_deref(),
                max_volume.as_deref(),
                min_holders.as_deref(),
                min_market_cap.as_deref(),
                max_market_cap.as_deref(),
                min_liquidity.as_deref(),
                max_liquidity.as_deref(),
            )
            .await
        }
        TrackerCommand::Watch { command } => execute_watch(command).await,
    }
}

// ── Public fetch functions (used by both CLI and MCP) ────────────────

pub fn resolve_tracker_type(t: &str) -> &str {
    match t {
        "smart_money" => "1",
        "kol" => "2",
        "multi_address" => "3",
        other => other,
    }
}

/// GET /api/v6/dex/market/address-tracker/trades
#[allow(clippy::too_many_arguments)]
pub async fn fetch_activities(
    client: &ApiClient,
    tracker_type: &str,
    wallet_address: Option<&str>,
    trade_type: Option<&str>,
    chain_index: Option<&str>,
    min_volume: Option<&str>,
    max_volume: Option<&str>,
    min_holders: Option<&str>,
    min_market_cap: Option<&str>,
    max_market_cap: Option<&str>,
    min_liquidity: Option<&str>,
    max_liquidity: Option<&str>,
) -> Result<Value> {
    let tracker_type_val = resolve_tracker_type(tracker_type);
    let mut query: Vec<(&str, &str)> = vec![("trackerType", tracker_type_val)];
    if let Some(w) = wallet_address {
        query.push(("walletAddress", w));
    }
    if let Some(t) = trade_type {
        query.push(("tradeType", t));
    }
    if let Some(c) = chain_index {
        query.push(("chainIndex", c));
    }
    if let Some(v) = min_volume {
        query.push(("minVolume", v));
    }
    if let Some(v) = max_volume {
        query.push(("maxVolume", v));
    }
    if let Some(h) = min_holders {
        query.push(("minHolders", h));
    }
    if let Some(m) = min_market_cap {
        query.push(("minMarketCap", m));
    }
    if let Some(m) = max_market_cap {
        query.push(("maxMarketCap", m));
    }
    if let Some(l) = min_liquidity {
        query.push(("minLiquidity", l));
    }
    if let Some(l) = max_liquidity {
        query.push(("maxLiquidity", l));
    }
    client
        .get("/api/v6/dex/market/address-tracker/trades", &query)
        .await
}

// ── CLI wrapper ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn tracker_activities(
    ctx: &Context,
    tracker_type: &str,
    wallet_address: Option<&str>,
    trade_type: Option<&str>,
    chain: Option<&str>,
    min_volume: Option<&str>,
    max_volume: Option<&str>,
    min_holders: Option<&str>,
    min_market_cap: Option<&str>,
    max_market_cap: Option<&str>,
    min_liquidity: Option<&str>,
    max_liquidity: Option<&str>,
) -> Result<()> {
    let resolved = resolve_tracker_type(tracker_type);
    if (resolved == "3" || tracker_type == "multi_address") && wallet_address.is_none() {
        anyhow::bail!("--wallet-address is required when --tracker-type is multi_address");
    }
    let chain_index = chain.map(|c| crate::chains::resolve_chain(c).to_string());
    let client = ctx.client_async().await?;
    output::success(
        fetch_activities(
            &client,
            tracker_type,
            wallet_address,
            trade_type,
            chain_index.as_deref(),
            min_volume,
            max_volume,
            min_holders,
            min_market_cap,
            max_market_cap,
            min_liquidity,
            max_liquidity,
        )
        .await?,
    );
    Ok(())
}

// ── watch ─────────────────────────────────────────────────────────────────────

async fn execute_watch(cmd: WatchCommand) -> Result<()> {
    match cmd {
        WatchCommand::Start { channel, wallet_addresses, env } => {
            let addrs = wallet_addresses
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            watch_start(channel, addrs, &env).await
        }
        WatchCommand::Poll {
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
        } => watch_poll(&id, channel, limit, min_quote_amount, min_market_cap, min_pnl, trader, tag, since, trade_type),
        WatchCommand::Stop { id, flush } => match id {
            Some(id) => watch_stop(&id, flush),
            None => watch_stop_all(flush),
        },
        WatchCommand::List => watch_list(),
        WatchCommand::RunDaemon { id } => run_daemon_entry(&id).await,
    }
}

// ── watch start ───────────────────────────────────────────────────────────────

async fn watch_start(channels: Vec<String>, wallet_addresses: Vec<String>, env: &str) -> Result<()> {
    let watch_env = match env {
        "pre" => WatchEnv::Pre,
        "prod" => WatchEnv::Prod,
        other => bail!("unknown --env '{}'; use pre or prod", other),
    };

    let mut channels = channels;
    if channels.is_empty() {
        channels = DEFAULT_CHANNELS.iter().map(|c| c.name.to_string()).collect();
    }
    channels.sort();
    channels.dedup();

    if channels.iter().any(|c| c == "address-tracker-activity") {
        if wallet_addresses.is_empty() {
            bail!("--wallet-addresses is required when using channel address-tracker-activity");
        }
        if wallet_addresses.len() > 20 {
            bail!("--wallet-addresses exceeds maximum of 20 addresses (got {})", wallet_addresses.len());
        }
    }

    // Return existing session if same channel set + env is already running
    let existing = store::list_watches()?;
    let mut wallet_addresses_sorted = wallet_addresses.clone();
    wallet_addresses_sorted.sort();
    wallet_addresses_sorted.dedup();
    let wallet_addresses = wallet_addresses_sorted;

    for w in &existing {
        if let Some(cfg) = &w.config {
            let mut existing_channels = cfg.channels.clone();
            existing_channels.sort();
            let mut existing_wallets = cfg.wallet_addresses.clone();
            existing_wallets.sort();
            if existing_channels == channels
                && existing_wallets == wallet_addresses
                && cfg.env == watch_env
                && matches!(w.state, DaemonState::Running | DaemonState::Reconnecting)
            {
                output::success(json!({
                    "id": w.id,
                    "status": "already_running",
                    "channels": channels,
                    "wallet_addresses": wallet_addresses,
                    "env": env
                }));
                return Ok(());
            }
        }
    }

    let id = format!("watch_{}", &uuid::Uuid::new_v4().to_string()[..6]);

    let config = WatchConfig {
        channels: channels.clone(),
        wallet_addresses: wallet_addresses.clone(),
        env: watch_env,
        created_at: now_ms(),
    };
    let dir = store::init_watch_dir(&id, &config)?;

    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["tracker", "watch", "run-daemon", "--id", &id]);
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
        "wallet_addresses": wallet_addresses,
        "env": env,
        "dir": dir.to_string_lossy()
    }));
    Ok(())
}

// ── watch poll ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn watch_poll(
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
        bail!("watch session '{}' not found", id);
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

    let result = store::read_events_from_cursor(&dir, &poll_channel, limit * 4)?;

    let tag_filter: Option<u8> = match tag.as_deref() {
        Some("smart_money") | Some("sm") | Some("1") => Some(1),
        Some("kol") | Some("2") => Some(2),
        Some(other) => bail!("unknown --tag value '{}'; use smart_money or kol", other),
        None => None,
    };

    let trade_type_filter = trade_type.as_deref().map(resolve_trade_type).map(str::to_string);

    let filtered: Vec<_> = result
        .events
        .into_iter()
        .filter(|e| {
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
                if e.realized_pnl_usd.parse::<f64>().unwrap_or(f64::NEG_INFINITY) < min {
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
        .collect();

    let last_trade_time = filtered
        .last()
        .map(|e| e.trade_time.as_str())
        .unwrap_or("")
        .to_string();
    let new_count = filtered.len();

    store::write_cursor(&dir, &poll_channel, result.new_cursor.file_no, result.new_cursor.offset)?;

    let status_str = match &daemon_state {
        DaemonState::Disconnected(reason) => format!("disconnected:{}", reason),
        other => other.as_str().to_string(),
    };

    output::success(json!({
        "daemon_status": status_str,
        "new_count": new_count,
        "last_trade_time": last_trade_time,
        "trades": filtered
    }));
    Ok(())
}

// ── watch stop ────────────────────────────────────────────────────────────────

fn stop_one(id: &str, flush: bool) -> Result<usize> {
    let dir = store::watch_dir(id)?;
    if !dir.exists() {
        bail!("watch session '{}' not found", id);
    }

    let flushed_count = if flush {
        let config = store::read_config(id)?;
        let mut n = 0usize;
        for ch in &config.channels {
            let result = store::read_events_from_cursor(&dir, ch, 1000)?;
            store::write_cursor(&dir, ch, result.new_cursor.file_no, result.new_cursor.offset)?;
            n += result.events.len();
        }
        n
    } else {
        0
    };

    let _ = kill_daemon(id);
    let _ = store::write_status(&dir, "stopped", None);
    store::remove_watch_dir(id)?;

    Ok(flushed_count)
}

fn watch_stop(id: &str, flush: bool) -> Result<()> {
    let flushed_count = stop_one(id, flush)?;
    output::success(json!({
        "id": id,
        "status": "stopped",
        "flushed_count": flushed_count,
    }));
    Ok(())
}

fn watch_stop_all(flush: bool) -> Result<()> {
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
        unsafe { libc_kill(pid, 15) }; // SIGTERM
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(100));
            if unsafe { libc_kill(pid, 0) } != 0 {
                return Ok(());
            }
        }
        unsafe { libc_kill(pid, 9) }; // SIGKILL
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
unsafe fn libc_kill(pid: u32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid as i32, sig)
}

// ── watch list ────────────────────────────────────────────────────────────────

fn watch_list() -> Result<()> {
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

// ── daemon entry ──────────────────────────────────────────────────────────────

async fn run_daemon_entry(id: &str) -> Result<()> {
    let dir = store::watch_dir(id)?;
    if !dir.exists() {
        bail!("watch dir for '{}' does not exist", id);
    }
    crate::watch::daemon::run_daemon(id, &dir).await
}
