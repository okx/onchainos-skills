#![allow(dead_code)]

pub mod chains;
mod client;
mod commands;
mod config;
mod output;

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
    let ctx = commands::Context::new(&cli);

    let result = match cli.command {
        Commands::Market { command } => commands::market::execute(&ctx, *command).await,
        Commands::Signal { command } => commands::signal::execute(&ctx, command).await,
        Commands::Memepump { command } => commands::memepump::execute(&ctx, *command).await,
        Commands::Token { command } => commands::token::execute(&ctx, *command).await,
        Commands::Swap { command } => commands::swap::execute(&ctx, command).await,
        Commands::Gateway { command } => commands::gateway::execute(&ctx, command).await,
        Commands::Portfolio { command } => commands::portfolio::execute(&ctx, command).await,
    };

    if let Err(e) = result {
        output::error(&format!("{e:#}"));
        std::process::exit(1);
    }
}
