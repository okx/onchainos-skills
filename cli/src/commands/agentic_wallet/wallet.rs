use anyhow::Result;
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
        /// Amount to send (e.g. "0.01")
        #[arg(long)]
        amount: String,
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
    /// Call a smart contract (EVM inputData or SOL unsigned tx)
    ContractCall {
        /// Contract address to interact with
        #[arg(long)]
        to: String,
        /// Chain ID (e.g. "1" for Ethereum, "501" for Solana, "56" for BSC)
        #[arg(long)]
        chain: String,
        /// Native token amount to send with the call (default "0")
        #[arg(long, default_value = "0")]
        value: String,
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
    },
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
            amount,
            receipt,
            chain,
            from,
            contract_token,
        } => {
            super::transfer::cmd_send(
                &amount,
                &receipt,
                &chain,
                from.as_deref(),
                contract_token.as_deref(),
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
        WalletCommand::ContractCall {
            to,
            chain,
            value,
            input_data,
            unsigned_tx,
            gas_limit,
            from,
            aa_dex_token_addr,
            aa_dex_token_amount,
            mev_protection,
            jito_unsigned_tx,
        } => {
            super::transfer::cmd_contract_call(
                &to,
                &chain,
                &value,
                input_data.as_deref(),
                unsigned_tx.as_deref(),
                gas_limit.as_deref(),
                from.as_deref(),
                aa_dex_token_addr.as_deref(),
                aa_dex_token_amount.as_deref(),
                mev_protection,
                jito_unsigned_tx.as_deref(),
            )
            .await
        }
    }
}
