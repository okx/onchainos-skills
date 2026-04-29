use anyhow::{bail, Result};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum WalletCommand {
    /// Start login flow — sends OTP to email, or AK login if no email provided
    Login {
        /// Email address to receive OTP (optional — omit for AK login)
        email: Option<String>,
        /// Locale (e.g. "en-US", "zh-CN"). Optional.
        #[arg(long)]
        locale: Option<String>,
        /// Force re-login, skip API Key switch confirmation
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Verify OTP code from email
    Verify {
        /// One-time password received via email
        otp: String,
    },
    /// Add a new wallet account
    Add,
    /// Switch active account
    Switch {
        /// Account ID to switch to
        account_id: String,
    },
    /// Show current wallet status
    Status,
    /// Show wallet addresses grouped by chain category (XLayer, EVM, Solana)
    Addresses {
        /// Chain name or ID (e.g. "ethereum" or "1", "solana" or "501", "xlayer" or "196")
        #[arg(long)]
        chain: Option<String>,
    },
    /// Logout and clear all stored credentials
    Logout,
    /// List all supported chains (cached locally, refreshes every 10 minutes)
    Chains,
    /// Query wallet balances
    Balance {
        /// Query all accounts' assets (uses accountId list)
        #[arg(long)]
        all: bool,
        /// Chain name or ID (e.g. "ethereum" or "1", "solana" or "501", "xlayer" or "196")
        #[arg(long)]
        chain: Option<String>,
        /// Filter by token contract address. Requires --chain.
        #[arg(long)]
        token_address: Option<String>,
        /// Force refresh: bypass all caches and re-fetch wallet accounts + balances from the API.
        /// Use when the user explicitly asks to refresh/sync/update their wallet data.
        #[arg(long, default_value = "false")]
        force: bool,
    },
    /// Send a transaction (native or token transfer)
    Send {
        /// Amount in minimal units — whole number, no decimals (e.g. "100000000000000000" for 0.1 ETH). Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amt: Option<String>,
        /// Human-readable amount (e.g. "1.5" for 1.5 USDC). CLI fetches token decimals and converts automatically. Mutually exclusive with --amt.
        #[arg(long, conflicts_with = "amt")]
        readable_amount: Option<String>,
        /// Recipient address
        #[arg(long)]
        recipient: String,
        /// Chain name or ID (e.g. "ethereum" or "1", "solana" or "501", "bsc" or "56")
        #[arg(long)]
        chain: String,
        /// Sender address (optional — defaults to selectedAccountId)
        #[arg(long)]
        from: Option<String>,
        /// Contract token address (optional — for ERC-20 / SPL token transfers)
        #[arg(long)]
        contract_token: Option<String>,
        /// Force execution: skip confirmation prompts from the backend
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
    /// Query transaction history or detail
    History {
        /// Account ID (defaults to current selectedAccountId)
        #[arg(long)]
        account_id: Option<String>,
        /// Chain name or ID (e.g. "ethereum" or "1", "solana" or "501"). Resolved to chainIndex internally.
        #[arg(long)]
        chain: Option<String>,
        /// Address (optional; passed to detail query if provided)
        #[arg(long)]
        address: Option<String>,
        /// Start time filter (ms timestamp)
        #[arg(long)]
        begin: Option<String>,
        /// End time filter (ms timestamp)
        #[arg(long)]
        end: Option<String>,
        /// Page cursor
        #[arg(long)]
        page_num: Option<String>,
        /// Page size limit
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
    /// Call a smart contract (EVM inputData or SOL unsigned tx).
    /// Supports Gas Station: if the account has Gas Station enabled, pass
    /// `--gas-token-address` + `--relayer-id` (and `--enable-gas-station` for
    /// first-time activation / re-enable) to pay gas with stablecoins.
    ContractCall {
        /// Contract address to interact with
        #[arg(long)]
        to: String,
        /// Chain name or ID (e.g. "ethereum" or "1", "solana" or "501", "bsc" or "56")
        #[arg(long)]
        chain: String,
        /// Native token amount in minimal units — whole number, no decimals (default "0")
        #[arg(long, default_value = "0")]
        amt: String,
        /// EVM call data (hex-encoded, e.g. "0xa9059cbb...")
        #[arg(long)]
        input_data: Option<String>,
        /// Solana unsigned transaction data (base58)
        #[arg(long)]
        unsigned_tx: Option<String>,
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
        /// Force execution: skip confirmation prompts from the backend
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
                let access_token = super::auth::ensure_tokens_refreshed().await?;
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
                let entry = info
                    .as_array()
                    .and_then(|arr| arr.first())
                    .unwrap_or(&info);
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

        return crate::commands::swap::readable_to_minimal_str(readable, decimal);
    }

    bail!("Either --amt or --readable-amount is required")
}

pub async fn execute(command: WalletCommand) -> Result<()> {
    match command {
        WalletCommand::Login {
            email,
            locale,
            force,
        } => super::auth::cmd_login(email.as_deref(), locale.as_deref(), force).await,
        WalletCommand::Verify { otp } => super::auth::cmd_verify(&otp).await,
        WalletCommand::Add => super::auth::cmd_add().await,
        WalletCommand::Switch { account_id } => super::account::cmd_switch(&account_id).await,
        WalletCommand::Status => super::account::cmd_status().await,
        WalletCommand::Addresses { chain } => super::account::cmd_addresses(chain.as_deref()).await,
        WalletCommand::Logout => super::auth::cmd_logout().await,
        WalletCommand::Chains => super::chain::execute(super::chain::ChainCommand::List).await,
        WalletCommand::Balance {
            all,
            chain,
            token_address,
            force,
        } => {
            super::balance::cmd_balance(all, chain.as_deref(), token_address.as_deref(), force)
                .await
        }
        WalletCommand::Send {
            amt,
            readable_amount,
            recipient,
            chain,
            from,
            contract_token,
            force,
            gas_token_address,
            relayer_id,
            enable_gas_station,
        } => {
            let chain = crate::chains::resolve_chain(&chain);
            let raw_amt = resolve_send_amount(
                amt.as_deref(),
                readable_amount.as_deref(),
                contract_token.as_deref(),
                &chain,
            )
            .await?;
            super::transfer::cmd_send(
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
            super::history::cmd_history(
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
        WalletCommand::ReportPluginInfo { plugin_parameter } => {
            super::plugin::cmd_report_plugin_info(&plugin_parameter).await
        }
        WalletCommand::SignMessage {
            r#type,
            message,
            chain,
            from,
            force,
        } => super::sign::cmd_sign_message(&r#type, &message, &chain, &from, force).await,
        WalletCommand::ContractCall {
            to,
            chain,
            amt,
            input_data,
            unsigned_tx,
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
            super::transfer::cmd_contract_call(
                &to,
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
        WalletCommand::GasStation { command } => {
            super::gas_station::execute(command).await
        }
    }
}
