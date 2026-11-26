use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::payment::a2a_pay::{self, A2aPayCommand};
use crate::commands::payment::addr::{is_valid_evm_address, parse_recipient_addr};
use crate::commands::payment::payment_flow;
use crate::commands::payment::{
    decode_receipt, quote, session_state, subscription_operation_state,
};
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::{keyring_store, wallet_store};
use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::SolValue;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::Subcommand;
use serde_json::{json, Value};
use zeroize::Zeroize;

#[derive(Subcommand)]
pub enum PaymentCommand {
    /// Sign a payment authorization for an HTTP 402-gated resource and return the
    /// assembled `PAYMENT-SIGNATURE` header (x402 v2). Signs from the currently
    /// selected wallet account via TEE.
    #[command(name = "pay")]
    X402Pay {
        /// base64 of the decoded 402 payload `{x402Version, resource, accepts}`
        /// — i.e. the raw `PAYMENT-REQUIRED` header value (base64 or base64url).
        /// The CLI decodes it, signs the selected accepts entry, and (v2) returns
        /// `{authorization_header, header_name, scheme, wallet}`. Legacy sign-only
        /// mode; mutually exclusive with `--payment-id`.
        #[arg(
            long,
            required_unless_present = "payment_id",
            conflicts_with = "payment_id"
        )]
        payload: Option<String>,
        /// Two-phase mode: complete the payment persisted by `payment quote`.
        /// Reads the paymentId state, signs via TEE, replays to the merchant, and
        /// returns the receipt. Never re-fetches the 402.
        #[arg(long)]
        payment_id: Option<String>,
        /// Optional 0-based index into the payload's `accepts[]` to sign. Use it
        /// to pin the exact scheme the user chose from a multi-scheme prompt.
        /// Omit to let the CLI auto-select (exact > aggr_deferred > first).
        #[arg(long)]
        selected_index: Option<usize>,
        /// Bind legacy payload signing to this payer instead of the selected account.
        #[arg(long, requires = "payload", conflicts_with = "payment_id")]
        from: Option<String>,
        /// Additional business param, repeatable: --param key=value. Passed to the
        /// merchant on replay (two-phase mode).
        #[arg(long = "param")]
        param: Vec<String>,
        /// Approve the fund-moving payment (bypass the confirming gate). Without
        /// it, two-phase `pay` returns a confirming prompt (exit 2).
        #[arg(long, visible_alias = "force")]
        yes: bool,
    },
    /// Check a NewsLiquid order/challenge or signed proof from stdin. No network or signing.
    Validate {
        #[arg(long, value_parser = ["newsliquid"])]
        service: String,
        #[arg(long, value_parser = ["prepare", "before-sign", "after-sign"])]
        stage: String,
        /// Wallet whose payment terms were selected and confirmed.
        #[arg(long)]
        from: String,
    },
    /// Probe a 402/A2MCP endpoint, parse the challenge, preflight balance,
    /// rank candidates, and persist a paymentId. Never signs.
    Quote {
        /// The A2MCP / merchant endpoint to probe.
        url: String,
        /// Known business param, repeatable: --param key=value
        #[arg(long = "param")]
        param: Vec<String>,
        /// HTTP method to probe the endpoint with (GET by default). Use POST/PUT/
        /// PATCH for A2MCP endpoints whose paid call is not a GET — known params
        /// then ride in the JSON body instead of the query string.
        #[arg(long, default_value = "GET")]
        method: String,
        /// MCP tool to invoke via tools/call. Forces the MCP branch; omit for tool discovery.
        #[arg(long)]
        tool: Option<String>,
    },
    /// Decode an x402 PAYMENT-RESPONSE header or a charge receipt into
    /// {status, transaction, amount, payer, chainId}. Read-only.
    DecodeReceipt {
        /// x402 PAYMENT-RESPONSE header (base64/base64url).
        #[arg(long, required_unless_present = "receipt", conflicts_with = "receipt")]
        header: Option<String>,
        /// Raw charge receipt JSON.
        #[arg(long)]
        receipt: Option<String>,
    },
    /// Sign an x402 payment locally with a hex private key (reads EVM_PRIVATE_KEY env var).
    /// Supports `exact + EIP-3009`, `exact + Permit2`, and `upto`; `aggr_deferred` is TEE-only.
    /// Same base64 `--payload` as `payment pay`; domain name/version are read from the
    /// selected entry's `extra.name` / `extra.version` for the EIP-3009 path.
    #[command(name = "pay-local")]
    Eip3009Sign {
        /// base64 of the decoded 402 payload (same as `payment pay --payload`).
        #[arg(long)]
        payload: String,
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

    /// Sign a one-shot Charge payment (EIP-3009 via TEE) or wrap a client-broadcast hash.
    /// Returns authorization_header for replaying the request.
    /// When challenge.methodDetails.feePayer == false, --tx-hash is required (hash mode).
    /// Splits are auto-detected from challenge.methodDetails.splits[] (max 10).
    #[command(name = "charge")]
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

    /// Channel session operations: open / voucher / topup / close.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },

    /// Subscription payments (x402 `period` scheme):
    /// subscribe / access / change / cancel / my-subscriptions / allowance-status.
    Subscription {
        #[command(subcommand)]
        command: crate::commands::payment::subscription::SubscriptionCommand,
    },

    /// APP/1 protocol (`protocol="app/1"`) buyer-side signing. Same
    /// `WWW-Authenticate: Payment` challenge wire-form as MPP, but the credential
    /// echoes `protocol` + `opaque` (both bound into `challenge.id`'s HMAC) and
    /// uses APP/1 payload shapes. Isolated from the MPP `charge`/`session` paths
    /// so those stay byte-for-byte untouched.
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
}

/// APP/1 (`protocol="app/1"`) buyer intents. Reuses the same TEE signing
/// primitives as MPP but assembles the APP/1 credential envelope.
#[derive(Subcommand)]
pub enum AppCommand {
    /// One-shot exact-amount charge. Signs `authorization` (EIP-3009 via TEE),
    /// `permit2` (SignatureTransfer+witness via TEE), or `hash` (wrap a
    /// client-broadcast tx, requires `--tx-hash`, needs `feePayer=false`).
    /// Type defaults to `authorization` (feePayer=true) / `hash` (feePayer=false).
    Charge {
        /// Full `WWW-Authenticate: Payment ...` header value from the 402 response.
        #[arg(long)]
        challenge: String,
        /// Payer address (optional, defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
        /// Tx hash of a client-broadcast `transferWithAuthorization` (hash mode).
        /// Required when `challenge.methodDetails.feePayer=false`.
        #[arg(long)]
        tx_hash: Option<String>,
        /// Signature type: `authorization` | `permit2` | `hash`. Omit to auto-pick
        /// by `feePayer`.
        #[arg(long = "type")]
        type_: Option<String>,
    },
    /// Variable-amount authorization up to a ceiling. Signs `authorization`
    /// (EIP-3009) or `permit2` for the ceiling (`challenge.request.amount`); the
    /// seller settles the actual amount (≤ ceiling) via the facilitator. No hash
    /// mode (the seller must relay so it can settle a lower actual).
    Upto {
        /// Full `WWW-Authenticate: Payment ...` header value from the 402 response.
        #[arg(long)]
        challenge: String,
        /// Payer address (optional, defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
        /// Signature type. Omit to auto-pick from the challenge's acceptedTypes
        /// (upto only offers `permit2`).
        #[arg(long = "type")]
        type_: Option<String>,
    },
    /// Streaming channel session: open / voucher / topup / close. Reuses the MPP
    /// channel primitives (channelId derivation, deposit EIP-3009, voucher
    /// EIP-712) but assembles APP/1 payloads — opens carry no initial voucher and
    /// the voucher field is `voucherSignature`.
    Session {
        #[command(subcommand)]
        command: AppSessionCommand,
    },
    /// Periodic subscription: `subscribe` (Permit2 AllowanceTransfer, one
    /// signature authorizes many periods) / `cancel` / `cancel-pending`.
    Subscription {
        #[command(subcommand)]
        command: AppSubscriptionCommand,
    },
}

/// APP/1 subscription sub-operations. `subscribe` signs a PermitSingle
/// (AllowanceTransfer) to create the subscription; `cancel` / `cancel-pending`
/// sign the cancel authorizations the seller submits. Per-period charges are
/// seller-driven (no buyer signature).
#[derive(Subcommand)]
pub enum AppSubscriptionCommand {
    /// Sign the PermitSingle allowance that creates the subscription. spender /
    /// permit2 contract / on-chain uint48 nonce come from the facilitator's
    /// allowance-status. NOTE: the SIGNED permit amount is CUMULATIVE — it is
    /// `reservedAmount (all your active subs' committed total) + this sub's
    /// commitment`, NOT the challenge's `allowance` (which is only this sub's
    /// share). Being many× the challenge value is expected, not over-authorization
    /// (Permit2 has ONE allowance per owner/token/spender). See references/app.md.
    Subscribe {
        /// Full `WWW-Authenticate: Payment ...` header (intent=subscription).
        #[arg(long)]
        challenge: String,
        /// Resource URL used as the subscription reconciliation key.
        #[arg(long)]
        url: Option<String>,
        /// Cumulative Permit2 cap confirmed by the user after allowance-status.
        #[arg(long)]
        expected_cap: String,
        /// Preserve the active mapping and stage metadata for later reconciliation.
        #[arg(long)]
        no_cache: bool,
        /// Payer address (optional, defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
    },
    /// Sign an up/downgrade from a change-offer 402 (challenge carries
    /// `changeFrom`). Same double-sign as subscribe, but migrates the subId.
    Change {
        /// Full `WWW-Authenticate: Payment ...` header (intent=subscription,
        /// request contains `changeFrom`).
        #[arg(long)]
        challenge: String,
        /// The subId being changed (overrides `challenge.request.changeFrom.fromSubId`).
        #[arg(long)]
        sub_id: Option<String>,
        /// Resource URL used as the subscription reconciliation key.
        #[arg(long)]
        url: Option<String>,
        /// Cumulative Permit2 cap confirmed by the user after allowance-status.
        #[arg(long)]
        expected_cap: String,
        /// Preserve the active mapping and stage metadata for later reconciliation.
        #[arg(long)]
        no_cache: bool,
        /// Payer address (optional, defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
    },
    /// Sign a cancelAuthorization (payer-initiated) for `POST /subscription/cancel`.
    Cancel {
        #[arg(long)]
        sub_id: String,
        /// Seller business key. Binds the pending operation to its receipt.
        #[arg(long)]
        offer_id: String,
        /// Subscription contract (EIP-712 verifyingContract for the cancel auth).
        /// Optional — if omitted, resolved from APP/1 allowance-status via `--token`
        /// (mirrors the x402 `resolve_contract` convenience).
        #[arg(long)]
        subscription_contract: Option<String>,
        /// Token contract — used to look up the subscription contract when
        /// `--subscription-contract` is omitted.
        #[arg(long)]
        token: Option<String>,
        #[arg(long)]
        chain_id: u64,
        #[arg(long)]
        from: Option<String>,
    },
    /// Sign an authorization to cancel a not-yet-effective downgrade, for
    /// `POST /subscription/cancel-pending-change`.
    CancelPending {
        #[arg(long)]
        sub_id: String,
        /// The pending downgrade's newSubId being canceled.
        #[arg(long)]
        new_sub_id: String,
        /// Seller business key. Binds the pending operation to its receipt.
        #[arg(long)]
        offer_id: String,
        /// Optional — resolved from APP/1 allowance-status via `--token` if omitted.
        #[arg(long)]
        subscription_contract: Option<String>,
        /// Token contract — used to resolve the subscription contract when
        /// `--subscription-contract` is omitted.
        #[arg(long)]
        token: Option<String>,
        #[arg(long)]
        chain_id: u64,
        #[arg(long)]
        from: Option<String>,
    },
    /// Resolve a signed cancel or cancel-pending operation after seller replay.
    Resolve {
        #[arg(long)]
        operation_id: String,
        #[arg(long, required_unless_present = "outcome", conflicts_with = "outcome")]
        receipt: Option<String>,
        #[arg(long, value_parser = ["network-error", "no-receipt"])]
        outcome: Option<String>,
    },
    /// Build an `APP-Access` proof header to access a Seller's protected resource
    /// after subscribing. Signs `personal_sign(keccak(subId ‖ payer ‖ timestamp))`;
    /// the Seller verifies the signer + checks the subscription is active.
    Access {
        /// Resource URL — resolves the active subId from the host→subId cache
        /// (written by `subscribe`/`change --url`). No manual subId needed.
        #[arg(long)]
        url: Option<String>,
        /// Explicit subId override (skips the cache lookup).
        #[arg(long)]
        sub_id: Option<String>,
        /// Payer address (optional, defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
        /// Chain name or index (default: xlayer).
        #[arg(long, default_value = "xlayer")]
        chain: String,
    },
    /// List the buyer's own APP/1 subscriptions through the buyer-direct endpoint.
    List {
        /// Buyer address (optional, defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
        /// Page size (1..=100).
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Page offset.
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// Query the APP/1 Permit2 allowance status: current uint48 nonce,
    /// reserved/committed amount, token balance, and the subscription/permit2
    /// contract addresses. Read-only; mirrors the x402 `allowance-status`.
    AllowanceStatus {
        /// Token contract (the subscription's settlement token).
        #[arg(long)]
        token: String,
        /// Chain id (numeric, e.g. 196 for X Layer).
        #[arg(long)]
        chain_id: u64,
        /// Buyer address (optional, defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
    },
}

/// APP/1 session sub-operations. `open` deposits and derives the channelId;
/// `voucher` signs a monotonic cumulative-amount voucher; `topup` adds deposit;
/// `close` signs the final voucher for the seller to submit to `/session/close`.
#[derive(Subcommand)]
pub enum AppSessionCommand {
    /// Open a channel: deposit authorization (EIP-3009 via TEE) or `--tx-hash`
    /// hash mode. APP/1 opens are deposit-only (no initial voucher).
    Open {
        /// Full `WWW-Authenticate: Payment ...` header (intent=session).
        #[arg(long)]
        challenge: String,
        /// Payer address (optional, defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
        /// Hash mode: tx hash of your on-chain open (requires `--salt`).
        #[arg(long)]
        tx_hash: Option<String>,
        /// Hash mode: the bytes32 salt used in your on-chain open.
        #[arg(long)]
        salt: Option<String>,
        /// Delegate voucher signer (must not equal payer); omit for self-signing.
        #[arg(long)]
        authorized_signer: Option<String>,
    },
    /// Sign a cumulative-amount voucher (EIP-712) → `voucherSignature`. The
    /// seller adds its own `payeeSignature`/`nonce`/`deadline` when settling.
    Voucher {
        /// The open challenge header (voucher echoes the same challenge id).
        #[arg(long)]
        challenge: String,
        #[arg(long)]
        channel_id: String,
        /// New absolute cumulative amount (strictly increasing), atomic units.
        #[arg(long)]
        cumulative_amount: String,
        /// Escrow/channel contract (binds the voucher EIP-712 domain).
        #[arg(long)]
        escrow: String,
        #[arg(long)]
        chain_id: u64,
        #[arg(long)]
        from: Option<String>,
    },
    /// Add deposit to a channel: EIP-3009 via TEE, or `--tx-hash` hash mode.
    Topup {
        #[arg(long)]
        challenge: String,
        #[arg(long)]
        channel_id: String,
        #[arg(long)]
        additional_deposit: String,
        #[arg(long)]
        escrow: String,
        #[arg(long)]
        chain_id: u64,
        /// Token address; required in transaction mode (omit for `--tx-hash`).
        #[arg(long)]
        currency: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        tx_hash: Option<String>,
    },
    /// Sign the final voucher for channel close. Emits `voucherSignature` +
    /// `cumulativeAmount`; the seller submits them to `POST /session/close`.
    Close {
        #[arg(long)]
        challenge: String,
        #[arg(long)]
        channel_id: String,
        #[arg(long)]
        cumulative_amount: String,
        #[arg(long)]
        escrow: String,
        #[arg(long)]
        chain_id: u64,
        #[arg(long)]
        from: Option<String>,
    },
    /// Resolve a signed topup or close after replaying it to the seller.
    Resolve {
        #[arg(long)]
        operation_id: String,
        #[arg(long, required_unless_present = "outcome", conflicts_with = "outcome")]
        receipt: Option<String>,
        #[arg(long, value_parser = ["network-error", "no-receipt"])]
        outcome: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum SessionCommand {
    /// Open a payment channel.
    /// - feePayer=true (default): TEE-sign EIP-3009 deposit + initial voucher, emit transaction payload.
    /// - feePayer=false: require --tx-hash AND --salt for the open tx you broadcast; still TEE-sign initial voucher.
    Open {
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
        /// Hash mode only: the bytes32 salt (0x + 64 hex) you passed to
        /// `escrow.open(...)` on-chain. Required with `--tx-hash`; mismatch
        /// produces a channelId reject. Ignored in transaction mode.
        #[arg(long)]
        salt: Option<String>,
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

    /// Sign an EIP-712 cumulative-amount voucher via TEE, or reuse a previously-signed
    /// voucher's bytes (`--reuse-signature`) to spend remaining channel balance without
    /// invoking TEE. Returns authorization_header for replaying business requests.
    Voucher {
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
        /// returned by an earlier `payment session voucher` / `payment session open` call.
        /// When provided, the CLI skips TEE signing and emits an
        /// authorization_header that wraps these bytes verbatim. Use to
        /// spend remaining balance under an existing voucher without
        /// burning a fresh signature. Server must support voucher reuse
        /// (mppx, OKX TS Session — OKX Rust SDK from this version onward).
        #[arg(long)]
        reuse_signature: Option<String>,
    },

    /// TopUp a payment channel (EIP-3009 to escrow via TEE, or hash-mode wrap).
    /// Returns authorization_header for sending topUp to Seller.
    #[command(name = "topup")]
    TopUp {
        /// Full WWW-Authenticate challenge header (for credential echo)
        #[arg(long)]
        challenge: String,
        /// Existing channel ID (from `payment session open`)
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

    /// Close a payment channel (sign final voucher via TEE).
    /// Returns authorization_header for sending close to Seller.
    Close {
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
        PaymentCommand::Validate {
            service: _,
            stage,
            from,
        } => {
            use std::io::Read;
            let mut input = Vec::new();
            std::io::stdin()
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut input)?;
            if input.len() > 8 * 1024 * 1024 {
                bail!("payment_validation_input_too_large");
            }
            let input = serde_json::from_slice(&input)
                .map_err(|_| anyhow!("invalid_payment_validation_json"))?;
            output::success(crate::services::newsliquid::validate_payment(
                &input, &from, &stage,
            )?);
            Ok(())
        }
        PaymentCommand::X402Pay {
            payload,
            payment_id,
            selected_index,
            from,
            param,
            yes,
        } => {
            if let Some(pid) = payment_id {
                // Two-phase mode: complete a previously-quoted payment.
                cmd_pay_two_phase(&pid, selected_index, &param, yes).await
            } else {
                // Legacy sign-only mode — byte-for-byte unchanged.
                let payload = payload
                    .ok_or_else(|| anyhow!("--payload is required unless --payment-id is set"))?;
                cmd_pay(&payload, selected_index, from.as_deref()).await
            }
        }
        PaymentCommand::Quote {
            url,
            param,
            method,
            tool,
        } => quote::run(&url, &param, &method, tool.as_deref()).await,
        PaymentCommand::DecodeReceipt { header, receipt } => {
            cmd_decode_receipt(header.as_deref(), receipt.as_deref())
        }
        PaymentCommand::Eip3009Sign { payload } => {
            // Local-key supports `exact + EIP-3009`, `exact + Permit2`, and `upto`
            // (no `aggr_deferred` — that needs a TEE-resident session key).
            let (accepts_val, resource_val) = decode_pay_payload(&payload)?;
            let (proof, entry) = payment_flow::sign_payment_local(&accepts_val, None).await?;
            output::success(emit_pay_result(&proof, &entry, resource_val.as_ref())?);
            Ok(())
        }
        PaymentCommand::Default { action } => cmd_default(action),
        PaymentCommand::A2aPay { command } => a2a_pay::execute(command).await,
        PaymentCommand::Subscription { command } => {
            crate::commands::payment::subscription::execute(command).await
        }
        PaymentCommand::MppCharge {
            challenge,
            from,
            tx_hash,
        } => cmd_mpp_charge(&challenge, from.as_deref(), tx_hash.as_deref()).await,
        PaymentCommand::App { command } => match command {
            AppCommand::Charge {
                challenge,
                from,
                tx_hash,
                type_,
            } => {
                cmd_app_charge(
                    &challenge,
                    from.as_deref(),
                    tx_hash.as_deref(),
                    type_.as_deref(),
                )
                .await
            }
            AppCommand::Upto {
                challenge,
                from,
                type_,
            } => cmd_app_upto(&challenge, from.as_deref(), type_.as_deref()).await,
            AppCommand::Session { command } => match command {
                AppSessionCommand::Open {
                    challenge,
                    from,
                    tx_hash,
                    salt,
                    authorized_signer,
                } => {
                    cmd_app_session_open(
                        &challenge,
                        from.as_deref(),
                        tx_hash.as_deref(),
                        salt.as_deref(),
                        authorized_signer.as_deref(),
                    )
                    .await
                }
                AppSessionCommand::Voucher {
                    challenge,
                    channel_id,
                    cumulative_amount,
                    escrow,
                    chain_id,
                    from,
                } => {
                    cmd_app_session_voucher(
                        &challenge,
                        &channel_id,
                        &cumulative_amount,
                        &escrow,
                        chain_id,
                        from.as_deref(),
                    )
                    .await
                }
                AppSessionCommand::Topup {
                    challenge,
                    channel_id,
                    additional_deposit,
                    escrow,
                    chain_id,
                    currency,
                    from,
                    tx_hash,
                } => {
                    cmd_app_session_topup(
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
                AppSessionCommand::Close {
                    challenge,
                    channel_id,
                    cumulative_amount,
                    escrow,
                    chain_id,
                    from,
                } => {
                    cmd_app_session_close(
                        &challenge,
                        &channel_id,
                        &cumulative_amount,
                        &escrow,
                        chain_id,
                        from.as_deref(),
                    )
                    .await
                }
                AppSessionCommand::Resolve {
                    operation_id,
                    receipt,
                    outcome,
                } => cmd_app_session_resolve(
                    &operation_id,
                    receipt.as_deref(),
                    outcome.as_deref(),
                ),
            },
            AppCommand::Subscription { command } => match command {
                AppSubscriptionCommand::Subscribe {
                    challenge,
                    url,
                    expected_cap,
                    no_cache,
                    from,
                } => cmd_app_subscription_subscribe(
                    &challenge,
                    url.as_deref(),
                    &expected_cap,
                    no_cache,
                    from.as_deref(),
                )
                .await,
                AppSubscriptionCommand::Change {
                    challenge,
                    sub_id,
                    url,
                    expected_cap,
                    no_cache,
                    from,
                } => {
                    cmd_app_subscription_change(
                        &challenge,
                        sub_id.as_deref(),
                        url.as_deref(),
                        &expected_cap,
                        no_cache,
                        from.as_deref(),
                    )
                    .await
                }
                AppSubscriptionCommand::Access {
                    url,
                    sub_id,
                    from,
                    chain,
                } => {
                    cmd_app_subscription_access(url.as_deref(), sub_id.as_deref(), from.as_deref(), &chain)
                        .await
                }
                AppSubscriptionCommand::Cancel {
                    sub_id,
                    offer_id,
                    subscription_contract,
                    token,
                    chain_id,
                    from,
                } => {
                    cmd_app_subscription_cancel(
                        &sub_id,
                        &offer_id,
                        subscription_contract.as_deref(),
                        token.as_deref(),
                        chain_id,
                        from.as_deref(),
                    )
                    .await
                }
                AppSubscriptionCommand::CancelPending {
                    sub_id,
                    new_sub_id,
                    offer_id,
                    subscription_contract,
                    token,
                    chain_id,
                    from,
                } => {
                    cmd_app_subscription_cancel_pending(
                        &sub_id,
                        &new_sub_id,
                        &offer_id,
                        subscription_contract.as_deref(),
                        token.as_deref(),
                        chain_id,
                        from.as_deref(),
                    )
                    .await
                }
                AppSubscriptionCommand::Resolve {
                    operation_id,
                    receipt,
                    outcome,
                } => cmd_app_subscription_resolve(
                    &operation_id,
                    receipt.as_deref(),
                    outcome.as_deref(),
                ),
                AppSubscriptionCommand::List {
                    from,
                    limit,
                    offset,
                } => cmd_app_subscription_list(from.as_deref(), limit, offset).await,
                AppSubscriptionCommand::AllowanceStatus {
                    token,
                    chain_id,
                    from,
                } => {
                    cmd_app_subscription_allowance_status(&token, chain_id, from.as_deref()).await
                }
            },
        },
        PaymentCommand::Session { command } => match command {
            SessionCommand::Open {
                challenge,
                deposit,
                from,
                tx_hash,
                salt,
                initial_cum,
                prepay_first,
            } => {
                cmd_mpp_session_open(
                    &challenge,
                    &deposit,
                    from.as_deref(),
                    tx_hash.as_deref(),
                    salt.as_deref(),
                    initial_cum.as_deref(),
                    prepay_first,
                )
                .await
            }
            SessionCommand::Voucher {
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
            SessionCommand::TopUp {
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
            SessionCommand::Close {
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
        },
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
    use crate::commands::payment::payment_flow::PaymentTier;
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
/// what the caller supplied via `--payload`.
async fn cmd_pay(payload: &str, selected_index: Option<usize>, from: Option<&str>) -> Result<()> {
    let (mut accepts_val, resource_val) = decode_pay_payload(payload)?;
    if let Some(i) = selected_index {
        accepts_val = select_accepts_index(&accepts_val, i)?;
    }
    let (proof, entry) =
        payment_flow::sign_payment_with_preference(&accepts_val, from, None, None).await?;
    output::success(emit_pay_result(&proof, &entry, resource_val.as_ref())?);
    Ok(())
}

/// Two-phase `payment pay --payment-id`: delegate to `fetch_pay`, which
/// enforces the confirming gate and the classified error tokens.
///
/// - `Err(CliConfirming)` → propagate so `main.rs` renders `{confirming,...}`
///   at exit 2 (the first `confirming` use in the payment family).
/// - `Ok(data)` → `output::success` (exit 0, including `status:"failed"` /
///   `"pending"` which are recorded outcomes, not errors).
/// - Any other `Err` (`quote_expired_or_missing` / `cross_user_payment_id` /
///   `invalid_input` / signing failure) → propagate so `main.rs` renders
///   `output::error` at exit 1.
async fn cmd_pay_two_phase(
    payment_id: &str,
    selected_index: Option<usize>,
    param: &[String],
    yes: bool,
) -> Result<()> {
    let data = payment_flow::fetch_pay(payment_id, selected_index, param, yes).await?;
    output::success(data);
    Ok(())
}

/// `payment decode-receipt`: decode a PAYMENT-RESPONSE header or a
/// charge receipt into the normalized shape. Malformed input → `invalid_input`
/// propagates as `Err` → `main.rs` `output::error` (exit 1).
fn cmd_decode_receipt(header: Option<&str>, receipt: Option<&str>) -> Result<()> {
    let data = decode_receipt::fetch_decode_receipt(header, receipt)?;
    output::success(data);
    Ok(())
}

/// Decode the base64 `--payload` (the raw decoded 402 payload — shell-safe, no
/// JSON-array quoting) into the `(accepts, resource)` the signer needs.
///
/// `resource` comes verbatim from the decoded payload (so the assembled header
/// round-trips exactly what the server sent); it drives v2 header assembly and
/// is absent on v1 (→ raw-proof output). To pin a single scheme, the caller
/// encodes a payload whose `accepts` holds just the chosen entry.
fn decode_pay_payload(payload: &str) -> Result<(Value, Option<Value>)> {
    let decoded = decode_payment_blob(payload)?;
    let accepts_val = decoded
        .get("accepts")
        .cloned()
        .ok_or_else(|| anyhow!("--payload decoded to JSON without an 'accepts' field"))?;
    let resource_val = decoded.get("resource").cloned();
    Ok((accepts_val, resource_val))
}

/// Narrow an `accepts[]` array to a single-entry array holding `accepts[index]`.
///
/// Lets the caller pin exactly which scheme the user chose (e.g. from a
/// multi-scheme recommendation card) by index into the decoded payload's
/// `accepts`, so the signer can't auto-select a different entry.
fn select_accepts_index(accepts: &Value, index: usize) -> Result<Value> {
    let arr = accepts.as_array().ok_or_else(|| {
        anyhow!("--selected-index requires the payload's 'accepts' to be an array")
    })?;
    let entry = arr.get(index).ok_or_else(|| {
        anyhow!(
            "--selected-index {} is out of range (accepts has {} entr{})",
            index,
            arr.len(),
            if arr.len() == 1 { "y" } else { "ies" }
        )
    })?;
    Ok(json!([entry]))
}

/// Shape the `payment pay` / `pay-local` output. When a `resource` is available
/// (x402 v2 — carried inside the decoded `--payload`), emit the assembled
/// `PAYMENT-SIGNATURE` header + routing metadata; otherwise emit the raw proof
/// (legacy contract — caller assembles the header).
fn emit_pay_result(
    proof: &payment_flow::PaymentProof,
    entry: &Value,
    resource: Option<&Value>,
) -> Result<Value> {
    match resource {
        Some(r) => payment_flow::pay_with_header_json(proof, entry, r),
        None => Ok(proof.to_pay_json()),
    }
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
    let content = header
        .get(..8)
        .filter(|prefix| prefix.eq_ignore_ascii_case("Payment "))
        .map(|_| &header[8..])
        .unwrap_or(header);
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
            "unsupported payment challenge method \"{}\"; this CLI only supports method=\"evm\"",
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

/// Decode any payment header value / body into JSON, so the agent never has to
/// hand-run `base64 -d` (which is standard-base64-only and silently fails on
/// base64url). Handles, in order:
///   1. a `WWW-Authenticate: Payment ...` challenge — parsed, with the base64url
///      `request` param decoded inline;
///   2. base64-encoded JSON (`PAYMENT-REQUIRED` / `PAYMENT-RESPONSE` /
///      `Payment-Receipt`), trying standard + url-safe, padded + unpadded;
///   3. plain JSON (an x402 v1 body that was never base64-encoded).
pub(crate) fn decode_payment_blob(input: &str) -> Result<Value> {
    use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

    let trimmed = input.trim();

    // 1. WWW-Authenticate challenge.
    let is_www_auth = trimmed
        .as_bytes()
        .get(..8)
        .map(|b| b.eq_ignore_ascii_case(b"Payment "))
        .unwrap_or(false);
    if is_www_auth {
        let mut challenge = parse_www_authenticate(trimmed)?;
        if challenge.get("request").and_then(|v| v.as_str()).is_some() {
            challenge["request"] = decode_challenge_request(&challenge)?;
        }
        return Ok(challenge);
    }

    // 2. base64-encoded JSON — try every common variant, accept the first whose
    //    decoded bytes parse as JSON. (`Engine` isn't dyn-compatible, so decode
    //    eagerly and filter; checking JSON per-variant also guards the case
    //    where one alphabet decodes to non-JSON bytes but another yields JSON.)
    for bytes in [
        B64.decode(trimmed).ok(),
        STANDARD_NO_PAD.decode(trimmed).ok(),
        URL_SAFE.decode(trimmed).ok(),
        URL_SAFE_NO_PAD.decode(trimmed).ok(),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            return Ok(v);
        }
    }

    // 3. plain JSON (x402 v1 body).
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Ok(v);
    }

    bail!(
        "could not decode payment blob: not a WWW-Authenticate challenge, \
         base64-encoded JSON, or plain JSON"
    )
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

/// One parsed split entry: amount + the recipient in both canonical (for
/// signing) and display (for echoing back into output JSON) forms.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSplit {
    /// Decimal amount in minimal units.
    amount: String,
    /// Canonical `0x`-form recipient — fed into `tee_sign_eip3009` and any
    /// other path that hashes the 20-byte payload.
    canonical: String,
    /// Display form — same content, original prefix (`XKO...` on XLayer or
    /// the same `0x...`). Echoed back in the CLI output JSON.
    display: String,
}

/// Parse a charge `request` into `(primary_remainder, splits)` with full spec validation.
///
/// Enforces §Split Payments constraints before any TEE signing:
/// - each `splits[i].amount` MUST be > 0
/// - `sum(splits[].amount)` MUST be strictly less than `amount`
/// - splits MAY be absent; if present, MUST have 1-10 entries
///
/// Returns `(primary_value_decimal_str, [ParsedSplit, ...])` in the same
/// order as `methodDetails.splits`. When no splits are present, returns
/// `(amount, vec![])`. `chain_id` is used to validate XKO-prefixed split
/// recipients (XLayer-only) and split them into canonical-for-signing +
/// display-for-output forms.
fn compute_primary_split_amounts(
    request: &serde_json::Value,
    chain_id: u64,
) -> Result<(String, Vec<ParsedSplit>)> {
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
        let (canonical, display) = parse_recipient_addr(r_str, chain_id)
            .with_context(|| format!("splits[{i}].recipient"))?;
        parsed.push(ParsedSplit {
            amount: a_str.to_string(),
            canonical,
            display,
        });
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
    let salt_bytes =
        hex::decode(top_up_salt_hex.trim_start_matches("0x")).context("topUpSalt must be hex")?;
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
/// `request.methodDetails.splits[]`. Recipients are normalized to canonical
/// `0x` form (XKO-prefixed XLayer recipients are accepted here too — the
/// nonce hash + alloy `Address::parse` always need the 20-byte payload, not
/// the prefix). Returns empty vecs when absent.
fn parse_session_splits(
    request: &serde_json::Value,
    chain_id: u64,
) -> Result<(Vec<String>, Vec<u16>)> {
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
        let (canonical, _display) =
            parse_recipient_addr(r, chain_id).with_context(|| format!("splits[{i}].recipient"))?;
        recipients.push(canonical);
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
    let salt_bytes = hex::decode(salt_hex.trim_start_matches("0x")).context("salt must be hex")?;
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
    let salt_bytes = hex::decode(salt_hex.trim_start_matches("0x")).context("salt must be hex")?;
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
#[allow(clippy::too_many_arguments)]
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
    let session = wallet_store::load_session()?.ok_or_else(|| {
        anyhow::anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN)
    })?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN))?;

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
    let session = wallet_store::load_session()?.ok_or_else(|| {
        anyhow::anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN)
    })?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN))?;

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
async fn resolve_chain_and_payer(chain_id: u64, from: Option<&str>) -> Result<(String, String)> {
    let chain_entry = crate::commands::agentic_wallet::chain::get_chain_by_real_chain_index(
        &chain_id.to_string(),
    )
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
    let wallets = wallet_store::load_wallets()?.ok_or_else(|| {
        anyhow::anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN)
    })?;
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

/// Validate and canonicalize a user-supplied bytes32 hex argument
/// (e.g. `--salt`). Accepts both `0x...` and bare `...`; returns the
/// `0x` + 64-lowercase-hex canonical form.
fn normalize_bytes32_hex(value: &str, label: &str) -> Result<String> {
    let body = value.trim_start_matches("0x");
    if body.len() != 64 {
        bail!(
            "{label} must be 32 bytes (0x + 64 hex chars), got {} chars",
            body.len()
        );
    }
    if !body.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("{label} contains non-hex characters");
    }
    Ok(format!("0x{}", body.to_ascii_lowercase()))
}

/// Build the APP/1 credential challenge echo.
///
/// Unlike MPP's [`build_challenge_echo`], this carries `protocol` and `opaque`
/// verbatim: both are folded into the HMAC that binds `challenge.id`
/// (`protocol|realm|method|intent|request|expires|opaque`), so the facilitator
/// recomputes over them. `request`/`opaque` are echoed as the original base64url
/// strings from the header (not the decoded object) — the HMAC is over those raw
/// bytes.
fn build_app_challenge_echo(challenge: &Value) -> Value {
    let s = |k: &str| challenge.get(k).and_then(|v| v.as_str()).unwrap_or("");
    json!({
        "id": s("id"),
        "realm": s("realm"),
        "protocol": s("protocol"),
        "method": s("method"),
        "intent": s("intent"),
        "expires": s("expires"),
        "request": s("request"),
        "opaque": s("opaque"),
    })
}

/// Shared APP/1 charge/upto signer. `intent` is `"charge"` (exact amount) or
/// `"upto"` (authorize a ceiling; seller settles the actual ≤ ceiling). Both
/// TEE-sign EIP-3009 `transferWithAuthorization` for `challenge.request.amount`;
/// charge additionally supports hash mode (`feePayer=false`, `--tx-hash`).
async fn cmd_app_charge_like(
    challenge_header: &str,
    from: Option<&str>,
    tx_hash: Option<&str>,
    sig_type: Option<&str>,
    intent: &str,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;

    // APP/1 command family only handles protocol="app/1"; route MPP elsewhere.
    let protocol = challenge
        .get("protocol")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if protocol != "app/1" {
        bail!(
            "APP/1 command requires challenge protocol=\"app/1\", got \"{protocol}\"; \
             use `payment charge` for MPP challenges"
        );
    }
    let ch_intent = challenge
        .get("intent")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if ch_intent != intent {
        bail!("challenge.intent=\"{ch_intent}\" does not match `payment app {intent}`");
    }

    let request = decode_challenge_request(&challenge)?;
    let recipient_in = request["recipient"]
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
    let fee_payer = request["methodDetails"]["feePayer"]
        .as_bool()
        .unwrap_or(true);

    // XKO/0x dual capture: canonical → signing, display → output JSON.
    let (recipient, recipient_display) =
        parse_recipient_addr(recipient_in, chain_id).with_context(|| "challenge.request.recipient")?;
    let recipient = recipient.as_str();
    let recipient_display = recipient_display.as_str();

    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();
    let source = format!("did:pkh:eip155:{chain_id}:{payer_addr}");

    // Pick signature type from the challenge's acceptedTypes (the seller/facilitator
    // capability matrix is authoritative — e.g. upto only offers permit2). Explicit
    // --type must be one of the offered types; if omitted, auto-pick by preference
    // (authorization > permit2 > hash), falling back to the feePayer heuristic only
    // when the challenge omits acceptedTypes.
    let accepted: Vec<&str> = request["methodDetails"]["acceptedTypes"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let sig_type = match sig_type {
        Some(t) => {
            if !accepted.is_empty() && !accepted.contains(&t) {
                bail!("--type {t} is not in challenge acceptedTypes {accepted:?}");
            }
            t
        }
        None => ["authorization", "permit2", "hash"]
            .into_iter()
            .find(|t| accepted.contains(t))
            .unwrap_or(if fee_payer { "authorization" } else { "hash" }),
    };

    // permit2 (SignatureTransfer + witness): shared by charge and upto; amount =
    // exact (charge) or ceiling (upto). Facilitator relays via the permit2 proxy,
    // so feePayer is irrelevant here.
    if sig_type == "permit2" {
        let spender = request["methodDetails"]["permit2"]["spender"]
            .as_str()
            .ok_or_else(|| anyhow!("type=permit2 requires request.methodDetails.permit2.spender"))?;
        // Pre-check the one-time ERC20→Permit2 allowance before signing: Permit2
        // SignatureTransfer needs `approve(Permit2, ≥amount)` on the token first.
        // Bails with an actionable approve hint if short; degrades to a warning if
        // the RPC probe is unavailable (the facilitator still gates at /verify).
        payment_flow::preflight_permit2_allowance(&chain_index, currency, payer_addr, amount)
            .await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        // deadline aligned to challenge.expires (+grace); take only the 256-bit
        // random nonce from the shared helper (its TTL/deadline is discarded).
        let deadline = compute_valid_before(&challenge, now)?;
        let (_, _, nonce) = payment_flow::permit2_timing_and_nonce(300)?;
        // upto proxy 的 witness 多 `facilitator`(链上强制 msg.sender == witness.facilitator);
        // exact(charge)的 witness 只有 {to, validAfter}。用 intent 分流两条签名路径。
        let (typed_data, witness) = if intent == "upto" {
            let facilitator = request["methodDetails"]["permit2"]["facilitator"]
                .as_str()
                .ok_or_else(|| {
                    anyhow!("upto permit2 requires request.methodDetails.permit2.facilitator")
                })?;
            let input = crate::payment::permit2::eip712::UptoPermit2Input {
                token: currency,
                amount,
                spender,
                nonce: &nonce,
                deadline: &deadline,
                witness_to: recipient,
                witness_facilitator: facilitator,
                witness_valid_after: "0",
                chain_id,
            };
            let td = crate::payment::permit2::eip712::build_upto_permit2_typed_data(&input);
            // witness 字段序必须 to, facilitator, validAfter(与 EIP-712 typehash 一致)。
            (td, json!({ "to": recipient, "facilitator": facilitator, "validAfter": "0" }))
        } else {
            let input = crate::payment::permit2::eip712::ExactPermit2Input {
                token: currency,
                amount,
                spender,
                nonce: &nonce,
                deadline: &deadline,
                witness_to: recipient,
                witness_valid_after: "0",
                chain_id,
            };
            let td = crate::payment::permit2::eip712::build_exact_permit2_typed_data(&input);
            (td, json!({ "to": recipient, "validAfter": "0" }))
        };
        let signature =
            crate::payment::permit2::sign::tee_sign_eip712(&chain_index, payer_addr, &typed_data)
                .await?;
        let credential = json!({
            "challenge": build_app_challenge_echo(&challenge),
            "source": source,
            "payload": {
                "type": "permit2",
                "from": payer_addr,
                "token": currency,
                "amount": amount,
                "spender": spender,
                "nonce": nonce,
                "deadline": deadline,
                "witness": witness,
                "signature": signature,
            }
        });
        let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);
        output::success(json!({
            "protocol": "app/1",
            "method": "evm",
            "intent": intent,
            "mode": "permit2",
            "authorization_header": authorization_header,
            "wallet": payer_addr,
            "challenge": { "id": challenge["id"], "realm": challenge["realm"] },
        }));
        return Ok(());
    }

    // Hash mode (charge only): client already broadcast transferWithAuthorization.
    if sig_type == "hash" {
        if fee_payer {
            bail!("type=hash requires challenge.methodDetails.feePayer=false");
        }
        if intent == "upto" {
            bail!(
                "upto does not support feePayer=false (hash mode): the seller must relay \
                 in order to settle the actual amount"
            );
        }
        let hash = tx_hash.ok_or_else(|| {
            anyhow!(
                "challenge.methodDetails.feePayer=false requires --tx-hash (broadcast \
                 transferWithAuthorization yourself first)"
            )
        })?;
        if !hash.starts_with("0x")
            || hash.len() != 66
            || !hash[2..].chars().all(|c| c.is_ascii_hexdigit())
        {
            bail!("--tx-hash must be 0x + 64 hex chars");
        }
        let credential = json!({
            "challenge": build_app_challenge_echo(&challenge),
            "source": source,
            "payload": {
                "type": "hash",
                "from": payer_addr,
                "to": recipient_display,
                "value": amount,
                "txHash": hash,
            }
        });
        let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);
        output::success(json!({
            "protocol": "app/1",
            "method": "evm",
            "intent": intent,
            "mode": "hash",
            "authorization_header": authorization_header,
            "wallet": payer_addr,
            "challenge": { "id": challenge["id"], "realm": challenge["realm"] },
        }));
        return Ok(());
    }

    if sig_type != "authorization" {
        bail!("unsupported signature type \"{sig_type}\"; expected authorization | permit2 | hash");
    }
    if tx_hash.is_some() {
        bail!("--tx-hash is only valid with type=hash (feePayer=false)");
    }

    // Transaction mode: TEE-sign EIP-3009 for `amount` (exact for charge, ceiling
    // for upto). No splits — the APP/1 authorization payload is single-recipient.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let valid_before = compute_valid_before(&challenge, now)?;
    let nonce = random_nonce_hex();
    let (signature, _) = tee_sign_eip3009(
        Eip3009AuthType::Transfer,
        &chain_index,
        payer_addr,
        recipient,
        amount,
        &valid_before,
        &nonce,
        currency,
    )
    .await?;

    let credential = json!({
        "challenge": build_app_challenge_echo(&challenge),
        "source": source,
        "payload": {
            "type": "authorization",
            "from": payer_addr,
            "to": recipient_display,
            "value": amount,
            "validAfter": "0",
            "validBefore": valid_before,
            "nonce": nonce,
            "signature": signature,
        }
    });
    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": intent,
        "mode": "transaction",
        "authorization_header": authorization_header,
        "wallet": payer_addr,
        "challenge": { "id": challenge["id"], "realm": challenge["realm"] },
    }));
    Ok(())
}

/// `payment app charge` — APP/1 one-shot exact charge.
async fn cmd_app_charge(
    challenge_header: &str,
    from: Option<&str>,
    tx_hash: Option<&str>,
    sig_type: Option<&str>,
) -> Result<()> {
    cmd_app_charge_like(challenge_header, from, tx_hash, sig_type, "charge").await
}

/// `payment app upto` — APP/1 variable amount up to a ceiling.
async fn cmd_app_upto(
    challenge_header: &str,
    from: Option<&str>,
    sig_type: Option<&str>,
) -> Result<()> {
    cmd_app_charge_like(challenge_header, from, None, sig_type, "upto").await
}

/// Ensure the challenge is APP/1 and (when given) matches the expected intent.
fn app_guard(challenge: &Value, intent: Option<&str>) -> Result<()> {
    let p = challenge
        .get("protocol")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if p != "app/1" {
        bail!("APP/1 command requires challenge protocol=\"app/1\", got \"{p}\"");
    }
    if let Some(want) = intent {
        let got = challenge
            .get("intent")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if got != want {
            bail!("challenge.intent=\"{got}\" does not match `payment app session` (expected {want})");
        }
    }
    Ok(())
}

/// `payment app session open` — deposit-only channel open (no initial voucher).
async fn cmd_app_session_open(
    challenge_header: &str,
    from: Option<&str>,
    tx_hash: Option<&str>,
    salt_arg: Option<&str>,
    authorized_signer_arg: Option<&str>,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;
    app_guard(&challenge, Some("session"))?;

    let request = decode_challenge_request(&challenge)?;
    let recipient_in = request["recipient"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'recipient' (payee) in challenge request"))?;
    let currency = request["currency"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'currency' in challenge request"))?;
    let chain_id = request["methodDetails"]["chainId"]
        .as_u64()
        .ok_or_else(|| anyhow!("missing 'methodDetails.chainId'"))?;
    let escrow = request["methodDetails"]["escrowContract"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'methodDetails.escrowContract'"))?;
    let fee_payer = request["methodDetails"]["feePayer"]
        .as_bool()
        .unwrap_or(true);
    let deposit = app_session_open_deposit(&request)?;

    let (recipient, _display) =
        parse_recipient_addr(recipient_in, chain_id).with_context(|| "challenge.request.recipient")?;
    let recipient = recipient.as_str();

    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();
    let source = format!("did:pkh:eip155:{chain_id}:{payer_addr}");

    const ZERO_SIGNER: &str = "0x0000000000000000000000000000000000000000";
    let authorized_signer = authorized_signer_arg.unwrap_or(ZERO_SIGNER);
    if authorized_signer != ZERO_SIGNER && authorized_signer.eq_ignore_ascii_case(payer_addr) {
        bail!("authorizedSigner must not equal payer (omit / use zero address for self-signing)");
    }

    // salt: hash mode reuses the on-chain bytes32; transaction mode generates one.
    let salt = match (fee_payer, salt_arg) {
        (false, Some(s)) => normalize_bytes32_hex(s, "--salt")?,
        (false, None) => bail!(
            "hash mode (feePayer=false) requires --salt (the bytes32 you passed to your \
             on-chain escrow.open)"
        ),
        (true, Some(_)) => bail!("--salt is only valid in hash mode (feePayer=false)"),
        (true, None) => {
            use rand::RngCore;
            let mut s = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut s);
            format!("0x{}", hex::encode(s))
        }
    };
    if fee_payer && tx_hash.is_some() {
        bail!("--tx-hash is only valid in hash mode (feePayer=false)");
    }

    let channel_id = compute_channel_id(
        payer_addr,
        recipient,
        currency,
        &salt,
        authorized_signer,
        escrow,
        chain_id,
    )?;

    let payload = if !fee_payer {
        let hash = tx_hash.ok_or_else(|| {
            anyhow!("hash mode requires --tx-hash (broadcast escrow.open yourself first)")
        })?;
        let hash = normalize_bytes32_hex(hash, "--tx-hash")?;
        let mut p = json!({
            "action": "open",
            "type": "hash",
            "channelId": channel_id,
            "hash": hash,
            "salt": salt,
        });
        if authorized_signer != ZERO_SIGNER {
            p["authorizedSigner"] = json!(authorized_signer);
        }
        p
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let valid_before = compute_valid_before(&challenge, now)?;
        // Deposit → escrow.receiveWithAuthorization (Receive typehash); nonce per
        // the contract's open-authorization formula (no splits in APP/1 sessions).
        let nonce = compute_open_nonce(
            payer_addr,
            recipient,
            currency,
            &salt,
            authorized_signer,
            &[],
            &[],
        )?;
        let (signature, _) = tee_sign_eip3009(
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
        let mut p = json!({
            "action": "open",
            "type": "authorization",
            "channelId": channel_id,
            "authorization": {
                "from": payer_addr,
                "to": escrow,
                "value": deposit,
                "validAfter": "0",
                "validBefore": valid_before,
                "nonce": nonce,
            },
            "signature": signature,
            "salt": salt,
        });
        if authorized_signer != ZERO_SIGNER {
            p["authorizedSigner"] = json!(authorized_signer);
        }
        p
    };

    let credential = json!({
        "challenge": build_app_challenge_echo(&challenge),
        "source": source,
        "payload": payload,
    });
    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);
    persist_channel_open(&channel_id, payer_addr, deposit, "0");
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "session",
        "action": "open",
        "mode": if fee_payer { "transaction" } else { "hash" },
        "authorization_header": authorization_header,
        "channel_id": channel_id,
        "escrow": escrow,
        "chain_id": chain_id,
        "deposit": deposit,
        "wallet": payer_addr,
        "challenge": { "id": challenge["id"], "realm": challenge["realm"] },
    }));
    Ok(())
}

/// `payment app session voucher` — sign a monotonic cumulative-amount voucher.
async fn cmd_app_session_voucher(
    challenge_header: &str,
    channel_id: &str,
    cumulative_amount: &str,
    escrow: &str,
    chain_id: u64,
    from: Option<&str>,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;
    app_guard(&challenge, Some("session"))?;
    let _channel_lock = session_state::lock(channel_id)?;

    // Fail closed before resolving the wallet or asking the TEE to sign. An
    // over-deposit voucher can become usable after a later top-up, so merely
    // refusing to advance the local cache after signing is not sufficient.
    let prior = session_state::read(channel_id).ok_or_else(|| {
        anyhow!(
            "APP session state not found for channel {channel_id}; cannot verify \
             --cumulative-amount before signing"
        )
    })?;
    prior.ensure_no_pending()?;
    validate_app_voucher_cumulative(&prior.cumulative, &prior.deposit, cumulative_amount)?;

    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    // Buyer produces only `voucherSignature`; the seller adds its own
    // payeeSignature / nonce / deadline (SettleAuthorization) when settling.
    let voucher_sig =
        tee_sign_voucher(&chain_index, payer_addr, channel_id, cumulative_amount, escrow, chain_id)
            .await?;

    let credential = json!({
        "challenge": build_app_challenge_echo(&challenge),
        "source": format!("did:pkh:eip155:{chain_id}:{payer_addr}"),
        "payload": {
            "action": "voucher",
            "channelId": channel_id,
            "cumulativeAmount": cumulative_amount,
            "voucherSignature": voucher_sig,
        }
    });
    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);

    // Advance persisted cumulative when viable (mirrors MPP voucher bookkeeping).
    let mut prior = prior;
    prior.cumulative = cumulative_amount.to_string();
    prior.updated_at = session_state::now_unix();
    let _ = prior.write();

    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "session",
        "action": "voucher",
        "authorization_header": authorization_header,
        "channel_id": channel_id,
        "cumulative_amount": cumulative_amount,
        "wallet": payer_addr,
        "challenge": { "id": challenge["id"], "realm": challenge["realm"] },
    }));
    Ok(())
}

/// `payment app session topup` — add deposit to an existing channel.
#[allow(clippy::too_many_arguments)]
async fn cmd_app_session_topup(
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
    app_guard(&challenge, Some("session"))?;
    let offer_id = app_session_offer_id(&challenge)?;
    let _channel_lock = session_state::lock(channel_id)?;

    let mut prior = session_state::read(channel_id).ok_or_else(|| {
        anyhow!(
            "APP session state not found for channel {channel_id}; cannot track topup before replay"
        )
    })?;
    prior.ensure_no_pending()?;
    parse_app_session_amount(additional_deposit, "additional-deposit")?;

    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    let payload = if let Some(hash) = tx_hash {
        let hash = normalize_bytes32_hex(hash, "--tx-hash")?;
        json!({
            "action": "topup",
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
        let top_up_salt = {
            use rand::RngCore;
            let mut s = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut s);
            format!("0x{}", hex::encode(s))
        };
        // EIP-3009 nonce per the contract's computeTopUpAuthorizationNonce formula.
        let nonce = compute_topup_nonce(payer_addr, channel_id, additional_deposit, &top_up_salt)?;
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
            "action": "topup",
            "type": "authorization",
            "channelId": channel_id,
            "authorization": {
                "from": payer_addr,
                "to": escrow,
                "value": additional_deposit,
                "validAfter": "0",
                "validBefore": valid_before,
                "nonce": nonce,
            },
            "signature": signature,
            "additionalDeposit": additional_deposit,
            "topUpSalt": top_up_salt,
        })
    };

    let credential = json!({
        "challenge": build_app_challenge_echo(&challenge),
        "source": format!("did:pkh:eip155:{chain_id}:{payer_addr}"),
        "payload": payload,
    });
    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);

    let operation_id = prior.begin_pending(
        session_state::PendingAction::Topup,
        additional_deposit,
        &offer_id,
    )?;
    prior.write()?;

    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "session",
        "action": "topup",
        "mode": if tx_hash.is_some() { "hash" } else { "transaction" },
        "authorization_header": authorization_header,
        "operation_id": operation_id,
        "channel_id": channel_id,
        "additional_deposit": additional_deposit,
        "wallet": payer_addr,
        "challenge": { "id": challenge["id"], "realm": challenge["realm"] },
    }));
    Ok(())
}

/// `payment app session close` — sign the final voucher for `/session/close`.
async fn cmd_app_session_close(
    challenge_header: &str,
    channel_id: &str,
    cumulative_amount: &str,
    escrow: &str,
    chain_id: u64,
    from: Option<&str>,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;
    app_guard(&challenge, Some("session"))?;
    let offer_id = app_session_offer_id(&challenge)?;
    let _channel_lock = session_state::lock(channel_id)?;

    // Match the MPP lifecycle rule: close reuses the latest valid cumulative
    // amount. It must not create a new authorization or exceed the deposit.
    let prior = session_state::read(channel_id).ok_or_else(|| {
        anyhow!(
            "APP session state not found for channel {channel_id}; cannot verify \
             --cumulative-amount before signing"
        )
    })?;
    prior.ensure_no_pending()?;
    validate_app_close_cumulative(&prior.cumulative, &prior.deposit, cumulative_amount)?;

    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    // Buyer signs only the final voucher; the seller adds its CloseAuthorization
    // (payeeSignature / nonce / deadline) and submits to POST /session/close.
    let voucher_sig =
        tee_sign_voucher(&chain_index, payer_addr, channel_id, cumulative_amount, escrow, chain_id)
            .await?;

    // Emit a `close` credential (symmetric with open/topup/voucher): the buyer
    // replays `Authorization: Payment <cred>` to the seller's session endpoint,
    // which dispatches by `payload.action` and adds its payee CloseAuthorization.
    let source = format!("did:pkh:eip155:{chain_id}:{payer_addr}");
    let credential = json!({
        "challenge": build_app_challenge_echo(&challenge),
        "source": source,
        "payload": {
            "action": "close",
            "channelId": channel_id,
            "cumulativeAmount": cumulative_amount,
            "signature": voucher_sig,
        }
    });
    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);

    let mut prior = prior;
    let operation_id = prior.begin_pending(
        session_state::PendingAction::Close,
        cumulative_amount,
        &offer_id,
    )?;
    prior.write()?;
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "session",
        "action": "close",
        "authorization_header": authorization_header,
        "operation_id": operation_id,
        "wallet": payer_addr,
        "channel_id": channel_id,
        "cumulative_amount": cumulative_amount,
        "challenge": { "id": challenge["id"], "realm": challenge["realm"] },
    }));
    Ok(())
}

fn app_session_offer_id(challenge: &Value) -> Result<String> {
    let opaque = challenge["opaque"]
        .as_str()
        .ok_or_else(|| anyhow!("APP session challenge is missing opaque"))?;
    let decoded = decode_payment_blob(opaque).context("decode APP session challenge opaque")?;
    decoded["offerId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("APP session challenge opaque is missing offerId"))
}

fn app_session_receipt_status(
    receipt: &str,
    state: &session_state::ChannelState,
    pending: &session_state::PendingOperation,
) -> Result<(String, Option<String>)> {
    let value = decode_payment_blob(receipt).context("decode APP session receipt")?;
    let require = |field: &str| {
        value[field]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("APP session receipt is missing {field}"))
    };
    if require("protocol")? != "app/1" {
        bail!("APP session receipt protocol must be app/1");
    }
    if require("intent")? != "session" {
        bail!("APP session receipt intent must be session");
    }
    if require("offerId")? != pending.offer_id {
        bail!("APP session receipt offerId does not match the pending operation");
    }
    if !require("payer")?.eq_ignore_ascii_case(&state.owner_wallet) {
        bail!("APP session receipt payer does not match the channel owner");
    }
    let status = decode_receipt::decode_receipt(Some(receipt), None)?
        .status
        .to_ascii_lowercase();
    if !matches!(status.as_str(), "success" | "failed" | "pending" | "timeout") {
        bail!("APP session receipt has unsupported status {status}");
    }
    if status != "success" {
        return Ok((status, None));
    }

    let amount = value
        .get("settledAmount")
        .or_else(|| value.get("amount"))
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .ok_or_else(|| anyhow!("successful APP session receipt is missing settledAmount"))?;
    let settled_amount = parse_app_session_amount(&amount, "receipt-settled-amount")?;
    let pending_amount = parse_app_session_amount(&pending.amount, "pending-operation-amount")?;
    let expected_amount = match pending.action {
        session_state::PendingAction::Topup => {
            let deposit = parse_app_session_amount(&state.deposit, "channel-deposit")?;
            deposit
                .checked_add(pending_amount)
                .ok_or_else(|| anyhow!("channel deposit overflow"))?
        }
        session_state::PendingAction::Close => pending_amount,
    };
    if settled_amount != expected_amount {
        bail!("APP session receipt amount does not match the pending operation");
    }
    let reference = require("reference")?.to_ascii_lowercase();
    Ok((status, Some(reference)))
}

fn cmd_app_session_resolve(
    operation_id: &str,
    receipt: Option<&str>,
    outcome: Option<&str>,
) -> Result<()> {
    if let Some(resolved) = session_state::read_resolution(operation_id) {
        emit_app_session_resolution(&resolved, true);
        return Ok(());
    }
    let candidate = match session_state::find_pending(operation_id)? {
        Some(candidate) => candidate,
        None => {
            if let Some(resolved) = session_state::read_resolution(operation_id) {
                emit_app_session_resolution(&resolved, true);
                return Ok(());
            }
            bail!(
                "APP session operation {operation_id} is not pending; it may already be resolved"
            );
        }
    };
    let _channel_lock = session_state::lock(&candidate.channel_id)?;
    if let Some(resolved) = session_state::read_resolution(operation_id) {
        emit_app_session_resolution(&resolved, true);
        return Ok(());
    }
    let mut state = session_state::read(&candidate.channel_id).ok_or_else(|| {
        anyhow!("APP session channel {} is no longer active", candidate.channel_id)
    })?;
    let pending = state
        .pending_operation
        .clone()
        .filter(|pending| pending.operation_id == operation_id)
        .ok_or_else(|| anyhow!("APP session operation {operation_id} is not pending"))?;
    let (resolution, receipt_reference) = if let Some(receipt) = receipt {
        app_session_receipt_status(receipt, &state, &pending)?
    } else {
        (
            outcome
                .ok_or_else(|| anyhow!("--receipt or --outcome is required"))?
                .to_string(),
            None,
        )
    };
    let success = resolution == "success";
    let _receipt_lock = if let Some(reference) = receipt_reference.as_deref() {
        Some(session_state::lock_receipt_reference(reference)?)
    } else {
        None
    };
    if let Some(reference) = receipt_reference.as_deref() {
        if session_state::receipt_reference_used(reference)? {
            bail!("APP session receipt reference was already used by another operation");
        }
    }

    let resolved = session_state::ResolvedOperation {
        operation_id: operation_id.to_string(),
        channel_id: state.channel_id.clone(),
        action: pending.action.clone(),
        resolution: resolution.clone(),
        committed: success,
        receipt_reference: receipt_reference.clone(),
        resolved_at: session_state::now_unix(),
    };
    match (&pending.action, success) {
        (session_state::PendingAction::Topup, true) => {
            let deposit = parse_app_session_amount(&state.deposit, "channel-deposit")?;
            let additional = parse_app_session_amount(&pending.amount, "additional-deposit")?;
            state.deposit = deposit
                .checked_add(additional)
                .ok_or_else(|| anyhow!("channel deposit overflow"))?
                .to_string();
            state.pending_operation = None;
            state.resolved_operations.push(resolved.clone());
            state.updated_at = session_state::now_unix();
            state.write()?;
        }
        (session_state::PendingAction::Close, true) => {
            session_state::archive_close_success(
                &state.channel_id,
                operation_id,
                receipt_reference
                    .as_deref()
                    .ok_or_else(|| anyhow!("successful receipt is missing reference"))?,
            )?;
        }
        (_, false) => {
            state.pending_operation = None;
            state.resolved_operations.push(resolved.clone());
            state.updated_at = session_state::now_unix();
            state.write()?;
        }
    }

    let operation = match &pending.action {
        session_state::PendingAction::Topup => "topup",
        session_state::PendingAction::Close => "close",
    };
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "session",
        "action": "resolve",
        "operation": operation,
        "operation_id": operation_id,
        "channel_id": state.channel_id,
        "resolution": resolution,
        "committed": success,
        "already_resolved": false,
    }));
    Ok(())
}

fn emit_app_session_resolution(
    resolved: &session_state::ResolvedOperation,
    already_resolved: bool,
) {
    let operation = match resolved.action {
        session_state::PendingAction::Topup => "topup",
        session_state::PendingAction::Close => "close",
    };
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "session",
        "action": "resolve",
        "operation": operation,
        "operation_id": resolved.operation_id,
        "channel_id": resolved.channel_id,
        "resolution": resolved.resolution,
        "committed": resolved.committed,
        "already_resolved": already_resolved,
    }));
}

/// Adapt an app/1 subscription `challenge.request` into the x402-shaped `accepted`
/// offer that the shared subscription double-sign consumes. Maps periodMode
/// string→number (calendar_month→1 with periodSec 0), sources the contracts from
/// the authoritative allowance-status, and requires the seller-pinned `facilitator`
/// (= capabilities.methods.evm.facilitator) — it is a signed SubscriptionTerms
/// field, so it must come from the challenge, not be invented.
fn build_app_subscription_accepted(
    request: &Value,
    token: &str,
    a: &crate::payment::subscription::types::AllowanceStatus,
    permit2_contract: &str,
) -> Result<Value> {
    let recipient = request["recipient"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'recipient' in subscription challenge"))?;
    // Prefer `amountPerPeriod`; accept `amount` from older sellers.
    let amount_per_period = request["amountPerPeriod"]
        .as_str()
        .or_else(|| request["amount"].as_str())
        .ok_or_else(|| anyhow!("subscription requires request.amountPerPeriod (per-period)"))?;
    let max_periods = request["maxPeriods"]
        .as_u64()
        .ok_or_else(|| anyhow!("missing 'maxPeriods'"))?;
    let plan = request.get("plan").ok_or_else(|| anyhow!("missing 'plan'"))?;
    let plan_id = plan
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| plan.get("planId").and_then(Value::as_str))
        .unwrap_or("");
    let plan_tier = plan
        .get("tier")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("missing 'plan.tier'"))?;
    let facilitator = request["facilitator"].as_str().ok_or_else(|| {
        anyhow!("subscription challenge missing 'facilitator' (seller must pin \
                 capabilities.methods.evm.facilitator; it is signed into SubscriptionTerms)")
    })?;
    // periodMode string → number; calendar_month has no fixed periodSec (0).
    let (period_mode, period_sec) = match request["periodMode"].as_str().unwrap_or("fixed_seconds") {
        "calendar_month" => (1u64, 0u64),
        _ => (0u64, request["periodSec"].as_u64().unwrap_or(0)),
    };
    let start_at = request["startAt"].as_u64().unwrap_or(0);
    // Normalize seller keys `periods`/`amount` to the `periodCount`/`totalAmount`
    // keys read by the shared x402 `extract_terms_params`. Accept either key set.
    let initial_charge = match request.get("initialCharge") {
        Some(ic) if !ic.is_null() => {
            let periods = ic
                .get("periods")
                .or_else(|| ic.get("periodCount"))
                .cloned()
                .unwrap_or_else(|| Value::from(0));
            let amount = ic
                .get("amount")
                .or_else(|| ic.get("totalAmount"))
                .cloned()
                .unwrap_or_else(|| Value::from("0"));
            json!({ "periodCount": periods, "totalAmount": amount })
        }
        _ => Value::Null,
    };
    // Plan-change offers carry `changeFrom` {fromSubId, direction|effectiveAt};
    // pass it through so the shared x402 `sign_change` can read the old subId and
    // derive changeEffectiveAt. Absent on a fresh subscribe.
    let change_from = request.get("changeFrom").cloned().unwrap_or(Value::Null);
    // Seller-provided terms salt (bytes32): the buyer MUST sign this exact value
    // into SubscriptionTerms so the facilitator's `terms.salt == request.salt`
    // binding holds. If the challenge omits it (older seller), the signer falls
    // back to a random salt (x402 behaviour).
    let salt = request.get("salt").cloned().unwrap_or(Value::Null);
    Ok(json!({
        "asset": token,
        "payTo": recipient,
        "maxTimeoutSeconds": 600,
        "extra": {
            "amountPerPeriod": amount_per_period,
            "periodSec": period_sec,
            "periodMode": period_mode,
            "maxPeriods": max_periods,
            "startAt": start_at,
            "initialCharge": initial_charge,
            "changeFrom": change_from,
            "salt": salt,
            // Seller-pinned signed terms field — the buyer MUST sign this exact
            // value (not recompute), else terms_signature_invalid.
            "termsDeadline": request.get("termsDeadline").cloned().unwrap_or(Value::Null),
            "plan": { "id": plan_id, "tier": plan_tier },
            "facilitator": facilitator,
            "contracts": { "subscription": a.subscription_contract, "permit2": permit2_contract }
        }
    }))
}

/// `payment app subscription subscribe` — double-sign (PermitSingle +
/// SubscriptionTerms) that creates the subscription, reusing the x402 subscription
/// terms logic. APP/1 subscriptions reuse the existing x402 subscription signer.
async fn cmd_app_subscription_subscribe(
    challenge_header: &str,
    url: Option<&str>,
    expected_cap: &str,
    no_cache: bool,
    from: Option<&str>,
) -> Result<()> {
    app_subscription_settle(challenge_header, from, None, url, expected_cap, no_cache).await
}

/// `payment app subscription change` — up/downgrade an existing subscription from
/// a change-offer 402 (challenge carries `changeFrom`). Mirrors the x402
/// `subscription change` command: same double-sign as subscribe, but signs the
/// old subId + derived `changeEffectiveAt` into the terms so the subId migrates.
/// `sub_id` overrides `challenge.request.changeFrom.fromSubId`.
async fn cmd_app_subscription_change(
    challenge_header: &str,
    sub_id: Option<&str>,
    url: Option<&str>,
    expected_cap: &str,
    no_cache: bool,
    from: Option<&str>,
) -> Result<()> {
    app_subscription_settle(
        challenge_header,
        from,
        Some(sub_id.unwrap_or("")),
        url,
        expected_cap,
        no_cache,
    )
    .await
}

/// Shared APP/1 subscription settle flow (subscribe + change). `change_sub_id`:
/// `None` = fresh subscribe (changeFromSubId = 0); `Some(old)` = plan change
/// (`old` may be `""` → fall back to challenge `changeFrom.fromSubId`). Both
/// double-sign, inject the buyer-computed `permitHash` into challenge.request,
/// and wrap the payload in an app/1 credential. On-chain state comes from the
/// APP/1 `GET /subscription/allowance-status` endpoint.
async fn app_subscription_settle(
    challenge_header: &str,
    from: Option<&str>,
    change_sub_id: Option<&str>,
    url: Option<&str>,
    expected_cap: &str,
    no_cache: bool,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;
    app_guard(&challenge, Some("subscription"))?;

    let request = decode_challenge_request(&challenge)?;
    let chain_id = request["methodDetails"]["chainId"]
        .as_u64()
        .ok_or_else(|| anyhow!("missing 'methodDetails.chainId'"))?;
    let token = request["currency"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'currency' in challenge request"))?;

    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    // APP/1 buyer-direct read (chainId): authoritative on-chain Permit2 state
    // (uint48 nonce, subscription contract = spender, Permit2 contract, reserved).
    let a = crate::payment::subscription::facilitator::app_allowance_status(
        payer_addr, token, chain_id,
    )
    .await?;
    if a.subscription_contract.is_empty() {
        bail!("allowance-status returned no subscription contract (spender)");
    }
    let permit2_contract = if a.permit2_contract.is_empty() {
        "0x000000000022D473030F116dDEE9F6B43aC78BA3".to_string()
    } else {
        a.permit2_contract.clone()
    };

    // Adapt app/1 challenge → x402 offer, then double-sign with the injected
    // allowance-status (so on-chain reads went through the app/1 endpoint).
    let accepted = build_app_subscription_accepted(&request, token, &a, &permit2_contract)?;
    let signed = match change_sub_id {
        Some(old) => {
            crate::payment::subscription::sign::sign_change_with_allowance(
                &chain_index,
                chain_id,
                payer_addr,
                old,
                &accepted,
                &a,
                Some(expected_cap),
            )
            .await
        }
        None => {
            crate::payment::subscription::sign::sign_subscribe_with_allowance(
                &chain_index,
                chain_id,
                payer_addr,
                &accepted,
                &a,
                Some(expected_cap),
            )
            .await
        }
    }
    .map_err(payment_flow::map_pre_signing_allowance_error)?;

    // The decoded subscription challenge.request must carry `permitHash`
    // (= keccak256 of the PermitSingle) to bind the terms signature to this
    // permit. Its absence produces invalid_params. The
    // seller can't compute it at challenge time (permit amount/nonce/expiration
    // are buyer-derived from allowance-status), so the buyer injects it after
    // signing and re-encodes challenge.request with JCS. termsDeadline and salt
    // are already seller-pinned. subscriptionContract is echoed from
    // allowance-status. Everything else (nested payload shape, spender =
    // subscription contract) stays as the reused x402 double-sign.
    let payload_val = serde_json::to_value(&signed.payload)?;
    let mut request_full = request.clone();
    if let Some(obj) = request_full.as_object_mut() {
        if let Some(ph) = payload_val.get("terms").and_then(|t| t.get("permitHash")) {
            obj.insert("permitHash".to_string(), ph.clone());
        }
        obj.insert(
            "subscriptionContract".to_string(),
            Value::String(a.subscription_contract.clone()),
        );
    }
    let mut challenge_echo = build_app_challenge_echo(&challenge);
    challenge_echo["request"] = Value::String(base64url_encode_json(&request_full)?);

    // The subscription settle `payload` is the flat Permit2 AllowanceTransfer
    // object, not the nested `permit{}` / `terms{}`
    // structs. The terms live in challenge.request (validated there); the payload
    // carries only the PermitSingle fields + BOTH signatures (`signature` =
    // PermitSingle sig, `termsSignature` = SubscriptionTerms sig). `nonce`,
    // `expiration`, `sigDeadline` are JSON numbers per the spec. `spender` is the
    // value that was actually signed (subscription contract — see sign.rs), so
    // the facilitator can recover the PermitSingle signature.
    let permit = &payload_val["permit"];
    let details = &permit["details"];
    let sig_deadline = permit["sigDeadline"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Value::from)
        .unwrap_or_else(|| permit["sigDeadline"].clone());
    let flat_payload = json!({
        "type": "permit2",
        "from": payer_addr,
        "token": details["token"],
        "allowance": details["amount"],
        "spender": permit["spender"],
        "nonce": details["nonce"],
        "expiration": details["expiration"],
        "sigDeadline": sig_deadline,
        "signature": payload_val["permitSignature"],
        "termsSignature": payload_val["termsSignature"],
    });

    let credential = json!({
        "challenge": challenge_echo,
        "source": format!("did:pkh:eip155:{chain_id}:{payer_addr}"),
        "payload": flat_payload,
    });
    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);
    let action = if change_sub_id.is_some() {
        "change"
    } else {
        "subscribe"
    };
    // With `--no-cache`, stage an inactive host→subId mapping. The authoritative
    // list read activates it after seller settlement, while access cannot use it
    // early. Existing active mappings survive unconfirmed changes.
    crate::commands::payment::subscription::cache_subscription(
        "app/1",
        url,
        &signed.payload,
        &signed.sub_id,
        &signed.plan_id,
        no_cache,
    );
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "subscription",
        "action": action,
        "authorization_header": authorization_header,
        "wallet": payer_addr,
        "sub_id": signed.sub_id,
        "cached_for_host": if no_cache {
            None
        } else {
            url.map(crate::payment::subscription::cache::host_of)
        },
        "reconcile_for_host": if no_cache {
            url.map(crate::payment::subscription::cache::host_of)
        } else {
            None
        },
        "challenge": { "id": challenge["id"], "realm": challenge["realm"] },
    }));
    Ok(())
}

/// `payment app subscription cancel` — sign a payer cancelAuthorization.
/// `payment app subscription access` — build an `APP-Access` proof header to
/// access a Seller's protected resource after subscribing (reuses the x402
/// `build_access_proof`: EIP-191 personal_sign over `keccak(subId ‖ payer ‖
/// timestamp)`). The buyer sends the returned header to the resource URL; the
/// Seller recovers the signer and checks the subscription is active. No 402.
async fn cmd_app_subscription_access(
    url: Option<&str>,
    sub_id: Option<&str>,
    from: Option<&str>,
    chain: &str,
) -> Result<()> {
    use crate::payment::subscription::cache::{host_of, SubscriptionCache};
    // Resolve the subId: explicit override, else the active cache entry for the
    // resource URL's host (written by subscribe/change --url). Mirrors x402.
    let (resolved_sub, source) = match sub_id {
        Some(sid) => (sid.to_string(), "override"),
        None => {
            let url = url.ok_or_else(|| {
                anyhow!("access needs --url <resourceUrl> (to resolve the subId from cache) or --sub-id")
            })?;
            let cache = SubscriptionCache::load();
            let entry = cache.resolve("app/1", url).ok_or_else(|| {
                anyhow!(
                    "no active subscription cached for host {}. Subscribe with \
                     `payment app subscription subscribe --url {url} …`, run \
                     `payment app subscription list` to reconcile, or pass --sub-id.",
                    host_of(url)
                )
            })?;
            (entry.sub_id.clone(), "cache")
        }
    };
    let (chain_index, _chain_id, payer) =
        payment_flow::resolve_chain_and_payer_by_chain(chain, from).await?;
    let (hname, hvalue) =
        crate::payment::subscription::sign::build_access_proof(&chain_index, &payer, &resolved_sub)
            .await?;
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "subscription",
        "action": "access",
        "sub_id": resolved_sub,
        "sub_id_source": source,
        "wallet": payer,
        "access_header_name": hname,
        "access_header_value": hvalue,
        "next_step": format!("send `{hname}: {hvalue}` to the protected resource URL (no `Payment ` prefix)"),
    }));
    Ok(())
}

/// Resolve the subscription contract for a cancel auth: use `--subscription-contract`
/// if given, else look it up from APP/1 allowance-status via `--token` (mirrors the
/// x402 `resolve_contract` — no more forcing the user to paste the contract).
async fn resolve_app_contract(
    contract: Option<&str>,
    token: Option<&str>,
    payer: &str,
    chain_id: u64,
) -> Result<String> {
    if let Some(c) = contract {
        return Ok(c.to_string());
    }
    let token =
        token.ok_or_else(|| anyhow!("cancel needs --subscription-contract or --token"))?;
    let a = crate::payment::subscription::facilitator::app_allowance_status(payer, token, chain_id)
        .await?;
    if a.subscription_contract.is_empty() {
        bail!("allowance-status returned no subscriptionContract for token {token}");
    }
    Ok(a.subscription_contract)
}

async fn cmd_app_subscription_cancel(
    sub_id: &str,
    offer_id: &str,
    subscription_contract: Option<&str>,
    token: Option<&str>,
    chain_id: u64,
    from: Option<&str>,
) -> Result<()> {
    if offer_id.trim().is_empty() {
        bail!("cancel requires a non-empty --offer-id");
    }
    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let contract =
        resolve_app_contract(subscription_contract, token, &payer_addr, chain_id).await?;
    let _operation_lock = subscription_operation_state::lock_subscription(sub_id)?;
    subscription_operation_state::ensure_no_pending(sub_id)?;
    let auth = crate::payment::subscription::sign::sign_cancel(
        &chain_index,
        chain_id,
        &payer_addr,
        sub_id,
        &contract,
        3600,
    )
    .await?;
    let operation = subscription_operation_state::begin_pending(
        subscription_operation_state::PendingAction::Cancel,
        sub_id,
        None,
        offer_id,
        &payer_addr,
    )?;
    // Signing only authorizes the seller to cancel. Keep the cache active until
    // a matching successful receipt resolves the pending operation.
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "subscription",
        "action": "cancel",
        "cancelAuthorization": serde_json::to_value(&auth)?,
        "operation_id": operation.operation_id,
        "offer_id": offer_id,
        "wallet": payer_addr,
        "next_step": "seller submits { offerId, cancelAuthorization } to POST /subscription/cancel, then resolve the operation with its Payment-Receipt",
    }));
    Ok(())
}

/// `payment app subscription allowance-status` — read-only APP/1 Permit2
/// status (uint48 nonce, reserved/committed, contracts). Mirrors the x402
/// `allowance-status`; backend wrapper `app_allowance_status` already exists.
async fn cmd_app_subscription_allowance_status(
    token: &str,
    chain_id: u64,
    from: Option<&str>,
) -> Result<()> {
    let (_chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let a = crate::payment::subscription::facilitator::app_allowance_status(
        &payer_addr,
        token,
        chain_id,
    )
    .await?;
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "subscription",
        "action": "allowance-status",
        "wallet": payer_addr,
        "token": token,
        "chainId": chain_id,
        "approvedAmount": a.approved_amount,
        "expiration": a.expiration,
        "nonce": a.nonce,
        "reservedAmount": a.reserved_amount,
        "reservedExpiration": a.reserved_expiration,
        "tokenBalance": a.token_balance,
        "availableAmount": a.available_amount,
        "permit2Allowance": a.permit2_allowance,
        "subscriptionContract": a.subscription_contract,
        "permit2Contract": a.permit2_contract,
    }));
    Ok(())
}

/// `payment app subscription cancel-pending` — sign an authorization to revoke a
/// not-yet-effective downgrade.
async fn cmd_app_subscription_cancel_pending(
    sub_id: &str,
    new_sub_id: &str,
    offer_id: &str,
    subscription_contract: Option<&str>,
    token: Option<&str>,
    chain_id: u64,
    from: Option<&str>,
) -> Result<()> {
    if offer_id.trim().is_empty() {
        bail!("cancel-pending requires a non-empty --offer-id");
    }
    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let contract =
        resolve_app_contract(subscription_contract, token, &payer_addr, chain_id).await?;
    let _operation_lock = subscription_operation_state::lock_subscription(sub_id)?;
    subscription_operation_state::ensure_no_pending(sub_id)?;
    let auth = crate::payment::subscription::sign::sign_cancel_pending_change(
        &chain_index,
        chain_id,
        &payer_addr,
        sub_id,
        new_sub_id,
        &contract,
    )
    .await?;
    let operation = subscription_operation_state::begin_pending(
        subscription_operation_state::PendingAction::CancelPending,
        sub_id,
        Some(new_sub_id),
        offer_id,
        &payer_addr,
    )?;
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "subscription",
        "action": "cancel-pending-change",
        "authorization": serde_json::to_value(&auth)?,
        "operation_id": operation.operation_id,
        "offer_id": offer_id,
        "wallet": payer_addr,
        "next_step": "seller submits { offerId, authorization } to POST /subscription/cancel-pending-change, then resolve the operation with its Payment-Receipt",
    }));
    Ok(())
}

fn app_subscription_receipt_status(
    receipt: &str,
    pending: &subscription_operation_state::OperationState,
) -> Result<(String, Option<String>)> {
    let value = decode_payment_blob(receipt).context("decode APP subscription receipt")?;
    let require = |field: &str| {
        value[field]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("APP subscription receipt is missing {field}"))
    };
    if require("protocol")? != "app/1" {
        bail!("APP subscription receipt protocol must be app/1");
    }
    if require("intent")? != "subscription" {
        bail!("APP subscription receipt intent must be subscription");
    }
    if require("offerId")? != pending.offer_id {
        bail!("APP subscription receipt offerId does not match the pending operation");
    }
    if !require("payer")?.eq_ignore_ascii_case(&pending.payer) {
        bail!("APP subscription receipt payer does not match the pending operation");
    }
    let status = decode_receipt::decode_receipt(Some(receipt), None)?
        .status
        .to_ascii_lowercase();
    if !matches!(status.as_str(), "success" | "failed" | "pending" | "timeout") {
        bail!("APP subscription receipt has unsupported status {status}");
    }
    if status != "success" {
        return Ok((status, None));
    }
    let reference = require("reference")?.to_ascii_lowercase();
    Ok((status, Some(reference)))
}

fn cmd_app_subscription_resolve(
    operation_id: &str,
    receipt: Option<&str>,
    outcome: Option<&str>,
) -> Result<()> {
    if let Some(resolved) = subscription_operation_state::read_resolution(operation_id) {
        let _operation_lock =
            subscription_operation_state::lock_subscription(&resolved.sub_id)?;
        finish_app_subscription_resolution(&resolved)?;
        emit_app_subscription_resolution(&resolved, true);
        return Ok(());
    }
    let candidate = subscription_operation_state::read_pending(operation_id).ok_or_else(|| {
        anyhow!(
            "APP subscription operation {operation_id} is not pending; it may already be resolved"
        )
    })?;
    let _operation_lock = subscription_operation_state::lock_subscription(&candidate.sub_id)?;
    if let Some(resolved) = subscription_operation_state::read_resolution(operation_id) {
        finish_app_subscription_resolution(&resolved)?;
        emit_app_subscription_resolution(&resolved, true);
        return Ok(());
    }
    let pending = subscription_operation_state::read_pending(operation_id)
        .ok_or_else(|| anyhow!("APP subscription operation {operation_id} is not pending"))?;
    let (resolution, receipt_reference) = if let Some(receipt) = receipt {
        app_subscription_receipt_status(receipt, &pending)?
    } else {
        (
            outcome
                .ok_or_else(|| anyhow!("--receipt or --outcome is required"))?
                .to_string(),
            None,
        )
    };
    let committed = resolution == "success";
    let _receipt_lock = if let Some(reference) = receipt_reference.as_deref() {
        Some(subscription_operation_state::lock_receipt_reference(reference)?)
    } else {
        None
    };
    if let Some(reference) = receipt_reference.as_deref() {
        if subscription_operation_state::receipt_reference_used(reference)? {
            bail!("APP subscription receipt reference was already used by another operation");
        }
    }

    // Persist the resolution first so a successful receipt reference is
    // claimed before any cache side effect. A retry completes that side effect
    // idempotently if the process exits after this durable commit point.
    let resolved = subscription_operation_state::resolve(
        pending,
        &resolution,
        committed,
        receipt_reference.as_deref(),
    )?;
    finish_app_subscription_resolution(&resolved)?;
    emit_app_subscription_resolution(&resolved, false);
    Ok(())
}

fn finish_app_subscription_resolution(
    resolved: &subscription_operation_state::OperationState,
) -> Result<()> {
    if resolved.committed
        && matches!(
            resolved.action,
            subscription_operation_state::PendingAction::Cancel
        )
    {
        let mut cache = crate::payment::subscription::cache::SubscriptionCache::load();
        if cache.mark_app_canceled(&resolved.sub_id) > 0 {
            cache.save()?;
        }
    }
    Ok(())
}

fn emit_app_subscription_resolution(
    resolved: &subscription_operation_state::OperationState,
    already_resolved: bool,
) {
    let operation = match resolved.action {
        subscription_operation_state::PendingAction::Cancel => "cancel",
        subscription_operation_state::PendingAction::CancelPending => "cancel-pending",
    };
    output::success(json!({
        "protocol": "app/1",
        "method": "evm",
        "intent": "subscription",
        "action": "resolve",
        "operation": operation,
        "operation_id": resolved.operation_id,
        "sub_id": resolved.sub_id,
        "new_sub_id": resolved.new_sub_id,
        "resolution": resolved.resolution,
        "committed": resolved.committed,
        "already_resolved": already_resolved,
    }));
}

/// `payment app subscription list` — the buyer's own APP/1 subscriptions
/// (buyer-direct read; EVM address is chain-independent, resolved
/// via X Layer for the selected account).
async fn cmd_app_subscription_list(from: Option<&str>, limit: u32, offset: u32) -> Result<()> {
    let (_chain_index, payer_addr) = resolve_chain_and_payer(196, from).await?;
    let resp = crate::payment::subscription::facilitator::app_list(&payer_addr, limit, offset).await?;
    // Reconcile the host→subId cache from authoritative on-chain state, following
    // `changedToSubId` migrations (so `access --url` tracks upgrades/downgrades).
    let mut cache = crate::payment::subscription::cache::SubscriptionCache::load();
    cache.reconcile_from(&resp.subscriptions);
    if let Err(e) = cache.save() {
        eprintln!("Warning: subscription cache reconcile save failed: {e:#}");
    }
    output::success(json!({
        "protocol": "app/1",
        "intent": "subscription",
        "action": "list",
        "buyer": payer_addr,
        "count": resp.subscriptions.len(),
        "subscriptions": serde_json::to_value(&resp.subscriptions)?,
    }));
    Ok(())
}

/// onchainos payment charge: Charge single payment.
/// - feePayer=true (default): TEE-sign EIP-3009, emit TransactionPayload (+ splits if present).
/// - feePayer=false: require --tx-hash, emit HashPayload (client self-broadcasts).
async fn cmd_mpp_charge(
    challenge_header: &str,
    from: Option<&str>,
    tx_hash: Option<&str>,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;
    let request = decode_challenge_request(&challenge)?;

    let recipient_in = request["recipient"]
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

    // XKO/0x dual capture: canonical → signing, display → CLI output JSON.
    let (recipient, recipient_display) = parse_recipient_addr(recipient_in, chain_id)
        .with_context(|| "challenge.request.recipient")?;
    let recipient = recipient.as_str();
    let recipient_display = recipient_display.as_str();

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
    let (primary_value, parsed_splits) = compute_primary_split_amounts(&request, chain_id)?;
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
        "to": recipient_display,
        "value": primary_value,
        "validAfter": "0",
        "validBefore": valid_before,
        "nonce": nonce,
        "signature": signature,
    });

    if !parsed_splits.is_empty() {
        let mut signed_splits = Vec::with_capacity(parsed_splits.len());
        for (i, split) in parsed_splits.iter().enumerate() {
            let split_nonce = random_nonce_hex();
            // Splits use the same transferWithAuthorization typehash as primary.
            let (split_sig, _) = tee_sign_eip3009(
                Eip3009AuthType::Transfer,
                &chain_index,
                payer_addr,
                &split.canonical,
                &split.amount,
                &valid_before,
                &split_nonce,
                currency,
            )
            .await
            .with_context(|| format!("splits[{}] TEE sign failed", i))?;
            signed_splits.push(json!({
                "from": payer_addr,
                "to": &split.display,
                "value": &split.amount,
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

/// Merge the decision-layer `data` fields (`strategy`, `cumulative_amount`,
/// `needsTopUp`, `sessionSnapshot`, `refund`, `recovery`, `reason_text`) from
/// `payment_flow::fetch_session` into a session handler's base output object, so
/// every session sub-op emits the spec'd fields — the CLI (not the agent) does
/// the reuse/sign + cumulative + top-up + refund arithmetic. Best-effort: on any
/// error the base output is emitted unchanged rather than failing a fund-adjacent
/// command that has already produced its authorization header.
async fn emit_session(mut base: Value, params: payment_flow::SessionParams) {
    if let Ok(decision) = payment_flow::fetch_session(params).await {
        if let (Some(obj), Some(extra)) = (base.as_object_mut(), decision.as_object()) {
            for (k, v) in extra {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    output::success(base);
}

/// Persist the channel deposit + initial cumulative at `open`, so later
/// `voucher`/`close` can compute `needsTopUp`/`refund` from the real deposit.
/// Best-effort: a persistence failure never blocks emitting the open credential.
fn persist_channel_open(channel_id: &str, payer_addr: &str, deposit: &str, initial_cum: &str) {
    let now = session_state::now_unix();
    let _ = session_state::ChannelState {
        channel_id: channel_id.to_string(),
        owner_wallet: payer_addr.to_string(),
        deposit: deposit.to_string(),
        cumulative: initial_cum.to_string(),
        created_at: now,
        updated_at: now,
        pending_operation: None,
        resolved_operations: Vec::new(),
    }
    .write();
}

/// Decision-layer params for `open`: strategy=sign (initial voucher), cumulative =
/// the baseline, deposit for the top-up guard.
fn session_open_params(
    channel_id: &str,
    deposit: &str,
    initial_cum: &str,
) -> payment_flow::SessionParams {
    payment_flow::SessionParams {
        action: "open".to_string(),
        channel_id: Some(channel_id.to_string()),
        cumulative_amount: Some(initial_cum.to_string()),
        unit_amount: Some("0".to_string()),
        deposit: Some(deposit.to_string()),
        ..Default::default()
    }
}

/// onchainos payment session open: Open payment channel.
/// - feePayer=true (default): TEE-sign EIP-3009 deposit + initial voucher → transaction payload.
/// - feePayer=false: require --tx-hash AND --salt of the client-broadcast open tx;
///   the salt MUST match the one passed to `escrow.open(...)` on-chain so we can
///   reproduce the channelId. Initial voucher is still TEE-signed.
#[allow(clippy::too_many_arguments)]
async fn cmd_mpp_session_open(
    challenge_header: &str,
    deposit: &str,
    from: Option<&str>,
    tx_hash: Option<&str>,
    salt_arg: Option<&str>,
    initial_cum_arg: Option<&str>,
    prepay_first: bool,
) -> Result<()> {
    let challenge = parse_www_authenticate(challenge_header)?;
    let request = decode_challenge_request(&challenge)?;

    let recipient_in = request["recipient"]
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

    // XKO/0x normalize: open path only consumes the canonical 0x form (channelId
    // hash + EIP-3009 nonce); recipient isn't echoed in the CLI output JSON.
    let (recipient, _recipient_display) = parse_recipient_addr(recipient_in, chain_id)
        .with_context(|| "challenge.request.recipient")?;
    let recipient = recipient.as_str();

    // Resolve chain + payer
    let (chain_index, payer_addr) = resolve_chain_and_payer(chain_id, from).await?;
    let payer_addr = payer_addr.as_str();

    // hash mode: client already broadcast escrow.open(salt=X); CLI must
    // recompute channelId with the SAME X, so --salt is required.
    // transaction mode: nothing on-chain yet, fresh OS-random is fine.
    let salt = match (fee_payer, salt_arg) {
        (false, Some(s)) => normalize_bytes32_hex(s, "--salt")?,
        (false, None) => bail!(
            "hash mode (feePayer=false) requires --salt: the same bytes32 you \
             passed to your on-chain `escrow.open(...)` call (0x + 64 hex chars)"
        ),
        (true, Some(_)) => bail!(
            "--salt is only valid when challenge.methodDetails.feePayer=false \
             (hash mode); transaction mode generates its own salt during \
             `escrow.openWithAuthorization(...)`. Drop --salt or switch modes."
        ),
        (true, None) => {
            use rand::RngCore;
            let mut s = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut s);
            format!("0x{}", hex::encode(s))
        }
    };
    // Reject wrong-mode --tx-hash before the TEE round-trip below.
    if fee_payer && tx_hash.is_some() {
        bail!("--tx-hash is only valid when challenge.methodDetails.feePayer=false");
    }
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
    let initial_voucher_sig = tee_sign_voucher(
        &chain_index,
        payer_addr,
        &channel_id,
        &initial_cum,
        escrow,
        chain_id,
    )
    .await?;

    if !fee_payer {
        // Hash mode: client already broadcast the open tx, just wrap the hash
        let hash = tx_hash.ok_or_else(|| {
            anyhow!(
                "hash mode (feePayer=false) requires --tx-hash (broadcast \
                 `escrow.open(...)` yourself first)"
            )
        })?;
        let hash = normalize_bytes32_hex(hash, "--tx-hash")?;
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
        persist_channel_open(&channel_id, payer_addr, deposit, &initial_cum);
        emit_session(
            json!({
                "protocol": "mpp",
                "action": "session_open",
                "mode": "hash",
                "authorization_header": authorization_header,
                "channel_id": channel_id,
                "escrow": escrow,
                "chain_id": chain_id,
                "deposit": deposit,
                "wallet": payer_addr,
            }),
            session_open_params(&channel_id, deposit, &initial_cum),
        )
        .await;
        return Ok(());
    }

    // Transaction mode: TEE sign EIP-3009 for deposit
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let valid_before = compute_valid_before(&challenge, now)?;

    // Parse splits (optional) — needed for nonce derivation. The contract
    // openWithAuthorization validates the EIP-3009 nonce with the same formula.
    let (split_recipients, split_bps) = parse_session_splits(&request, chain_id)?;

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

    persist_channel_open(&channel_id, payer_addr, deposit, &initial_cum);
    emit_session(
        json!({
            "protocol": "mpp",
            "action": "session_open",
            "mode": "transaction",
            "authorization_header": authorization_header,
            "channel_id": channel_id,
            "escrow": escrow,
            "chain_id": chain_id,
            "deposit": deposit,
            "wallet": payer_addr,
        }),
        session_open_params(&channel_id, deposit, &initial_cum),
    )
    .await;
    Ok(())
}

/// Viability gate for advancing the persisted session cumulative after signing a
/// voucher. Advance only when the voucher moves the cumulative forward (`unit > 0`)
/// and — when the deposit is known — the new cumulative stays within it. When the
/// deposit is unknown we still advance (best-effort). Mirrors `fetch_session`'s
/// `needsTopUp` predicate: a voucher that over-spends the deposit / needs top-up
/// (which the seller rejects) must NOT push the local cumulative forward, or the
/// next voucher's `unit = new − prior` baseline becomes optimistic.
fn voucher_advances_cumulative(unit: u128, new_cum_u: u128, deposit_u: Option<u128>) -> bool {
    unit > 0 && deposit_u.is_none_or(|d| new_cum_u <= d)
}

fn parse_app_session_amount(value: &str, label: &str) -> Result<u128> {
    crate::validators::validate_non_negative_integer(value, label)?;
    value
        .parse::<u128>()
        .with_context(|| format!("--{label} exceeds the supported uint128 range"))
}

/// APP/1 Session open uses the Seller's fixed challenge amount as its only
/// deposit source. Validate it before wallet resolution or TEE signing.
fn app_session_open_deposit(request: &Value) -> Result<&str> {
    let deposit = request["amount"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'amount' in APP session challenge request"))?;
    crate::validators::validate_amount(deposit)
        .context("invalid APP session challenge request.amount")?;
    deposit
        .parse::<u128>()
        .context("APP session challenge request.amount exceeds the supported uint128 range")?;
    Ok(deposit)
}

#[cfg(test)]
mod app_session_open_tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: PaymentCommand,
    }

    #[test]
    fn accepts_only_the_challenge_deposit() {
        let request = json!({ "amount": "1000000" });
        assert_eq!(app_session_open_deposit(&request).unwrap(), "1000000");
        let max_supported = json!({
            "amount": "340282366920938463463374607431768211455"
        });
        assert_eq!(
            app_session_open_deposit(&max_supported).unwrap(),
            "340282366920938463463374607431768211455"
        );

        let cli = TestCli::parse_from([
            "test",
            "app",
            "session",
            "open",
            "--challenge",
            "Payment id=\"1\", realm=\"r\", protocol=\"app/1\", method=\"evm\", intent=\"session\", request=\"e30\"",
        ]);
        assert!(matches!(
            cli.command,
            PaymentCommand::App {
                command: AppCommand::Session {
                    command: AppSessionCommand::Open { .. }
                }
            }
        ));
    }

    #[test]
    fn rejects_removed_deposit_flag() {
        let parsed = TestCli::try_parse_from([
            "test",
            "app",
            "session",
            "open",
            "--challenge",
            "Payment id=\"1\", realm=\"r\", protocol=\"app/1\", method=\"evm\", intent=\"session\", request=\"e30\"",
            "--deposit",
            "999999",
        ]);
        assert!(parsed.is_err());
    }

    #[test]
    fn rejects_invalid_challenge_deposit() {
        for amount in ["0", "01", "-1", "1.5", " 1"] {
            let request = json!({ "amount": amount });
            assert!(
                app_session_open_deposit(&request).is_err(),
                "amount {amount:?} should be rejected"
            );
        }

        let missing = json!({});
        assert!(app_session_open_deposit(&missing).is_err());
        let non_string = json!({ "amount": 100 });
        assert!(app_session_open_deposit(&non_string).is_err());
        let session_range_overflow = json!({
            "amount": "340282366920938463463374607431768211456"
        });
        assert!(app_session_open_deposit(&session_range_overflow).is_err());
    }
}

/// APP vouchers must advance the cumulative amount without exceeding the
/// channel's currently persisted deposit. Call this before any TEE signing.
fn validate_app_voucher_cumulative(current: &str, deposit: &str, requested: &str) -> Result<()> {
    let current = parse_app_session_amount(current, "current-cumulative-amount")?;
    let deposit = parse_app_session_amount(deposit, "channel-deposit")?;
    let requested = parse_app_session_amount(requested, "cumulative-amount")?;

    if current > deposit {
        bail!(
            "APP session state is inconsistent: current cumulative {current} exceeds deposit \
             {deposit}; refusing to sign"
        );
    }
    if requested <= current {
        bail!(
            "--cumulative-amount must be greater than the current cumulative {current}; \
             got {requested}"
        );
    }
    if requested > deposit {
        bail!(
            "--cumulative-amount {requested} exceeds the current channel deposit {deposit}; \
             top up the channel before signing"
        );
    }
    Ok(())
}

/// APP close reuses the latest voucher cumulative. It never raises or lowers
/// the authorization while closing the channel.
fn validate_app_close_cumulative(current: &str, deposit: &str, requested: &str) -> Result<()> {
    let current = parse_app_session_amount(current, "current-cumulative-amount")?;
    let deposit = parse_app_session_amount(deposit, "channel-deposit")?;
    let requested = parse_app_session_amount(requested, "cumulative-amount")?;

    if current > deposit {
        bail!(
            "APP session state is inconsistent: current cumulative {current} exceeds deposit \
             {deposit}; refusing to sign"
        );
    }
    if requested != current {
        bail!(
            "APP session close must reuse the latest cumulative amount {current}; got {requested}"
        );
    }
    Ok(())
}

/// onchainos payment session voucher: sign EIP-712 voucher (or wrap an existing
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
        let escrow = escrow
            .ok_or_else(|| anyhow!("--escrow is required when not using --reuse-signature"))?;
        let chain_id = chain_id
            .ok_or_else(|| anyhow!("--chain-id is required when not using --reuse-signature"))?;

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

    // Decision layer: pull the channel's persisted deposit + prior
    // cumulative so the CLI — not the agent — computes strategy/needsTopUp.
    // The voucher flag is the ABSOLUTE new cumulative; the prior cumulative comes
    // from persisted state (0 if unknown), and unit = new − prior.
    let prior = session_state::read(channel_id);
    let prior_cum = prior
        .as_ref()
        .map(|s| s.cumulative.clone())
        .unwrap_or_else(|| "0".to_string());
    let deposit = prior.as_ref().map(|s| s.deposit.clone());
    let new_cum_u = cumulative_amount.parse::<u128>().unwrap_or(0);
    let prior_cum_u = prior_cum.parse::<u128>().unwrap_or(0);
    let unit = new_cum_u.saturating_sub(prior_cum_u);

    // Advance persisted cumulative only when the voucher is viable — i.e.
    // unit > 0 and (deposit known ⇒ new cumulative within deposit). The stored
    // value is the just-SIGNED cumulative, NOT one the seller has confirmed; a
    // voucher that over-spends the deposit / needs top-up will be rejected by
    // the seller, so advancing on it would make the next voucher's
    // `unit = new − prior` baseline optimistic. When the deposit is unknown we
    // still advance (best-effort). Over-deposit / top-up drift is ultimately
    // corrected by the 70015 `server_cumulative` re-sign (seller SDK is the
    // final authority). Same `needsTopUp` predicate as `fetch_session`.
    let deposit_u = deposit.as_deref().and_then(|d| d.parse::<u128>().ok());
    let viable = voucher_advances_cumulative(unit, new_cum_u, deposit_u);
    if viable {
        if let Some(mut s) = prior {
            s.cumulative = cumulative_amount.to_string();
            s.updated_at = session_state::now_unix();
            let _ = s.write();
        }
    }

    emit_session(
        json!({
            "protocol": "mpp",
            "action": "voucher",
            "mode": mode,
            "authorization_header": authorization_header,
            "channel_id": channel_id,
            "signature": voucher_sig,
        }),
        payment_flow::SessionParams {
            action: "voucher".to_string(),
            channel_id: Some(channel_id.to_string()),
            cumulative_amount: Some(prior_cum),
            unit_amount: Some(unit.to_string()),
            deposit,
            reuse_signature: reuse_signature.map(str::to_string),
            ..Default::default()
        },
    )
    .await;
    Ok(())
}

/// onchainos payment session topup: TopUp an existing session channel.
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
        let nonce = compute_topup_nonce(payer_addr, channel_id, additional_deposit, &top_up_salt)?;

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

    // Grow the persisted channel deposit by the additional amount so the
    // next `voucher`'s top-up guard / `close`'s refund see the new balance.
    let prior = session_state::read(channel_id);
    let prior_cum = prior
        .as_ref()
        .map(|s| s.cumulative.clone())
        .unwrap_or_else(|| "0".to_string());
    let new_deposit = prior
        .as_ref()
        .map(|s| {
            (s.deposit.parse::<u128>().unwrap_or(0))
                .saturating_add(additional_deposit.parse::<u128>().unwrap_or(0))
                .to_string()
        })
        .unwrap_or_else(|| additional_deposit.to_string());
    let now = session_state::now_unix();
    let _ = session_state::ChannelState {
        channel_id: channel_id.to_string(),
        owner_wallet: payer_addr.to_string(),
        deposit: new_deposit.clone(),
        cumulative: prior_cum.clone(),
        created_at: prior.as_ref().map(|s| s.created_at).unwrap_or(now),
        updated_at: now,
        pending_operation: prior
            .as_ref()
            .and_then(|state| state.pending_operation.clone()),
        resolved_operations: prior
            .map(|state| state.resolved_operations)
            .unwrap_or_default(),
    }
    .write();

    emit_session(
        json!({
            "protocol": "mpp",
            "action": "session_topup",
            "mode": if tx_hash.is_some() { "hash" } else { "transaction" },
            "authorization_header": authorization_header,
            "channel_id": channel_id,
            "additional_deposit": additional_deposit,
            "wallet": payer_addr,
        }),
        payment_flow::SessionParams {
            action: "topup".to_string(),
            channel_id: Some(channel_id.to_string()),
            cumulative_amount: Some(prior_cum),
            unit_amount: Some("0".to_string()),
            deposit: Some(new_deposit),
            ..Default::default()
        },
    )
    .await;
    Ok(())
}

/// onchainos payment session close: Sign final voucher + build close credential
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

    let authorization_header = format!("Payment {}", base64url_encode_json(&credential)?);

    // Decision layer: refund = deposit − final_cum, from the channel's
    // persisted deposit. `--cumulative-amount` is the final ABSOLUTE cum.
    let prior = session_state::read(channel_id);
    let prior_cum = prior
        .as_ref()
        .map(|s| s.cumulative.clone())
        .unwrap_or_else(|| "0".to_string());
    let deposit = prior.as_ref().map(|s| s.deposit.clone());
    let final_cum_u = cumulative_amount.parse::<u128>().unwrap_or(0);
    let prior_cum_u = prior_cum.parse::<u128>().unwrap_or(0);
    let unit = final_cum_u.saturating_sub(prior_cum_u);

    emit_session(
        json!({
            "protocol": "mpp",
            "action": "session_close",
            "authorization_header": authorization_header,
            "channel_id": channel_id,
        }),
        payment_flow::SessionParams {
            action: "close".to_string(),
            channel_id: Some(channel_id.to_string()),
            cumulative_amount: Some(prior_cum),
            unit_amount: Some(unit.to_string()),
            deposit,
            ..Default::default()
        },
    )
    .await;

    // Channel is closed — drop its persisted state.
    session_state::cleanup(channel_id);
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

    #[test]
    fn cli_payload_payment_binds_payer_without_changing_two_phase_mode() {
        match TestCli::parse_from([
            "test",
            "pay",
            "--payload",
            "unsigned-fixture",
            "--selected-index",
            "1",
            "--from",
            "0x1111111111111111111111111111111111111111",
        ])
        .command
        {
            PaymentCommand::X402Pay {
                from,
                selected_index,
                ..
            } => {
                assert_eq!(
                    from.as_deref(),
                    Some("0x1111111111111111111111111111111111111111")
                );
                assert_eq!(selected_index, Some(1));
            }
            _ => panic!("expected payload payment"),
        }
        assert!(TestCli::try_parse_from([
            "test",
            "pay",
            "--payment-id",
            "payment-fixture",
            "--from",
            "wallet-fixture",
        ])
        .is_err());
    }

    /// Wrapper so clap can parse PaymentCommand as a top-level subcommand.
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: PaymentCommand,
    }

    // ── CLI argument parsing ──────────────────────────────────────────

    #[test]
    fn cli_x402_pay_payload_and_optional_index() {
        let b64 = "eyJ4NDAyVmVyc2lvbiI6Mn0"; // base64url of {"x402Version":2}
                                             // payload only → no index (CLI auto-selects).
        match TestCli::parse_from(["test", "pay", "--payload", b64]).command {
            PaymentCommand::X402Pay {
                payload,
                selected_index,
                ..
            } => {
                assert_eq!(payload.as_deref(), Some(b64));
                assert_eq!(selected_index, None);
            }
            _ => panic!("expected X402Pay"),
        }
        // multi-scheme: pin the user's pick by index.
        match TestCli::parse_from(["test", "pay", "--payload", b64, "--selected-index", "1"])
            .command
        {
            PaymentCommand::X402Pay { selected_index, .. } => {
                assert_eq!(selected_index, Some(1));
            }
            _ => panic!("expected X402Pay"),
        }
    }

    #[test]
    fn cli_x402_pay_requires_payload_or_payment_id() {
        // Neither --payload nor --payment-id → clap rejects.
        assert!(TestCli::try_parse_from(["test", "pay"]).is_err());
        // legacy flags are gone
        assert!(TestCli::try_parse_from(["test", "pay", "--accepts", "[]"]).is_err());
        // --payload and --payment-id are mutually exclusive.
        assert!(
            TestCli::try_parse_from(["test", "pay", "--payload", "x", "--payment-id", "p"])
                .is_err()
        );
    }

    #[test]
    fn cli_pay_two_phase_parses_payment_id_yes_force() {
        // Two-phase mode: --payment-id, --selected-index, --param, --yes.
        match TestCli::parse_from([
            "test",
            "pay",
            "--payment-id",
            "pay_abc",
            "--selected-index",
            "0",
            "--param",
            "orderId=1",
            "--yes",
        ])
        .command
        {
            PaymentCommand::X402Pay {
                payment_id,
                selected_index,
                param,
                yes,
                payload,
                ..
            } => {
                assert_eq!(payment_id.as_deref(), Some("pay_abc"));
                assert_eq!(selected_index, Some(0));
                assert_eq!(param, vec!["orderId=1".to_string()]);
                assert!(yes);
                assert!(payload.is_none());
            }
            _ => panic!("expected X402Pay"),
        }
        // --force is a visible alias of --yes.
        match TestCli::parse_from(["test", "pay", "--payment-id", "p", "--force"]).command {
            PaymentCommand::X402Pay { yes, .. } => assert!(yes),
            _ => panic!("expected X402Pay"),
        }
    }

    #[test]
    fn cli_quote_parses_url_and_params() {
        match TestCli::parse_from([
            "test",
            "quote",
            "https://m.example/x",
            "--param",
            "a=1",
            "--param",
            "b=2",
        ])
        .command
        {
            PaymentCommand::Quote {
                url,
                param,
                method,
                tool,
            } => {
                assert_eq!(url, "https://m.example/x");
                assert_eq!(param, vec!["a=1".to_string(), "b=2".to_string()]);
                assert_eq!(method, "GET"); // default when --method is omitted
                assert_eq!(tool, None); // --tool is absent by default
            }
            _ => panic!("expected Quote"),
        }
    }

    #[test]
    fn cli_quote_parses_tool_flag() {
        // --tool forces the MCP branch; it parses into Some(name).
        match TestCli::parse_from([
            "test",
            "quote",
            "https://m.example/mcp",
            "--tool",
            "get_weather",
            "--param",
            "zip=01234",
        ])
        .command
        {
            PaymentCommand::Quote {
                url, tool, param, ..
            } => {
                assert_eq!(url, "https://m.example/mcp");
                assert_eq!(tool.as_deref(), Some("get_weather"));
                assert_eq!(param, vec!["zip=01234".to_string()]);
            }
            _ => panic!("expected Quote"),
        }
    }

    #[test]
    fn cli_decode_receipt_requires_one_of_header_or_receipt() {
        // header form
        match TestCli::parse_from(["test", "decode-receipt", "--header", "b64"]).command {
            PaymentCommand::DecodeReceipt { header, receipt } => {
                assert_eq!(header.as_deref(), Some("b64"));
                assert!(receipt.is_none());
            }
            _ => panic!("expected DecodeReceipt"),
        }
        // neither → error; both → error (mutually exclusive).
        assert!(TestCli::try_parse_from(["test", "decode-receipt"]).is_err());
        assert!(TestCli::try_parse_from([
            "test",
            "decode-receipt",
            "--header",
            "h",
            "--receipt",
            "r"
        ])
        .is_err());
    }

    // ── eip3009-sign CLI parsing ─────────────────────────────────────

    #[test]
    fn cli_eip3009_sign_takes_only_payload() {
        let b64 = "eyJhY2NlcHRzIjpbXX0"; // base64url of {"accepts":[]}
        let cli = TestCli::parse_from(["test", "pay-local", "--payload", b64]);
        match cli.command {
            PaymentCommand::Eip3009Sign { payload } => assert_eq!(payload, b64),
            _ => panic!("expected Eip3009Sign"),
        }
    }

    #[test]
    fn cli_eip3009_sign_requires_payload() {
        assert!(TestCli::try_parse_from(["test", "pay-local"]).is_err());
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
        let expected = "0xa38cd33d0b42b9654d5077dccc63849159206c9da56748d8d225a1c79100e2b2";
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
            "session",
            "voucher",
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
            "charge",
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
            "session",
            "open",
            "--challenge",
            "Payment id=\"1\", realm=\"r\", method=\"evm\", intent=\"session\", request=\"e30\"",
            "--deposit",
            "1000000",
            "--tx-hash",
            "0x2222222222222222222222222222222222222222222222222222222222222222",
        ]);
        match cli.command {
            PaymentCommand::Session {
                command: SessionCommand::Open { tx_hash, .. },
            } => {
                assert!(tx_hash.is_some());
            }
            _ => panic!("expected Session::Open"),
        }
    }

    #[test]
    fn cli_mpp_session_topup_transaction_mode() {
        let cli = TestCli::parse_from([
            "test",
            "session",
            "topup",
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
            PaymentCommand::Session {
                command:
                    SessionCommand::TopUp {
                        tx_hash, currency, ..
                    },
            } => {
                assert!(tx_hash.is_none());
                assert_eq!(currency.as_deref(), Some("0xUSDC"));
            }
            _ => panic!("expected Session::TopUp"),
        }
    }

    #[test]
    fn cli_mpp_session_topup_hash_mode() {
        let cli = TestCli::parse_from([
            "test",
            "session",
            "topup",
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
            PaymentCommand::Session {
                command: SessionCommand::TopUp { tx_hash, .. },
            } => {
                assert!(tx_hash.is_some());
            }
            _ => panic!("expected Session::TopUp"),
        }
    }

    #[test]
    fn cli_mpp_session_topup_requires_required_fields() {
        // missing --channel-id
        let result = TestCli::try_parse_from([
            "test",
            "session",
            "topup",
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

    // ── select_accepts_index ─────────────────────────────────────────

    #[test]
    fn select_accepts_index_picks_entry() {
        // Multi-scheme: the skill passes the raw PAYMENT-REQUIRED header + the
        // index the user picked in the recommendation card; CLI narrows to that
        // single entry so it can't deviate from the user's choice.
        let accepts = json!([
            {"scheme":"exact","asset":"0xA"},
            {"scheme":"aggr_deferred","asset":"0xB"}
        ]);
        let picked = select_accepts_index(&accepts, 1).unwrap();
        assert_eq!(picked, json!([{"scheme":"aggr_deferred","asset":"0xB"}]));
    }

    #[test]
    fn select_accepts_index_out_of_range_errors() {
        assert!(select_accepts_index(&json!([{"scheme":"exact"}]), 5).is_err());
    }

    #[test]
    fn select_accepts_index_non_array_errors() {
        assert!(select_accepts_index(&json!({"scheme":"exact"}), 0).is_err());
    }

    // ── decode_pay_payload ───────────────────────────────────────────

    #[test]
    fn decode_pay_payload_from_base64() {
        // One base64 blob = the decoded 402 payload; accepts + resource come out.
        let payload = json!({
            "x402Version": 2,
            "resource": {"url":"https://api.example.com/data","mimeType":"application/json"},
            "accepts": [{"scheme":"exact","network":"eip155:196","amount":"1000","payTo":"0xA","asset":"0xB"}]
        });
        let b64 = B64.encode(serde_json::to_vec(&payload).unwrap());
        let (accepts, resource) = decode_pay_payload(&b64).unwrap();
        assert_eq!(accepts, payload["accepts"]);
        assert_eq!(resource, Some(payload["resource"].clone())); // verbatim from payload
    }

    #[test]
    fn decode_pay_payload_accepts_base64url() {
        // PAYMENT-REQUIRED headers may be base64url — reuse decode_payment_blob.
        let payload = json!({"resource":{"url":"u"},"accepts":[{"scheme":"exact"}]});
        let b64url = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&payload).unwrap());
        let (accepts, _resource) = decode_pay_payload(&b64url).unwrap();
        assert_eq!(accepts[0]["scheme"], "exact");
    }

    #[test]
    fn decode_pay_payload_without_accepts_errors() {
        let b64 = B64.encode(br#"{"x402Version":2,"resource":{"url":"u"}}"#);
        assert!(decode_pay_payload(&b64).is_err());
    }

    #[test]
    fn decode_pay_payload_rejects_garbage() {
        assert!(decode_pay_payload("@@@ not base64, not json @@@").is_err());
    }

    // ── decode_payment_blob ──────────────────────────────────────────

    #[test]
    fn decode_blob_standard_base64_json() {
        // PAYMENT-RESPONSE / Payment-Receipt: standard base64 of a JSON body.
        let body = json!({"status":"settled","amount":"1000","payer":"0xA"});
        let b64 = B64.encode(serde_json::to_vec(&body).unwrap());
        assert_eq!(decode_payment_blob(&b64).unwrap(), body);
    }

    #[test]
    fn decode_blob_base64url_no_pad() {
        // A base64url body (with '-'/'_', no padding) that the skill's
        // `base64 -d` (standard only) would choke on — the CLI must handle it.
        let body = json!({"transaction":"0xab__--cd","amount":"0"});
        let b64url = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&body).unwrap());
        assert_eq!(decode_payment_blob(&b64url).unwrap(), body);
    }

    #[test]
    fn decode_blob_www_authenticate_decodes_request_inline() {
        // WWW-Authenticate: Payment ... — parse the header AND decode the
        // base64url `request` param into JSON inline.
        let request = json!({"recipient":"0xMerchant","amount":"1000000","currency":"0xUSDC"});
        let req_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&request).unwrap());
        let header = format!(
            "Payment id=\"abc\", realm=\"api.shop.com\", method=\"evm\", intent=\"charge\", request=\"{req_b64}\""
        );
        let out = decode_payment_blob(&header).unwrap();
        assert_eq!(out["intent"], "charge");
        assert_eq!(out["request"], request);
    }

    #[test]
    fn decode_blob_plain_json_v1_body() {
        // v1 PAYMENT body is plain JSON (not base64) — returned as-is.
        let out =
            decode_payment_blob(r#"{"x402Version":1,"accepts":[{"scheme":"exact"}]}"#).unwrap();
        assert_eq!(out["x402Version"], 1);
        assert_eq!(out["accepts"][0]["scheme"], "exact");
    }

    #[test]
    fn decode_blob_rejects_unrecognized_input() {
        assert!(decode_payment_blob("@@@ not base64, not json @@@").is_err());
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
    fn parse_www_authenticate_accepts_case_insensitive_payment_scheme() {
        for scheme in ["Payment", "payment", "PAYMENT", "PaYmEnT"] {
            let header = format!(
                "{scheme} id=\"abc123\", realm=\"api.shop.com\", method=\"evm\", intent=\"charge\", request=\"e30\""
            );
            let parsed = parse_www_authenticate(&header).unwrap();
            assert_eq!(parsed["id"].as_str(), Some("abc123"));
            assert_eq!(parsed["method"].as_str(), Some("evm"));
            assert_eq!(parsed["intent"].as_str(), Some("charge"));

            let decoded = decode_payment_blob(&header).unwrap();
            assert_eq!(decoded["id"].as_str(), Some("abc123"));
            assert_eq!(decoded["request"], json!({}));
        }
    }

    #[test]
    fn parse_www_authenticate_rejects_missing_required() {
        let header = "Payment realm=\"x\""; // missing id/method/intent
        assert!(parse_www_authenticate(header).is_err());
    }

    #[test]
    fn parse_www_authenticate_rejects_non_evm_method() {
        // tempo / svm / stripe etc. must be rejected loudly — this CLI is EVM-only.
        let tempo =
            "Payment id=\"a\", realm=\"r\", method=\"tempo\", intent=\"charge\", request=\"e30\"";
        let err = parse_www_authenticate(tempo).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported payment challenge method")
                && err.to_string().contains("tempo"),
            "error should name the unsupported method: {}",
            err
        );

        let svm =
            "Payment id=\"a\", realm=\"r\", method=\"svm\", intent=\"charge\", request=\"e30\"";
        assert!(parse_www_authenticate(svm).is_err());

        // Sanity: evm still accepted.
        let evm =
            "Payment id=\"a\", realm=\"r\", method=\"evm\", intent=\"charge\", request=\"e30\"";
        assert!(parse_www_authenticate(evm).is_ok());
    }

    #[test]
    fn parse_www_authenticate_handles_quoted_value_with_comma() {
        // Embedded comma inside a quoted value must NOT split the pair.
        let header = "Payment id=\"a\", realm=\"r\", method=\"evm\", intent=\"charge\", request=\"e30\", description=\"buy coffee, with sugar\"";
        let parsed = parse_www_authenticate(header).unwrap();
        assert_eq!(
            parsed["description"].as_str(),
            Some("buy coffee, with sugar")
        );
        assert_eq!(parsed["request"].as_str(), Some("e30"));
    }

    #[test]
    fn parse_www_authenticate_tolerates_extra_whitespace_and_single_space_separator() {
        // Some servers separate pairs with a single space (not double). The parser
        // should still extract every field correctly.
        let header =
            "Payment id=\"a\",realm=\"r\", method=\"evm\" ,intent=\"charge\",request=\"e30\"";
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
        let (primary, splits) = compute_primary_split_amounts(&req, 196).unwrap();
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
        let (primary, splits) = compute_primary_split_amounts(&req, 196).unwrap();
        assert_eq!(primary, "940000");
        assert_eq!(splits.len(), 2);
        assert_eq!(splits[0].amount, "50000");
        assert_eq!(splits[1].amount, "10000");
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
        let (primary, _) = compute_primary_split_amounts(&req, 196).unwrap();
        assert_eq!(primary, "50");
    }

    #[test]
    fn primary_split_xko_recipient_normalizes_to_canonical() {
        // XKO-prefixed split recipient on XLayer maps to canonical 0x for signing,
        // and preserves the original XKO form in the display column.
        let req = json!({
            "amount": "1000",
            "methodDetails": {
                "splits": [
                    { "amount": "100", "recipient": "XKO1111111111111111111111111111111111111111" },
                ]
            }
        });
        let (_primary, splits) = compute_primary_split_amounts(&req, 196).unwrap();
        assert_eq!(splits.len(), 1);
        assert_eq!(
            splits[0].canonical,
            "0x1111111111111111111111111111111111111111"
        );
        assert_eq!(
            splits[0].display,
            "XKO1111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn primary_split_xko_recipient_rejected_off_xlayer() {
        let req = json!({
            "amount": "1000",
            "methodDetails": {
                "splits": [
                    { "amount": "100", "recipient": "XKO1111111111111111111111111111111111111111" },
                ]
            }
        });
        // `{:#}` flattens the anyhow context chain so we can match against the
        // inner "only supported on X Layer" reason added by parse_recipient_addr.
        let err = format!("{:#}", compute_primary_split_amounts(&req, 1).unwrap_err());
        assert!(err.contains("only supported on X Layer"), "got: {}", err);
    }

    #[test]
    fn primary_split_rejects_sum_equal_to_amount() {
        // Spec §Constraints: sum MUST be strictly less than amount.
        let req = json!({
            "amount": "100",
            "methodDetails": { "splits": [
                { "amount": "100", "recipient": "0x1111111111111111111111111111111111111111" }
            ] }
        });
        let err = compute_primary_split_amounts(&req, 196)
            .unwrap_err()
            .to_string();
        assert!(err.contains("strictly less than"), "got: {}", err);
    }

    #[test]
    fn primary_split_rejects_sum_greater_than_amount() {
        let req = json!({
            "amount": "100",
            "methodDetails": { "splits": [
                { "amount": "70", "recipient": "0x1111111111111111111111111111111111111111" },
                { "amount": "40", "recipient": "0x2222222222222222222222222222222222222222" },
            ] }
        });
        assert!(compute_primary_split_amounts(&req, 196).is_err());
    }

    #[test]
    fn primary_split_rejects_zero_amount_entry() {
        let req = json!({
            "amount": "100",
            "methodDetails": { "splits": [ { "amount": "0", "recipient": "0xabc" } ] }
        });
        let err = compute_primary_split_amounts(&req, 196)
            .unwrap_err()
            .to_string();
        assert!(err.contains("> 0"), "got: {}", err);
    }

    #[test]
    fn primary_split_rejects_empty_splits_array() {
        let req = json!({
            "amount": "100",
            "methodDetails": { "splits": [] }
        });
        let err = compute_primary_split_amounts(&req, 196)
            .unwrap_err()
            .to_string();
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
        let err = compute_primary_split_amounts(&req, 196)
            .unwrap_err()
            .to_string();
        assert!(err.contains("exceeds spec max of 10"), "got: {}", err);
    }

    #[test]
    fn primary_split_rejects_non_base10_amount() {
        let req = json!({
            "amount": "0x64",
            "methodDetails": { "splits": [ { "amount": "10", "recipient": "0xabc" } ] }
        });
        assert!(compute_primary_split_amounts(&req, 196).is_err());
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
        let err = compute_valid_before(&ch, 2_000_000_000)
            .unwrap_err()
            .to_string();
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
        assert_eq!(
            names,
            vec!["name", "version", "chainId", "verifyingContract"]
        );
    }

    // ── voucher_advances_cumulative (session viability gate) ──────────
    // Regression guard: the persisted session cumulative
    // is advanced only for a viable voucher, so a rejected over-deposit voucher
    // cannot make the next voucher's `unit = new − prior` baseline optimistic.

    #[test]
    fn voucher_advances_when_within_known_deposit() {
        // unit > 0 and new cumulative within deposit ⇒ advance.
        assert!(voucher_advances_cumulative(50, 150, Some(200)));
    }

    #[test]
    fn voucher_advances_at_exact_deposit_boundary() {
        // new cumulative == deposit is still within budget ⇒ advance.
        assert!(voucher_advances_cumulative(200, 200, Some(200)));
    }

    #[test]
    fn voucher_does_not_advance_over_deposit() {
        // new_cum > deposit (needs top-up / seller rejects) ⇒ do NOT advance.
        assert!(!voucher_advances_cumulative(50, 250, Some(200)));
    }

    #[test]
    fn voucher_advances_when_deposit_unknown() {
        // deposit = None ⇒ advance best-effort (only bounded by unit > 0).
        assert!(voucher_advances_cumulative(50, 250, None));
    }

    #[test]
    fn voucher_does_not_advance_when_unit_zero() {
        // unit == 0 (no forward progress) ⇒ do NOT advance, even within deposit.
        assert!(!voucher_advances_cumulative(0, 100, Some(200)));
        // ...and also when the deposit is unknown.
        assert!(!voucher_advances_cumulative(0, 100, None));
    }

    // APP enforces the channel budget before TEE signing. MPP keeps its existing
    // orchestration and decision-layer behavior unchanged.

    #[test]
    fn app_voucher_accepts_strict_increase_within_deposit() {
        assert!(validate_app_voucher_cumulative("100", "300", "250").is_ok());
        assert!(validate_app_voucher_cumulative("100", "300", "300").is_ok());
    }

    #[test]
    fn app_voucher_rejects_non_increasing_cumulative() {
        assert!(validate_app_voucher_cumulative("100", "300", "100").is_err());
        assert!(validate_app_voucher_cumulative("100", "300", "99").is_err());
    }

    #[test]
    fn app_voucher_rejects_cumulative_over_deposit() {
        let err = validate_app_voucher_cumulative("100", "300", "301")
            .unwrap_err()
            .to_string();
        assert!(err.contains("top up the channel before signing"));
    }

    #[test]
    fn app_close_accepts_only_latest_cumulative() {
        assert!(validate_app_close_cumulative("250", "300", "250").is_ok());
        assert!(validate_app_close_cumulative("250", "300", "251").is_err());
        assert!(validate_app_close_cumulative("250", "300", "249").is_err());
    }

    #[test]
    fn app_guards_reject_inconsistent_persisted_state() {
        assert!(validate_app_voucher_cumulative("301", "300", "302").is_err());
        assert!(validate_app_close_cumulative("301", "300", "301").is_err());
    }

    #[test]
    fn app_subscription_signing_requires_confirmed_cap() {
        for action in ["subscribe", "change"] {
            let parsed = TestCli::try_parse_from([
                "test",
                "app",
                "subscription",
                action,
                "--challenge",
                "challenge-fixture",
            ]);
            assert!(parsed.is_err(), "{action} must require --expected-cap");
        }
    }

    #[test]
    fn app_subscription_signing_parses_cap_and_no_cache() {
        let parsed = TestCli::parse_from([
            "test",
            "app",
            "subscription",
            "subscribe",
            "--challenge",
            "challenge-fixture",
            "--expected-cap",
            "60000000",
            "--no-cache",
        ]);
        assert!(matches!(
            parsed.command,
            PaymentCommand::App {
                command: AppCommand::Subscription {
                    command: AppSubscriptionCommand::Subscribe {
                        expected_cap,
                        no_cache: true,
                        ..
                    }
                }
            } if expected_cap == "60000000"
        ));
    }

    #[test]
    fn app_subscription_cancel_requires_offer_id() {
        let parsed = TestCli::try_parse_from([
            "test",
            "app",
            "subscription",
            "cancel",
            "--sub-id",
            "0xsub",
            "--subscription-contract",
            "0x1111111111111111111111111111111111111111",
            "--chain-id",
            "196",
        ]);
        assert!(parsed.is_err());
    }

    #[test]
    fn app_subscription_resolve_requires_receipt_or_outcome() {
        let missing = TestCli::try_parse_from([
            "test",
            "app",
            "subscription",
            "resolve",
            "--operation-id",
            "op_1",
        ]);
        assert!(missing.is_err());

        let parsed = TestCli::parse_from([
            "test",
            "app",
            "subscription",
            "resolve",
            "--operation-id",
            "op_1",
            "--receipt",
            r#"{"status":"success"}"#,
        ]);
        assert!(matches!(
            parsed.command,
            PaymentCommand::App {
                command: AppCommand::Subscription {
                    command: AppSubscriptionCommand::Resolve { .. }
                }
            }
        ));
    }

    #[test]
    fn app_session_resolve_commits_topup_once() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("app_session_resolve_topup");
        std::env::set_var("ONCHAINOS_HOME", &dir);
        let mut state = session_state::ChannelState {
            channel_id: "0xresolve".into(),
            owner_wallet: "wallet-1".into(),
            deposit: "1000".into(),
            cumulative: "20".into(),
            created_at: 1,
            updated_at: 1,
            pending_operation: None,
            resolved_operations: Vec::new(),
        };
        let operation_id = state
            .begin_pending(session_state::PendingAction::Topup, "1000", "offer-1")
            .unwrap();
        state.write().unwrap();

        assert!(cmd_app_session_resolve(
            &operation_id,
            Some(r#"{"status":"success"}"#),
            None,
        )
        .is_err());
        assert_eq!(session_state::read("0xresolve").unwrap().deposit, "1000");
        let wrong_amount_receipt = r#"{"status":"success","protocol":"app/1","intent":"session","offerId":"offer-1","payer":"wallet-1","settledAmount":"1000","reference":"0xtx-wrong"}"#;
        assert!(cmd_app_session_resolve(
            &operation_id,
            Some(wrong_amount_receipt),
            None,
        )
        .is_err());
        assert_eq!(session_state::read("0xresolve").unwrap().deposit, "1000");

        let receipt = r#"{"status":"success","protocol":"app/1","intent":"session","offerId":"offer-1","payer":"wallet-1","settledAmount":"2000","reference":"0xtx1"}"#;
        cmd_app_session_resolve(&operation_id, Some(receipt), None).unwrap();
        assert_eq!(session_state::read("0xresolve").unwrap().deposit, "2000");
        cmd_app_session_resolve(&operation_id, Some(receipt), None).unwrap();
        assert_eq!(session_state::read("0xresolve").unwrap().deposit, "2000");

        let mut state = session_state::read("0xresolve").unwrap();
        let second_operation = state
            .begin_pending(session_state::PendingAction::Topup, "1000", "offer-1")
            .unwrap();
        state.write().unwrap();
        assert!(cmd_app_session_resolve(&second_operation, Some(receipt), None).is_err());
        assert_eq!(session_state::read("0xresolve").unwrap().deposit, "2000");
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn app_session_resolve_clears_close_after_network_error() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("app_session_resolve_close_error");
        std::env::set_var("ONCHAINOS_HOME", &dir);
        let mut state = session_state::ChannelState {
            channel_id: "0xclose".into(),
            owner_wallet: "wallet-1".into(),
            deposit: "100".into(),
            cumulative: "20".into(),
            created_at: 1,
            updated_at: 1,
            pending_operation: None,
            resolved_operations: Vec::new(),
        };
        let operation_id = state
            .begin_pending(session_state::PendingAction::Close, "20", "offer-1")
            .unwrap();
        state.write().unwrap();

        cmd_app_session_resolve(&operation_id, None, Some("network-error")).unwrap();
        let state = session_state::read("0xclose").unwrap();
        assert_eq!(state.deposit, "100");
        assert!(state.pending_operation.is_none());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn app_session_resolve_archives_successful_close_once() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("app_session_resolve_close_success");
        std::env::set_var("ONCHAINOS_HOME", &dir);
        let mut state = session_state::ChannelState {
            channel_id: "0xclosed".into(),
            owner_wallet: "wallet-1".into(),
            deposit: "100".into(),
            cumulative: "20".into(),
            created_at: 1,
            updated_at: 1,
            pending_operation: None,
            resolved_operations: Vec::new(),
        };
        let operation_id = state
            .begin_pending(session_state::PendingAction::Close, "20", "offer-1")
            .unwrap();
        state.write().unwrap();
        let receipt = r#"{"status":"success","protocol":"app/1","intent":"session","offerId":"offer-1","payer":"wallet-1","settledAmount":"20","reference":"0xclose1"}"#;

        cmd_app_session_resolve(&operation_id, Some(receipt), None).unwrap();
        assert!(session_state::read("0xclosed").is_none());
        cmd_app_session_resolve(&operation_id, Some(receipt), None).unwrap();
        assert!(session_state::read("0xclosed").is_none());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn app_session_non_success_receipts_need_no_settled_amount() {
        let state = session_state::ChannelState {
            channel_id: "0xstatus".into(),
            owner_wallet: "wallet-1".into(),
            deposit: "100".into(),
            cumulative: "20".into(),
            created_at: 1,
            updated_at: 1,
            pending_operation: None,
            resolved_operations: Vec::new(),
        };
        let pending = session_state::PendingOperation {
            operation_id: "op_status".into(),
            action: session_state::PendingAction::Topup,
            amount: "25".into(),
            offer_id: "offer-1".into(),
            created_at: 1,
        };
        for status in ["failed", "pending", "timeout"] {
            let receipt = format!(
                r#"{{"status":"{status}","protocol":"app/1","intent":"session","offerId":"offer-1","payer":"wallet-1"}}"#
            );
            assert_eq!(
                app_session_receipt_status(&receipt, &state, &pending).unwrap(),
                (status.to_string(), None)
            );
        }
        let unknown = r#"{"status":"unknown","protocol":"app/1","intent":"session","offerId":"offer-1","payer":"wallet-1"}"#;
        assert!(app_session_receipt_status(unknown, &state, &pending).is_err());
    }

    fn seed_app_subscription_cache(sub_id: &str) {
        let mut cache = crate::payment::subscription::cache::SubscriptionCache::default();
        cache.put(
            crate::payment::subscription::types::SubscriptionCacheEntry {
                sub_id: sub_id.to_string(),
                resource_host: "api.example.com".to_string(),
                merchant: "0xmerchant".to_string(),
                plan_id: "basic".to_string(),
                plan_tier: 1,
                max_periods: 12,
                state: "active".to_string(),
                changed_to_sub_id: None,
                protocol: "app/1".to_string(),
            },
        );
        cache.save().unwrap();
    }

    #[test]
    fn app_subscription_resolve_commits_cancel_once() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("app_subscription_resolve_cancel");
        std::env::set_var("ONCHAINOS_HOME", &dir);
        seed_app_subscription_cache("0xsub");
        let pending = subscription_operation_state::begin_pending(
            subscription_operation_state::PendingAction::Cancel,
            "0xsub",
            None,
            "offer-1",
            "0xpayer",
        )
        .unwrap();

        assert!(cmd_app_subscription_resolve(
            &pending.operation_id,
            Some(r#"{"status":"success"}"#),
            None,
        )
        .is_err());
        assert_eq!(
            crate::payment::subscription::cache::SubscriptionCache::load()
                .get("app/1", "api.example.com")
                .unwrap()
                .state,
            "active"
        );

        let receipt = r#"{"status":"success","protocol":"app/1","intent":"subscription","offerId":"offer-1","payer":"0xpayer","reference":"0xtx1"}"#;
        cmd_app_subscription_resolve(&pending.operation_id, Some(receipt), None).unwrap();
        assert_eq!(
            crate::payment::subscription::cache::SubscriptionCache::load()
                .get("app/1", "api.example.com")
                .unwrap()
                .state,
            "canceled"
        );
        cmd_app_subscription_resolve(&pending.operation_id, Some(receipt), None).unwrap();
        let resolved =
            subscription_operation_state::read_resolution(&pending.operation_id).unwrap();
        assert!(resolved.committed);
        assert_eq!(resolved.resolution.as_deref(), Some("success"));

        let second = subscription_operation_state::begin_pending(
            subscription_operation_state::PendingAction::Cancel,
            "0xother",
            None,
            "offer-1",
            "0xpayer",
        )
        .unwrap();
        assert!(cmd_app_subscription_resolve(&second.operation_id, Some(receipt), None).is_err());
        assert!(subscription_operation_state::read_pending(&second.operation_id).is_some());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn app_subscription_resolve_cancel_pending_keeps_current_subscription_active() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("app_subscription_resolve_cancel_pending");
        std::env::set_var("ONCHAINOS_HOME", &dir);
        seed_app_subscription_cache("0xsub");
        let pending = subscription_operation_state::begin_pending(
            subscription_operation_state::PendingAction::CancelPending,
            "0xsub",
            Some("0xnew"),
            "offer-2",
            "0xpayer",
        )
        .unwrap();
        let receipt = r#"{"status":"success","protocol":"app/1","intent":"subscription","offerId":"offer-2","payer":"0xpayer","reference":"0xtx2"}"#;

        cmd_app_subscription_resolve(&pending.operation_id, Some(receipt), None).unwrap();

        assert_eq!(
            crate::payment::subscription::cache::SubscriptionCache::load()
                .get("app/1", "api.example.com")
                .unwrap()
                .state,
            "active"
        );
        let resolved =
            subscription_operation_state::read_resolution(&pending.operation_id).unwrap();
        assert!(resolved.committed);
        assert_eq!(resolved.new_sub_id.as_deref(), Some("0xnew"));
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn app_subscription_resolve_recovers_cache_after_durable_commit() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("app_subscription_resolve_recovery");
        std::env::set_var("ONCHAINOS_HOME", &dir);
        seed_app_subscription_cache("0xsub");
        let pending = subscription_operation_state::begin_pending(
            subscription_operation_state::PendingAction::Cancel,
            "0xsub",
            None,
            "offer-recovery",
            "0xpayer",
        )
        .unwrap();

        // Simulate a process exit after the receipt claim was persisted but
        // before the cache side effect ran.
        subscription_operation_state::resolve(
            pending.clone(),
            "success",
            true,
            Some("0xtx-recovery"),
        )
        .unwrap();
        assert_eq!(
            crate::payment::subscription::cache::SubscriptionCache::load()
                .get("app/1", "api.example.com")
                .unwrap()
                .state,
            "active"
        );

        cmd_app_subscription_resolve(&pending.operation_id, None, None).unwrap();
        assert_eq!(
            crate::payment::subscription::cache::SubscriptionCache::load()
                .get("app/1", "api.example.com")
                .unwrap()
                .state,
            "canceled"
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn app_subscription_resolve_non_success_does_not_change_cache() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("app_subscription_resolve_failed");
        std::env::set_var("ONCHAINOS_HOME", &dir);
        seed_app_subscription_cache("0xsub");
        let pending = subscription_operation_state::begin_pending(
            subscription_operation_state::PendingAction::Cancel,
            "0xsub",
            None,
            "offer-3",
            "0xpayer",
        )
        .unwrap();
        let receipt = r#"{"status":"failed","protocol":"app/1","intent":"subscription","offerId":"offer-3","payer":"0xpayer"}"#;

        cmd_app_subscription_resolve(&pending.operation_id, Some(receipt), None).unwrap();

        assert_eq!(
            crate::payment::subscription::cache::SubscriptionCache::load()
                .get("app/1", "api.example.com")
                .unwrap()
                .state,
            "active"
        );
        let resolved =
            subscription_operation_state::read_resolution(&pending.operation_id).unwrap();
        assert!(!resolved.committed);
        assert_eq!(resolved.resolution.as_deref(), Some("failed"));
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn app_subscription_receipt_must_match_pending_context() {
        let pending = subscription_operation_state::OperationState {
            operation_id: "op_test".into(),
            action: subscription_operation_state::PendingAction::Cancel,
            sub_id: "0xsub".into(),
            new_sub_id: None,
            offer_id: "offer-1".into(),
            payer: "0xPayer".into(),
            created_at: 1,
            resolution: None,
            committed: false,
            receipt_reference: None,
            resolved_at: None,
        };
        let valid = r#"{"status":"success","protocol":"app/1","intent":"subscription","offerId":"offer-1","payer":"0xpayer","reference":"0xTX1"}"#;
        assert_eq!(
            app_subscription_receipt_status(valid, &pending).unwrap(),
            ("success".to_string(), Some("0xtx1".to_string()))
        );

        for invalid in [
            r#"{"status":"success","protocol":"app/2","intent":"subscription","offerId":"offer-1","payer":"0xpayer","reference":"0xtx1"}"#,
            r#"{"status":"success","protocol":"app/1","intent":"session","offerId":"offer-1","payer":"0xpayer","reference":"0xtx1"}"#,
            r#"{"status":"success","protocol":"app/1","intent":"subscription","offerId":"offer-2","payer":"0xpayer","reference":"0xtx1"}"#,
            r#"{"status":"success","protocol":"app/1","intent":"subscription","offerId":"offer-1","payer":"0xother","reference":"0xtx1"}"#,
            r#"{"status":"success","protocol":"app/1","intent":"subscription","offerId":"offer-1","payer":"0xpayer"}"#,
        ] {
            assert!(app_subscription_receipt_status(invalid, &pending).is_err());
        }
    }

    // ── APP/1 (protocol="app/1") ──────────────────────────────────────

    /// The APP/1 echo must carry `protocol` and `opaque` verbatim (both feed the
    /// `challenge.id` HMAC), unlike the MPP echo which omits them.
    #[test]
    fn app_echo_carries_protocol_and_opaque() {
        let challenge = json!({
            "id": "abc123",
            "realm": "api.shop.com",
            "protocol": "app/1",
            "method": "evm",
            "intent": "charge",
            "expires": "2026-07-16T00:00:00Z",
            "request": "eyJyZXF1ZXN0IjoxfQ",
            "opaque": "eyJvZmZlcklkIjoieCJ9",
        });
        let echo = build_app_challenge_echo(&challenge);
        for k in [
            "id", "realm", "protocol", "method", "intent", "expires", "request", "opaque",
        ] {
            assert!(echo.get(k).and_then(|v| v.as_str()).is_some(), "missing {k}");
        }
        assert_eq!(echo["protocol"], "app/1");
        // `request` / `opaque` echoed as the original base64url strings (not decoded).
        assert_eq!(echo["opaque"], "eyJvZmZlcklkIjoieCJ9");
        assert_eq!(echo["request"], "eyJyZXF1ZXN0IjoxfQ");
    }

    /// The `payment app` family rejects MPP challenges (protocol != "app/1")
    /// before any TEE call, so MPP always routes through `payment charge`.
    #[tokio::test]
    async fn app_charge_rejects_mpp_protocol() {
        // A well-formed MPP challenge (method=evm, no protocol field).
        let header = r#"Payment id="x", realm="api.shop.com", method="evm", intent="charge", request="e30", opaque="e30""#;
        let err = cmd_app_charge(header, None, None, None).await.unwrap_err();
        assert!(
            err.to_string().contains("protocol"),
            "expected protocol reject, got: {err}"
        );
    }

    /// The subcommand intent must match `challenge.intent` — a `charge` command
    /// against a `session` challenge is rejected before any TEE call.
    #[tokio::test]
    async fn app_charge_rejects_intent_mismatch() {
        let header = r#"Payment id="x", realm="api.shop.com", protocol="app/1", method="evm", intent="session", request="e30", opaque="e30""#;
        let err = cmd_app_charge(header, None, None, None).await.unwrap_err();
        assert!(
            err.to_string().contains("intent"),
            "expected intent mismatch reject, got: {err}"
        );
    }
}
