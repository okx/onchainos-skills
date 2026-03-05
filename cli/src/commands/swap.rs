use anyhow::Result;
use clap::Subcommand;

use super::Context;
use crate::output;

/// All aggregator endpoints are GET requests.
#[derive(Subcommand)]
pub enum SwapCommand {
    /// Get swap quote (read-only price estimate)
    Quote {
        /// Source token contract address
        #[arg(long)]
        from: String,
        /// Destination token contract address
        #[arg(long)]
        to: String,
        /// Amount in minimal units (wei/lamports)
        #[arg(long)]
        amount: String,
        /// Chain (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Swap mode: exactIn or exactOut
        #[arg(long, default_value = "exactIn")]
        swap_mode: String,
    },
    /// Get swap transaction data (quote → sign → broadcast)
    Swap {
        /// Source token contract address
        #[arg(long)]
        from: String,
        /// Destination token contract address
        #[arg(long)]
        to: String,
        /// Amount in minimal units
        #[arg(long)]
        amount: String,
        /// Chain
        #[arg(long)]
        chain: String,
        /// Slippage tolerance in percent (e.g. "1" for 1%)
        #[arg(long, default_value = "1")]
        slippage: String,
        /// User wallet address
        #[arg(long)]
        wallet: String,
        /// Swap mode: exactIn or exactOut
        #[arg(long, default_value = "exactIn")]
        swap_mode: String,
    },
    /// Get ERC-20 approval transaction data
    Approve {
        /// Token contract address to approve
        #[arg(long)]
        token: String,
        /// Approval amount in minimal units
        #[arg(long)]
        amount: String,
        /// Chain
        #[arg(long)]
        chain: String,
    },
    /// Get supported chains for DEX aggregator
    Chains,
    /// Get available liquidity sources on a chain
    Liquidity {
        /// Chain
        #[arg(long)]
        chain: String,
    },
}

pub async fn execute(ctx: &Context, cmd: SwapCommand) -> Result<()> {
    match cmd {
        SwapCommand::Quote {
            from,
            to,
            amount,
            chain,
            swap_mode,
        } => quote(ctx, &from, &to, &amount, &chain, &swap_mode).await,
        SwapCommand::Swap {
            from,
            to,
            amount,
            chain,
            slippage,
            wallet,
            swap_mode,
        } => {
            swap(
                ctx, &from, &to, &amount, &chain, &slippage, &wallet, &swap_mode,
            )
            .await
        }
        SwapCommand::Approve {
            token,
            amount,
            chain,
        } => approve(ctx, &token, &amount, &chain).await,
        SwapCommand::Chains => chains(ctx).await,
        SwapCommand::Liquidity { chain } => liquidity(ctx, &chain).await,
    }
}

/// GET /api/v6/dex/aggregator/quote
async fn quote(
    ctx: &Context,
    from: &str,
    to: &str,
    amount: &str,
    chain: &str,
    swap_mode: &str,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/aggregator/quote",
            &[
                ("chainIndex", chain_index.as_str()),
                ("fromTokenAddress", from),
                ("toTokenAddress", to),
                ("amount", amount),
                ("swapMode", swap_mode),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/aggregator/swap
#[allow(clippy::too_many_arguments)]
async fn swap(
    ctx: &Context,
    from: &str,
    to: &str,
    amount: &str,
    chain: &str,
    slippage: &str,
    wallet: &str,
    swap_mode: &str,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/aggregator/swap",
            &[
                ("chainIndex", chain_index.as_str()),
                ("fromTokenAddress", from),
                ("toTokenAddress", to),
                ("amount", amount),
                ("slippagePercent", slippage),
                ("userWalletAddress", wallet),
                ("swapMode", swap_mode),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/aggregator/approve-transaction
async fn approve(ctx: &Context, token: &str, amount: &str, chain: &str) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/aggregator/approve-transaction",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", token),
                ("approveAmount", amount),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/aggregator/supported/chain
async fn chains(ctx: &Context) -> Result<()> {
    let client = ctx.client()?;
    let data = client
        .get("/api/v6/dex/aggregator/supported/chain", &[])
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/aggregator/get-liquidity
async fn liquidity(ctx: &Context, chain: &str) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/aggregator/get-liquidity",
            &[("chainIndex", chain_index.as_str())],
        )
        .await?;
    output::success(data);
    Ok(())
}
