pub mod account;
pub mod auth;
pub mod balance;
pub mod broadcast;
pub mod chain;
pub mod chain_profile;
pub mod common;
pub mod gas_station;
pub mod geoblock;
pub mod history;
mod shared;
mod inscription;
pub mod plugin;
pub mod sign;
pub mod strategy;
pub mod transfer;
mod utxo;

use anyhow::{bail, Result};
use clap::{Subcommand, ValueEnum};

/// Stage of the social-login flow. The skill orchestrates `init` → `open` →
/// `poll` so the login URL is returned immediately (before the browser opens),
/// avoiding a stall if the browser can't launch. `init` is the default, so a
/// bare `wallet login` just mints and returns the login URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Default)]
pub enum LoginPhase {
    /// Generate the login URL and return it. No browser, no polling.
    #[default]
    Init,
    /// Open the given `--url` in the system browser (best-effort). Internal
    /// step in the skill's orchestration — not a user-facing command.
    Open,
    /// Poll for the login result (using state from `init`) and persist it.
    Poll,
}

#[derive(Subcommand)]
pub enum WalletCommand {
    /// Start social login (Google / Apple / Email via browser). A bare `login`
    /// mints and returns the login URL (`--phase init`); the skill orchestrates
    /// `init` → `open` → `poll`.
    Login {
        /// Login phase. Defaults to `init` (mint + return the login URL).
        #[arg(long, value_enum, default_value_t = LoginPhase::Init)]
        phase: LoginPhase,
        /// Login URL to open — required for `--phase open`.
        #[arg(long)]
        url: Option<String>,
        /// Auth session id to poll — used by `--phase poll`. Defaults to the
        /// most recent `init` session when omitted.
        #[arg(long = "session-id")]
        session_id: Option<String>,
    },
    /// Add a wallet account. Use after login when the user needs a separate account.
    Add,
    /// Switch the active wallet account. Subsequent balance and write commands use this account.
    Switch {
        /// Account ID to switch to
        account_id: String,
    },
    /// Show login and active-account status. Use before account-sensitive operations when status is needed.
    Status {
        /// Include the best-effort post-login subscription/device snapshot.
        /// Use only for an explicit user-facing login/status request; ordinary
        /// internal authentication preconditions should omit it.
        #[arg(long = "include-subscriptions")]
        include_subscriptions: bool,
    },
    /// Show wallet addresses. Use to obtain an owned address for a selected chain.
    Addresses {
        /// Chain name or ID. Use for one chain's address; omit for all available addresses.
        #[arg(long)]
        chain: Option<String>,
    },
    /// Render a Unicode-block QR code for an address (or any string)
    Qrcode {
        /// Address (or arbitrary string) to encode verbatim into the QR
        #[arg(long)]
        address: String,
    },
    /// Logout and clear all stored credentials
    Logout,
    /// List all supported chains (cached locally, refreshes every 10 minutes)
    Chains,
    /// Check Polymarket geoblock status. Prints `{"blocked":true|false}` on success;
    /// exits non-zero on any failure (skill should treat that as fail-closed).
    Geoblock,
    /// Query wallet balances. Use --chain for one chain, --token-address for one token, or --all for every account.
    Balance {
        /// Query all accounts' assets. Use only when the user asks for every account.
        #[arg(long)]
        all: bool,
        /// Chain name or ID. Required with --token-address; use bitcoin for BTC or BRC-20.
        #[arg(long)]
        chain: Option<String>,
        /// Query one token. Use a contract address for tokens, a Coin Type for SUI, or btc-brc20-<ticker> for BRC-20. Requires --chain.
        #[arg(long)]
        token_address: Option<String>,
        /// Force refresh: bypass all caches and re-fetch wallet accounts + balances from the API.
        /// Use when the user explicitly asks to refresh/sync/update their wallet data.
        #[arg(long, default_value = "false")]
        force: bool,
    },
    // Confirming-gate override (onchainos_check): the destructive wallet variants below
    // (Send / call-contract / broadcast / …) gate user confirmation via output::CliConfirming
    // inside their handler modules (transfer/mod.rs, broadcast.rs, common.rs) plus each
    // command's --force flag — not in this command-dispatch enum definition.
    /// Send a native or token transfer. Use for ordinary transfers, BTC, BRC-20 selected inscription transfers, and SUI Coin<T> transfers; use contract-call for contract interaction.
    Send {
        /// Amount in minimal units — whole number, no decimals (e.g. "100000000000000000" for 0.1 ETH). Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amt: Option<String>,
        /// Human-readable amount (e.g. "1.5" for 1.5 USDC). CLI fetches token decimals and converts automatically. Mutually exclusive with --amt.
        #[arg(long, conflicts_with = "amt")]
        readable_amount: Option<String>,
        /// Recipient address on --chain. Required for every transfer.
        #[arg(long)]
        recipient: String,
        /// Destination chain name or ID. Use bitcoin for BTC/BRC-20 and sui for SUI transfers.
        #[arg(long)]
        chain: String,
        /// Sender address (optional — defaults to selectedAccountId)
        #[arg(long)]
        from: Option<String>,
        /// Token identifier for non-native transfers: ERC-20/SPL address, SUI Coin Type, or btc-brc20-<ticker>.
        #[arg(long)]
        contract_token: Option<String>,
        /// Transferable BRC-20 inscription UTXO (`txHash:voutIndex`); repeat to combine inputs
        #[arg(long = "brc20-outpoint", requires = "contract_token")]
        brc20_outpoint: Vec<String>,
        /// Bitcoin fee rate in sat/vB. Applies only to this BTC or BRC-20 transaction.
        #[arg(long)]
        fee_rate: Option<String>,
        /// Run only after this exact command returned confirming and the user explicitly confirmed.
        #[arg(long, default_value_t = false)]
        force: bool,
        // ── Gas Station params (second-phase call) ──
        /// Gas token contract address for Gas Station payment (from tokenList)
        #[arg(long)]
        gas_token_address: Option<String>,
        /// Relayer ID for Gas Station (from tokenList)
        #[arg(long)]
        relayer_id: Option<String>,
        /// Enable Gas Station (first-time activation, sets gasTokenAddress as default)
        #[arg(long, default_value_t = false)]
        enable_gas_station: bool,
    },
    /// Query transaction history or one transaction/order detail. Use an ID flag for detail; omit all ID flags for a paged list.
    History {
        /// Account ID. Omit to use the active account.
        #[arg(long)]
        account_id: Option<String>,
        /// Chain name or ID (e.g. "ethereum" or "1", "solana" or "501"). Resolved to chainIndex internally.
        #[arg(long)]
        chain: Option<String>,
        /// Address (optional; passed to detail query if provided)
        #[arg(long)]
        address: Option<String>,
        /// List-mode start time in Unix milliseconds.
        #[arg(long)]
        begin: Option<String>,
        /// List-mode end time in Unix milliseconds.
        #[arg(long)]
        end: Option<String>,
        /// List-mode page cursor.
        #[arg(long)]
        page_num: Option<String>,
        /// List-mode page size.
        #[arg(long)]
        limit: Option<String>,
        /// Order ID — when present, queries /order/detail by orderId
        #[arg(long)]
        order_id: Option<String>,
        /// Transaction hash — when present, queries /order/detail by txHash
        #[arg(long)]
        tx_hash: Option<String>,
        /// User operation hash — when present, queries /order/detail by uopHash
        #[arg(long)]
        uop_hash: Option<String>,
    },
    /// Create or query a BRC-20 transfer inscription. Use create when no exact transferable inscription combination exists.
    Inscription {
        #[command(subcommand)]
        command: InscriptionCommand,
    },
    /// Query and manage Bitcoin UTXOs with asset protection.
    Utxo {
        #[command(subcommand)]
        command: UtxoCommand,
    },
    /// Sign a message (personalSign for EVM & Solana, EIP-712 for EVM only)
    SignMessage {
        /// Signing type: "personal" (default) or "eip712"
        #[arg(long, default_value = "personal")]
        r#type: String,
        /// Message to sign (arbitrary string for personal, JSON string for eip712)
        #[arg(long)]
        message: String,
        /// Chain name or ID (e.g. "ethereum" or "1", "solana" or "501", "bsc" or "56")
        #[arg(long)]
        chain: String,
        /// Sender address (the address whose private key is used to sign)
        #[arg(long)]
        from: String,
        /// Force execution: skip confirmation prompts from the backend
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Report plugin info
    ReportPluginInfo {
        /// Plugin parameter payload to report
        #[arg(long)]
        plugin_parameter: String,
    },
    /// Call a smart contract. Use EVM calldata, a Solana unsigned transaction, or a SUI PTB; use send for token transfers.
    /// Supports Gas Station: if the account has Gas Station enabled, pass
    /// `--gas-token-address` + `--relayer-id` (and `--enable-gas-station` for
    /// first-time activation / re-enable) to pay gas with stablecoins.
    ContractCall {
        /// Contract or program address. Required for EVM and Solana; optional service metadata for SUI.
        #[arg(long)]
        to: Option<String>,
        /// Chain name or ID for the contract call.
        #[arg(long)]
        chain: String,
        /// Native token amount in minimal units — whole number, no decimals (default "0")
        #[arg(long, default_value = "0")]
        amt: String,
        /// EVM call data (hex-encoded, e.g. "0xa9059cbb...")
        #[arg(long)]
        input_data: Option<String>,
        /// Solana unsigned transaction data (base58)
        #[arg(long, conflicts_with = "sui_tx_bytes")]
        unsigned_tx: Option<String>,
        /// SUI pre-built TransactionData / PTB (base64 BCS)
        #[arg(long, conflicts_with_all = ["input_data", "unsigned_tx"])]
        sui_tx_bytes: Option<String>,
        /// Gas limit override (EVM only)
        #[arg(long)]
        gas_limit: Option<String>,
        /// Sender address (optional — defaults to selectedAccountId)
        #[arg(long)]
        from: Option<String>,
        /// AA DEX token contract address (optional)
        #[arg(long)]
        aa_dex_token_addr: Option<String>,
        /// AA DEX token amount (optional)
        #[arg(long)]
        aa_dex_token_amount: Option<String>,
        /// Enable MEV protection (supported on Ethereum, BSC, Base, Solana)
        #[arg(long, default_value_t = false)]
        mev_protection: bool,
        /// Jito unsigned transaction data for Solana MEV protection (required when --mev-protection is used on Solana)
        #[arg(long)]
        jito_unsigned_tx: Option<String>,
        /// Run only after this exact command returned confirming and the user explicitly confirmed.
        #[arg(long, default_value_t = false)]
        force: bool,
        // ── Gas Station params (Phase 2: execution with chosen token) ──
        /// Gas token contract address for Gas Station payment (from tokenList)
        #[arg(long)]
        gas_token_address: Option<String>,
        /// Relayer ID for Gas Station (from tokenList)
        #[arg(long)]
        relayer_id: Option<String>,
        /// Enable Gas Station (first-time activation or re-enable, sets gasTokenAddress as default)
        #[arg(long, default_value_t = false)]
        enable_gas_station: bool,
        /// Transaction category for broadcast (agentBizType), e.g. "dex", "defi", "dapp"
        #[arg(long)]
        biz_type: Option<String>,
        /// Strategy / skill name used for this call (agentSkillName)
        #[arg(long)]
        strategy: Option<String>,
    },
    /// Gas Station management commands
    GasStation {
        #[command(subcommand)]
        command: GasStationCommand,
    },
}

#[derive(Subcommand)]
pub enum InscriptionCommand {
    /// Create an asynchronous BRC-20 transfer inscription. Use when the requested amount needs a new transferable inscription.
    Create {
        /// Use bitcoin.
        #[arg(long)]
        chain: String,
        /// BRC-20 token identifier: btc-brc20-<ticker>.
        #[arg(long)]
        token_address: String,
        /// Exact human-readable BRC-20 amount to inscribe.
        #[arg(long)]
        readable_amount: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        operation_token: Option<String>,
        /// Bitcoin fee rate in sat/vB. Applies only to this transfer inscription.
        #[arg(long)]
        fee_rate: Option<String>,
        /// Run only after this exact creation command returned confirming and the user explicitly confirmed.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Query BRC-20 transfer-inscription status by transaction hash or order ID.
    Status {
        /// Use bitcoin.
        #[arg(long)]
        chain: String,
        #[arg(
            long,
            conflicts_with = "order_id",
            required_unless_present = "order_id"
        )]
        tx_hash: Option<String>,
        #[arg(long, conflicts_with = "tx_hash", required_unless_present = "tx_hash")]
        order_id: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum UtxoCommand {
    /// Query user-ignored Bitcoin UTXOs after asset protection is removed.
    UserIgnored {
        /// Use bitcoin.
        #[arg(long)]
        chain: String,
    },
    /// Query unavailable or locked Bitcoin UTXOs grouped by the current service reason.
    Unavailable {
        /// Use bitcoin.
        #[arg(long)]
        chain: String,
    },
    /// Query currently available Bitcoin UTXOs and total spendable BTC in sats.
    Available {
        #[arg(long)]
        chain: String,
    },
    /// Query transferable inscription UTXOs for one BRC-20 token. Add --readable-amount to receive exact transfer combinations.
    Brc20Transferable {
        #[arg(long)]
        chain: String,
        /// BRC-20 token identifier: btc-brc20-<ticker>.
        #[arg(long)]
        token_address: String,
        /// Human-readable target amount used to find up to three exact UTXO combinations
        #[arg(long)]
        readable_amount: Option<String>,
    },
    /// Remove asset protection from one or all currently protected UTXOs. Use only after the user selects current returned outpoints.
    Unlock {
        /// Use bitcoin.
        #[arg(long)]
        chain: String,
        /// Repeat for selected outpoints in txHash:voutIndex form.
        #[arg(long, conflicts_with = "all", required_unless_present = "all")]
        outpoint: Vec<String>,
        /// Remove protection from every currently protected UTXO.
        #[arg(long, conflicts_with = "outpoint")]
        all: bool,
        #[arg(long)]
        operation_token: Option<String>,
        /// Run only after this exact command returned confirming and the user explicitly confirmed.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Restore asset protection for one or all user-ignored UTXOs. Use only after the user selects current returned outpoints.
    Lock {
        #[arg(long)]
        chain: String,
        #[arg(long, conflicts_with = "all", required_unless_present = "all")]
        outpoint: Vec<String>,
        #[arg(long, conflicts_with = "outpoint")]
        all: bool,
        #[arg(long)]
        operation_token: Option<String>,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Close mempool-removed transactions and reclaim their still-unspent inputs. Use after history reports MEMPOOL_REMOVED.
    Reclaim {
        /// Use bitcoin.
        #[arg(long)]
        chain: String,
        /// Original transaction hash; repeat to reclaim inputs from multiple transactions.
        #[arg(long, required = true)]
        tx_hash: Vec<String>,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum GasStationCommand {
    /// Update the default Gas Token for a chain
    UpdateDefaultToken {
        /// Chain name or ID (e.g. "ethereum" or "1")
        #[arg(long)]
        chain: String,
        /// Gas token contract address to set as default
        #[arg(long)]
        gas_token_address: String,
    },
    /// Enable Gas Station for a chain (DB flag only). Requires 7702 delegation to exist
    /// on-chain (set earlier via the first-time GS flow). If this chain was never enabled,
    /// backend returns a msg in the response body.
    Enable {
        /// Chain name or ID (e.g. "ethereum" or "1")
        #[arg(long)]
        chain: String,
    },
    /// Disable Gas Station for a chain (DB flag only, no on-chain action).
    /// The 7702 delegation remains on-chain, so re-enabling later does NOT require a new upgrade.
    Disable {
        /// Chain name or ID (e.g. "ethereum" or "1")
        #[arg(long)]
        chain: String,
    },
    /// Read-only Gas Station readiness check on a chain.
    /// Used by third-party plugin pre-flight: agent runs this before invoking
    /// a plugin's on-chain command, branches on the returned `recommendation`.
    /// Never broadcasts; safe to call repeatedly.
    Status {
        /// Chain name or ID (e.g. "ethereum" or "1")
        #[arg(long)]
        chain: String,
        /// Sender address (optional — defaults to selectedAccountId)
        #[arg(long)]
        from: Option<String>,
    },
    /// Standalone Gas Station first-time activation.
    /// Decoupled from `wallet send` so the agent can activate GS before
    /// invoking a third-party plugin (which calls `wallet contract-call --force`).
    /// Idempotent: re-calling with the same default token returns alreadyActivated=true.
    Setup {
        /// Chain name or ID (e.g. "ethereum" or "1")
        #[arg(long)]
        chain: String,
        /// Gas token contract address (picked by the user from Scene A `tokenList`)
        #[arg(long)]
        gas_token_address: String,
        /// Relayer ID (paired with `--gas-token-address` from Scene A `tokenList`)
        #[arg(long)]
        relayer_id: String,
        /// Sender address (optional — defaults to selectedAccountId)
        #[arg(long)]
        from: Option<String>,
    },
}

/// Resolve the effective raw amount for `wallet send`.
/// - `--amt` → validate (no decimals, non-zero) and return as-is
/// - `--readable-amount` + native token → use hardcoded chain decimals
/// - `--readable-amount` + ERC-20/SPL → fetch token decimals via token info API
async fn resolve_send_amount(
    amt: Option<&str>,
    readable_amount: Option<&str>,
    contract_token: Option<&str>,
    chain: &str,
) -> Result<String> {
    if let Some(raw) = amt {
        let raw = raw.trim();
        if raw.is_empty() {
            bail!("--amt must not be empty");
        }
        if raw.contains('.') {
            bail!("--amt must be a whole number in minimal units (no decimals)");
        }
        if !raw.chars().all(|c| c.is_ascii_digit()) {
            bail!(
                "--amt must be a whole number in minimal units, got \"{}\"",
                raw
            );
        }
        if raw.chars().all(|c| c == '0') {
            bail!("--amt must be greater than zero");
        }
        if raw.starts_with('0') {
            bail!("--amt must not have leading zeros, got \"{}\"", raw);
        }
        return Ok(raw.to_string());
    }

    if let Some(readable) = readable_amount {
        let readable = readable.trim();
        if readable.is_empty() {
            bail!("--readable-amount must not be empty");
        }

        let decimal: u32 = match contract_token {
            None => {
                // Native token — decimals are fixed per chain
                match chain {
                    "501" => 9, // SOL (lamports)
                    "784" => 9, // SUI (MIST)
                    _ => 18,    // All EVM native tokens (ETH, BNB, MATIC, OKB, AVAX, …)
                }
            }
            Some(token_addr) => {
                // ERC-20 / SPL — fetch decimals via wallet-side token info endpoint
                // (works for chains not covered by the DEX, e.g. Tempo).
                let access_token = auth::ensure_tokens_refreshed().await?;
                let mut client = crate::wallet_api::WalletApiClient::new()?;
                let chain_index_str = crate::chains::resolve_chain(chain);
                let chain_index_num: u64 = chain_index_str.parse().map_err(|_| {
                    anyhow::anyhow!(
                        "chain id '{}' is not a valid number for token-info lookup",
                        chain_index_str
                    )
                })?;
                let info = client
                    .get_token_info(&access_token, chain_index_num, token_addr)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to fetch token decimals for {}: {}. \
                             Use --amt with raw minimal units instead.",
                            token_addr,
                            e
                        )
                    })?;
                if cfg!(feature = "debug-log") {
                    eprintln!(
                        "[DEBUG][get_token_info] chainIndex={}, token={}, raw_response={}",
                        chain_index_num, token_addr, info
                    );
                }
                // Server returns either an array `[{...}]` or a single object;
                // field name is `decimals` (plural) in practice, `decimal` in older spec.
                let entry = info.as_array().and_then(|arr| arr.first()).unwrap_or(&info);
                let decimal_val = if !entry["decimals"].is_null() {
                    &entry["decimals"]
                } else {
                    &entry["decimal"]
                };
                match decimal_val {
                    serde_json::Value::String(s) => s.parse().map_err(|_| {
                        anyhow::anyhow!("Invalid decimal value \"{}\" for token {}", s, token_addr)
                    })?,
                    serde_json::Value::Number(n) => n.as_u64().ok_or_else(|| {
                        anyhow::anyhow!("Invalid decimal value for token {}", token_addr)
                    })? as u32,
                    _ => bail!(
                        "Token decimal not found for {}. Use --amt with raw minimal units instead.",
                        token_addr
                    ),
                }
            }
        };

        return crate::validators::readable_to_minimal_str(readable, decimal);
    }

    bail!("Either --amt or --readable-amount is required")
}

fn cmd_qrcode(address: &str) -> Result<()> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        bail!("--address must not be empty");
    }
    // Delegate to the single shared in-process encoder (crate::qr). Output stays
    // byte-for-byte identical (same builder chain / render params) and continues
    // to go to stdout.
    let rendered = crate::qr::render_address_qr_unicode(trimmed)
        .map_err(|e| anyhow::anyhow!("Failed to encode QR for {}: {}", trimmed, e))?;
    println!("{}", rendered);
    Ok(())
}

/// Resolves `chain` and accepts it only when the UTXO command can use Bitcoin.
async fn ensure_bitcoin_command_chain(chain: &str) -> Result<()> {
    let profile = chain_profile::resolve(chain).await?;
    if !profile.is_bitcoin() {
        bail!("this UTXO command is only supported for Bitcoin");
    }
    Ok(())
}

pub async fn execute(command: WalletCommand) -> Result<()> {
    match command {
        WalletCommand::Login {
            phase,
            url,
            session_id,
        } => match phase {
            LoginPhase::Init => auth::cmd_login_init().await,
            LoginPhase::Open => {
                let Some(url) = url.as_deref() else {
                    bail!("`--url` is required for `--phase open`");
                };
                auth::cmd_login_open(url).await
            }
            LoginPhase::Poll => auth::cmd_login_poll(session_id.as_deref()).await,
        },
        WalletCommand::Add => auth::cmd_add().await,
        WalletCommand::Switch { account_id } => account::cmd_switch(&account_id).await,
        WalletCommand::Status {
            include_subscriptions,
        } => account::cmd_status(include_subscriptions).await,
        WalletCommand::Addresses { chain } => account::cmd_addresses(chain.as_deref()).await,
        WalletCommand::Qrcode { address } => cmd_qrcode(&address),
        WalletCommand::Logout => auth::cmd_logout().await,
        WalletCommand::Chains => chain::execute(chain::ChainCommand::List).await,
        WalletCommand::Geoblock => geoblock::cmd_check().await,
        WalletCommand::Balance {
            all,
            chain,
            token_address,
            force,
        } => {
            let normalized_chain_token = if let (Some(raw_chain), Some(token_address)) =
                (chain.as_deref(), token_address.as_deref())
            {
                match chain_profile::resolve(raw_chain).await? {
                    profile
                        if profile.capabilities.transfer
                            == chain_profile::TransferDriver::Bitcoin =>
                    {
                        Some(shared::adapters::bitcoin::validation::normalize_brc20_token_address(
                            token_address,
                        )?)
                    }
                    profile
                        if profile.capabilities.transfer == chain_profile::TransferDriver::Sui =>
                    {
                        Some(shared::adapters::sui::identifiers::normalize_coin_type(token_address)?)
                    }
                    _ => None,
                }
            } else {
                None
            };
            balance::cmd_balance(
                all,
                chain.as_deref(),
                normalized_chain_token
                    .as_deref()
                    .or(token_address.as_deref()),
                force,
            )
            .await
        }
        WalletCommand::Send {
            amt,
            readable_amount,
            recipient,
            chain,
            from,
            contract_token,
            brc20_outpoint,
            fee_rate,
            force,
            gas_token_address,
            relayer_id,
            enable_gas_station,
        } => {
            {
                let profile = chain_profile::resolve(&chain).await?;
                if profile.capabilities.transfer == chain_profile::TransferDriver::Bitcoin {
                    if amt.is_some() {
                        bail!("Bitcoin transfers require --readable-amount");
                    }
                    if gas_token_address.is_some() || relayer_id.is_some() || enable_gas_station {
                        bail!("Gas Station is not supported for Bitcoin transfers");
                    }
                    return transfer::bitcoin::cmd_send(
                        readable_amount.as_deref(),
                        &recipient,
                        from.as_deref(),
                        contract_token.as_deref(),
                        &brc20_outpoint,
                        fee_rate.as_deref(),
                        force,
                    )
                    .await;
                }
                if profile.capabilities.transfer == chain_profile::TransferDriver::Sui {
                    if !brc20_outpoint.is_empty() {
                        bail!("--brc20-outpoint is only supported for Bitcoin BRC-20 transfers");
                    }
                    if amt.is_some() {
                        bail!("SUI transfers require --readable-amount");
                    }
                    if gas_token_address.is_some() || relayer_id.is_some() || enable_gas_station {
                        bail!("Gas Station is not supported for SUI transfers");
                    }
                    if fee_rate.is_some() {
                        bail!("--fee-rate is only supported for Bitcoin transfers");
                    }
                    let readable_amount = readable_amount
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("--readable-amount is required"))?;
                    return transfer::sui::cmd_send(
                        readable_amount,
                        &recipient,
                        from.as_deref(),
                        contract_token.as_deref(),
                        force,
                    )
                    .await;
                }
            }
            if !brc20_outpoint.is_empty() {
                bail!("--brc20-outpoint is only supported for Bitcoin BRC-20 transfers");
            }
            if fee_rate.is_some() {
                bail!("--fee-rate is only supported for Bitcoin transfers");
            }
            let chain = crate::chains::resolve_chain(&chain);
            // Resolve `--contract-token` alias (`usdc`, `usdt`, ...) → CA and
            // run the same chain-aware format check that swap uses, so a
            // typo / symbol leak is rejected here (before any BE round-trip
            // or auth refresh) rather than at the BE error layer.
            let contract_token = match contract_token {
                Some(ct) => Some(crate::token_alias::resolve_and_validate(
                    &chain,
                    &ct,
                    "contract-token",
                )?),
                None => None,
            };
            let raw_amt = resolve_send_amount(
                amt.as_deref(),
                readable_amount.as_deref(),
                contract_token.as_deref(),
                &chain,
            )
            .await?;
            transfer::cmd_send(
                &raw_amt,
                &recipient,
                &chain,
                from.as_deref(),
                contract_token.as_deref(),
                force,
                gas_token_address.as_deref(),
                relayer_id.as_deref(),
                enable_gas_station,
            )
            .await
        }
        WalletCommand::History {
            account_id,
            chain,
            address,
            begin,
            end,
            page_num,
            limit,
            order_id,
            tx_hash,
            uop_hash,
        } => {
            history::cmd_query_history(
                account_id.as_deref(),
                chain.as_deref(),
                address.as_deref(),
                begin.as_deref(),
                end.as_deref(),
                page_num.as_deref(),
                limit.as_deref(),
                order_id.as_deref(),
                tx_hash.as_deref(),
                uop_hash.as_deref(),
            )
            .await
        }
        WalletCommand::Inscription { command } => match command {
            InscriptionCommand::Create {
                chain,
                token_address,
                readable_amount,
                from,
                operation_token,
                fee_rate,
                force,
            } => {
                let profile = chain_profile::resolve(&chain).await?;
                if profile.capabilities.inscription != chain_profile::InscriptionDriver::Bitcoin {
                    bail!(
                        "wallet inscription is not supported for chain '{}'",
                        profile.chain_name
                    );
                }
                inscription::bitcoin::cmd_create(
                    &token_address,
                    &readable_amount,
                    from.as_deref(),
                    operation_token.as_deref(),
                    fee_rate.as_deref(),
                    force,
                )
                .await
            }
            InscriptionCommand::Status {
                chain,
                tx_hash,
                order_id,
            } => {
                let profile = chain_profile::resolve(&chain).await?;
                if profile.capabilities.inscription != chain_profile::InscriptionDriver::Bitcoin {
                    bail!(
                        "wallet inscription is not supported for chain '{}'",
                        profile.chain_name
                    );
                }
                inscription::bitcoin::cmd_query_status(tx_hash.as_deref(), order_id.as_deref())
                    .await
            }
        },
        WalletCommand::Utxo { command } => match command {
            UtxoCommand::UserIgnored { chain } => {
                ensure_bitcoin_command_chain(&chain).await?;
                utxo::cmd_user_ignored().await
            }
            UtxoCommand::Unavailable { chain } => {
                ensure_bitcoin_command_chain(&chain).await?;
                utxo::cmd_unavailable().await
            }
            UtxoCommand::Available { chain } => {
                ensure_bitcoin_command_chain(&chain).await?;
                utxo::cmd_available().await
            }
            UtxoCommand::Brc20Transferable {
                chain,
                token_address,
                readable_amount,
            } => {
                ensure_bitcoin_command_chain(&chain).await?;
                utxo::cmd_brc20_transferable(&token_address, readable_amount.as_deref()).await
            }
            UtxoCommand::Unlock {
                chain,
                outpoint,
                all,
                operation_token,
                force,
            } => {
                ensure_bitcoin_command_chain(&chain).await?;
                utxo::cmd_unlock(&outpoint, all, operation_token.as_deref(), force).await
            }
            UtxoCommand::Lock {
                chain,
                outpoint,
                all,
                operation_token,
                force,
            } => {
                ensure_bitcoin_command_chain(&chain).await?;
                utxo::cmd_lock(&outpoint, all, operation_token.as_deref(), force).await
            }
            UtxoCommand::Reclaim {
                chain,
                tx_hash,
                force,
            } => {
                ensure_bitcoin_command_chain(&chain).await?;
                utxo::cmd_reclaim(&tx_hash, force).await
            }
        },
        WalletCommand::ReportPluginInfo { plugin_parameter } => {
            plugin::cmd_report_plugin_info(&plugin_parameter).await
        }
        WalletCommand::SignMessage {
            r#type,
            message,
            chain,
            from,
            force,
        } => {
            {
                let profile = chain_profile::resolve(&chain).await?;
                match profile.capabilities.message_sign {
                    chain_profile::MessageSignDriver::Unsupported => {
                        bail!(
                            "wallet sign-message is not supported for chain '{}'",
                            profile.chain_name
                        );
                    }
                    chain_profile::MessageSignDriver::LegacyAccount => {}
                }
            }
            sign::cmd_sign_message(&r#type, &message, &chain, &from, force).await
        }
        WalletCommand::ContractCall {
            to,
            chain,
            amt,
            input_data,
            unsigned_tx,
            sui_tx_bytes,
            gas_limit,
            from,
            aa_dex_token_addr,
            aa_dex_token_amount,
            mev_protection,
            jito_unsigned_tx,
            force,
            gas_token_address,
            relayer_id,
            enable_gas_station,
            biz_type,
            strategy,
        } => {
            {
                let profile = chain_profile::resolve(&chain).await?;
                if !profile.capabilities.contract_call {
                    bail!(
                        "wallet contract-call is not supported for chain '{}'",
                        profile.chain_name
                    );
                }
                if profile.capabilities.transfer == chain_profile::TransferDriver::Sui {
                    if input_data.is_some() || unsigned_tx.is_some() {
                        bail!("SUI contract calls require --sui-tx-bytes, not --input-data or --unsigned-tx");
                    }
                    let tx_bytes = sui_tx_bytes.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("--sui-tx-bytes is required for SUI contract calls")
                    })?;
                    if gas_limit.is_some()
                        || aa_dex_token_addr.is_some()
                        || aa_dex_token_amount.is_some()
                        || mev_protection
                        || jito_unsigned_tx.is_some()
                        || gas_token_address.is_some()
                        || relayer_id.is_some()
                        || enable_gas_station
                    {
                        bail!(
                            "EVM/Solana-only contract-call options are not supported with --sui-tx-bytes"
                        );
                    }
                    return transfer::sui::cmd_contract_call(
                        tx_bytes,
                        to.as_deref(),
                        &amt,
                        from.as_deref(),
                        force,
                        biz_type.as_deref(),
                        strategy.as_deref(),
                    )
                    .await;
                }
            }
            if sui_tx_bytes.is_some() {
                bail!("--sui-tx-bytes is only supported for SUI contract calls");
            }
            let to = to.as_deref().ok_or_else(|| {
                anyhow::anyhow!("--to is required for EVM and Solana contract calls")
            })?;
            transfer::cmd_contract_call(
                to,
                &chain,
                &amt,
                input_data.as_deref(),
                unsigned_tx.as_deref(),
                gas_limit.as_deref(),
                from.as_deref(),
                aa_dex_token_addr.as_deref(),
                aa_dex_token_amount.as_deref(),
                mev_protection,
                jito_unsigned_tx.as_deref(),
                force,
                gas_token_address.as_deref(),
                relayer_id.as_deref(),
                enable_gas_station,
                biz_type.as_deref(),
                strategy.as_deref(),
            )
            .await
        }
        WalletCommand::GasStation { command } => gas_station::execute(command).await,
    }
}
