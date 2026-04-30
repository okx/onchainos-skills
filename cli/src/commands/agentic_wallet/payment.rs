use anyhow::{anyhow, bail, Context, Result};
use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::SolValue;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::Subcommand;
use serde_json::{json, Value};
use zeroize::Zeroize;
use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::agentic_wallet::common::is_valid_evm_address;
use crate::commands::agentic_wallet::payment_flow;
use crate::commands::payment::a2a_pay::{self, A2aPayCommand};
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::{keyring_store, wallet_store};

#[derive(Subcommand)]
pub enum PaymentCommand {
    /// Sign an x402 payment and return the payment proof
    X402Pay {
        /// JSON accepts array from the 402 response (decoded.accepts).
        /// The CLI selects the best scheme automatically
        /// (prefers "exact", falls back to "aggr_deferred", then first entry).
        #[arg(long)]
        accepts: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
    },
    /// Sign an EIP-3009 TransferWithAuthorization locally with a hex private key
    /// (reads EVM_PRIVATE_KEY env var). Accepts the same JSON accepts array as x402-pay;
    /// domain name/version are read from accepts[].extra.name / extra.version.
    Eip3009Sign {
        /// JSON accepts array from the 402 response (same format as x402-pay).
        /// domain name/version are extracted from the selected entry's `extra.name` / `extra.version`.
        #[arg(long)]
        accepts: String,
    },
    /// Manage the default payment asset used when the server offers multiple options.
    Default {
        #[command(subcommand)]
        action: DefaultAction,
    },
    /// A2A Pay — Buyer ↔ Seller charge flow (create / pay / status)
    #[command(name = "a2a-pay")]
    A2aPay {
        #[command(subcommand)]
        command: A2aPayCommand,
    },

    // ── MPP Commands ────────────────────────────────────────────────

    /// Sign an MPP Charge payment (EIP-3009 via TEE) or wrap a client-broadcast hash.
    /// Returns authorization_header for replaying the request.
    /// When challenge.methodDetails.feePayer == false, --tx-hash is required (hash mode).
    /// Splits are auto-detected from challenge.methodDetails.splits[] (max 10).
    MppCharge {
        /// Full WWW-Authenticate header value from 402 response
        #[arg(long)]
        challenge: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
        /// Optional: tx hash of a client-broadcast transferWithAuthorization (hash mode).
        /// Required when challenge.methodDetails.feePayer=false.
        #[arg(long)]
        tx_hash: Option<String>,
    },

    /// Open an MPP Session payment channel.
    /// - feePayer=true (default): TEE-sign EIP-3009 deposit + initial voucher, emit transaction payload.
    /// - feePayer=false: require --tx-hash for the open tx you broadcast; still TEE-sign initial voucher.
    MppSessionOpen {
        /// Full WWW-Authenticate header value from 402 response
        #[arg(long)]
        challenge: String,
        /// Deposit amount in atomic units (e.g. "1000000" for $1 with 6 decimals)
        #[arg(long)]
        deposit: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
        /// Optional: tx hash of the client-broadcast open tx (hash mode).
        /// Required when challenge.methodDetails.feePayer=false.
        #[arg(long)]
        tx_hash: Option<String>,
        /// Initial voucher cumulativeAmount (atomic units). Default "0" (no prepay).
        /// Takes priority over --prepay-first when both given.
        #[arg(long)]
        initial_cum: Option<String>,
        /// Pre-authorize one unit of payment at open time. Reads challenge.amount;
        /// if challenge.amount is "0" or missing, falls back silently to no prepay.
        /// Ignored if --initial-cum is provided.
        #[arg(long, default_value_t = false)]
        prepay_first: bool,
    },

    /// Sign an MPP Session voucher (EIP-712 cumulative amount via TEE), or
    /// reuse a previously-signed voucher's bytes (`--reuse-signature`) to
    /// spend remaining channel balance without invoking TEE.
    /// Returns authorization_header for replaying business requests.
    MppSessionVoucher {
        /// Full WWW-Authenticate challenge header (for credential echo)
        #[arg(long)]
        challenge: String,
        /// Channel ID from session open
        #[arg(long)]
        channel_id: String,
        /// Cumulative authorized amount. In normal (TEE-sign) mode, this is the
        /// new cum to sign — must be strictly greater than the prior accepted
        /// voucher. In reuse mode, this is the existing cum that matches
        /// `--reuse-signature` (no signing performed).
        #[arg(long)]
        cumulative_amount: String,
        /// Escrow contract address. Required only in TEE-sign mode (the
        /// EIP-712 voucher domain binds to it). Ignored in reuse mode —
        /// the existing signature already encodes the original escrow.
        #[arg(long)]
        escrow: Option<String>,
        /// Chain ID (e.g. 196 for X Layer). Required only in TEE-sign mode
        /// (the EIP-712 voucher domain binds to it). Ignored in reuse mode.
        #[arg(long)]
        chain_id: Option<u64>,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
        /// Reuse a previously-signed voucher: hex 65-byte signature that was
        /// returned by an earlier `mpp-session-voucher` / `mpp-session-open` call.
        /// When provided, the CLI skips TEE signing and emits an
        /// authorization_header that wraps these bytes verbatim. Use to
        /// spend remaining balance under an existing voucher without
        /// burning a fresh signature. Server must support voucher reuse
        /// (mppx, OKX TS Session — OKX Rust SDK from this version onward).
        #[arg(long)]
        reuse_signature: Option<String>,
    },

    /// TopUp an MPP Session payment channel (EIP-3009 to escrow via TEE, or hash-mode wrap).
    /// Returns authorization_header for sending topUp to Seller.
    #[command(name = "mpp-session-topup")]
    MppSessionTopUp {
        /// Full WWW-Authenticate challenge header (for credential echo)
        #[arg(long)]
        challenge: String,
        /// Existing channel ID (from mpp-session-open)
        #[arg(long)]
        channel_id: String,
        /// Additional deposit amount in atomic units
        #[arg(long)]
        additional_deposit: String,
        /// Escrow contract address
        #[arg(long)]
        escrow: String,
        /// Chain ID (e.g. 196 for X Layer)
        #[arg(long)]
        chain_id: u64,
        /// ERC-20 token contract (required in transaction mode)
        #[arg(long)]
        currency: Option<String>,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
        /// Optional: tx hash of client-broadcast topUp tx (hash mode).
        /// When provided, --currency is not required.
        #[arg(long)]
        tx_hash: Option<String>,
    },

    /// Close an MPP Session payment channel (sign final voucher via TEE).
    /// Returns authorization_header for sending close to Seller.
    MppSessionClose {
        /// Channel ID from session open
        #[arg(long)]
        channel_id: String,
        /// Final cumulative amount
        #[arg(long)]
        cumulative_amount: String,
        /// Escrow contract address
        #[arg(long)]
        escrow: String,
        /// Chain ID (e.g. 196 for X Layer)
        #[arg(long)]
        chain_id: u64,
        /// Full WWW-Authenticate challenge header (for credential echo)
        #[arg(long)]
        challenge: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum DefaultAction {
    /// Save an asset + chain as the default; used first when matching `accepts`.
    Set {
        /// EVM token contract address, e.g. 0xUSDG
        #[arg(long)]
        asset: String,
        /// Numeric EVM chain id, e.g. "1" (Ethereum), "196" (X Layer), "8453" (Base)
        #[arg(long)]
        chain: String,
        /// Display name shown in notifications, e.g. "USDT"
        #[arg(long)]
        name: Option<String>,
        /// Tier the user just confirmed: `basic` or `premium`. Skills
        /// pass this from the OVER_QUOTA `notifications[].data.tier` so
        /// only the acknowledged tier advances to `ChargingConfirmed`.
        /// Omit for manual invocations that don't act on a prompt.
        #[arg(long)]
        tier: Option<String>,
    },
    /// Show the saved default payment asset (if any).
    Get,
    /// Clear the saved default payment asset.
    Unset,
}

pub async fn execute(cmd: PaymentCommand) -> Result<()> {
    match cmd {
        PaymentCommand::X402Pay { accepts, from } => cmd_pay(&accepts, from.as_deref()).await,
        PaymentCommand::Eip3009Sign { accepts } => {
            let accepts_val: Value =
                serde_json::from_str(&accepts).context("--accepts must be a valid JSON array")?;
            let (proof, _entry) = payment_flow::sign_payment_local(&accepts_val, None).await?;
            output::success(json!({
                "signature": proof.signature,
                "authorization": proof.authorization,
            }));
            Ok(())
        }
        PaymentCommand::Default { action } => cmd_default(action),
        PaymentCommand::A2aPay { command } => a2a_pay::execute(command).await,
        PaymentCommand::MppCharge {
            challenge,
            from,
            tx_hash,
        } => cmd_mpp_charge(&challenge, from.as_deref(), tx_hash.as_deref()).await,
        PaymentCommand::MppSessionOpen {
            challenge,
            deposit,
            from,
            tx_hash,
            initial_cum,
            prepay_first,
        } => {
            cmd_mpp_session_open(
                &challenge,
                &deposit,
                from.as_deref(),
                tx_hash.as_deref(),
                initial_cum.as_deref(),
                prepay_first,
            )
                .await
        }
        PaymentCommand::MppSessionVoucher {
            challenge,
            channel_id,
            cumulative_amount,
            escrow,
            chain_id,
            from,
            reuse_signature,
        } => {
            cmd_mpp_session_voucher(
                &challenge,
                &channel_id,
                &cumulative_amount,
                escrow.as_deref(),
                chain_id,
                from.as_deref(),
                reuse_signature.as_deref(),
            )
                .await
        }
        PaymentCommand::MppSessionTopUp {
            challenge,
            channel_id,
            additional_deposit,
            escrow,
            chain_id,
            currency,
            from,
            tx_hash,
        } => {
            cmd_mpp_session_topup(
                &challenge,
                &channel_id,
                &additional_deposit,
                &escrow,
                chain_id,
                currency.as_deref(),
                from.as_deref(),
                tx_hash.as_deref(),
            )
                .await
        }
        PaymentCommand::MppSessionClose {
            channel_id,
            cumulative_amount,
            escrow,
            chain_id,
            challenge,
            from,
        } => {
            cmd_mpp_session_close(
                &channel_id,
                &cumulative_amount,
                &escrow,
                chain_id,
                &challenge,
                from.as_deref(),
            )
                .await
        }
    }
}

/// Convert a numeric EVM chain id (e.g. `"196"`) to CAIP-2 form
/// (`"eip155:196"`) for storage. Only plain decimal integers are
/// accepted — chain names (`"xlayer"`) and pre-formed CAIP-2 strings
/// (`"eip155:196"`) are rejected. Non-EVM chain ids are rejected too
/// (x402 payments are EIP-712 signed).
fn chain_id_to_caip2(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("--chain must not be empty");
    }
    let n: u64 = trimmed.parse().with_context(|| {
        format!(
            "--chain must be a numeric chain id (e.g. \"1\" for Ethereum, \
             \"196\" for X Layer), got: {input}"
        )
    })?;
    if matches!(n, 195 | 501 | 607 | 784) {
        bail!("x402 payments are EVM-only; chain id {n} is not supported");
    }
    Ok(format!("eip155:{n}"))
}

/// Extract the numeric chain id from a CAIP-2 `eip155:<id>` string for
/// display. Returns an empty string if the prefix is missing (never
/// happens for values written by `chain_id_to_caip2`).
fn caip2_to_chain_id(caip2: &str) -> String {
    caip2.strip_prefix("eip155:").unwrap_or(caip2).to_string()
}

fn cmd_default(action: DefaultAction) -> Result<()> {
    use crate::commands::agentic_wallet::payment_flow::PaymentTier;
    use crate::payment_cache::{PaymentCache, PaymentDefault};
    use crate::payment_notify::TierState;

    match action {
        DefaultAction::Set {
            asset,
            chain,
            name,
            tier,
        } => {
            let asset = asset.trim().to_string();
            if !is_valid_evm_address(&asset) {
                bail!("--asset must be a valid EVM address (0x + 40 hex chars)");
            }
            let chain = chain.trim().to_string();
            let network = chain_id_to_caip2(&chain)?;
            let name = name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let tier = match tier.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Some(
                    PaymentTier::from_server_str(s)
                        .ok_or_else(|| anyhow!("--tier must be `basic` or `premium`"))?,
                ),
                None => None,
            };

            let mut cache = PaymentCache::load().unwrap_or_default();
            cache.default_asset = Some(PaymentDefault {
                asset: asset.clone(),
                network,
                name: name.clone(),
            });
            // Explicit consent promotes only the tier the user just
            // confirmed — untagged calls (manual use) never change
            // state, so a pending prompt on another tier still fires.
            if let Some(t) = tier {
                let slot = match t {
                    PaymentTier::Basic => &mut cache.basic_state,
                    PaymentTier::Premium => &mut cache.premium_state,
                };
                if *slot == TierState::ChargingUnconfirmed {
                    *slot = TierState::ChargingConfirmed;
                }
            }
            cache.save().context("failed to save payment cache")?;
            output::success(json!({
                "asset": asset,
                "chain": chain,
                "name": name,
            }));
            Ok(())
        }
        DefaultAction::Get => {
            let cache = PaymentCache::load().unwrap_or_default();
            match cache.default_asset {
                Some(d) => output::success(json!({
                    "asset": d.asset,
                    "chain": caip2_to_chain_id(&d.network),
                    "name": d.name,
                })),
                None => output::success_empty(),
            }
            Ok(())
        }
        DefaultAction::Unset => {
            let mut cache = PaymentCache::load().unwrap_or_default();
            cache.default_asset = None;
            cache.save().context("failed to save payment cache")?;
            output::success_empty();
            Ok(())
        }
    }
}

/// Validate common payment inputs: amount, pay_to, asset.
/// Returns the parsed amount as u128.
fn validate_payment_inputs(amount: &str, pay_to: &str, asset: &str) -> Result<u128> {
    if amount.is_empty() {
        bail!("--amount must not be empty");
    }
    let parsed_amount = amount
        .parse::<u128>()
        .context("--amount must be a non-negative integer in minimal units")?;
    if parsed_amount == 0 {
        bail!("--amount must be greater than zero");
    }
    if !is_valid_evm_address(pay_to) {
        bail!("--pay-to must be a valid EVM address (0x + 40 hex chars)");
    }
    if !is_valid_evm_address(asset) {
        bail!("--asset must be a valid EVM contract address (0x + 40 hex chars)");
    }
    Ok(parsed_amount)
}

/// Sign an x402 payment authorization and print the proof as JSON.
/// All crypto happens in `payment_flow::sign_payment_with_preference`. Passes
/// `None` for the preference so the user's saved default asset does NOT
/// influence which accepts entry gets signed — this command signs exactly
/// what the caller supplied via `--accepts`.
async fn cmd_pay(accepts_json: &str, from: Option<&str>) -> Result<()> {
    let accepts: Value =
        serde_json::from_str(accepts_json).context("--accepts must be a valid JSON array")?;
    let (proof, _entry) =
        payment_flow::sign_payment_with_preference(&accepts, from, None, None).await?;
    let mut out = json!({
        "signature": proof.signature,
        "authorization": proof.authorization,
    });
    if let Some(cert) = proof.session_cert {
        out["sessionCert"] = json!(cert);
    }
    output::success(out);
    Ok(())
}

// ── MPP Challenge Parser ─────────────────────────────────────────

/// Parse a `WWW-Authenticate: Payment ...` header into key-value pairs.
///
/// Follows RFC 7235 auth-param grammar: `key=token` or `key="quoted-string"`,
/// pairs separated by `,` with optional surrounding whitespace. Embedded
/// commas inside quoted values are preserved (e.g. `description="a, b"`).
/// Backslash escapes inside quoted strings (`\"`, `\\`) are honored.
///
/// Example:
///   `Payment id="abc", realm="api.shop.com", method="evm", request="eyJ..."`
fn parse_www_authenticate(header: &str) -> Result<serde_json::Value> {
    let content = header.strip_prefix("Payment ").unwrap_or(header);
    let mut map = serde_json::Map::new();

    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip leading whitespace and stray commas.
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // Read key up to '='.
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' && bytes[i] != b',' {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            // No '=' before end / next comma → malformed pair, skip.
            continue;
        }
        let key = content[key_start..i].trim().to_string();
        i += 1; // consume '='

        // Read value: either quoted-string or token.
        let value = if i < bytes.len() && bytes[i] == b'"' {
            i += 1; // consume opening quote
            let mut val = String::new();
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' if i + 1 < bytes.len() => {
                        // Backslash escape: take the next byte verbatim.
                        val.push(bytes[i + 1] as char);
                        i += 2;
                    }
                    b'"' => {
                        i += 1; // consume closing quote
                        break;
                    }
                    b => {
                        val.push(b as char);
                        i += 1;
                    }
                }
            }
            val
        } else {
            // Token: read until next comma or whitespace.
            let val_start = i;
            while i < bytes.len() && bytes[i] != b',' && bytes[i] != b' ' && bytes[i] != b'\t' {
                i += 1;
            }
            content[val_start..i].to_string()
        };

        if !key.is_empty() {
            map.insert(key, serde_json::Value::String(value));
        }
    }

    if !map.contains_key("id") || !map.contains_key("method") || !map.contains_key("intent") {
        bail!("invalid WWW-Authenticate header: missing required fields (id, method, intent)");
    }
    // This CLI only supports EVM-based MPP; reject other methods (e.g. "tempo", "svm", "stripe")
    // loudly rather than silently producing wrong-shape signatures.
    let method = map.get("method").and_then(|v| v.as_str()).unwrap_or("");
    if method != "evm" {
        bail!(
            "unsupported MPP method \"{}\"; this CLI only supports method=\"evm\"",
            method
        );
    }
    Ok(serde_json::Value::Object(map))
}

/// Decode the base64url-encoded `request` field from a challenge.
fn decode_challenge_request(challenge: &serde_json::Value) -> Result<serde_json::Value> {
    let request_b64 = challenge["request"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'request' in challenge"))?;
    // base64url decode (no padding)
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(request_b64)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(request_b64))
        .context("invalid base64url in challenge request")?;
    serde_json::from_slice(&decoded).context("invalid JSON in challenge request")
}

/// Build a challenge echo object for the credential.
fn build_challenge_echo(challenge: &serde_json::Value) -> serde_json::Value {
    json!({
        "id": challenge["id"],
        "realm": challenge["realm"],
        "method": challenge["method"],
        "intent": challenge["intent"],
        "request": challenge["request"],
        "expires": challenge.get("expires").cloned().unwrap_or(serde_json::Value::Null),
    })
}

/// Base64url encode a JSON value (JCS canonicalized per RFC8785, no padding).
fn base64url_encode_json(value: &serde_json::Value) -> Result<String> {
    let json_str = serde_jcs::to_string(value).context("failed to JCS canonicalize JSON")?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes()))
}

/// Parse challenge `expires` (RFC3339) into Unix seconds.
/// Returns `Ok(None)` when the field is missing/null; `Err` when present but malformed.
fn parse_challenge_expires_unix(challenge: &serde_json::Value) -> Result<Option<u64>> {
    let Some(s) = challenge.get("expires").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let dt = chrono::DateTime::parse_from_rfc3339(s)
        .with_context(|| format!("challenge.expires is not RFC3339: {}", s))?;
    let ts = dt.timestamp();
    if ts < 0 {
        bail!("challenge.expires is before Unix epoch: {}", s);
    }
    Ok(Some(ts as u64))
}

/// Compute EIP-3009 `validBefore` honoring the challenge's `expires`.
///
/// Per spec §EIP-3009 Authorization: `validBefore` MUST be >= the challenge
/// `expires` auth-param converted to Unix seconds, so the server has enough
/// time to submit. We keep a 5-minute floor past `now` and add a 60s grace
/// past `challenge.expires` to absorb RTT.
fn compute_valid_before(challenge: &serde_json::Value, now_unix: u64) -> Result<String> {
    const DEFAULT_TTL_SECS: u64 = 300;
    const GRACE_SECS: u64 = 60;
    let from_now = now_unix.saturating_add(DEFAULT_TTL_SECS);
    let vb = match parse_challenge_expires_unix(challenge)? {
        Some(exp) if exp < now_unix => {
            bail!("challenge.expires is already in the past");
        }
        Some(exp) => from_now.max(exp.saturating_add(GRACE_SECS)),
        None => from_now,
    };
    Ok(vb.to_string())
}

/// Parse a charge `request` into `(primary_remainder, splits)` with full spec validation.
///
/// Enforces §Split Payments constraints before any TEE signing:
/// - each `splits[i].amount` MUST be > 0
/// - `sum(splits[].amount)` MUST be strictly less than `amount`
/// - splits MAY be absent; if present, MUST have 1-10 entries
///
/// Returns `(primary_value_decimal_str, [(amount, recipient), ...])` in the
/// same order as `methodDetails.splits`. When no splits are present, returns
/// `(amount, vec![])`.
fn compute_primary_split_amounts(
    request: &serde_json::Value,
) -> Result<(String, Vec<(String, String)>)> {
    let amount_str = request["amount"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'amount' in challenge request"))?;
    let amount = U256::from_str_radix(amount_str, 10).map_err(|e| {
        anyhow!(
            "challenge amount '{}' is not a base-10 integer: {}",
            amount_str,
            e
        )
    })?;

    let splits_json = match request["methodDetails"]["splits"].as_array() {
        Some(a) => a,
        None => return Ok((amount_str.to_string(), Vec::new())),
    };
    if splits_json.is_empty() {
        bail!("challenge methodDetails.splits is present but empty (spec requires >= 1 entry)");
    }
    if splits_json.len() > 10 {
        bail!(
            "challenge splits count {} exceeds spec max of 10",
            splits_json.len()
        );
    }

    let mut split_sum = U256::ZERO;
    let mut parsed = Vec::with_capacity(splits_json.len());
    for (i, s) in splits_json.iter().enumerate() {
        let a_str = s["amount"]
            .as_str()
            .ok_or_else(|| anyhow!("splits[{}].amount missing or not a string", i))?;
        let r_str = s["recipient"]
            .as_str()
            .ok_or_else(|| anyhow!("splits[{}].recipient missing or not a string", i))?;
        let a = U256::from_str_radix(a_str, 10).map_err(|e| {
            anyhow!(
                "splits[{}].amount '{}' is not a base-10 integer: {}",
                i,
                a_str,
                e
            )
        })?;
        if a.is_zero() {
            bail!("splits[{}].amount must be > 0", i);
        }
        split_sum = split_sum
            .checked_add(a)
            .ok_or_else(|| anyhow!("splits sum overflow at index {}", i))?;
        parsed.push((a_str.to_string(), r_str.to_string()));
    }

    if split_sum >= amount {
        bail!(
            "splits sum ({}) must be strictly less than challenge amount ({}) per spec §Constraints",
            split_sum,
            amount
        );
    }
    let primary = amount - split_sum;
    Ok((primary.to_string(), parsed))
}

/// Compute the EIP-3009 topUp nonce. Must match the escrow contract's
/// `computeTopUpAuthorizationNonce(bytes32 channelId, uint128 additionalDeposit,
/// address from, bytes32 topUpSalt)` view function — same arg order, keccak256
/// of `abi.encode(...)`.
fn compute_topup_nonce(
    payer: &str,
    channel_id: &str,
    additional_deposit: &str,
    top_up_salt_hex: &str,
) -> Result<String> {
    use tiny_keccak::{Hasher, Keccak};

    let payer_addr: Address = payer.parse().context("invalid payer address")?;
    let channel_id_bytes =
        hex::decode(channel_id.trim_start_matches("0x")).context("channelId must be hex")?;
    if channel_id_bytes.len() != 32 {
        bail!("channelId must be 32 bytes (64 hex chars)");
    }
    let cid_b: B256 = B256::from_slice(&channel_id_bytes);
    let additional: u128 = additional_deposit
        .parse()
        .context("additionalDeposit must be decimal uint128")?;
    let salt_bytes = hex::decode(top_up_salt_hex.trim_start_matches("0x"))
        .context("topUpSalt must be hex")?;
    if salt_bytes.len() != 32 {
        bail!("topUpSalt must be 32 bytes (64 hex chars)");
    }
    let salt_b: B256 = B256::from_slice(&salt_bytes);

    // `abi_encode_params` matches Solidity `abi.encode(a, b, c)`; `abi_encode`
    // would wrap as a tuple and add a 32-byte offset header.
    let encoded = (cid_b, additional, payer_addr, salt_b).abi_encode_params();

    let mut hasher = Keccak::v256();
    hasher.update(&encoded);
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    Ok(format!("0x{}", hex::encode(out)))
}

/// Extract `(recipients, bps)` from session challenge
/// `request.methodDetails.splits[]`. Returns empty vecs when absent.
fn parse_session_splits(request: &serde_json::Value) -> Result<(Vec<String>, Vec<u16>)> {
    let splits = match request["methodDetails"]["splits"].as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return Ok((vec![], vec![])),
    };
    let mut recipients = Vec::with_capacity(splits.len());
    let mut bps = Vec::with_capacity(splits.len());
    for (i, s) in splits.iter().enumerate() {
        let r = s["recipient"]
            .as_str()
            .ok_or_else(|| anyhow!("splits[{}].recipient missing", i))?;
        let b = s["bps"]
            .as_u64()
            .ok_or_else(|| anyhow!("splits[{}].bps missing or not integer", i))?;
        if !(1..=9999).contains(&b) {
            bail!("splits[{}].bps out of range 1-9999: {}", i, b);
        }
        recipients.push(r.to_string());
        bps.push(b as u16);
    }
    Ok((recipients, bps))
}

/// Compute the EIP-3009 open nonce. Must match the escrow contract's
/// `computeOpenAuthorizationNonce(from, payee, token, salt, authorizedSigner,
/// splitRecipients, splitBps)` view function — keccak256 of `abi.encode(...)`.
///
/// **`authorizedSigner` must be the `0x0` sentinel** (also omitted from the
/// payload), even when the effective signer is the payer. Passing the payer
/// address triggers `InvalidAuthorizedSigner()` — `0x0` is the contract
/// sentinel for "payer is the voucher signer".
fn compute_open_nonce(
    payer: &str,
    payee: &str,
    token: &str,
    salt_hex: &str,
    authorized_signer: &str,
    split_recipients: &[String],
    split_bps: &[u16],
) -> Result<String> {
    use tiny_keccak::{Hasher, Keccak};

    let payer_addr: Address = payer.parse().context("invalid payer address")?;
    let payee_addr: Address = payee.parse().context("invalid payee address")?;
    let token_addr: Address = token.parse().context("invalid token address")?;
    let auth_signer_addr: Address = authorized_signer
        .parse()
        .context("invalid authorizedSigner address")?;
    let salt_bytes = hex::decode(salt_hex.trim_start_matches("0x"))
        .context("salt must be hex")?;
    if salt_bytes.len() != 32 {
        bail!("salt must be 32 bytes (64 hex chars)");
    }
    let salt_b: B256 = B256::from_slice(&salt_bytes);

    let recipients: Vec<Address> = split_recipients
        .iter()
        .map(|s| s.parse::<Address>().context("invalid split recipient"))
        .collect::<Result<_>>()?;
    let bps: Vec<u16> = split_bps.to_vec();

    // `abi_encode_params` = Solidity `abi.encode(a, b, c)`; `abi_encode` adds
    // a 32-byte tuple offset header (different value).
    let encoded = (
        payer_addr,
        payee_addr,
        token_addr,
        salt_b,
        auth_signer_addr,
        recipients,
        bps,
    )
        .abi_encode_params();

    let mut hasher = Keccak::v256();
    hasher.update(&encoded);
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    Ok(format!("0x{}", hex::encode(out)))
}

/// Compute channelId per MPP EVM spec:
/// `channelId = keccak256(abi.encode(payer, payee, token, salt, authorizedSigner, escrow, chainId))`
/// All arguments are hex strings; `salt` and the returned value are 0x-prefixed bytes32.
fn compute_channel_id(
    payer: &str,
    payee: &str,
    token: &str,
    salt_hex: &str,
    authorized_signer: &str,
    escrow: &str,
    chain_id: u64,
) -> Result<String> {
    let payer_addr: Address = payer.parse().context("invalid payer address")?;
    let payee_addr: Address = payee.parse().context("invalid payee address")?;
    let token_addr: Address = token.parse().context("invalid token address")?;
    let auth_signer_addr: Address = authorized_signer
        .parse()
        .context("invalid authorizedSigner address")?;
    let escrow_addr: Address = escrow.parse().context("invalid escrow address")?;
    let salt_bytes = hex::decode(salt_hex.trim_start_matches("0x"))
        .context("salt must be hex")?;
    if salt_bytes.len() != 32 {
        bail!("salt must be 32 bytes (64 hex chars)");
    }
    let salt_b: B256 = B256::from_slice(&salt_bytes);
    let chain_id_u: U256 = U256::from(chain_id);

    // `abi_encode_params` matches Solidity `abi.encode(a, b, c)` exactly,
    // staying consistent with the sibling nonce helpers and with the
    // contract's `keccak256(abi.encode(...))`. For all-static tuples the
    // bytes coincide with `abi_encode`, but the params form is the
    // semantically correct encoder and is robust if a dynamic field is
    // ever added to the channelId derivation.
    let encoded = (
        payer_addr,
        payee_addr,
        token_addr,
        salt_b,
        auth_signer_addr,
        escrow_addr,
        chain_id_u,
    )
        .abi_encode_params();

    use tiny_keccak::{Hasher, Keccak};
    let mut hasher = Keccak::v256();
    hasher.update(&encoded);
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    Ok(format!("0x{}", hex::encode(out)))
}

/// Selects the EIP-3009 typehash for `gen-msg-hash` / `sign-msg`:
/// - `Transfer` → no `msgType`/`signType` field (default — charge / x402)
/// - `Receive`  → `msgType = signType = "eip3009ReceiveAuth"` (session
///   `escrow.receiveWithAuthorization`)
#[derive(Clone, Copy, Debug)]
enum Eip3009AuthType {
    Transfer,
    Receive,
}

impl Eip3009AuthType {
    /// `Transfer` omits the type field; `Receive` returns `Some("eip3009ReceiveAuth")`.
    fn override_type(self) -> Option<&'static str> {
        match self {
            Self::Transfer => None,
            Self::Receive => Some("eip3009ReceiveAuth"),
        }
    }
}

/// TEE-sign an EIP-3009 authorization. Use [`Eip3009AuthType::Transfer`] for
/// charge / x402 (direct EOA transfer) and [`Eip3009AuthType::Receive`] for
/// session deposits (`escrow.receiveWithAuthorization`).
///
/// The TEE looks up `domainHash` by token name/version internally, so the
/// client doesn't need an RPC URL to fetch `DOMAIN_SEPARATOR()`.
async fn tee_sign_eip3009(
    auth_type: Eip3009AuthType,
    chain_index: &str,
    from: &str,
    to: &str,
    amount: &str,
    valid_before: &str,
    nonce: &str,
    asset: &str,
) -> Result<(String, String)> {
    let access_token = ensure_tokens_refreshed().await?;
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let mut base_fields = json!({
        "chainIndex":        chain_index,
        "from":              from,
        "to":                to,
        "value":             amount,
        "validAfter":        "0",
        "validBefore":       valid_before,
        "nonce":             nonce,
        "verifyingContract": asset,
    });

    let mut client = WalletApiClient::new()?;

    // Step 1: gen-msg-hash — TEE resolves domainHash + msgHash from the token's
    // name/version. Receive adds `msgType`; Transfer omits.
    let mut gen_body = base_fields.clone();
    if let Some(msg_type) = auth_type.override_type() {
        gen_body["msgType"] = json!(msg_type);
    }
    let hash_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &gen_body,
        )
        .await
        .map_err(format_api_error)
        .context("eip3009 gen-msg-hash failed")?;
    let msg_hash = hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing msgHash in gen-msg-hash response"))?;
    let domain_hash = hash_resp[0]["domainHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing domainHash in gen-msg-hash response"))?;

    // Step 2: locally sign msgHash with the ed25519 session key (user authz token).
    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    signing_seed.zeroize();
    let session_sig_b64 = B64.encode(&session_signature);

    // Step 3: sign-msg — TEE secp256k1-signs the digest with the wallet key.
    // Receive adds `signType`; Transfer omits.
    base_fields["domainHash"] = json!(domain_hash);
    base_fields["sessionCert"] = json!(&session.session_cert);
    base_fields["sessionSignature"] = json!(session_sig_b64);
    // open / topUp target the escrow contract; bypass TRANSFER_TO_CONTRACT_ADDRESS check.
    base_fields["skipWarning"] = json!(true);
    if let Some(sign_type) = auth_type.override_type() {
        base_fields["signType"] = json!(sign_type);
    }

    let sign_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &base_fields,
        )
        .await
        .map_err(format_api_error)
        .context("eip3009 sign-msg failed")?;
    let signature = sign_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing signature in sign-msg response"))?
        .to_string();

    Ok((signature, from.to_string()))
}

/// Build the MPP Voucher EIP-712 typed data (pure function).
///
/// Single source of truth shared by client, Seller SDK, and on-chain contract:
/// - `domain.name = "EVM Payment Channel"`, `domain.version = "1"`
/// - `Voucher(bytes32 channelId, uint128 cumulativeAmount)`
///
/// Any field change must be synced with `mpp/src/eip712/{domain,voucher}.rs`
/// (Seller SDK) and the EvmPaymentChannel contract.
fn build_voucher_typed_data(
    channel_id: &str,
    cumulative_amount: &str,
    escrow: &str,
    chain_id: u64,
) -> serde_json::Value {
    json!({
        "domain": {
            "name":              "EVM Payment Channel",
            "version":           "1",
            "chainId":           chain_id,
            "verifyingContract": escrow,
        },
        "types": {
            "EIP712Domain": [
                { "name": "name",              "type": "string"  },
                { "name": "version",           "type": "string"  },
                { "name": "chainId",           "type": "uint256" },
                { "name": "verifyingContract", "type": "address" }
            ],
            "Voucher": [
                { "name": "channelId",        "type": "bytes32" },
                { "name": "cumulativeAmount", "type": "uint128" }
            ]
        },
        "primaryType": "Voucher",
        "message": {
            "channelId":        channel_id,
            "cumulativeAmount": cumulative_amount,
        }
    })
}

/// TEE-sign an MPP Voucher via the **generic EIP-712 path** (same as
/// `wallet sign-message --type eip712`). The client builds the full typed
/// data and feeds it to the TEE — no voucher-specific template — so the
/// domain and struct stay in client source-of-truth and align with the
/// Seller SDK's local verification.
async fn tee_sign_voucher(
    chain_index: &str,
    payer_addr: &str,
    channel_id: &str,
    cumulative_amount: &str,
    escrow: &str,
    chain_id: u64,
) -> Result<String> {
    let access_token = ensure_tokens_refreshed().await?;
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let typed_data = build_voucher_typed_data(channel_id, cumulative_amount, escrow, chain_id);

    let mut client = WalletApiClient::new()?;

    // Step 1: gen-msg-hash — TEE computes the EIP-712 digest from typed_data.
    let gen_hash_body = json!({
        "chainIndex": chain_index,
        "payload": [{
            "msgType": "eip712",
            "message": typed_data,
        }]
    });
    let hash_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &gen_hash_body,
        )
        .await
        .map_err(format_api_error)
        .context("mpp voucher gen-msg-hash failed")?;
    let msg_hash = hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing msgHash"))?;

    // Step 2: locally sign msgHash with the ed25519 session key (user authz token).
    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let mut signing_seed_b64 = B64.encode(signing_seed.as_slice());
    signing_seed.zeroize();
    let session_signature = crate::crypto::ed25519_sign_hex(msg_hash, &signing_seed_b64)?;
    signing_seed_b64.zeroize();

    // Step 3: sign-msg — TEE secp256k1-signs the digest with the wallet key.
    // Voucher is a high-frequency automated path; skip the wallet warning prompt.
    let sign_body = json!({
        "chainIndex":  chain_index,
        "from":        payer_addr,
        "sessionCert": &session.session_cert,
        "payload": [{
            "signType":         "eip712",
            "message":          typed_data,
            "sessionSignature": session_signature,
        }],
        "skipWarning": true,
    });
    let sign_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)
        .context("mpp voucher sign-msg failed")?;
    let signature = sign_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing signature"))?
        .to_string();

    Ok(signature)
}

// ── MPP Command Implementations ─────────────────────────────────

/// Resolve the OKX `chainIndex` and the selected payer address for a given
/// EVM `chain_id`. Centralises 5 identical chain+wallet lookups across
/// `cmd_mpp_charge` / `cmd_mpp_session_open` / `cmd_mpp_session_voucher` /
/// `cmd_mpp_session_topup` / `cmd_mpp_session_close`.
///
/// Returns `(chain_index, payer_addr)` — both already normalised to `String`.
async fn resolve_chain_and_payer(
    chain_id: u64,
    from: Option<&str>,
) -> Result<(String, String)> {
    let chain_entry =
        crate::commands::agentic_wallet::chain::get_chain_by_real_chain_index(&chain_id.to_string())
            .await?
            .ok_or_else(|| anyhow!("chain not found for chainId {}", chain_id))?;
    let chain_index = chain_entry["chainIndex"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| chain_entry["chainIndex"].as_u64().map(|n| n.to_string()))
        .ok_or_else(|| anyhow!("missing chainIndex"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow!("missing chainName"))?;
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, from, chain_name)?;
    Ok((chain_index, addr_info.address.clone()))
}

/// Generate a fresh random 32-byte nonce as 0x + 64 hex.
fn random_nonce_hex() -> String {
    use rand::RngCore;
    let mut n = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut n);
    format!("0x{}", hex::encode(n))
}

/// onchainos payment mpp-charge: Charge single payment.
/// - feePayer=true (default): TEE-sign EIP-3009, emit TransactionPayload (+ splits if present).
/// - feePayer=false: require --tx-hash, emit HashPayload (client self-broadcasts).
async fn cmd_mpp_charge(
    challenge_header: &str,
    from: Option<&str>,
    tx_hash: Option<&str>,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;
    let request = decode_challenge_request(&challenge)?;

    let recipient = request["recipient"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'recipient' in challenge request"))?;
    let amount = request["amount"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'amount' in challenge request"))?;
    let currency = request["currency"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'currency' in challenge request"))?;
    let chain_id = request["methodDetails"]["chainId"]
        .as_u64()
        .ok_or_else(|| anyhow!("missing 'methodDetails.chainId'"))?;
    // feePayer defaults to true per spec
    let fee_payer = request["methodDetails"]["feePayer"]
        .as_bool()
        .unwrap_or(true);

    // Resolve chain index + payer address (needed for both transaction and hash modes for `source` DID)
    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    // Hash mode: client has already broadcast the transfer, just wrap the hash
    if !fee_payer {
        let hash = tx_hash.ok_or_else(|| {
            anyhow!(
                "challenge.methodDetails.feePayer=false requires --tx-hash (broadcast transferWithAuthorization yourself first)"
            )
        })?;
        if !hash.starts_with("0x")
            || hash.len() != 66
            || !hash[2..].chars().all(|c| c.is_ascii_hexdigit())
        {
            bail!("--tx-hash must be 0x + 64 hex chars");
        }
        let credential = json!({
            "challenge": build_challenge_echo(&challenge),
            "source": format!("did:pkh:eip155:{}:{}", chain_id, payer_addr),
            "payload": {
                "type": "hash",
                "hash": hash,
            }
        });
        let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);
        output::success(json!({
            "protocol": "mpp",
            "method": "evm",
            "intent": "charge",
            "mode": "hash",
            "authorization_header": authorization_header,
            "wallet": payer_addr,
            "challenge": {
                "id": challenge["id"],
                "realm": challenge["realm"],
            }
        }));
        return Ok(());
    }

    if tx_hash.is_some() {
        bail!("--tx-hash is only valid when challenge.methodDetails.feePayer=false");
    }

    // Validate splits invariants and derive primary `authorization.value` BEFORE any TEE call.
    // Per spec §Split Payments: primary receives `amount - sum(splits[].amount)`.
    let (primary_value, parsed_splits) = compute_primary_split_amounts(&request)?;
    // Sanity: `amount` from the request and `primary_value` agree when no splits exist.
    debug_assert!(parsed_splits.is_empty() == (primary_value == amount));

    // Transaction mode: TEE sign EIP-3009 for the main payment
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let valid_before = compute_valid_before(&challenge, now)?;
    let nonce = random_nonce_hex();

    // Charge: SA relay-broadcasts token.transferWithAuthorization → TEE
    // defaults to the TransferWithAuthorization typehash.
    let (signature, _) = tee_sign_eip3009(
        Eip3009AuthType::Transfer,
        &chain_index,
        payer_addr,
        recipient,
        &primary_value,
        &valid_before,
        &nonce,
        currency,
    )
        .await?;

    let mut authorization = json!({
        "type": "eip-3009",
        "from": payer_addr,
        "to": recipient,
        "value": primary_value,
        "validAfter": "0",
        "validBefore": valid_before,
        "nonce": nonce,
        "signature": signature,
    });

    if !parsed_splits.is_empty() {
        let mut signed_splits = Vec::with_capacity(parsed_splits.len());
        for (i, (split_amount, split_recipient)) in parsed_splits.iter().enumerate() {
            let split_nonce = random_nonce_hex();
            // Splits use the same transferWithAuthorization typehash as primary.
            let (split_sig, _) = tee_sign_eip3009(
                Eip3009AuthType::Transfer,
                &chain_index,
                payer_addr,
                split_recipient,
                split_amount,
                &valid_before,
                &split_nonce,
                currency,
            )
                .await
                .with_context(|| format!("splits[{}] TEE sign failed", i))?;
            signed_splits.push(json!({
                "from": payer_addr,
                "to": split_recipient,
                "value": split_amount,
                "validAfter": "0",
                "validBefore": valid_before,
                "nonce": split_nonce,
                "signature": split_sig,
            }));
        }
        authorization["splits"] = json!(signed_splits);
    }

    let credential = json!({
        "challenge": build_challenge_echo(&challenge),
        "source": format!("did:pkh:eip155:{}:{}", chain_id, payer_addr),
        "payload": {
            "type": "transaction",
            "authorization": authorization,
        }
    });

    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);

    output::success(json!({
        "protocol": "mpp",
        "method": "evm",
        "intent": "charge",
        "mode": "transaction",
        "authorization_header": authorization_header,
        "wallet": payer_addr,
        "challenge": {
            "id": challenge["id"],
            "realm": challenge["realm"],
        }
    }));
    Ok(())
}

/// onchainos payment mpp-session-open: Open payment channel.
/// - feePayer=true (default): TEE-sign EIP-3009 deposit + initial voucher → transaction payload.
/// - feePayer=false: require --tx-hash of the client-broadcast open tx; still TEE-sign initial voucher.
async fn cmd_mpp_session_open(
    challenge_header: &str,
    deposit: &str,
    from: Option<&str>,
    tx_hash: Option<&str>,
    initial_cum_arg: Option<&str>,
    prepay_first: bool,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;
    let request = decode_challenge_request(&challenge)?;

    let recipient = request["recipient"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'recipient'"))?;
    let currency = request["currency"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'currency'"))?;
    let chain_id = request["methodDetails"]["chainId"]
        .as_u64()
        .ok_or_else(|| anyhow!("missing 'methodDetails.chainId'"))?;
    let escrow = request["methodDetails"]["escrowContract"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'methodDetails.escrowContract'"))?;
    let fee_payer = request["methodDetails"]["feePayer"]
        .as_bool()
        .unwrap_or(true);

    // Resolve chain + payer
    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    // Generate salt + compute channelId (needed for both modes — voucher signature binds to it)
    let salt = {
        use rand::RngCore;
        let mut s = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut s);
        format!("0x{}", hex::encode(s))
    };
    // authorizedSigner must be the 0x0 sentinel (not payer). The contract
    // sentinel means "payer is the voucher signer"; the same value goes into
    // both the channelId hash and the EIP-3009 nonce. Passing payer triggers
    // InvalidAuthorizedSigner().
    const ZERO_SIGNER: &str = "0x0000000000000000000000000000000000000000";
    let channel_id = compute_channel_id(
        payer_addr,
        recipient,
        currency,
        &salt,
        ZERO_SIGNER,
        escrow,
        chain_id,
    )?;

    // Initial voucher cumulativeAmount (open carries a baseline voucher, per
    // upstream mpp-rs Tempo). Precedence: --initial-cum > --prepay-first
    // (reads challenge.amount) > default "0".
    let initial_cum = if let Some(explicit) = initial_cum_arg {
        explicit.to_string()
    } else if prepay_first {
        request["amount"]
            .as_str()
            .filter(|s| !s.is_empty() && *s != "0")
            .unwrap_or("0")
            .to_string()
    } else {
        "0".to_string()
    };

    // Sign the initial voucher EIP-712 (channelId, cumulativeAmount=initial_cum).
    // The seller SDK verifies it locally and stores it as the channel baseline.
    let initial_voucher_sig =
        tee_sign_voucher(&chain_index, payer_addr, &channel_id, &initial_cum, escrow, chain_id)
            .await?;

    if !fee_payer {
        // Hash mode: client already broadcast the open tx, just wrap the hash
        let hash = tx_hash.ok_or_else(|| {
            anyhow!(
                "challenge.methodDetails.feePayer=false requires --tx-hash (broadcast open tx yourself first)"
            )
        })?;
        if !hash.starts_with("0x")
            || hash.len() != 66
            || !hash[2..].chars().all(|c| c.is_ascii_hexdigit())
        {
            bail!("--tx-hash must be 0x + 64 hex chars");
        }
        // authorizedSigner omitted = payer (both contract and SDK resolve
        // 0x0 / omitted to payer; channelId derivation matches).
        //
        // cumulativeAmount + the initial voucher signature are SDK-only:
        // the seller SDK verifies them and stores the baseline voucher,
        // then strips them before forwarding to SA. Hash mode has no
        // EIP-3009 deposit signature, so the voucher signature occupies
        // the `signature` key directly (transaction mode uses
        // `voucherSignature` because `signature` is the EIP-3009 sig).
        let credential = json!({
            "challenge": build_challenge_echo(&challenge),
            "source": format!("did:pkh:eip155:{}:{}", chain_id, payer_addr),
            "payload": {
                "action": "open",
                "type": "hash",
                "channelId": channel_id,
                "salt": salt,
                "hash": hash,
                "cumulativeAmount": initial_cum,
                "signature": initial_voucher_sig,
            }
        });
        let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);
        output::success(json!({
            "protocol": "mpp",
            "action": "session_open",
            "mode": "hash",
            "authorization_header": authorization_header,
            "channel_id": channel_id,
            "escrow": escrow,
            "chain_id": chain_id,
            "deposit": deposit,
            "wallet": payer_addr,
        }));
        return Ok(());
    }

    if tx_hash.is_some() {
        bail!("--tx-hash is only valid when challenge.methodDetails.feePayer=false");
    }

    // Transaction mode: TEE sign EIP-3009 for deposit
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let valid_before = compute_valid_before(&challenge, now)?;

    // Parse splits (optional) — needed for nonce derivation. The contract
    // openWithAuthorization validates the EIP-3009 nonce with the same formula.
    let (split_recipients, split_bps) = parse_session_splits(&request)?;

    // Contract computeOpenAuthorizationNonce formula:
    //   keccak256(abi.encode(from, payee, token, salt, authorizedSigner,
    //                        splitRecipients, splitBps))
    // authorizedSigner = 0x0 sentinel (same as channelId derivation).
    let nonce = compute_open_nonce(
        payer_addr,
        recipient,
        currency,
        &salt,
        ZERO_SIGNER,
        &split_recipients,
        &split_bps,
    )?;

    // session-open: escrow calls token.receiveWithAuthorization → Receive.
    let (eip3009_signature, _) = tee_sign_eip3009(
        Eip3009AuthType::Receive,
        &chain_index,
        payer_addr,
        escrow,
        deposit,
        &valid_before,
        &nonce,
        currency,
    )
        .await?;

    // authorizedSigner omitted — equivalent to default=payer (per spec).
    // cumulativeAmount + voucherSignature are SDK-only: the seller SDK
    // verifies and stores the baseline voucher, then strips them before
    // forwarding to SA. Here `signature` is the EIP-3009 deposit sig
    // (SA-required); the voucher signature lives in `voucherSignature`.
    // Hash mode reverses the naming (see hash branch above).
    let credential = json!({
        "challenge": build_challenge_echo(&challenge),
        "source": format!("did:pkh:eip155:{}:{}", chain_id, payer_addr),
        "payload": {
            "action": "open",
            "type": "transaction",
            "channelId": channel_id,
            "salt": salt,
            "authorization": {
                "type": "eip-3009",
                "from": payer_addr,
                "to": escrow,
                "value": deposit,
                "validAfter": "0",
                "validBefore": valid_before,
                "nonce": nonce,
            },
            "signature": eip3009_signature,
            "cumulativeAmount": initial_cum,
            "voucherSignature": initial_voucher_sig,
        }
    });

    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);

    output::success(json!({
        "protocol": "mpp",
        "action": "session_open",
        "mode": "transaction",
        "authorization_header": authorization_header,
        "channel_id": channel_id,
        "escrow": escrow,
        "chain_id": chain_id,
        "deposit": deposit,
        "wallet": payer_addr,
    }));
    Ok(())
}

/// onchainos payment mpp-session-voucher: sign EIP-712 voucher (or wrap an existing
/// signature when `reuse_signature` is supplied).
/// Returns authorization_header for replaying business requests.
async fn cmd_mpp_session_voucher(
    challenge_header: &str,
    channel_id: &str,
    cumulative_amount: &str,
    escrow: Option<&str>,
    chain_id: Option<u64>,
    from: Option<&str>,
    reuse_signature: Option<&str>,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;

    let (voucher_sig, mode) = if let Some(sig) = reuse_signature {
        let normalized = sig.trim();
        let hex_part = normalized.strip_prefix("0x").unwrap_or(normalized);
        if hex_part.len() != 130 || !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("--reuse-signature must be a 0x-prefixed 65-byte hex string (130 hex chars)");
        }
        let canonical = if normalized.starts_with("0x") {
            normalized.to_string()
        } else {
            format!("0x{}", normalized)
        };
        (canonical, "reuse")
    } else {
        // TEE-sign branch: --escrow / --chain-id are required (the EIP-712
        // voucher domain binds to them). Reuse path skips this entirely.
        let escrow = escrow.ok_or_else(|| {
            anyhow!("--escrow is required when not using --reuse-signature")
        })?;
        let chain_id = chain_id.ok_or_else(|| {
            anyhow!("--chain-id is required when not using --reuse-signature")
        })?;

        // Resolve chain + payer (the voucher takes the generic EIP-712 path
        // and needs chainIndex / from).
        let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;

        let sig = tee_sign_voucher(
            &chain_index,
            &payer_addr,
            channel_id,
            cumulative_amount,
            escrow,
            chain_id,
        )
            .await?;
        (sig, "sign")
    };

    let payload = json!({
        "action": "voucher",
        "channelId": channel_id,
        "cumulativeAmount": cumulative_amount,
        "signature": voucher_sig,
    });

    let credential = json!({
        "challenge": build_challenge_echo(&challenge),
        "payload": payload,
    });

    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);

    output::success(json!({
        "protocol": "mpp",
        "action": "voucher",
        "mode": mode,
        "authorization_header": authorization_header,
        "channel_id": channel_id,
        "cumulative_amount": cumulative_amount,
        "signature": voucher_sig,
    }));
    Ok(())
}

/// onchainos payment mpp-session-topup: TopUp an existing session channel.
/// - Transaction mode (default): TEE-sign EIP-3009 to escrow; --currency required.
/// - Hash mode (--tx-hash): client has broadcast the topUp tx; --currency not needed.
#[allow(clippy::too_many_arguments)]
async fn cmd_mpp_session_topup(
    challenge_header: &str,
    channel_id: &str,
    additional_deposit: &str,
    escrow: &str,
    chain_id: u64,
    currency: Option<&str>,
    from: Option<&str>,
    tx_hash: Option<&str>,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;

    // Resolve chain + payer (needed in both modes for source DID)
    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    let payload = if let Some(hash) = tx_hash {
        if !hash.starts_with("0x")
            || hash.len() != 66
            || !hash[2..].chars().all(|c| c.is_ascii_hexdigit())
        {
            bail!("--tx-hash must be 0x + 64 hex chars");
        }
        json!({
            "action": "topUp",
            "type": "hash",
            "channelId": channel_id,
            "hash": hash,
            "additionalDeposit": additional_deposit,
        })
    } else {
        let currency = currency.ok_or_else(|| {
            anyhow!("--currency is required in transaction mode (omit --tx-hash for hash mode)")
        })?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let valid_before = compute_valid_before(&challenge, now)?;

        // Random 32-byte topUpSalt — contract topUpWithAuthorization uses it
        // for EIP-3009 nonce derivation. Payload field name is `topUpSalt`
        // (matches contract ABI).
        let top_up_salt = {
            use rand::RngCore;
            let mut s = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut s);
            format!("0x{}", hex::encode(s))
        };

        // EIP-3009 nonce must be derived per the contract's
        // computeTopUpAuthorizationNonce formula — random values trigger
        // EIP-3009 nonce mismatch on-chain.
        let nonce =
            compute_topup_nonce(payer_addr, channel_id, additional_deposit, &top_up_salt)?;

        // session-topUp: like open — escrow calls token.receiveWithAuthorization → Receive.
        let (signature, _) = tee_sign_eip3009(
            Eip3009AuthType::Receive,
            &chain_index,
            payer_addr,
            escrow,
            additional_deposit,
            &valid_before,
            &nonce,
            currency,
        )
            .await?;

        json!({
            "action": "topUp",
            "type": "transaction",
            "channelId": channel_id,
            "topUpSalt": top_up_salt,
            "authorization": {
                "type": "eip-3009",
                "from": payer_addr,
                "to": escrow,
                "value": additional_deposit,
                "validAfter": "0",
                "validBefore": valid_before,
                "nonce": nonce,
            },
            "signature": signature,
            "additionalDeposit": additional_deposit,
        })
    };

    let credential = json!({
        "challenge": build_challenge_echo(&challenge),
        "source": format!("did:pkh:eip155:{}:{}", chain_id, payer_addr),
        "payload": payload,
    });

    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);

    output::success(json!({
        "protocol": "mpp",
        "action": "session_topup",
        "mode": if tx_hash.is_some() { "hash" } else { "transaction" },
        "authorization_header": authorization_header,
        "channel_id": channel_id,
        "additional_deposit": additional_deposit,
        "wallet": payer_addr,
    }));
    Ok(())
}

/// onchainos payment mpp-session-close: Sign final voucher + build close credential
async fn cmd_mpp_session_close(
    channel_id: &str,
    cumulative_amount: &str,
    escrow: &str,
    chain_id: u64,
    challenge_header: &str,
    from: Option<&str>,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;

    // Resolve chain + payer (the generic EIP-712 path needs chainIndex / from).
    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    let signature = tee_sign_voucher(
        &chain_index,
        payer_addr,
        channel_id,
        cumulative_amount,
        escrow,
        chain_id,
    )
        .await?;

    let credential = json!({
        "challenge": build_challenge_echo(&challenge),
        "payload": {
            "action": "close",
            "channelId": channel_id,
            "cumulativeAmount": cumulative_amount,
            "signature": signature,
        }
    });

    let authorization_header = format!(
        "Payment {}",
        base64url_encode_json(&credential)?
    );

    output::success(json!({
        "protocol": "mpp",
        "action": "session_close",
        "authorization_header": authorization_header,
        "channel_id": channel_id,
        "cumulative_amount": cumulative_amount,
    }));
    Ok(())
}


// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ── parse_eip155_chain_id ─────────────────────────────────────────

    #[test]
    fn parse_eip155_base() {
        assert_eq!(
            payment_flow::parse_eip155_chain_id("eip155:8453").unwrap(),
            8453
        );
    }

    #[test]
    fn parse_eip155_ethereum() {
        assert_eq!(payment_flow::parse_eip155_chain_id("eip155:1").unwrap(), 1);
    }

    #[test]
    fn parse_eip155_xlayer() {
        assert_eq!(
            payment_flow::parse_eip155_chain_id("eip155:196").unwrap(),
            196
        );
    }

    #[test]
    fn parse_eip155_missing_prefix() {
        let err = payment_flow::parse_eip155_chain_id("8453").unwrap_err();
        assert!(err.to_string().contains("eip155:"));
    }

    #[test]
    fn parse_eip155_wrong_prefix() {
        let err = payment_flow::parse_eip155_chain_id("solana:101").unwrap_err();
        assert!(err.to_string().contains("eip155:"));
    }

    #[test]
    fn parse_eip155_empty() {
        assert!(payment_flow::parse_eip155_chain_id("").is_err());
    }

    #[test]
    fn parse_eip155_non_numeric() {
        let err = payment_flow::parse_eip155_chain_id("eip155:abc").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    #[test]
    fn parse_eip155_negative() {
        let err = payment_flow::parse_eip155_chain_id("eip155:-1").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    #[test]
    fn parse_eip155_overflow() {
        let err = payment_flow::parse_eip155_chain_id("eip155:99999999999999999999").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    // ── CLI argument parsing ──────────────────────────────────────────

    /// Wrapper so clap can parse PaymentCommand as a top-level subcommand.
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: PaymentCommand,
    }

    // ── CLI argument parsing ──────────────────────────────────────────

    #[test]
    fn cli_x402_pay_accepts_and_from() {
        let json = r#"[{"scheme":"aggr_deferred","network":"eip155:196","amount":"1000","payTo":"0xA","asset":"0xB"}]"#;
        let cli = TestCli::parse_from(["test", "x402-pay", "--accepts", json, "--from", "0xPayer"]);
        match cli.command {
            PaymentCommand::X402Pay { accepts, from } => {
                assert_eq!(accepts, json);
                assert_eq!(from.as_deref(), Some("0xPayer"));
            }
            _ => panic!("expected X402Pay"),
        }
    }

    #[test]
    fn cli_x402_pay_accepts_only() {
        let json = r#"[{"network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}]"#;
        let cli = TestCli::parse_from(["test", "x402-pay", "--accepts", json]);
        match cli.command {
            PaymentCommand::X402Pay { accepts, from } => {
                assert_eq!(accepts, json);
                assert_eq!(from, None);
            }
            _ => panic!("expected X402Pay"),
        }
    }

    #[test]
    fn cli_x402_pay_missing_accepts() {
        let result = TestCli::try_parse_from(["test", "x402-pay"]);
        assert!(result.is_err());
    }

    // ── eip3009-sign CLI parsing ─────────────────────────────────────

    #[test]
    fn cli_eip3009_sign_accepts_and_from() {
        let json = r#"[{"scheme":"exact","network":"eip155:8453","amount":"1000000","payTo":"0xA","asset":"0xB","extra":{"name":"USD Coin","version":"2"}}]"#;
        let cli = TestCli::parse_from(["test", "eip3009-sign", "--accepts", json]);
        match cli.command {
            PaymentCommand::Eip3009Sign { accepts } => {
                assert_eq!(accepts, json);
            }
            _ => panic!("expected Eip3009Sign"),
        }
    }

    #[test]
    fn cli_eip3009_sign_no_from_required() {
        let json = r#"[{"network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}]"#;
        let result = TestCli::try_parse_from(["test", "eip3009-sign", "--accepts", json]);
        assert!(result.is_ok(), "eip3009-sign should parse without --from");
    }

    #[test]
    fn cli_eip3009_sign_missing_accepts() {
        let result = TestCli::try_parse_from(["test", "eip3009-sign", "--from", "0xPayer"]);
        assert!(result.is_err());
    }

    // ── default subcommand CLI parsing ────────────────────────────────

    #[test]
    fn cli_default_set_passes_numeric_chain_through() {
        let cli = TestCli::parse_from([
            "test",
            "default",
            "set",
            "--asset",
            "0x1234567890123456789012345678901234567890",
            "--chain",
            "196",
            "--name",
            "USDG",
        ]);
        match cli.command {
            PaymentCommand::Default {
                action:
                    DefaultAction::Set {
                        asset,
                        chain,
                        name,
                        tier,
                    },
            } => {
                assert_eq!(asset, "0x1234567890123456789012345678901234567890");
                assert_eq!(chain, "196");
                assert_eq!(name.as_deref(), Some("USDG"));
                assert_eq!(tier, None);
            }
            _ => panic!("expected Default::Set"),
        }
    }

    #[test]
    fn cli_default_get_and_unset_parse() {
        let cli = TestCli::parse_from(["test", "default", "get"]);
        assert!(matches!(
            cli.command,
            PaymentCommand::Default {
                action: DefaultAction::Get
            }
        ));
        let cli = TestCli::parse_from(["test", "default", "unset"]);
        assert!(matches!(
            cli.command,
            PaymentCommand::Default {
                action: DefaultAction::Unset
            }
        ));
    }

    // ── chain_id_to_caip2 / caip2_to_chain_id ─────────────────────────

    #[test]
    fn chain_id_to_caip2_accepts_numeric_evm_ids() {
        assert_eq!(chain_id_to_caip2("196").unwrap(), "eip155:196");
        assert_eq!(chain_id_to_caip2("1").unwrap(), "eip155:1");
        assert_eq!(chain_id_to_caip2("  8453  ").unwrap(), "eip155:8453");
    }

    #[test]
    fn chain_id_to_caip2_rejects_non_numeric_inputs() {
        assert!(chain_id_to_caip2("xlayer").is_err());
        assert!(chain_id_to_caip2("ethereum").is_err());
        // Pre-formed CAIP-2 is rejected: only plain chain id is accepted.
        assert!(chain_id_to_caip2("eip155:196").is_err());
    }

    #[test]
    fn chain_id_to_caip2_rejects_non_evm_chains() {
        assert!(chain_id_to_caip2("195").is_err()); // TRON
        assert!(chain_id_to_caip2("501").is_err()); // Solana
        assert!(chain_id_to_caip2("607").is_err()); // TON
        assert!(chain_id_to_caip2("784").is_err()); // SUI
    }

    #[test]
    fn chain_id_to_caip2_rejects_empty_and_negative() {
        assert!(chain_id_to_caip2("").is_err());
        assert!(chain_id_to_caip2("   ").is_err());
        assert!(chain_id_to_caip2("-1").is_err());
    }

    #[test]
    fn caip2_to_chain_id_strips_prefix() {
        assert_eq!(caip2_to_chain_id("eip155:196"), "196");
        assert_eq!(caip2_to_chain_id("eip155:1"), "1");
    }

    // ── default set advances pending tiers to confirmed ──────────────

    fn tmp_home(sub: &str) -> std::path::PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(sub);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).ok();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_set_with_tier_basic_promotes_only_basic() {
        use crate::payment_cache::PaymentCache;
        use crate::payment_notify::TierState;

        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_default_set_tier_basic");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let seed = PaymentCache {
            basic_state: TierState::ChargingUnconfirmed,
            premium_state: TierState::ChargingUnconfirmed,
            ..Default::default()
        };
        seed.save().unwrap();

        cmd_default(DefaultAction::Set {
            asset: "0x1234567890123456789012345678901234567890".into(),
            chain: "196".into(),
            name: Some("USDG".into()),
            tier: Some("basic".into()),
        })
        .expect("cmd_default set succeeds");

        let loaded = PaymentCache::load().expect("cache written");
        assert_eq!(loaded.basic_state, TierState::ChargingConfirmed);
        assert_eq!(
            loaded.premium_state,
            TierState::ChargingUnconfirmed,
            "premium must stay Unconfirmed so its prompt still fires"
        );

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn default_set_without_tier_leaves_all_states_untouched() {
        use crate::payment_cache::PaymentCache;
        use crate::payment_notify::TierState;

        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_default_set_no_tier");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let seed = PaymentCache {
            basic_state: TierState::ChargingUnconfirmed,
            premium_state: TierState::ChargingUnconfirmed,
            ..Default::default()
        };
        seed.save().unwrap();

        cmd_default(DefaultAction::Set {
            asset: "0x1234567890123456789012345678901234567890".into(),
            chain: "196".into(),
            name: None,
            tier: None,
        })
        .expect("cmd_default set succeeds");

        let loaded = PaymentCache::load().expect("cache written");
        assert_eq!(loaded.basic_state, TierState::ChargingUnconfirmed);
        assert_eq!(loaded.premium_state, TierState::ChargingUnconfirmed);

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn default_set_rejects_unknown_tier() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_default_set_bad_tier");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let err = cmd_default(DefaultAction::Set {
            asset: "0x1234567890123456789012345678901234567890".into(),
            chain: "196".into(),
            name: None,
            tier: Some("gold".into()),
        })
        .unwrap_err();
        assert!(err.to_string().contains("basic"));

        std::env::remove_var("ONCHAINOS_HOME");
    }

    // ── compute_channel_id (ABI encode + keccak) ──────────────────────

    /// Known-vector test: deterministic inputs reproducible via
    /// `cast keccak $(cast abi-encode 'f(address,address,address,bytes32,address,address,uint256)' ...)`.
    /// `cast abi-encode` produces Solidity `abi.encode(...)` (the params form),
    /// matching `abi_encode_params()` in this crate.
    ///
    /// TODO: cross-check against the on-chain `computeChannelId` view function
    /// from the EvmPaymentChannel contract for end-to-end protocol parity.
    #[test]
    fn channel_id_abi_encode_roundtrip() {
        // Reproducible 7-tuple
        let payer = "0x1111111111111111111111111111111111111111";
        let payee = "0x2222222222222222222222222222222222222222";
        let token = "0x3333333333333333333333333333333333333333";
        let salt = "0x4444444444444444444444444444444444444444444444444444444444444444";
        let auth = "0x1111111111111111111111111111111111111111"; // = payer
        let escrow = "0x5555555555555555555555555555555555555555";
        let chain_id: u64 = 196;

        let got = compute_channel_id(payer, payee, token, salt, auth, escrow, chain_id).unwrap();

        // Locked-in value from `cast abi-encode` + keccak256 (Solidity
        // `abi.encode` semantics). Guards against silent changes to tuple
        // encoding or hash rules. For all-static tuples this also matches
        // alloy `abi_encode()`, but `abi_encode_params()` is the canonical
        // form and stays correct if any field becomes dynamic later.
        let expected =
            "0xa38cd33d0b42b9654d5077dccc63849159206c9da56748d8d225a1c79100e2b2";
        assert_eq!(got, expected, "channelId ABI-encoded keccak mismatch");
    }

    #[test]
    fn channel_id_different_salt_gives_different_id() {
        let a = compute_channel_id(
            "0x1111111111111111111111111111111111111111",
            "0x2222222222222222222222222222222222222222",
            "0x3333333333333333333333333333333333333333",
            "0x4444444444444444444444444444444444444444444444444444444444444444",
            "0x1111111111111111111111111111111111111111",
            "0x5555555555555555555555555555555555555555",
            196,
        )
        .unwrap();
        let b = compute_channel_id(
            "0x1111111111111111111111111111111111111111",
            "0x2222222222222222222222222222222222222222",
            "0x3333333333333333333333333333333333333333",
            "0x5555555555555555555555555555555555555555555555555555555555555555",
            "0x1111111111111111111111111111111111111111",
            "0x5555555555555555555555555555555555555555",
            196,
        )
        .unwrap();
        assert_ne!(a, b, "different salts must produce different channelIds");
    }

    #[test]
    fn channel_id_rejects_bad_salt_length() {
        let e = compute_channel_id(
            "0x1111111111111111111111111111111111111111",
            "0x2222222222222222222222222222222222222222",
            "0x3333333333333333333333333333333333333333",
            "0xdeadbeef",
            "0x1111111111111111111111111111111111111111",
            "0x5555555555555555555555555555555555555555",
            196,
        )
        .unwrap_err();
        assert!(e.to_string().contains("32 bytes"));
    }

    #[test]
    fn channel_id_rejects_bad_address() {
        let e = compute_channel_id(
            "not-an-address",
            "0x2222222222222222222222222222222222222222",
            "0x3333333333333333333333333333333333333333",
            "0x4444444444444444444444444444444444444444444444444444444444444444",
            "0x1111111111111111111111111111111111111111",
            "0x5555555555555555555555555555555555555555",
            196,
        )
        .unwrap_err();
        assert!(e.to_string().contains("invalid payer address"));
    }

    // ── base64url_encode_json (JCS canonicalization) ─────────────────

    #[test]
    fn base64url_encode_canonical_sorts_keys() {
        // Two semantically identical JSON objects with different key ordering
        // must encode to the same string under JCS.
        let v1 = json!({"b": 2, "a": 1, "c": 3});
        let v2 = json!({"a": 1, "b": 2, "c": 3});
        let v3 = json!({"c": 3, "b": 2, "a": 1});
        let e1 = base64url_encode_json(&v1).unwrap();
        let e2 = base64url_encode_json(&v2).unwrap();
        let e3 = base64url_encode_json(&v3).unwrap();
        assert_eq!(e1, e2, "JCS must sort keys deterministically");
        assert_eq!(e2, e3, "JCS must sort keys deterministically");
    }

    #[test]
    fn base64url_encode_no_padding() {
        // JCS of {"k":"v"} is 9 bytes — base64 would pad; URL_SAFE_NO_PAD must not.
        let enc = base64url_encode_json(&json!({"k": "v"})).unwrap();
        assert!(!enc.ends_with('='), "no padding allowed");
    }

    #[test]
    fn base64url_encode_nested_keys_sorted() {
        let v1 = json!({"outer": {"z": 1, "a": 2}});
        let v2 = json!({"outer": {"a": 2, "z": 1}});
        assert_eq!(
            base64url_encode_json(&v1).unwrap(),
            base64url_encode_json(&v2).unwrap(),
            "JCS must recurse into nested objects"
        );
    }

    // ── CLI argument parsing: new MPP shapes ─────────────────────────

    #[test]
    fn cli_mpp_session_voucher_requires_challenge() {
        // Without --challenge, parsing must fail
        let result = TestCli::try_parse_from([
            "test",
            "mpp-session-voucher",
            "--channel-id",
            "0xabc",
            "--cumulative-amount",
            "100",
            "--escrow",
            "0xdef",
            "--chain-id",
            "196",
        ]);
        assert!(result.is_err(), "--challenge should now be required");
    }

    #[test]
    fn cli_mpp_charge_accepts_tx_hash() {
        let cli = TestCli::parse_from([
            "test",
            "mpp-charge",
            "--challenge",
            "Payment id=\"1\", realm=\"r\", method=\"evm\", intent=\"charge\", request=\"e30\"",
            "--tx-hash",
            "0x1111111111111111111111111111111111111111111111111111111111111111",
        ]);
        match cli.command {
            PaymentCommand::MppCharge { tx_hash, .. } => {
                assert_eq!(tx_hash.as_deref().map(|s| s.len()), Some(66));
            }
            _ => panic!("expected MppCharge"),
        }
    }

    #[test]
    fn cli_mpp_session_open_accepts_tx_hash() {
        let cli = TestCli::parse_from([
            "test",
            "mpp-session-open",
            "--challenge",
            "Payment id=\"1\", realm=\"r\", method=\"evm\", intent=\"session\", request=\"e30\"",
            "--deposit",
            "1000000",
            "--tx-hash",
            "0x2222222222222222222222222222222222222222222222222222222222222222",
        ]);
        match cli.command {
            PaymentCommand::MppSessionOpen { tx_hash, .. } => {
                assert!(tx_hash.is_some());
            }
            _ => panic!("expected MppSessionOpen"),
        }
    }

    #[test]
    fn cli_mpp_session_topup_transaction_mode() {
        let cli = TestCli::parse_from([
            "test",
            "mpp-session-topup",
            "--challenge",
            "Payment id=\"1\", realm=\"r\", method=\"evm\", intent=\"session\", request=\"e30\"",
            "--channel-id",
            "0xchan",
            "--additional-deposit",
            "500000",
            "--escrow",
            "0xescrow",
            "--chain-id",
            "196",
            "--currency",
            "0xUSDC",
        ]);
        match cli.command {
            PaymentCommand::MppSessionTopUp {
                tx_hash, currency, ..
            } => {
                assert!(tx_hash.is_none());
                assert_eq!(currency.as_deref(), Some("0xUSDC"));
            }
            _ => panic!("expected MppSessionTopUp"),
        }
    }

    #[test]
    fn cli_mpp_session_topup_hash_mode() {
        let cli = TestCli::parse_from([
            "test",
            "mpp-session-topup",
            "--challenge",
            "Payment id=\"1\", realm=\"r\", method=\"evm\", intent=\"session\", request=\"e30\"",
            "--channel-id",
            "0xchan",
            "--additional-deposit",
            "500000",
            "--escrow",
            "0xescrow",
            "--chain-id",
            "196",
            "--tx-hash",
            "0x3333333333333333333333333333333333333333333333333333333333333333",
        ]);
        match cli.command {
            PaymentCommand::MppSessionTopUp { tx_hash, .. } => {
                assert!(tx_hash.is_some());
            }
            _ => panic!("expected MppSessionTopUp"),
        }
    }

    #[test]
    fn cli_mpp_session_topup_requires_required_fields() {
        // missing --channel-id
        let result = TestCli::try_parse_from([
            "test",
            "mpp-session-topup",
            "--challenge",
            "Payment id=\"1\", realm=\"r\", method=\"evm\", intent=\"session\", request=\"e30\"",
            "--additional-deposit",
            "500000",
            "--escrow",
            "0xescrow",
            "--chain-id",
            "196",
        ]);
        assert!(result.is_err());
    }

    // ── parse_www_authenticate ───────────────────────────────────────

    #[test]
    fn parse_www_authenticate_extracts_fields() {
        let header = "Payment id=\"abc123\", realm=\"api.shop.com\", method=\"evm\", intent=\"charge\", request=\"eyJyZWNpcGllbnQiOiJBIn0\"";
        let parsed = parse_www_authenticate(header).unwrap();
        assert_eq!(parsed["id"].as_str(), Some("abc123"));
        assert_eq!(parsed["realm"].as_str(), Some("api.shop.com"));
        assert_eq!(parsed["method"].as_str(), Some("evm"));
        assert_eq!(parsed["intent"].as_str(), Some("charge"));
    }

    #[test]
    fn parse_www_authenticate_rejects_missing_required() {
        let header = "Payment realm=\"x\""; // missing id/method/intent
        assert!(parse_www_authenticate(header).is_err());
    }

    #[test]
    fn parse_www_authenticate_rejects_non_evm_method() {
        // tempo / svm / stripe etc. must be rejected loudly — this CLI is EVM-only.
        let tempo = "Payment id=\"a\", realm=\"r\", method=\"tempo\", intent=\"charge\", request=\"e30\"";
        let err = parse_www_authenticate(tempo).unwrap_err();
        assert!(
            err.to_string().contains("unsupported MPP method")
                && err.to_string().contains("tempo"),
            "error should name the unsupported method: {}",
            err
        );

        let svm = "Payment id=\"a\", realm=\"r\", method=\"svm\", intent=\"charge\", request=\"e30\"";
        assert!(parse_www_authenticate(svm).is_err());

        // Sanity: evm still accepted.
        let evm = "Payment id=\"a\", realm=\"r\", method=\"evm\", intent=\"charge\", request=\"e30\"";
        assert!(parse_www_authenticate(evm).is_ok());
    }

    #[test]
    fn parse_www_authenticate_handles_quoted_value_with_comma() {
        // Embedded comma inside a quoted value must NOT split the pair.
        let header = "Payment id=\"a\", realm=\"r\", method=\"evm\", intent=\"charge\", request=\"e30\", description=\"buy coffee, with sugar\"";
        let parsed = parse_www_authenticate(header).unwrap();
        assert_eq!(parsed["description"].as_str(), Some("buy coffee, with sugar"));
        assert_eq!(parsed["request"].as_str(), Some("e30"));
    }

    #[test]
    fn parse_www_authenticate_tolerates_extra_whitespace_and_single_space_separator() {
        // Some servers separate pairs with a single space (not double). The parser
        // should still extract every field correctly.
        let header = "Payment id=\"a\",realm=\"r\", method=\"evm\" ,intent=\"charge\",request=\"e30\"";
        let parsed = parse_www_authenticate(header).unwrap();
        assert_eq!(parsed["id"].as_str(), Some("a"));
        assert_eq!(parsed["realm"].as_str(), Some("r"));
        assert_eq!(parsed["intent"].as_str(), Some("charge"));
    }

    #[test]
    fn parse_www_authenticate_unescapes_backslash_in_quoted_value() {
        // Backslash escape inside quoted-string: \" produces literal ", \\ produces \.
        let header = "Payment id=\"a\", realm=\"r\", method=\"evm\", intent=\"charge\", request=\"e30\", note=\"hello \\\"world\\\"\"";
        let parsed = parse_www_authenticate(header).unwrap();
        assert_eq!(parsed["note"].as_str(), Some("hello \"world\""));
    }

    // ── compute_primary_split_amounts ────────────────────────────────

    #[test]
    fn primary_split_no_splits_returns_amount() {
        let req = json!({
            "amount": "100",
            "currency": "0xabc",
            "recipient": "0xdef",
            "methodDetails": { "chainId": 196 }
        });
        let (primary, splits) = compute_primary_split_amounts(&req).unwrap();
        assert_eq!(primary, "100");
        assert!(splits.is_empty());
    }

    #[test]
    fn primary_split_subtracts_sum_per_spec_example() {
        // Spec §Split Payments example: amount=1_000_000, splits=[50_000, 10_000] → primary=940_000.
        let req = json!({
            "amount": "1000000",
            "methodDetails": {
                "chainId": 196,
                "splits": [
                    { "amount": "50000", "recipient": "0x1111111111111111111111111111111111111111" },
                    { "amount": "10000", "recipient": "0x2222222222222222222222222222222222222222" },
                ]
            }
        });
        let (primary, splits) = compute_primary_split_amounts(&req).unwrap();
        assert_eq!(primary, "940000");
        assert_eq!(splits.len(), 2);
        assert_eq!(splits[0].0, "50000");
        assert_eq!(splits[1].0, "10000");
    }

    #[test]
    fn primary_split_matches_log_case_100_30_20_yields_50() {
        // The case from the reported bug: amount=100, splits=[30, 20] → primary=50.
        let req = json!({
            "amount": "100",
            "methodDetails": {
                "splits": [
                    { "amount": "30", "recipient": "0x0300b4b34d3403e66afd04c789594d103962ef2f" },
                    { "amount": "20", "recipient": "0x63180b1fd707ee3bbb3a4c7b6410b95860c536eb" },
                ]
            }
        });
        let (primary, _) = compute_primary_split_amounts(&req).unwrap();
        assert_eq!(primary, "50");
    }

    #[test]
    fn primary_split_rejects_sum_equal_to_amount() {
        // Spec §Constraints: sum MUST be strictly less than amount.
        let req = json!({
            "amount": "100",
            "methodDetails": { "splits": [ { "amount": "100", "recipient": "0xabc" } ] }
        });
        let err = compute_primary_split_amounts(&req).unwrap_err().to_string();
        assert!(err.contains("strictly less than"), "got: {}", err);
    }

    #[test]
    fn primary_split_rejects_sum_greater_than_amount() {
        let req = json!({
            "amount": "100",
            "methodDetails": { "splits": [
                { "amount": "70", "recipient": "0xa" },
                { "amount": "40", "recipient": "0xb" },
            ] }
        });
        assert!(compute_primary_split_amounts(&req).is_err());
    }

    #[test]
    fn primary_split_rejects_zero_amount_entry() {
        let req = json!({
            "amount": "100",
            "methodDetails": { "splits": [ { "amount": "0", "recipient": "0xabc" } ] }
        });
        let err = compute_primary_split_amounts(&req).unwrap_err().to_string();
        assert!(err.contains("> 0"), "got: {}", err);
    }

    #[test]
    fn primary_split_rejects_empty_splits_array() {
        let req = json!({
            "amount": "100",
            "methodDetails": { "splits": [] }
        });
        let err = compute_primary_split_amounts(&req).unwrap_err().to_string();
        assert!(err.contains("empty"), "got: {}", err);
    }

    #[test]
    fn primary_split_rejects_more_than_ten_splits() {
        let many: Vec<_> = (0..11)
            .map(|_| json!({ "amount": "1", "recipient": "0xabc" }))
            .collect();
        let req = json!({
            "amount": "1000",
            "methodDetails": { "splits": many }
        });
        let err = compute_primary_split_amounts(&req).unwrap_err().to_string();
        assert!(err.contains("exceeds spec max of 10"), "got: {}", err);
    }

    #[test]
    fn primary_split_rejects_non_base10_amount() {
        let req = json!({
            "amount": "0x64",
            "methodDetails": { "splits": [ { "amount": "10", "recipient": "0xabc" } ] }
        });
        assert!(compute_primary_split_amounts(&req).is_err());
    }

    // ── compute_valid_before ─────────────────────────────────────────

    #[test]
    fn valid_before_falls_back_to_now_plus_default_when_expires_missing() {
        let ch = json!({ "id": "x" });
        let vb = compute_valid_before(&ch, 1_000).unwrap();
        assert_eq!(vb, "1300"); // 1000 + 300 default
    }

    #[test]
    fn valid_before_honors_expires_past_default_window() {
        // Challenge expires 600s in the future; default is 300s. Spec requires
        // validBefore >= challenge.expires, so we go past expires + 60s grace.
        let ch = json!({ "expires": "1970-01-01T00:10:00Z" }); // Unix 600
        let vb = compute_valid_before(&ch, 0).unwrap();
        assert_eq!(vb, "660"); // 600 + 60s grace
    }

    #[test]
    fn valid_before_keeps_floor_when_expires_sooner_than_default() {
        // Challenge expires in 100s, default floor is 300s. Use the floor.
        let ch = json!({ "expires": "1970-01-01T00:01:40Z" }); // Unix 100
        let vb = compute_valid_before(&ch, 0).unwrap();
        assert_eq!(vb, "300");
    }

    #[test]
    fn valid_before_rejects_already_expired_challenge() {
        let ch = json!({ "expires": "2000-01-01T00:00:00Z" });
        let err = compute_valid_before(&ch, 2_000_000_000).unwrap_err().to_string();
        assert!(err.contains("already in the past"), "got: {}", err);
    }

    #[test]
    fn valid_before_rejects_malformed_expires() {
        let ch = json!({ "expires": "not-a-timestamp" });
        assert!(compute_valid_before(&ch, 1_000).is_err());
    }

    // ── build_voucher_typed_data ──────────────────────────────────────
    //
    // These assertions lock the exact shape of the EIP-712 typed data. Any
    // change must be synced with the seller SDK and on-chain contract;
    // otherwise verification fails (settle / close get rejected).

    #[test]
    fn voucher_typed_data_domain_name_is_evm_payment_channel() {
        let td = build_voucher_typed_data("0xchan", "1000", "0xesc", 196);
        assert_eq!(td["domain"]["name"], "EVM Payment Channel");
        assert_eq!(td["domain"]["version"], "1");
    }

    #[test]
    fn voucher_typed_data_domain_carries_chain_id_and_escrow() {
        let td = build_voucher_typed_data(
            "0xchan",
            "0",
            "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1",
            196,
        );
        assert_eq!(td["domain"]["chainId"], 196);
        assert_eq!(
            td["domain"]["verifyingContract"],
            "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1"
        );
    }

    #[test]
    fn voucher_typed_data_struct_uses_uint128_cumulative_amount() {
        // cumulativeAmount must be uint128 to match the contract. uint256
        // would change the typehash and break every signature.
        let td = build_voucher_typed_data("0xchan", "1000", "0xesc", 196);
        let voucher_fields = td["types"]["Voucher"].as_array().unwrap();
        assert_eq!(voucher_fields.len(), 2);
        assert_eq!(voucher_fields[0]["name"], "channelId");
        assert_eq!(voucher_fields[0]["type"], "bytes32");
        assert_eq!(voucher_fields[1]["name"], "cumulativeAmount");
        assert_eq!(voucher_fields[1]["type"], "uint128");
    }

    #[test]
    fn voucher_typed_data_primary_type_is_voucher() {
        let td = build_voucher_typed_data("0xchan", "0", "0xesc", 196);
        assert_eq!(td["primaryType"], "Voucher");
    }

    #[test]
    fn voucher_typed_data_message_is_passthrough_strings() {
        // Strings pass through verbatim — cumulativeAmount stays as the
        // caller's decimal string; the TEE parses it as uint128.
        let td = build_voucher_typed_data(
            "0x1111111111111111111111111111111111111111111111111111111111111111",
            "250000",
            "0xesc",
            196,
        );
        assert_eq!(
            td["message"]["channelId"],
            "0x1111111111111111111111111111111111111111111111111111111111111111"
        );
        assert_eq!(td["message"]["cumulativeAmount"], "250000");
    }

    #[test]
    fn voucher_typed_data_eip712_domain_has_four_fields() {
        // EIP712Domain field order is part of the typehash — do not reorder.
        let td = build_voucher_typed_data("0xchan", "0", "0xesc", 196);
        let domain_fields = td["types"]["EIP712Domain"].as_array().unwrap();
        let names: Vec<&str> = domain_fields
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["name", "version", "chainId", "verifyingContract"]);
    }
}
