use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;
use crate::watch::store::{self, now_ms};
use crate::watch::types::{DaemonState, WatchConfig, WatchEnv, ALL_CHANNELS};

/// Resolve tracker type alias to API integer string.
fn resolve_tracker_type(s: &str) -> &str {
    match s.to_lowercase().as_str() {
        "smart_money" | "smartmoney" | "smart-money" | "sm" | "1" => "1",
        "kol" | "2" => "2",
        "multi_address" | "multi-address" | "custom" | "3" => "3",
        _ => s,
    }
}

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
pub enum TrackerCommand {
    /// Get on-chain trading activity of tracked addresses (KOL / smart money / multi-address)
    Trades {
        /// Tracker type: kol (default), smart_money/sm, multi_address/custom. Also accepts 1/2/3.
        #[arg(long, default_value = "kol")]
        tracker_type: String,
        /// Wallet address(es) to track — required when --tracker-type is multi_address/custom/3.
        /// Comma-separated, max 20 addresses.
        #[arg(long)]
        wallet_address: Option<String>,
        /// Trade type: all/0 (default), buy/1, sell/2
        #[arg(long)]
        trade_type: Option<String>,
        /// Chain: all (default), ethereum/eth, solana/sol, bsc/bnb, base, xlayer, or numeric chainIndex
        #[arg(long)]
        chain: Option<String>,
        /// Minimum trade volume in USD
        #[arg(long)]
        min_volume: Option<String>,
        /// Maximum trade volume in USD
        #[arg(long)]
        max_volume: Option<String>,
        /// Minimum holder count of the traded token
        #[arg(long)]
        min_holders: Option<String>,
        /// Minimum market cap in USD
        #[arg(long)]
        min_market_cap: Option<String>,
        /// Maximum market cap in USD
        #[arg(long)]
        max_market_cap: Option<String>,
        /// Minimum liquidity in USD
        #[arg(long)]
        min_liquidity: Option<String>,
        /// Maximum liquidity in USD
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
        /// Channel to poll (e.g. kol_smartmoney-tracker-activity). Defaults to the session's subscribed channel.
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
        /// Filter: only return events from this traderAddress (exact or prefix match)
        #[arg(long)]
        trader: Option<String>,
        /// Filter: only return events matching tag type — smart_money (1) or kol (2)
        #[arg(long)]
        tag: Option<String>,
        /// Filter: only return events with tradeTime >= this ms timestamp (for catching up history)
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
        TrackerCommand::Trades {
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
            tracker_trades(
                ctx,
                &tracker_type,
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
            )
            .await
        }
        TrackerCommand::Watch { command } => execute_watch(command).await,
    }
}

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

    // Default to all known channels when none specified
    let mut channels = channels;
    if channels.is_empty() {
        channels = ALL_CHANNELS.iter().map(|c| c.name.to_string()).collect();
    }
    channels.sort();
    channels.dedup();

    // Validate: address-tracker-activity requires at least one wallet address, max 20
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

    // Generate watch ID
    let id = format!("watch_{}", &uuid::Uuid::new_v4().to_string()[..6]);

    let config = WatchConfig {
        channels: channels.clone(),
        wallet_addresses: wallet_addresses.clone(),
        env: watch_env,
        created_at: now_ms(),
    };
    let dir = store::init_watch_dir(&id, &config)?;

    // Spawn daemon as detached child process
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["tracker", "watch", "run-daemon", "--id", &id]);

    // Redirect stdio so daemon doesn't inherit the terminal
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x00000008); // DETACHED_PROCESS
    }

    let child = cmd.spawn()?;
    let pid = child.id();

    // Write PID immediately (daemon will overwrite with same value on start)
    store::write_pid(&dir, pid)?;

    // Drop child handle — parent exits, daemon is reparented to init on Unix
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

    // Resolve channel: use --channel arg, or fall back to first channel in session config
    let poll_channel = match channel {
        Some(c) => c,
        None => {
            let config = store::read_config(id)?;
            config.channels.into_iter().next()
                .ok_or_else(|| anyhow::anyhow!("session has no channels configured"))?
        }
    };

    // Read events from cursor
    let result = store::read_events_from_cursor(&dir, &poll_channel, limit * 4)?; // over-fetch before filter

    // Resolve tag filter
    let tag_filter: Option<u8> = match tag.as_deref() {
        Some("smart_money") | Some("sm") | Some("1") => Some(1),
        Some("kol") | Some("2") => Some(2),
        Some(other) => bail!("unknown --tag value '{}'; use smart_money or kol", other),
        None => None,
    };

    let trade_type_filter = trade_type.as_deref().map(resolve_trade_type).map(str::to_string);

    // Apply client-side filters
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
                // actual data uses "1"=buy, "2"=sell; "0" means all
                if !tt.is_empty() && tt != "0" {
                    if e.trade_type != tt.as_str() {
                        return false;
                    }
                }
            }
            true
        })
        .take(limit)
        .collect();

    let last_trade_time = filtered.last().map(|e| e.trade_time.as_str()).unwrap_or("").to_string();
    let new_count = filtered.len();

    // Persist updated cursor
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

    let kill_result = kill_daemon(id);
    let _ = store::write_status(&dir, "stopped", None);
    store::remove_watch_dir(id)?;
    let _ = kill_result;

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
        // Give it up to 3s for graceful shutdown
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(100));
            if unsafe { libc_kill(pid, 0) } != 0 {
                return Ok(()); // process gone
            }
        }
        unsafe { libc_kill(pid, 9) }; // SIGKILL
    }

    #[cfg(windows)]
    {
        // On Windows use taskkill
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

// ── tracker trades ────────────────────────────────────────────────────────────

/// GET /api/v6/dex/market/address-tracker/trades (MCP-callable)
#[allow(clippy::too_many_arguments)]
pub async fn fetch_tracker_trades(
    client: &ApiClient,
    tracker_type: Option<&str>,
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
) -> Result<Value> {
    let tracker_type_resolved = resolve_tracker_type(tracker_type.unwrap_or("kol"));

    if tracker_type_resolved == "3" && wallet_address.is_none() {
        bail!("wallet_address is required when tracker_type is multi_address/custom/3");
    }

    let chain_index = match chain {
        None => String::new(),
        Some(c) if c.eq_ignore_ascii_case("all") => String::new(),
        Some(c) => crate::chains::resolve_chain(c),
    };

    let wallet_address = wallet_address.unwrap_or_default();
    let trade_type = trade_type.map(resolve_trade_type).unwrap_or_default();
    let min_volume = min_volume.unwrap_or_default();
    let max_volume = max_volume.unwrap_or_default();
    let min_holders = min_holders.unwrap_or_default();
    let min_market_cap = min_market_cap.unwrap_or_default();
    let max_market_cap = max_market_cap.unwrap_or_default();
    let min_liquidity = min_liquidity.unwrap_or_default();
    let max_liquidity = max_liquidity.unwrap_or_default();

    client
        .get(
            "/api/v6/dex/market/address-tracker/trades",
            &[
                ("trackerType", tracker_type_resolved),
                ("walletAddress", wallet_address),
                ("tradeType", trade_type),
                ("chainIndex", chain_index.as_str()),
                ("minVolume", min_volume),
                ("maxVolume", max_volume),
                ("minHolders", min_holders),
                ("minMarketCap", min_market_cap),
                ("maxMarketCap", max_market_cap),
                ("minLiquidity", min_liquidity),
                ("maxLiquidity", max_liquidity),
            ],
        )
        .await
}

/// GET /api/v6/dex/market/address-tracker/trades
#[allow(clippy::too_many_arguments)]
async fn tracker_trades(
    ctx: &Context,
    tracker_type: &str,
    wallet_address: Option<String>,
    trade_type: Option<String>,
    chain: Option<String>,
    min_volume: Option<String>,
    max_volume: Option<String>,
    min_holders: Option<String>,
    min_market_cap: Option<String>,
    max_market_cap: Option<String>,
    min_liquidity: Option<String>,
    max_liquidity: Option<String>,
) -> Result<()> {
    let tracker_type_resolved = resolve_tracker_type(tracker_type);

    if tracker_type_resolved == "3" && wallet_address.is_none() {
        bail!("--wallet-address is required when --tracker-type is multi_address/custom/3");
    }

    let chain_index = match chain {
        None => String::new(),
        Some(ref c) if c.eq_ignore_ascii_case("all") => String::new(),
        Some(ref c) => crate::chains::resolve_chain(c),
    };

    let wallet_address = wallet_address.unwrap_or_default();
    let trade_type = trade_type
        .as_deref()
        .map(resolve_trade_type)
        .unwrap_or_default()
        .to_string();
    let min_volume = min_volume.unwrap_or_default();
    let max_volume = max_volume.unwrap_or_default();
    let min_holders = min_holders.unwrap_or_default();
    let min_market_cap = min_market_cap.unwrap_or_default();
    let max_market_cap = max_market_cap.unwrap_or_default();
    let min_liquidity = min_liquidity.unwrap_or_default();
    let max_liquidity = max_liquidity.unwrap_or_default();

    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/address-tracker/trades",
            &[
                ("trackerType", tracker_type_resolved),
                ("walletAddress", wallet_address.as_str()),
                ("tradeType", trade_type.as_str()),
                ("chainIndex", chain_index.as_str()),
                ("minVolume", min_volume.as_str()),
                ("maxVolume", max_volume.as_str()),
                ("minHolders", min_holders.as_str()),
                ("minMarketCap", min_market_cap.as_str()),
                ("maxMarketCap", max_market_cap.as_str()),
                ("minLiquidity", min_liquidity.as_str()),
                ("maxLiquidity", max_liquidity.as_str()),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}
