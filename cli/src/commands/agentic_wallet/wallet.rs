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
        /// Filter by chain ID (e.g. "1" for Ethereum, "501" for Solana, "196" for XLayer)
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
        /// Filter by chain ID (e.g. "1" for Ethereum, "501" for Solana, "196" for XLayer)
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
        receipt: String,
        /// Chain ID (e.g. "1" for Ethereum, "501" for Solana, "56" for BSC)
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
    },
    /// Query transaction history or detail
    History {
        /// Account ID (defaults to current selectedAccountId)
        #[arg(long)]
        account_id: Option<String>,
        /// Chain ID (e.g. "1" for Ethereum, "501" for Solana). Resolved to chainIndex internally.
        #[arg(long)]
        chain: Option<String>,
        /// Address (required when --tx-hash is present for detail query)
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
        /// Order ID filter
        #[arg(long)]
        order_id: Option<String>,
        /// Transaction hash — when present, queries order detail instead of list
        #[arg(long)]
        tx_hash: Option<String>,
        /// User operation hash filter
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
        /// Chain ID (e.g. "1" for Ethereum, "501" for Solana, "56" for BSC)
        #[arg(long)]
        chain: String,
        /// Sender address (the address whose private key is used to sign)
        #[arg(long)]
        from: String,
        /// Force execution: skip confirmation prompts from the backend
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Call a smart contract (EVM inputData or SOL unsigned tx)
    ContractCall {
        /// Contract address to interact with
        #[arg(long)]
        to: String,
        /// Chain ID (e.g. "1" for Ethereum, "501" for Solana, "56" for BSC)
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
            bail!("--amt must be a whole number in minimal units, got \"{}\"", raw);
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
                    "501" => 9,  // SOL (lamports)
                    "784" => 9,  // SUI (MIST)
                    _ => 18,     // All EVM native tokens (ETH, BNB, MATIC, OKB, AVAX, …)
                }
            }
            Some(token_addr) => {
                // ERC-20 / SPL — fetch decimals from token info API
                let client = crate::client::ApiClient::new(None).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to create API client to fetch token decimals: {}. \
                         Use --amt with raw minimal units instead.",
                        e
                    )
                })?;
                let info = crate::commands::token::fetch_info(&client, token_addr, chain)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to fetch token decimals for {}: {}. \
                             Use --amt with raw minimal units instead.",
                            token_addr, e
                        )
                    })?;
                let info_arr = info.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Token not found for address {} on chain {}. \
                         Verify the address is correct. Use --amt with raw minimal units instead.",
                        token_addr, chain
                    )
                })?;
                match &info_arr[0]["decimal"] {
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
            receipt,
            chain,
            from,
            contract_token,
            force,
        } => {
            let raw_amt = resolve_send_amount(
                amt.as_deref(),
                readable_amount.as_deref(),
                contract_token.as_deref(),
                &chain,
            )
            .await?;
            super::transfer::cmd_send(
                &raw_amt,
                &receipt,
                &chain,
                from.as_deref(),
                contract_token.as_deref(),
                force,
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
            )
            .await
        }
    }
}
