#![allow(dead_code)]

pub mod chains;
mod client;
mod commands;
mod config;
mod mcp;
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
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    /// Market data
    Market {
        #[command(subcommand)]
        command: commands::market::MarketCommand,
    },
    /// Token information
    Token {
        #[command(subcommand)]
        command: commands::token::TokenCommand,
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
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();
    let ctx = commands::Context::new(&cli);

    let result = match cli.command {
        Commands::Market { command } => commands::market::execute(&ctx, command).await,
        Commands::Token { command } => commands::token::execute(&ctx, command).await,
        Commands::Swap { command } => commands::swap::execute(&ctx, command).await,
        Commands::Gateway { command } => commands::gateway::execute(&ctx, command).await,
        Commands::Portfolio { command } => commands::portfolio::execute(&ctx, command).await,
        Commands::Mcp { base_url } => mcp::serve(base_url.as_deref()).await,
    };

    if let Err(e) = result {
        output::error(&format!("{e:#}"));
        std::process::exit(1);
    }
}
