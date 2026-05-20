//! 4 P0 subcommand handlers: create-limit / cancel / list / resume.

use anyhow::{anyhow, bail, Context as _, Result};
use chrono::{SecondsFormat, Utc};
use clap::{Args, ValueEnum};
use serde_json::json;

use crate::client::ApiClient;
use crate::commands::token;
use crate::commands::Context;
use crate::output;
use crate::token_alias;

use super::api;
use super::session;
use super::status::{execution_event_for, is_upgrade_required, status_label, OrderStatus};
use super::supported_chains;
use super::trader_mode::{self, ActivateCtx, BuildIntentArgs};
use super::types::{
    direction, strategy_type, CancelReq, CreateOrderReq, ListOrdersReq, ListOrdersResp,
    OrderListResp, ReactivateReq, Rule, VerifySignInfo,
};

const DEFAULT_EXPIRES_SECS: i64 = 7 * 24 * 60 * 60; // 7 days
/// `--slippage` default in percent. BE wire is decimal (15 → "0.15").
const DEFAULT_SLIPPAGE_VALUE: &str = "15";
/// BE `routerModeType`: 1 = default (BE picks), 2 = MEV ON, 3 = MEV OFF.
const ROUTER_MODE_DEFAULT: i64 = 1;
const ROUTER_MODE_MEV_ON: i64 = 2;
const ROUTER_MODE_MEV_OFF: i64 = 3;
/// BE-confirmed 2026-05-08: fee derives from `limitOrderFeeLevel`; `limitOrderFeeValue` not sent.
const DEFAULT_LIMIT_ORDER_FEE_LEVEL: i64 = 2;
/// SD-A activation TTL: always 30 days.
const ACTIVATE_DEFAULT_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// `sourceType` value for Agentic-Wallet origin (BE-confirmed 2026-05-12).
const SOURCE_TYPE_AGENTIC: i32 = 4;

// ── create-limit ──

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

    /// USD trigger price. Required for strategy type derivation.
    #[arg(long)]
    pub trigger_price: String,

    /// Slippage tolerance in percent (e.g. `20` or `20%` → wire `"0.20"`). Default 15.
    #[arg(long)]
    pub slippage: Option<String>,

    /// MEV protection: `on` / `off` / `default` (BE picks).
    #[arg(long, value_enum, default_value_t = MevChoice::Default)]
    pub mev_protection: MevChoice,

    /// Trade direction `buy` / `sell` (required). Strategy type is derived
    /// from direction + trigger vs current price — no explicit `--type`.
    #[arg(long, value_parser = parse_direction_value)]
    pub direction: i32,

    /// Current USD price of the comparison token (to-token for `buy`,
    /// from-token for `sell`). Optional — CLI fetches via `market price`
    /// when omitted. Pass it to save one HTTP round-trip.
    #[arg(long)]
    pub current_price: Option<String>,
}

fn parse_direction_value(raw: &str) -> Result<i32, String> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Ok(direction::BUY),
        "sell" => Ok(direction::SELL),
        other => Err(format!(
            "unknown direction `{other}` — expected `buy` or `sell`"
        )),
    }
}

/// Tri-state MEV preset (clap parses `on` / `off` / `default` lowercase).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum MevChoice {
    On,
    Off,
    #[default]
    Default,
}

impl MevChoice {
    /// Map to `build_default_preset`'s `Option<bool>` contract.
    fn to_opt_bool(self) -> Option<bool> {
        match self {
            MevChoice::On => Some(true),
            MevChoice::Off => Some(false),
            MevChoice::Default => None,
        }
    }
}

/// Phase 1 strategy type derivation. Equality goes to the aggressive side
/// (`trigger == current` → CHASE_HIGH for buy, STOP_LOSS for sell).
///
/// | dir  | trigger vs current | result      |
/// |------|--------------------|-------------|
/// | buy  | <                  | BUY_DIP     |
/// | buy  | ≥                  | CHASE_HIGH  |
/// | sell | >                  | TAKE_PROFIT |
/// | sell | ≤                  | STOP_LOSS   |
fn derive_strategy_type(
    direction: i32,
    trigger_price: f64,
    current_price: f64,
) -> Result<i32> {
    match direction {
        direction::BUY => Ok(if trigger_price < current_price {
            strategy_type::BUY_DIP
        } else {
            strategy_type::CHASE_HIGH
        }),
        direction::SELL => Ok(if trigger_price > current_price {
            strategy_type::TAKE_PROFIT
        } else {
            strategy_type::STOP_LOSS
        }),
        other => bail!(
            "unsupported direction integer {other}; expected BUY ({}) or SELL ({})",
            direction::BUY,
            direction::SELL
        ),
    }
}

/// Fetch USD price via `market::fetch_price`, parse `data[0].price` → f64.
async fn fetch_token_price(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<f64> {
    let resp = crate::commands::market::fetch_price(client, address, chain_index)
        .await
        .context("market price HTTP call failed")?;
    let item = resp.get(0).ok_or_else(|| {
        anyhow!(
            "market price response empty — got: {}",
            serde_json::to_string(&resp).unwrap_or_default()
        )
    })?;
    let price_str = item.get("price").and_then(|v| v.as_str()).ok_or_else(|| {
        anyhow!(
            "market price item missing `price` — got: {}",
            serde_json::to_string(item).unwrap_or_default()
        )
    })?;
    price_str
        .parse::<f64>()
        .map_err(|e| anyhow!("market price `{price_str}` is not a number: {e}"))
}

pub async fn create_limit(ctx: &Context, args: CreateLimitArgs) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let session = session::load()?;

    // Resolve alias → chainIndex, then whitelist-check pre-flight to give
    // a friendly error instead of round-tripping BE for 10106.
    let resolved_chain = crate::chains::resolve_chain(&args.chain_id);
    supported_chains::ensure_strategy_chain(&resolved_chain, &args.chain_id)?;
    let user_wallet_address = session.wallet_address_for(&resolved_chain).to_string();
    if user_wallet_address.is_empty() {
        bail!(
            "no wallet address for chain `{}` — login with the right chain enabled first",
            resolved_chain
        );
    }

    // Alias → CA + chain-aware format check (shared with swap / wallet send).
    // Catches `--to-token usdc` / `--from-token aaa` before they leak to BE.
    // `label` is bare (no `--` prefix) — validate_address_for_chain adds it.
    let from_token =
        token_alias::resolve_and_validate(&resolved_chain, &args.from_token, "from-token")?;
    let to_token =
        token_alias::resolve_and_validate(&resolved_chain, &args.to_token, "to-token")?;

    let dir = args.direction;

    let trigger_price_num: f64 = args.trigger_price.parse().map_err(|e| {
        anyhow!("--trigger-price `{}` is not a number: {e}", args.trigger_price)
    })?;
    if trigger_price_num <= 0.0 || !trigger_price_num.is_finite() {
        bail!(
            "--trigger-price must be a positive finite number, got `{}`",
            args.trigger_price
        );
    }

    // Buy compares against to-token price, sell against from-token.
    let price_query_token = match dir {
        direction::BUY => &to_token,
        direction::SELL => &from_token,
        _ => unreachable!("clap restricts --direction to buy/sell"),
    };
    let current_price_num: f64 = match args.current_price.as_deref() {
        Some(s) => {
            let v: f64 = s
                .parse()
                .map_err(|e| anyhow!("--current-price `{s}` is not a number: {e}"))?;
            if v <= 0.0 || !v.is_finite() {
                bail!("--current-price must be a positive finite number, got `{s}`");
            }
            v
        }
        None => fetch_token_price(&mut client, price_query_token, &resolved_chain)
            .await
            .with_context(|| {
                format!(
                    "fetch current price for {} on chain {}",
                    price_query_token, resolved_chain
                )
            })?,
    };
    let strat = derive_strategy_type(dir, trigger_price_num, current_price_num)?;

    // Raw amount used only in signMsg "From Amount(precision adjusted)";
    // rule.fromAmount stays human-readable (BE contract 2026-05-07).
    let from_decimals = fetch_token_decimals(&mut client, &from_token, &resolved_chain)
        .await
        .with_context(|| {
            format!(
                "fetch decimals for fromToken `{}` on chain `{}`",
                from_token, resolved_chain
            )
        })?;
    let from_amount_raw = trader_mode::shift_value(&args.amount, from_decimals)?;

    let rule = Rule {
        from_token_address: from_token.clone(),
        to_token_address: to_token.clone(),
        from_amount: args.amount.clone(),
        trigger_price: Some(args.trigger_price.clone()),
    };

    let slippage_raw = args.slippage.as_deref().unwrap_or(DEFAULT_SLIPPAGE_VALUE);
    crate::commands::swap::validate_slippage(slippage_raw)?;
    let preset = build_default_preset(slippage_raw, args.mev_protection.to_opt_bool(), dir);

    // 2. Build the time fields of the intent: Created At / Expired At /
    //    Timestamp.
    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let created_at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let expire_time_ms = now_ms.saturating_add(DEFAULT_EXPIRES_SECS.saturating_mul(1000));
    let expired_at = chrono::DateTime::<Utc>::from_timestamp_millis(expire_time_ms)
        .ok_or_else(|| anyhow!("expire_time {expire_time_ms} ms out of chrono range"))?
        .to_rfc3339_opts(SecondsFormat::Millis, true);

    // `verifySignInfo.chainId` is Long (numeric); reject non-numeric aliases.
    let chain_id_long: i64 = resolved_chain.parse().map_err(|_| {
        anyhow!(
            "verifySignInfo.chainId requires a numeric chain id, got `{}`",
            resolved_chain
        )
    })?;

    let intent_str = trader_mode::build_intent(BuildIntentArgs {
        chain_id: chain_id_long,
        recipient: &user_wallet_address,
        from_token: &from_token,
        to_token: &to_token,
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
        strategy_type: strat,
        strategy_direction: dir,
        verify_sign_info,
        expire_time: Some(expire_time_ms.to_string()),
        service_fee_info: None,
        source_type: Some(SOURCE_TYPE_AGENTIC),
        estimate_gas_fee: None,
        referrer_address: None,
    };

    let activate_ctx = ActivateCtx {
        account_id: session.account_id.clone(),
        session_cert: session.session_cert.clone(),
        session_seed_b64: session.seed_b64.clone(),
        expire_ms_from_now: ACTIVATE_DEFAULT_TTL_MS,
    };

    // 60018 → SD-A → retry once. Spec: `trader_mode::retry_on_upgrade` + tests.
    let order = match api::create_order(&mut client, &req).await {
        Err(e) if is_upgrade_required(&e) => {
            trader_mode::activate(&mut client, &activate_ctx).await?;
            api::create_order(&mut client, &req).await?
        }
        Err(e) => return Err(e),
        Ok(o) => o,
    };

    let label = status_label(order.status);
    output::success(json!({
        "orderId": order.order_id,
        "status": order.status,
        "statusLabel": label,
        "estimatedWaitTime": order.estimated_wait_time,
        "eventCursor": order.event_cursor,
    }));
    Ok(())
}

/// Fetch token `decimals` via `token::fetch_info` (basic-info endpoint).
async fn fetch_token_decimals(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<u32> {
    let resp = token::fetch_info(client, address, chain_index)
        .await
        .context("token info HTTP call failed")?;
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

/// CLI percent (`"15"` / `"20%"`) → BE decimal fraction (`"0.15"`).
/// Bad input falls through unchanged so BE rejects it explicitly.
fn percent_to_decimal(percent: &str) -> String {
    let cleaned = percent.trim().trim_end_matches('%').trim();
    match cleaned.parse::<f64>() {
        Ok(v) => format!("{}", v / 100.0),
        Err(_) => percent.to_string(),
    }
}

/// Build the BE limit-order preset. SELL → `sellPreset`, else `buyPreset`
/// (BE-confirmed 2026-05-07). Inner shape is identical.
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
    let inner = json!({
        "slippageType": 2,
        "slippageLevel": 4,
        "slippageValue": percent_to_decimal(slippage_percent),
        "dynamicMaxSlippageValue": serde_json::Value::Null,
        "routerModeType": router_mode,
        "limitOrderFeeLevel": DEFAULT_LIMIT_ORDER_FEE_LEVEL,
    });
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


// ── cancel ──

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
}

pub async fn cancel(ctx: &Context, args: CancelArgs) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let session = session::load()?;

    let req = build_cancel_request(&session.account_id, &args)?;
    let resp = api::cancel(&mut client, &req).await?;

    output::success(json!({
        "updateNum": resp.update_num,
        "estimatedWaitTime": resp.estimated_wait_time,
    }));
    Ok(())
}

/// BE expects orderId as Long (≤ 32 digits to stay clearly within i64); reject
/// non-numeric strings early so BE doesn't have to.
fn validate_order_id_numeric(id: &str, label: &str) -> Result<()> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("--{label} must not be empty");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        bail!("--{label} must be a numeric order id, got `{trimmed}`");
    }
    if trimmed.len() > 32 {
        bail!(
            "--{label} `{trimmed}` is too long ({} digits); order ids must be ≤ 32 digits",
            trimmed.len()
        );
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
        for id in &parsed {
            validate_order_id_numeric(id, "order-ids")?;
        }
        return Ok(CancelReq {
            account_id: account_id.to_string(),
            order_ids: Some(parsed),
            cancel_all: Some(false),
        });
    }
    if let Some(id) = args.order_id.as_ref() {
        validate_order_id_numeric(id, "order-id")?;
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

// ── list ──

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
}

pub async fn list(ctx: &Context, args: ListArgs) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let session = session::load()?;

    if let Some(id) = args.order_id.as_ref() {
        let order =
            api::open_order_detail(&mut client, &session.account_id, id, args.strategy_mode)
                .await?;
        return print_orders(&[order], None);
    }

    // Resolve + whitelist each `--chain-id` entry pre-flight.
    let chain_id_list = {
        let raw_list = csv_to_strings(args.chain_id.as_deref());
        if raw_list.is_empty() {
            None
        } else {
            let mut resolved = Vec::with_capacity(raw_list.len());
            for raw in raw_list {
                let idx = crate::chains::resolve_chain(&raw);
                supported_chains::ensure_strategy_chain(&idx, &raw)?;
                resolved.push(idx);
            }
            Some(resolved)
        }
    };

    // BE schema 2026-05-09: `tokenAddress` is single-valued. Agent must
    // call list once per token to filter multiple.
    let token_address = args
        .token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.contains(',') {
                bail!(
                    "--token accepts only a single address; run `list` once per token."
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
    print_orders(&resp.list, resp.cursor.as_deref())
}

fn print_orders(list: &[OrderListResp], next_cursor: Option<&str>) -> Result<()> {
    let mut serialised: Vec<serde_json::Value> = Vec::with_capacity(list.len());
    for o in list {
        let mut v = serde_json::to_value(o)
            .context("serialise OrderListResp for JSON output")?;
        if let Some(s) = v.get("status").and_then(|s| s.as_i64()) {
            v["statusLabel"] = json!(status_label(s as i32));
        }
        enrich_execution_history(&mut v);
        serialised.push(v);
    }
    output::success(json!({
        "list": serialised,
        "nextCursor": next_cursor,
    }));
    Ok(())
}

/// For each `executionHistoryList[].code` we recognise, inject the product-
/// authored `name` + `message` so the Agent can surface a user-facing string
/// without consulting a sidecar table. Unknown codes are left untouched —
/// whatever BE returned passes through verbatim.
fn enrich_execution_history(order: &mut serde_json::Value) {
    let Some(history) = order
        .get_mut("executionHistoryList")
        .and_then(|h| h.as_array_mut())
    else {
        return;
    };
    for entry in history.iter_mut() {
        let Some(code) = entry.get("code").and_then(|c| c.as_i64()) else {
            continue;
        };
        let Some(meta) = execution_event_for(code as i32) else {
            continue;
        };
        let Some(obj) = entry.as_object_mut() else {
            continue;
        };
        obj.insert("name".into(), json!(meta.name));
        obj.insert("message".into(), json!(meta.message));
        obj.insert("terminal".into(), json!(meta.is_terminal));
    }
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

/// CSV → trimmed non-empty parts. Empty / whitespace / all-commas input
/// returns `Vec::new()` — caller checks `.is_empty()` explicitly.
fn csv_to_strings(s: Option<&str>) -> Vec<String> {
    s.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// Default filter when `--status` is omitted: the 5 non-terminal codes
/// (Cancelling, Trading, Creating, Active, Suspended).
fn default_non_terminal_status_list() -> Option<Vec<i32>> {
    Some(vec![
        OrderStatus::Cancelling as i32,
        OrderStatus::Trading as i32,
        OrderStatus::Creating as i32,
        OrderStatus::Active as i32,
        OrderStatus::Suspended as i32,
    ])
}

fn parse_status_filter(s: Option<&str>) -> Option<Vec<i32>> {
    let parts = csv_to_strings(s);
    if parts.is_empty() {
        return None;
    }
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

// ── resume ──

#[derive(Args, Debug)]
pub struct ResumeArgs {
    /// Comma-separated order ids to resume. Omit to auto-pick all
    /// SUSPENDED + canResume orders on the active wallet.
    #[arg(long)]
    pub order_ids: Option<String>,
}

pub async fn resume(ctx: &Context, args: ResumeArgs) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let session = session::load()?;

    let order_ids = if let Some(ids) = args.order_ids.as_ref() {
        let parsed: Vec<String> = ids
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        for id in &parsed {
            validate_order_id_numeric(id, "order-ids")?;
        }
        parsed
    } else {
        discover_resumable(&mut client, &session).await?
    };

    if order_ids.is_empty() {
        let empty: Vec<String> = Vec::new();
        output::success(json!({
            "successIds": empty,
            "failIds": empty,
            "note": "no resumable orders found",
        }));
        return Ok(());
    }

    let activate_ctx = ActivateCtx {
        account_id: session.account_id.clone(),
        session_cert: session.session_cert.clone(),
        session_seed_b64: session.seed_b64.clone(),
        expire_ms_from_now: ACTIVATE_DEFAULT_TTL_MS,
    };

    // Reactivate is unsigned; BE only checks accountId + orderIds.
    let req = ReactivateReq {
        account_id: session.account_id.clone(),
        order_ids: order_ids.clone(),
    };

    // 60018 → SD-A → retry once. Spec: `trader_mode::retry_on_upgrade`.
    let resp = match api::reactivate(&mut client, &req).await {
        Err(e) if is_upgrade_required(&e) => {
            trader_mode::activate(&mut client, &activate_ctx).await?;
            api::reactivate(&mut client, &req).await?
        }
        Err(e) => return Err(e),
        Ok(r) => r,
    };

    output::success(json!({
        "successIds": resp.success_ids,
        "failIds": resp.fail_ids,
    }));
    Ok(())
}

/// List SUSPENDED orders, then keep only those with `canResume=true`.
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

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    // ── cancel ───────────────────────────────────────────────────

    fn cancel_args() -> CancelArgs {
        CancelArgs {
            order_id: None,
            order_ids: None,
            all: false,
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
        a.order_id = Some("17296046425729984".into());
        let req = build_cancel_request("acc-1", &a).unwrap();
        assert_eq!(
            req.order_ids.as_deref(),
            Some(&["17296046425729984".to_string()][..])
        );
        assert_eq!(req.cancel_all, Some(false));
    }

    #[test]
    fn build_cancel_csv_splits_and_trims() {
        let mut a = cancel_args();
        a.order_ids = Some("17296046425729984, 17296046425729985 ,,17296046425729986 ".into());
        let req = build_cancel_request("acc-1", &a).unwrap();
        assert_eq!(
            req.order_ids.as_deref(),
            Some(
                &[
                    "17296046425729984".to_string(),
                    "17296046425729985".to_string(),
                    "17296046425729986".to_string(),
                ][..]
            )
        );
    }

    #[test]
    fn build_cancel_rejects_non_numeric_id() {
        let mut a = cancel_args();
        a.order_id = Some("ord-1".into());
        assert!(build_cancel_request("acc-1", &a).is_err());
    }

    #[test]
    fn build_cancel_rejects_non_numeric_csv() {
        let mut a = cancel_args();
        a.order_ids = Some("17296046425729984,not-a-number".into());
        assert!(build_cancel_request("acc-1", &a).is_err());
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
        assert!(csv_to_strings(None).is_empty());
        assert!(csv_to_strings(Some(" ,, ")).is_empty());
        assert_eq!(
            csv_to_strings(Some("a, b ,c ")),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
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
    fn parse_direction_value_accepts_buy_sell_case_insensitive() {
        assert_eq!(parse_direction_value("buy"), Ok(direction::BUY));
        assert_eq!(parse_direction_value("BUY"), Ok(direction::BUY));
        assert_eq!(parse_direction_value("sell"), Ok(direction::SELL));
        assert_eq!(parse_direction_value("Sell"), Ok(direction::SELL));
    }

    #[test]
    fn parse_direction_value_rejects_all_and_unknown() {
        // `all` was supported by the legacy `--direction` flag but Phase 1's
        // CLI surface restricts to buy/sell only (strategy type derivation
        // has no entry for ALL).
        assert!(parse_direction_value("all").is_err());
        assert!(parse_direction_value("hodl").is_err());
        assert!(parse_direction_value("").is_err());
    }

    #[test]
    fn mev_choice_maps_to_opt_bool() {
        assert_eq!(MevChoice::On.to_opt_bool(), Some(true));
        assert_eq!(MevChoice::Off.to_opt_bool(), Some(false));
        assert_eq!(MevChoice::Default.to_opt_bool(), None);
    }

    // ── derive_strategy_type ─────────────────────────────────────

    #[test]
    fn derive_buy_below_current_is_buy_dip() {
        // trigger 0.10 < current 0.15 → BUY_DIP
        assert_eq!(
            derive_strategy_type(direction::BUY, 0.10, 0.15).unwrap(),
            strategy_type::BUY_DIP
        );
    }

    #[test]
    fn derive_buy_above_current_is_chase_high() {
        // trigger 0.20 > current 0.15 → CHASE_HIGH
        assert_eq!(
            derive_strategy_type(direction::BUY, 0.20, 0.15).unwrap(),
            strategy_type::CHASE_HIGH
        );
    }

    #[test]
    fn derive_buy_equal_to_current_folds_into_chase_high() {
        // trigger == current → aggressive side (CHASE_HIGH), per locked rule.
        assert_eq!(
            derive_strategy_type(direction::BUY, 0.15, 0.15).unwrap(),
            strategy_type::CHASE_HIGH
        );
    }

    #[test]
    fn derive_sell_above_current_is_take_profit() {
        // trigger 0.20 > current 0.15 → TAKE_PROFIT
        assert_eq!(
            derive_strategy_type(direction::SELL, 0.20, 0.15).unwrap(),
            strategy_type::TAKE_PROFIT
        );
    }

    #[test]
    fn derive_sell_below_current_is_stop_loss() {
        // trigger 0.10 < current 0.15 → STOP_LOSS
        assert_eq!(
            derive_strategy_type(direction::SELL, 0.10, 0.15).unwrap(),
            strategy_type::STOP_LOSS
        );
    }

    #[test]
    fn derive_sell_equal_to_current_folds_into_stop_loss() {
        // trigger == current → aggressive side (STOP_LOSS), per locked rule.
        assert_eq!(
            derive_strategy_type(direction::SELL, 0.15, 0.15).unwrap(),
            strategy_type::STOP_LOSS
        );
    }

    #[test]
    fn derive_unknown_direction_errors() {
        assert!(derive_strategy_type(direction::ALL, 0.10, 0.15).is_err());
        assert!(derive_strategy_type(99, 0.10, 0.15).is_err());
    }

    #[test]
    fn build_default_preset_converts_percent_to_decimal() {
        // CLI passes "25" (= 25%); BE wants "0.25" on the wire.
        let v = build_default_preset("25", None, direction::BUY);
        assert_eq!(v["buyPreset"]["slippageValue"], serde_json::json!("0.25"));
    }

    #[test]
    fn build_default_preset_default_15_percent_to_15hundreths() {
        // Default `15` → `"0.15"` on the wire.
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

    // ── enrich_execution_history ────────────────────────────────

    #[test]
    fn enrich_history_injects_name_message_terminal_for_known_codes() {
        let mut order = json!({
            "executionHistoryList": [
                { "code": 3016, "txHash": null },
                { "code": 0,    "txHash": "0xabc" },
            ]
        });
        super::enrich_execution_history(&mut order);
        let h = order["executionHistoryList"].as_array().unwrap();
        assert_eq!(h[0]["name"], "noLiquidty");
        assert_eq!(h[0]["message"], "No quote due to low liquidity");
        assert_eq!(h[0]["terminal"], false);
        // Existing BE fields untouched.
        assert!(h[0]["txHash"].is_null());

        assert_eq!(h[1]["name"], "tradeSuccessed");
        assert_eq!(h[1]["message"], "Trade successful");
        assert_eq!(h[1]["txHash"], "0xabc");
    }

    #[test]
    fn enrich_history_leaves_unknown_codes_untouched() {
        // Product-design rule: unknown code => pass through whatever BE
        // returned, including any BE-supplied msg field.
        let mut order = json!({
            "executionHistoryList": [
                { "code": 9999, "msg": "raw be string" }
            ]
        });
        super::enrich_execution_history(&mut order);
        let entry = &order["executionHistoryList"][0];
        assert!(entry.get("name").is_none());
        assert!(entry.get("message").is_none());
        assert!(entry.get("terminal").is_none());
        assert_eq!(entry["msg"], "raw be string");
    }

    #[test]
    fn enrich_history_skips_missing_or_non_array_history() {
        // No `executionHistoryList` key — no-op.
        let mut o1 = json!({ "orderId": "x" });
        super::enrich_execution_history(&mut o1);
        assert!(o1.get("executionHistoryList").is_none());

        // Field exists but is not an array — no-op (no panic).
        let mut o2 = json!({ "executionHistoryList": null });
        super::enrich_execution_history(&mut o2);
        assert!(o2["executionHistoryList"].is_null());
    }

    #[test]
    fn enrich_history_terminal_codes_are_flagged() {
        let mut order = json!({
            "executionHistoryList": [
                { "code": 3019 }, { "code": 3023 }, { "code": 3015 },
            ]
        });
        super::enrich_execution_history(&mut order);
        let h = order["executionHistoryList"].as_array().unwrap();
        assert_eq!(h[0]["terminal"], true);  // 3019 riskToken
        assert_eq!(h[1]["terminal"], true);  // 3023 orderExpired
        assert_eq!(h[2]["terminal"], false); // 3015 exceedSlippage retries
    }
}
