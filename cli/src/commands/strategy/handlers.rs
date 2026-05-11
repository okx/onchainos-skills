//! 4 P0 subcommand handlers: create-limit / cancel / list / resume.
//!
//! Each handler has its own clap `Args` struct + `execute` async fn.
//! Common pre-flight (`ctx.client_async()` + `session::load()`) is repeated
//! in each handler — keeps each entry-point self-contained without an
//! umbrella init function (mirrors how swap.rs / signal.rs are structured).

use anyhow::{anyhow, bail, Context as _, Result};
use chrono::{SecondsFormat, Utc};
use clap::Args;
use serde_json::json;

use crate::client::ApiClient;
use crate::commands::token;
use crate::commands::Context;
use crate::output;

use super::api;
use super::session;
use super::status::{is_upgrade_required, status_label, OrderStatus};
use super::supported_chains;
use super::trader_mode::{self, ActivateCtx, BuildIntentArgs};
use super::types::{
    direction, strategy_type, CancelReq, CreateOrderReq, ListOrdersReq, ListOrdersResp,
    OrderListResp, ReactivateReq, Rule, VerifySignInfo,
};

const DEFAULT_EXPIRES_SECS: i64 = 7 * 24 * 60 * 60; // 7 days
/// User-facing default for `--slippage`, expressed as a **percent**
/// (`"15"` = 15% per PRD §5.2). Mirrors the percent convention `swap`
/// uses, so a user moving between commands sees the same units.
/// `build_default_preset` divides this by 100 before sending it as
/// `buyPreset.slippageValue` (the BE wire format is a decimal fraction
/// — e.g. `"0.15"` for 15%).
const DEFAULT_SLIPPAGE_VALUE: &str = "15";
/// Router mode bound to `buyPreset.routerModeType` / `sellPreset.routerModeType`.
/// BE-defined tri-state:
/// - `1` = default (user did NOT pass `--mev-protection` either way)
/// - `2` = MEV protection ON  (`--mev-protection`)
/// - `3` = MEV protection OFF (`--no-mev-protection`)
const ROUTER_MODE_DEFAULT: i64 = 1;
const ROUTER_MODE_MEV_ON: i64 = 2;
const ROUTER_MODE_MEV_OFF: i64 = 3;
/// Fixed fee level for limit orders (BE-confirmed 2026-05-08). `limitOrderFeeValue`
/// is no longer sent — BE derives it server-side.
const DEFAULT_LIMIT_ORDER_FEE_LEVEL: i64 = 2;
const ACTIVATE_DEFAULT_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000; // 30 days (PRD §4.2: re-activation always extends to a full 30-day window)

// ════════════════════════════════════════════════════════════════════
// create-limit
// ════════════════════════════════════════════════════════════════════

#[derive(Args, Debug)]
pub struct CreateLimitArgs {
    /// Chain id or alias (e.g. `1`, `solana`, `bsc`).
    #[arg(long)]
    pub chain_id: String,

    /// Sell-side token address.
    #[arg(long)]
    pub from_token: String,

    /// Buy-side token address.
    #[arg(long)]
    pub to_token: String,

    /// Amount of `from_token` to sell.
    #[arg(long)]
    pub amount: String,

    /// USD trigger price (Advanced mode); mutually exclusive with `--trigger-rate`.
    #[arg(long, conflicts_with = "trigger_rate")]
    pub trigger_price: Option<String>,

    /// Exchange-rate trigger; mutually exclusive with `--trigger-price`.
    #[arg(long)]
    pub trigger_rate: Option<String>,

    /// Slippage tolerance — passed verbatim as `buyPreset.slippageValue`.
    /// Slippage tolerance in percent. Default `15` per PRD §5.2 (paired with
    /// the dynamic-tier preset BE expects for limit orders).
    /// Accepts plain number (`20`) or with `%` suffix (`20%`) — both → wire `"0.20"`.
    /// Made Option so we can detect user-explicit override and surface the
    /// percent interpretation in human output.
    #[arg(long)]
    pub slippage: Option<String>,

    /// MEV protection. Tri-state:
    /// - flag absent → `routerModeType=1` (BE default; CLI does not opt in or out)
    /// - `--mev-protection` → `routerModeType=2` (MEV protection ON)
    /// - `--no-mev-protection` → `routerModeType=3` (MEV protection OFF)
    #[arg(long, overrides_with = "_no_mev_protection")]
    pub mev_protection: bool,
    #[arg(long = "no-mev-protection", overrides_with = "mev_protection", hide = true)]
    pub _no_mev_protection: bool,

    /// Strategy type — buy_dip / take_profit / stop_loss / chase_high.
    #[arg(long, value_parser = parse_strategy_type)]
    pub r#type: i32,

    /// Direction — buy / sell / all. Default = derived from `--type`.
    #[arg(long)]
    pub direction: Option<String>,

    /// Order TTL in seconds (default 604800 = 7 days).
    #[arg(long, default_value_t = DEFAULT_EXPIRES_SECS)]
    pub expires_in: i64,

    /// Output mode.
    #[arg(long, default_value = "human")]
    pub format: String,
}

fn parse_strategy_type(raw: &str) -> Result<i32, String> {
    match raw.to_ascii_lowercase().as_str() {
        "buy_dip" | "buy-dip" => Ok(strategy_type::BUY_DIP),
        "take_profit" | "take-profit" => Ok(strategy_type::TAKE_PROFIT),
        "stop_loss" | "stop-loss" => Ok(strategy_type::STOP_LOSS),
        "chase_high" | "chase-high" => Ok(strategy_type::CHASE_HIGH),
        other => Err(format!(
            "unknown strategy type `{other}` — expected buy_dip / take_profit / stop_loss / chase_high"
        )),
    }
}

fn default_direction(strat: i32) -> i32 {
    match strat {
        strategy_type::BUY_DIP | strategy_type::CHASE_HIGH => direction::BUY,
        strategy_type::TAKE_PROFIT | strategy_type::STOP_LOSS => direction::SELL,
        _ => direction::ALL,
    }
}

fn parse_direction(raw: Option<&str>, strat: i32) -> Result<i32> {
    match raw {
        None => Ok(default_direction(strat)),
        Some(s) => match s.to_ascii_lowercase().as_str() {
            "buy" => Ok(direction::BUY),
            "sell" => Ok(direction::SELL),
            "all" => Ok(direction::ALL),
            other => Err(anyhow!(
                "unknown direction `{other}` — expected buy / sell / all"
            )),
        },
    }
}

pub async fn create_limit(ctx: &Context, args: CreateLimitArgs) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let session = session::load()?;

    if args.trigger_price.is_none() && args.trigger_rate.is_none() {
        bail!("must pass either --trigger-price (USD) or --trigger-rate");
    }

    // Resolve chain alias if given (e.g. "solana" -> "501"). For unknown
    // strings the helper returns the original — BE will reject if invalid.
    let resolved_chain = crate::chains::resolve_chain(&args.chain_id);
    // Validate against the strategy-specific 6-chain whitelist BEFORE calling
    // BE, so the user gets a friendly error instead of round-tripping to BE
    // for code 10106 CHAIN_NOT_SUPPORT_ERROR.
    supported_chains::ensure_strategy_chain(&resolved_chain, &args.chain_id)?;
    let user_wallet_address = session.wallet_address_for(&resolved_chain).to_string();
    if user_wallet_address.is_empty() {
        bail!(
            "no wallet address for chain `{}` — login with the right chain enabled first",
            resolved_chain
        );
    }

    let dir = parse_direction(args.direction.as_deref(), args.r#type)?;

    // 1. Fetch fromToken decimals (one HTTP call) and shift the amount to
    //    the raw integer representation. The raw integer is only used for the
    //    `signMsg` line "From Amount(precision adjusted)"; `rule.fromAmount`
    //    keeps the human-readable form (see comment below).
    let from_decimals = fetch_token_decimals(&mut client, &args.from_token, &resolved_chain)
        .await
        .with_context(|| {
            format!(
                "fetch decimals for fromToken `{}` on chain `{}`",
                args.from_token, resolved_chain
            )
        })?;
    let from_amount_raw = trader_mode::shift_value(&args.amount, from_decimals)?;

    let rule = Rule {
        from_token_address: args.from_token.clone(),
        to_token_address: args.to_token.clone(),
        // BE contract (2026-05-07): `rule.fromAmount` uses the human-readable
        // decimal form (e.g. "0.1"); only the `verifySignInfo.signMsg` line
        // "From Amount(precision adjusted)" uses the raw integer
        // (amount * 10^decimals).
        from_amount: args.amount.clone(),
        // U-pegged Phase 1 (strategyMode=7): toAmount + exChangeRate are
        // SwapMode-only fields, not required by BE — omit. (Confirmed 2026-05-07.)
        to_amount: None,
        exchange_rate: None,
        trigger_price: args.trigger_price.clone(),
        trigger_market_capacity: None,
        min_return_amount: None,
    };

    let slippage_raw = args.slippage.as_deref().unwrap_or(DEFAULT_SLIPPAGE_VALUE);
    let mev_choice = if args._no_mev_protection {
        Some(false)
    } else if args.mev_protection {
        Some(true)
    } else {
        None
    };
    let preset = build_default_preset(slippage_raw, mev_choice, dir);

    // 2. Build the time fields of the intent: Created At / Expired At /
    //    Timestamp.
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let created_at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let expire_time_ms = now_ms.saturating_add(args.expires_in.saturating_mul(1000));
    let expired_at = chrono::DateTime::<Utc>::from_timestamp_millis(expire_time_ms)
        .ok_or_else(|| anyhow!("expire_time {expire_time_ms} ms out of chrono range"))?
        .to_rfc3339_opts(SecondsFormat::Millis, true);

    // `verifySignInfo.chainId` is a Long — non-numeric chain aliases fail
    // here (a defensive check before sending to BE).
    let chain_id_long: i64 = resolved_chain.parse().map_err(|_| {
        anyhow!(
            "verifySignInfo.chainId requires a numeric chain id, got `{}`",
            resolved_chain
        )
    })?;

    // 3. Build the `signMsg` (Phase 1 U-pegged template) and sign it with
    //    the session ed25519 seed.
    let intent_str = trader_mode::build_intent(BuildIntentArgs {
        chain_id: chain_id_long,
        recipient: &user_wallet_address,
        from_token: &args.from_token,
        to_token: &args.to_token,
        from_amount_raw: &from_amount_raw,
        created_at: &created_at,
        expired_at: &expired_at,
        timestamp_ms: now_ms,
    });
    let signature = trader_mode::sign_intent(&intent_str, &resolved_chain, &session.seed_b64)?;

    let verify_sign_info = VerifySignInfo {
        account_id: session.account_id.clone(),
        address: user_wallet_address.clone(),
        chain_id: chain_id_long,
        sign_msg: intent_str,
        signature,
        session_cert: session.session_cert.clone(),
        tee_id: session.tee_id.clone(),
    };

    let req = CreateOrderReq {
        chain_id: resolved_chain.clone(),
        user_wallet_address: user_wallet_address.clone(),
        rule,
        preset,
        strategy_type: args.r#type,
        strategy_direction: dir,
        verify_sign_info,
        expire_time: Some(expire_time_ms.to_string()),
        service_fee_info: None,
        // 0 swap / 1 meme / 2 market_condition / 3 advancedMode
        source_type: Some(2),
        estimate_gas_fee: None,
        referrer_address: None,
    };

    let activate_ctx = ActivateCtx {
        account_id: session.account_id.clone(),
        session_cert: session.session_cert.clone(),
        session_seed_b64: session.seed_b64.clone(),
        expire_ms_from_now: ACTIVATE_DEFAULT_TTL_MS,
    };

    // 60018 UpgradeRequired → SD-A → retry once. Inlined here because
    // `client` is borrowed `&mut`, which conflicts with the `'static`
    // BoxFuture signature of `trader_mode::retry_on_upgrade`. The semantics
    // (single-retry, no activation on other errors, no retry on activation
    // failure) must stay in sync with `trader_mode::retry_on_upgrade` and
    // its unit tests — treat that helper as the spec.
    let order = match api::create_order(&mut client, &req).await {
        Err(e) if is_upgrade_required(&e) => {
            trader_mode::activate(&mut client, &activate_ctx).await?;
            api::create_order(&mut client, &req).await?
        }
        Err(e) => return Err(e),
        Ok(o) => o,
    };

    let label = status_label(order.status);
    if args.format == "json" {
        let payload = json!({
            "orderId": order.order_id,
            "status": order.status,
            "statusLabel": label,
            "estimatedWaitTime": order.estimated_wait_time,
            "eventCursor": order.event_cursor,
        });
        output::success(payload);
    } else {
        println!(
            "{}",
            trader_mode::format_create_followup(
                &order.order_id,
                &label,
                order.estimated_wait_time.unwrap_or(0),
            )
        );
    }
    Ok(())
}

/// Resolve a token's `decimals` via the existing `/api/v6/dex/market/token/basic-info`
/// endpoint (wrapped by `token::fetch_info`). This is the CLI equivalent of the
/// client-side `OKWSecurityBridge shiftValue:shift:` helper, but kept fully
/// HTTP-driven so no native SDK bridge is required.
async fn fetch_token_decimals(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<u32> {
    let resp = token::fetch_info(client, address, chain_index)
        .await
        .context("token info HTTP call failed")?;
    // `ApiClient` already strips the `{code, msg, data}` envelope, so `resp`
    // is the inner `data` payload — `[{...}]`.
    let item = resp.get(0).ok_or_else(|| {
        anyhow!(
            "token info response empty — got: {}",
            serde_json::to_string(&resp).unwrap_or_default()
        )
    })?;
    let decimal_str = item.get("decimal").and_then(|d| d.as_str()).ok_or_else(|| {
        anyhow!(
            "token info item missing `decimal` — got: {}",
            serde_json::to_string(item).unwrap_or_default()
        )
    })?;
    decimal_str
        .parse::<u32>()
        .map_err(|e| anyhow!("token decimal `{decimal_str}` is not a u32: {e}"))
}

/// Convert a CLI percent string (e.g. `"15"`) into the decimal-fraction
/// string the BE expects in `slippageValue` (e.g. `"0.15"`). Falls back to
/// the raw input on parse failure so a malformed user value still surfaces
/// as a BE-side validation error rather than a silent zero.
fn percent_to_decimal(percent: &str) -> String {
    // Strip optional trailing '%' so users / Agent can pass "20%" or "20".
    let cleaned = percent.trim().trim_end_matches('%').trim();
    match cleaned.parse::<f64>() {
        Ok(v) => format!("{}", v / 100.0),
        Err(_) => percent.to_string(),
    }
}

/// Build the limit-order preset envelope.
///
/// Direction-aware key naming: BE expects `buyPreset` for BUY direction
/// orders (BUY_DIP / CHASE_HIGH) and `sellPreset` for SELL direction
/// orders (TAKE_PROFIT / STOP_LOSS) — same inner shape, different outer key.
/// (Confirmed with BE 2026-05-07.) ALL direction (-1) defaults to buyPreset.
///
/// Field shape comes from BE-supplied reference (see preset payload in
/// conversation log / `.claude/strategyTrading/log/...createOrder*.json`):
/// - `presetType: 1` — selects the structured preset path the BE expects
///   for limit orders (Phase 1).
/// - `slippageType: 2` + `slippageLevel: 4` (custom-tier) — fixed for limit
///   orders today; user controls the magnitude through `--slippage`.
/// - `slippageValue` — decimal fraction (`"0.15"` for 15%). The CLI flag is
///   in percent units to match `swap`; we divide by 100 here.
/// - `dynamicMaxSlippageValue: null` — BE-controlled cap; CLI does not set.
/// - `routerModeType` — tri-state from CLI flags:
///   - flag absent → `1` (BE default; CLI passes through, no opt-in)
///   - `--mev-protection` → `2` (MEV protection ON)
///   - `--no-mev-protection` → `3` (MEV protection OFF)
/// - `limitOrderFeeLevel: 2` — fixed (BE-confirmed 2026-05-08). The peer
///   field `limitOrderFeeValue` is **not sent** — BE derives it server-side
///   from `limitOrderFeeLevel`.
fn build_default_preset(
    slippage_percent: &str,
    mev_choice: Option<bool>,
    direction: i32,
) -> serde_json::Value {
    let router_mode = match mev_choice {
        None => ROUTER_MODE_DEFAULT,
        Some(true) => ROUTER_MODE_MEV_ON,
        Some(false) => ROUTER_MODE_MEV_OFF,
    };
    let slippage_decimal = percent_to_decimal(slippage_percent);
    let inner = json!({
        "slippageType": 2,
        "slippageLevel": 4,
        "slippageValue": slippage_decimal,
        "dynamicMaxSlippageValue": serde_json::Value::Null,
        "routerModeType": router_mode,
        "limitOrderFeeLevel": DEFAULT_LIMIT_ORDER_FEE_LEVEL,
    });
    // BE preset shape: { presetType, name?, buyPreset?, sellPreset? }.
    // Fill the matching side based on direction (SELL → sellPreset, else buyPreset).
    // ALL (-1) defaults to buyPreset; can be revisited if BE adds a both-sides mode.
    let preset_key = if direction == direction::SELL {
        "sellPreset"
    } else {
        "buyPreset"
    };
    let mut outer = serde_json::Map::new();
    outer.insert("presetType".to_string(), serde_json::Value::from(1));
    outer.insert(preset_key.to_string(), inner);
    serde_json::Value::Object(outer)
}


// ════════════════════════════════════════════════════════════════════
// cancel
// ════════════════════════════════════════════════════════════════════

#[derive(Args, Debug)]
pub struct CancelArgs {
    /// Cancel a single order by id.
    #[arg(long, conflicts_with_all = ["order_ids", "all"])]
    pub order_id: Option<String>,

    /// Cancel a batch — comma-separated order ids.
    #[arg(long, conflicts_with_all = ["order_id", "all"])]
    pub order_ids: Option<String>,

    /// Cancel every active order on the active account.
    #[arg(long, conflicts_with_all = ["order_id", "order_ids"])]
    pub all: bool,

    /// Output mode (`human` default; `json` emits the parsed response).
    #[arg(long, default_value = "human")]
    pub format: String,
}

pub async fn cancel(ctx: &Context, args: CancelArgs) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let session = session::load()?;

    let req = build_cancel_request(&session.account_id, &args)?;
    let resp = api::cancel(&mut client, &req).await?;

    if args.format == "json" {
        output::success(json!({
            "updateNum": resp.update_num,
            "estimatedWaitTime": resp.estimated_wait_time,
        }));
    } else {
        let line = trader_mode::format_cancel_followup(
            resp.update_num,
            resp.estimated_wait_time,
        );
        println!("{line}");
    }
    Ok(())
}

fn build_cancel_request(account_id: &str, args: &CancelArgs) -> Result<CancelReq> {
    if args.all {
        return Ok(CancelReq {
            account_id: account_id.to_string(),
            order_ids: None,
            cancel_all: Some(true),
        });
    }
    if let Some(ids) = args.order_ids.as_ref() {
        let parsed: Vec<String> = ids
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if parsed.is_empty() {
            bail!("--order-ids parsed into an empty list");
        }
        return Ok(CancelReq {
            account_id: account_id.to_string(),
            order_ids: Some(parsed),
            cancel_all: Some(false),
        });
    }
    if let Some(id) = args.order_id.as_ref() {
        return Ok(CancelReq {
            account_id: account_id.to_string(),
            order_ids: Some(vec![id.clone()]),
            cancel_all: Some(false),
        });
    }
    Err(anyhow!(
        "must pass exactly one of --order-id, --order-ids, or --all"
    ))
}

// ════════════════════════════════════════════════════════════════════
// list
// ════════════════════════════════════════════════════════════════════

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Get a single order's full detail (uses GET openOrderDetail).
    #[arg(long)]
    pub order_id: Option<String>,

    /// Comma-separated statuses (`active,suspended,creating,...`) or integer values.
    #[arg(long)]
    pub status: Option<String>,

    /// Comma-separated chain ids (e.g. `1,501`).
    #[arg(long)]
    pub chain_id: Option<String>,

    /// Single token address to filter on. For multi-token queries, run `list`
    /// once per token (BE schema 2026-05-09: dropped multi-string `tokenAddressList`).
    #[arg(long)]
    pub token: Option<String>,

    /// Page size (BE default 100, max 100).
    #[arg(long)]
    pub limit: Option<i32>,

    /// Pagination cursor — pass the previous response's `nextCursor`.
    #[arg(long)]
    pub cursor: Option<String>,

    /// Strategy mode for openOrderDetail (default 7 = U-pegged Phase 1).
    #[arg(long, default_value_t = 7)]
    pub strategy_mode: i32,

    /// Output mode.
    #[arg(long, default_value = "human")]
    pub format: String,
}

pub async fn list(ctx: &Context, args: ListArgs) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let session = session::load()?;

    if let Some(id) = args.order_id.as_ref() {
        let order =
            api::open_order_detail(&mut client, &session.account_id, id, args.strategy_mode)
                .await?;
        return print_orders(&[order], None, &args.format);
    }

    // Resolve every `--chain-id` entry, then validate against the strategy
    // whitelist (rejects polygon / optimism / linea / etc. before BE call).
    let chain_id_list = match csv_to_strings(args.chain_id.as_deref()) {
        None => None,
        Some(raw_list) => {
            let mut resolved = Vec::with_capacity(raw_list.len());
            for raw in raw_list {
                let idx = crate::chains::resolve_chain(&raw);
                supported_chains::ensure_strategy_chain(&idx, &raw)?;
                resolved.push(idx);
            }
            Some(resolved)
        }
    };

    // `--token` accepts a single token address (BE schema 2026-05-09 — see
    // types.rs::ListOrdersReq.token_address). For multi-token queries,
    // the agent should call `list` once per token.
    let token_address = args
        .token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.contains(',') {
                bail!(
                    "--token now accepts only a single token address (BE schema 2026-05-09). \
                     For multiple tokens, run `list` once per token."
                );
            }
            Ok::<String, anyhow::Error>(s.to_string())
        })
        .transpose()?;

    let req = ListOrdersReq {
        account_id: session.account_id.clone(),
        wallet_address_list: collect_wallet_addresses(&session),
        chain_id_list,
        order_status_list: parse_status_filter(args.status.as_deref())
            .or_else(default_non_terminal_status_list),
        order_type_list: None,
        id_list: None,
        token_address,
        limit: args.limit,
        cursor: args.cursor,
    };
    let resp: ListOrdersResp = api::get_open_order(&mut client, &req).await?;
    print_orders(&resp.list, resp.cursor.as_deref(), &args.format)
}

fn print_orders(
    list: &[OrderListResp],
    next_cursor: Option<&str>,
    format: &str,
) -> Result<()> {
    if format == "json" {
        let mut serialised: Vec<serde_json::Value> = Vec::with_capacity(list.len());
        for o in list {
            let mut v = serde_json::to_value(o)
                .context("serialise OrderListResp for JSON output")?;
            if let Some(s) = v.get("status").and_then(|s| s.as_i64()) {
                v["statusLabel"] = json!(status_label(s as i32));
            }
            serialised.push(v);
        }
        output::success(json!({
            "list": serialised,
            "nextCursor": next_cursor,
        }));
        return Ok(());
    }

    if list.is_empty() {
        println!("No orders found.");
        return Ok(());
    }
    for o in list {
        println!(
            "id={}  status={} ({})  chain={}  fromAmount={}  triggerInfo={}",
            o.order_id,
            o.status,
            status_label(o.status),
            o.chain_id.as_deref().unwrap_or("?"),
            o.from_token
                .as_ref()
                .and_then(|v| v.get("amount"))
                .and_then(|v| v.as_str())
                .unwrap_or("?"),
            o.trigger_info
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".into()),
        );
    }
    if let Some(cursor) = next_cursor {
        if !cursor.is_empty() {
            println!("\nNext page cursor: {cursor}");
        }
    }
    Ok(())
}

fn collect_wallet_addresses(s: &session::WalletSession) -> Vec<String> {
    let mut v = Vec::new();
    if !s.evm_address.is_empty() {
        v.push(s.evm_address.clone());
    }
    if !s.sol_address.is_empty() {
        v.push(s.sol_address.clone());
    }
    v
}

fn csv_to_strings(s: Option<&str>) -> Option<Vec<String>> {
    s.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    })
    .filter(|v: &Vec<String>| !v.is_empty())
}

/// Default `orderStatusList` when the user did not pass `--status`.
/// Returns the 5 non-terminal codes per BE: CANCELLING(-3), TRADING(0),
/// CREATING(2), ACTIVE(3), SUSPENDED(4). To see terminal orders
/// (cancelled / failed / expired / completed) the user must pass
/// `--status` explicitly. SPEEDING_UP (-4) was removed 2026-05-08.
fn default_non_terminal_status_list() -> Option<Vec<i32>> {
    Some(vec![
        OrderStatus::Cancelling as i32, // -3
        OrderStatus::Trading as i32,    //  0
        OrderStatus::Creating as i32,   //  2
        OrderStatus::Active as i32,     //  3
        OrderStatus::Suspended as i32,  //  4
    ])
}

fn parse_status_filter(s: Option<&str>) -> Option<Vec<i32>> {
    let parts = csv_to_strings(s)?;
    let mut out = Vec::with_capacity(parts.len());
    for p in parts {
        if let Ok(n) = p.parse::<i32>() {
            out.push(n);
            continue;
        }
        if let Some(n) = string_to_status(&p) {
            out.push(n);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn string_to_status(label: &str) -> Option<i32> {
    let normalized = label.to_ascii_lowercase().replace('-', "_");
    let enums = [
        OrderStatus::Expired,
        OrderStatus::Cancelling,
        OrderStatus::Cancelled,
        OrderStatus::Failed,
        OrderStatus::Trading,
        OrderStatus::Completed,
        OrderStatus::Creating,
        OrderStatus::Active,
        OrderStatus::Suspended,
    ];
    enums
        .into_iter()
        .find(|s| s.as_str() == normalized.as_str())
        .map(|s| s as i32)
}

// ════════════════════════════════════════════════════════════════════
// resume
// ════════════════════════════════════════════════════════════════════

#[derive(Args, Debug)]
pub struct ResumeArgs {
    /// Comma-separated order ids to resume. Omit to auto-pick all
    /// SUSPENDED + canResume orders on the active wallet.
    #[arg(long)]
    pub order_ids: Option<String>,

    /// Output mode.
    #[arg(long, default_value = "human")]
    pub format: String,
}

pub async fn resume(ctx: &Context, args: ResumeArgs) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let session = session::load()?;

    let order_ids = if let Some(ids) = args.order_ids.as_ref() {
        ids.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
    } else {
        discover_resumable(&mut client, &session).await?
    };

    if order_ids.is_empty() {
        if args.format == "json" {
            let empty: Vec<String> = Vec::new();
            output::success(json!({
                "successIds": empty,
                "failIds": empty,
                "note": "no resumable orders found",
            }));
        } else {
            println!("No suspended orders eligible for resume.");
        }
        return Ok(());
    }

    let activate_ctx = ActivateCtx {
        account_id: session.account_id.clone(),
        session_cert: session.session_cert.clone(),
        session_seed_b64: session.seed_b64.clone(),
        expire_ms_from_now: ACTIVATE_DEFAULT_TTL_MS,
    };

    // BE only requires `accountId` + `orderIds` for reactivate — no
    // signature is sent (the reactivate path does not verify a signature).
    let req = ReactivateReq {
        account_id: session.account_id.clone(),
        order_ids: order_ids.clone(),
    };

    // 60018 UpgradeRequired → SD-A → retry once. Same single-retry contract
    // as `create_limit`; see the comment there. The spec lives in
    // `trader_mode::retry_on_upgrade` and its unit tests.
    let resp = match api::reactivate(&mut client, &req).await {
        Err(e) if is_upgrade_required(&e) => {
            trader_mode::activate(&mut client, &activate_ctx).await?;
            api::reactivate(&mut client, &req).await?
        }
        Err(e) => return Err(e),
        Ok(r) => r,
    };

    if args.format == "json" {
        output::success(json!({
            "successIds": resp.success_ids,
            "failIds": resp.fail_ids,
        }));
    } else {
        println!(
            "Resume submitted: {} succeeded, {} failed.",
            resp.success_ids.len(),
            resp.fail_ids.len()
        );
        if !resp.fail_ids.is_empty() {
            println!("  failed: {:?}", resp.fail_ids);
        }
        if !resp.success_ids.is_empty() {
            println!(
                "  Some orders may execute immediately if their trigger condition is already met. Use `strategy list` to confirm."
            );
        }
    }
    Ok(())
}

/// Run getOpenOrder filtered to status=SUSPENDED, then keep only orders
/// whose `canResume` flag is true.
async fn discover_resumable(
    client: &mut ApiClient,
    s: &session::WalletSession,
) -> Result<Vec<String>> {
    let wallets = collect_wallet_addresses(s);
    if wallets.is_empty() {
        bail!("active account has no addresses to query");
    }
    let req = ListOrdersReq {
        account_id: s.account_id.clone(),
        wallet_address_list: wallets,
        chain_id_list: None,
        order_status_list: Some(vec![OrderStatus::Suspended as i32]),
        order_type_list: None,
        id_list: None,
        token_address: None,
        limit: Some(100),
        cursor: None,
    };
    let resp = api::get_open_order(client, &req).await?;
    Ok(resp
        .list
        .into_iter()
        .filter(|o| o.can_resume.unwrap_or(false))
        .map(|o| o.order_id)
        .collect())
}

// ════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── cancel ───────────────────────────────────────────────────

    fn cancel_args() -> CancelArgs {
        CancelArgs {
            order_id: None,
            order_ids: None,
            all: false,
            format: "human".into(),
        }
    }

    #[test]
    fn build_cancel_all_sets_cancel_all_true() {
        let mut a = cancel_args();
        a.all = true;
        let req = build_cancel_request("acc-1", &a).unwrap();
        assert_eq!(req.account_id, "acc-1");
        assert_eq!(req.cancel_all, Some(true));
        assert!(req.order_ids.is_none());
    }

    #[test]
    fn build_cancel_single_id() {
        let mut a = cancel_args();
        a.order_id = Some("ord-1".into());
        let req = build_cancel_request("acc-1", &a).unwrap();
        assert_eq!(req.order_ids.as_deref(), Some(&["ord-1".to_string()][..]));
        assert_eq!(req.cancel_all, Some(false));
    }

    #[test]
    fn build_cancel_csv_splits_and_trims() {
        let mut a = cancel_args();
        a.order_ids = Some("a, b ,,c ".into());
        let req = build_cancel_request("acc-1", &a).unwrap();
        assert_eq!(
            req.order_ids.as_deref(),
            Some(&["a".to_string(), "b".to_string(), "c".to_string()][..])
        );
    }

    #[test]
    fn build_cancel_rejects_no_input() {
        let req = build_cancel_request("acc-1", &cancel_args());
        assert!(req.is_err(), "must require one of the three flags");
    }

    #[test]
    fn build_cancel_rejects_empty_csv() {
        let mut a = cancel_args();
        a.order_ids = Some(",, ,".into());
        let req = build_cancel_request("acc-1", &a);
        assert!(req.is_err());
    }

    // ── list helpers ─────────────────────────────────────────────

    #[test]
    fn parse_status_filter_handles_mixed_int_and_string() {
        let v = parse_status_filter(Some("active, 4, suspended,1")).unwrap();
        assert_eq!(v, vec![3, 4, 4, 1]);
    }

    #[test]
    fn parse_status_filter_returns_none_for_blank() {
        assert!(parse_status_filter(None).is_none());
        assert!(parse_status_filter(Some("")).is_none());
        assert!(parse_status_filter(Some(" , ,")).is_none());
    }

    #[test]
    fn parse_status_filter_skips_unknown_strings_silently() {
        let v = parse_status_filter(Some("garbage,active")).unwrap();
        assert_eq!(v, vec![3]);
    }

    #[test]
    fn csv_to_strings_normalises() {
        assert_eq!(csv_to_strings(None), None);
        assert_eq!(csv_to_strings(Some(" ,, ")), None);
        assert_eq!(
            csv_to_strings(Some("a, b ,c ")),
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
    }

    #[test]
    fn string_to_status_round_trip() {
        assert_eq!(string_to_status("active"), Some(3));
        assert_eq!(string_to_status("ACTIVE"), Some(3));
        assert_eq!(string_to_status("foo"), None);
        // SPEEDING_UP removed 2026-05-08 — string lookup must NOT resolve it.
        assert_eq!(string_to_status("speeding-up"), None);
        assert_eq!(string_to_status("speeding_up"), None);
    }

    // ── create-limit arg parsing ─────────────────────────────────

    #[test]
    fn parse_strategy_type_accepts_documented_aliases() {
        assert_eq!(parse_strategy_type("buy_dip"), Ok(strategy_type::BUY_DIP));
        assert_eq!(parse_strategy_type("buy-dip"), Ok(strategy_type::BUY_DIP));
        assert_eq!(
            parse_strategy_type("TAKE_PROFIT"),
            Ok(strategy_type::TAKE_PROFIT)
        );
        assert_eq!(parse_strategy_type("stop_loss"), Ok(strategy_type::STOP_LOSS));
        assert_eq!(
            parse_strategy_type("chase_high"),
            Ok(strategy_type::CHASE_HIGH)
        );
    }

    #[test]
    fn parse_strategy_type_rejects_unknown() {
        assert!(parse_strategy_type("hodl").is_err());
    }

    #[test]
    fn default_direction_matches_strategy_intent() {
        assert_eq!(default_direction(strategy_type::BUY_DIP), direction::BUY);
        assert_eq!(default_direction(strategy_type::CHASE_HIGH), direction::BUY);
        assert_eq!(default_direction(strategy_type::TAKE_PROFIT), direction::SELL);
        assert_eq!(default_direction(strategy_type::STOP_LOSS), direction::SELL);
    }

    #[test]
    fn parse_direction_overrides_default_when_explicit() {
        assert_eq!(
            parse_direction(Some("buy"), strategy_type::TAKE_PROFIT).unwrap(),
            direction::BUY
        );
        assert_eq!(
            parse_direction(None, strategy_type::TAKE_PROFIT).unwrap(),
            direction::SELL
        );
        assert_eq!(
            parse_direction(Some("all"), strategy_type::BUY_DIP).unwrap(),
            direction::ALL
        );
    }

    #[test]
    fn parse_direction_rejects_unknown() {
        assert!(parse_direction(Some("up"), strategy_type::BUY_DIP).is_err());
    }

    #[test]
    fn build_default_preset_converts_percent_to_decimal() {
        // CLI passes "25" (= 25%); BE wants "0.25" on the wire.
        let v = build_default_preset("25", None, direction::BUY);
        assert_eq!(v["buyPreset"]["slippageValue"], serde_json::json!("0.25"));
    }

    #[test]
    fn build_default_preset_default_15_percent_to_15hundreths() {
        // PRD §5.2 default `15` → `"0.15"` on the wire.
        let v = build_default_preset(DEFAULT_SLIPPAGE_VALUE, None, direction::BUY);
        assert_eq!(v["buyPreset"]["slippageValue"], serde_json::json!("0.15"));
    }

    #[test]
    fn percent_to_decimal_passes_through_unparseable_input() {
        // Malformed input is forwarded as-is so BE returns a real validation
        // error instead of a silent zero.
        assert_eq!(percent_to_decimal("nope"), "nope");
    }

    #[test]
    fn percent_to_decimal_strips_trailing_percent_sign() {
        // Agent / user may type "20%" — strip the suffix before parsing.
        assert_eq!(percent_to_decimal("20%"), "0.2");
        assert_eq!(percent_to_decimal("15%"), "0.15");
        assert_eq!(percent_to_decimal(" 25 % "), "0.25");
    }

    #[test]
    fn build_default_preset_router_mode_default_when_no_flag() {
        // No flag passed → tri-state default = 1 (BE default; CLI does not opt in/out).
        let v = build_default_preset("15", None, direction::BUY);
        assert_eq!(v["buyPreset"]["routerModeType"], serde_json::json!(1));
    }

    #[test]
    fn build_default_preset_router_mode_on_when_mev_enabled() {
        // --mev-protection → 2.
        let v = build_default_preset("15", Some(true), direction::BUY);
        assert_eq!(v["buyPreset"]["routerModeType"], serde_json::json!(2));
    }

    #[test]
    fn build_default_preset_router_mode_off_when_mev_explicitly_disabled() {
        // --no-mev-protection → 3.
        let v = build_default_preset("15", Some(false), direction::BUY);
        assert_eq!(v["buyPreset"]["routerModeType"], serde_json::json!(3));
    }

    #[test]
    fn build_default_preset_uses_typed_dynamic_levels() {
        // BE expects presetType=1, slippageType=2, slippageLevel=4 (custom-tier).
        // dynamicMaxSlippageValue must be JSON null, not the string "0".
        let v = build_default_preset("15", None, direction::BUY);
        assert_eq!(v["presetType"], serde_json::json!(1));
        assert_eq!(v["buyPreset"]["slippageType"], serde_json::json!(2));
        assert_eq!(v["buyPreset"]["slippageLevel"], serde_json::json!(4));
        assert!(v["buyPreset"]["dynamicMaxSlippageValue"].is_null());
        assert_eq!(
            v["buyPreset"]["limitOrderFeeLevel"],
            serde_json::json!(DEFAULT_LIMIT_ORDER_FEE_LEVEL)
        );
        // limitOrderFeeValue must NOT be sent — BE derives it from level.
        assert!(
            v["buyPreset"].get("limitOrderFeeValue").is_none(),
            "limitOrderFeeValue should not be present (BE derives from level)"
        );
    }

    #[test]
    fn build_default_preset_sell_direction_uses_sell_preset_key() {
        // SELL direction must produce `sellPreset` only; `buyPreset` must
        // not appear.
        let v = build_default_preset("15", None, direction::SELL);
        assert_eq!(v["presetType"], serde_json::json!(1));
        assert!(v.get("buyPreset").is_none(), "BUY preset must NOT be present for SELL");
        assert_eq!(v["sellPreset"]["slippageType"], serde_json::json!(2));
        assert_eq!(v["sellPreset"]["slippageValue"], serde_json::json!("0.15"));
    }

    #[test]
    fn build_default_preset_all_direction_falls_back_to_buy_preset() {
        // ALL (-1) defaults to `buyPreset`; `sellPreset` must not appear.
        let v = build_default_preset("15", None, direction::ALL);
        assert!(v.get("sellPreset").is_none());
        assert_eq!(v["buyPreset"]["slippageValue"], serde_json::json!("0.15"));
    }
}
