#![allow(dead_code)]

pub mod audit;
pub mod chains;
mod client;
mod commands;
mod config;
pub mod crypto;
mod file_keyring;
mod home;
mod keyring_store;
mod mcp;
mod output;
mod wallet_api;
mod wallet_store;
mod watch;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "onchainos",
    version,
    about = "onchainOS CLI - interact with OKX Web3 backend"
)]
pub struct Cli {
    /// Backend service URL (overrides config)
    #[arg(long, global = true)]
    pub base_url: Option<String>,

    /// Chain: ethereum, solana, base, bsc, polygon, arbitrum, sui, etc.
    #[arg(long, global = true)]
    pub chain: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Market data: prices, charts, wallet PnL
    Market {
        #[command(subcommand)]
        command: Box<commands::market::MarketCommand>,
    },
    /// Smart money / whale / KOL signal tracking
    Signal {
        #[command(subcommand)]
        command: commands::signal::SignalCommand,
    },
    /// Meme / pump.fun token scanning and analysis
    Memepump {
        #[command(subcommand)]
        command: Box<commands::memepump::MemepumpCommand>,
    },
    /// Leaderboard (top traders ranked by PnL, win rate, or volume)
    Leaderboard {
        #[command(subcommand)]
        command: commands::leaderboard::LeaderboardCommand,
    },
    /// Token information
    Token {
        #[command(subcommand)]
        command: Box<commands::token::TokenCommand>,
    },
    /// DEX swap
    Swap {
        #[command(subcommand)]
        command: commands::swap::SwapCommand,
    },
    /// On-chain gateway
    Gateway {
        #[command(subcommand)]
        command: commands::gateway::GatewayCommand,
    },
    /// Wallet portfolio and balances
    Portfolio {
        #[command(subcommand)]
        command: commands::portfolio::PortfolioCommand,
    },
    /// Start as MCP server (JSON-RPC 2.0 over stdio)
    Mcp {
        /// Backend service URL override
        #[arg(long)]
        base_url: Option<String>,
    },
    /// Agentic wallet: login, verify, create, switch, status, logout, balance
    Wallet {
        #[command(subcommand)]
        command: commands::agentic_wallet::wallet::WalletCommand,
    },
    /// Security scanning (tx-scan, token-scan, dapp-scan, sig-scan)
    Security {
        #[command(subcommand)]
        command: commands::security::SecurityCommand,
    },
    /// Payment protocols — auto-pay gated APIs (x402, etc.)
    Payment {
        #[command(subcommand)]
        command: commands::agentic_wallet::payment::PaymentCommand,
    },
    /// Address tracker: REST activities for KOL / smart money / custom address activity
    Tracker {
        #[command(subcommand)]
        command: commands::tracker::TrackerCommand,
    },
    /// Real-time WebSocket subscriptions for DEX data
    Ws {
        #[command(subcommand)]
        command: commands::ws::WsCommand,
    },
    /// DeFi product discovery, investment, redemption, and portfolio
    Defi {
        #[command(subcommand)]
        command: commands::defi::DefiCommand,
    },
    /// Upgrade onchainos to the latest version
    Upgrade(commands::upgrade::UpgradeArgs),
}

fn main() {
    // Clap's recursive command-tree builder uses ~900+ KB of stack in debug
    // builds. Windows default is 1 MB — not enough headroom. Spawn with 8 MB
    // to match macOS/Linux defaults.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(run)
        .expect("failed to spawn main thread")
        .join()
        .expect("main thread panicked");
}

#[tokio::main]
async fn run() {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    // MCP server runs indefinitely — skip audit for it (MCP tools log individually).
    if matches!(cli.command, Commands::Mcp { .. }) {
        if let Commands::Mcp { base_url } = cli.command {
            if let Err(e) = mcp::serve(base_url.as_deref()).await {
                output::error(&format!("{e:#}"));
                std::process::exit(1);
            }
        }
        return;
    }

    let raw_args: Vec<String> = std::env::args().collect();
    let redacted_args = audit::redact_args(&raw_args);
    let command_name = audit::cli_command_name(&cli.command);
    let start = std::time::Instant::now();

    let ctx = commands::Context::new(&cli);

    let result = match cli.command {
        Commands::Market { command } => commands::market::execute(&ctx, *command).await,
        Commands::Signal { command } => commands::signal::execute(&ctx, command).await,
        Commands::Memepump { command } => commands::memepump::execute(&ctx, *command).await,
        Commands::Leaderboard { command } => commands::leaderboard::execute(&ctx, command).await,
        Commands::Tracker { command } => commands::tracker::execute(&ctx, command).await,
        Commands::Token { command } => commands::token::execute(&ctx, *command).await,
        Commands::Swap { command } => commands::swap::execute(&ctx, command).await,
        Commands::Gateway { command } => commands::gateway::execute(&ctx, command).await,
        Commands::Portfolio { command } => commands::portfolio::execute(&ctx, command).await,
        Commands::Mcp { .. } => unreachable!("handled above"),
        Commands::Wallet { command } => commands::agentic_wallet::wallet::execute(command).await,
        Commands::Security { command } => commands::security::execute(&ctx, command).await,
        Commands::Payment { command } => commands::agentic_wallet::payment::execute(command).await,
        Commands::Defi { command } => commands::defi::execute(&ctx, command).await,
        Commands::Ws { command } => commands::ws::execute(command).await,
        Commands::Upgrade(args) => commands::upgrade::execute(args).await,
    };

    let elapsed = start.elapsed();
    audit::log(
        "cli",
        &command_name,
        result.is_ok(),
        elapsed,
        Some(redacted_args),
        result.as_ref().err().map(|e| format!("{e:#}")).as_deref(),
    );

    if let Err(e) = result {
        match e.downcast::<output::CliConfirming>() {
            Ok(c) => {
                output::confirming(&c.message, &c.next);
                std::process::exit(2);
            }
            Err(e) => {
                output::error(&format!("{e:#}"));
                std::process::exit(1);
            }
        }
    }
}
