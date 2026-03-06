use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use super::Context;
use crate::output;

#[derive(Subcommand)]
pub enum GatewayCommand {
    /// Get current gas prices for a chain
    Gas {
        /// Chain (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
    },
    /// Estimate gas limit for a transaction
    GasLimit {
        /// Sender address
        #[arg(long)]
        from: String,
        /// Recipient / contract address
        #[arg(long)]
        to: String,
        /// Transfer value in minimal units (default "0")
        #[arg(long, default_value = "0")]
        amount: String,
        /// Encoded calldata (hex, for contract interactions)
        #[arg(long)]
        data: Option<String>,
        /// Chain
        #[arg(long)]
        chain: String,
    },
    /// Simulate a transaction (dry-run)
    Simulate {
        /// Sender address
        #[arg(long)]
        from: String,
        /// Recipient / contract address
        #[arg(long)]
        to: String,
        /// Transfer value in minimal units
        #[arg(long, default_value = "0")]
        amount: String,
        /// Encoded calldata (hex)
        #[arg(long)]
        data: String,
        /// Chain
        #[arg(long)]
        chain: String,
    },
    /// Broadcast a signed transaction
    Broadcast {
        /// Fully signed transaction (hex for EVM, base58 for Solana)
        #[arg(long)]
        signed_tx: String,
        /// Sender wallet address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: String,
    },
    /// Track broadcast order status
    Orders {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: String,
        /// Specific order ID (from broadcast response)
        #[arg(long)]
        order_id: Option<String>,
    },
    /// Get supported chains for gateway
    Chains,
}

pub async fn execute(ctx: &Context, cmd: GatewayCommand) -> Result<()> {
    match cmd {
        GatewayCommand::Gas { chain } => gas(ctx, &chain).await,
        GatewayCommand::GasLimit {
            from,
            to,
            amount,
            data,
            chain,
        } => gas_limit(ctx, &from, &to, &amount, data.as_deref(), &chain).await,
        GatewayCommand::Simulate {
            from,
            to,
            amount,
            data,
            chain,
        } => simulate(ctx, &from, &to, &amount, &data, &chain).await,
        GatewayCommand::Broadcast {
            signed_tx,
            address,
            chain,
        } => broadcast(ctx, &signed_tx, &address, &chain).await,
        GatewayCommand::Orders {
            address,
            chain,
            order_id,
        } => orders(ctx, &address, &chain, order_id.as_deref()).await,
        GatewayCommand::Chains => chains(ctx).await,
    }
}

/// GET /api/v6/dex/pre-transaction/gas-price
async fn gas(ctx: &Context, chain: &str) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/pre-transaction/gas-price",
            &[("chainIndex", chain_index.as_str())],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// POST /api/v6/dex/pre-transaction/gas-limit
async fn gas_limit(
    ctx: &Context,
    from: &str,
    to: &str,
    amount: &str,
    data: Option<&str>,
    chain: &str,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let mut body = json!({
        "chainIndex": chain_index,
        "fromAddress": from,
        "toAddress": to,
        "txAmount": amount,
    });
    if let Some(input_data) = data {
        body["extJson"] = json!({ "inputData": input_data });
    }
    let result = client
        .post("/api/v6/dex/pre-transaction/gas-limit", &body)
        .await?;
    output::success(result);
    Ok(())
}

/// POST /api/v6/dex/pre-transaction/simulate
async fn simulate(
    ctx: &Context,
    from: &str,
    to: &str,
    amount: &str,
    data: &str,
    chain: &str,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let body = json!({
        "chainIndex": chain_index,
        "fromAddress": from,
        "toAddress": to,
        "txAmount": amount,
        "extJson": { "inputData": data },
    });
    let result = client
        .post("/api/v6/dex/pre-transaction/simulate", &body)
        .await?;
    output::success(result);
    Ok(())
}

/// POST /api/v6/dex/pre-transaction/broadcast-transaction
async fn broadcast(ctx: &Context, signed_tx: &str, address: &str, chain: &str) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let body = json!({
        "signedTx": signed_tx,
        "chainIndex": chain_index,
        "address": address,
    });
    let result = client
        .post("/api/v6/dex/pre-transaction/broadcast-transaction", &body)
        .await?;
    output::success(result);
    Ok(())
}

/// GET /api/v6/dex/post-transaction/orders
async fn orders(ctx: &Context, address: &str, chain: &str, order_id: Option<&str>) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let mut query: Vec<(&str, &str)> =
        vec![("address", address), ("chainIndex", chain_index.as_str())];
    if let Some(oid) = order_id {
        query.push(("orderId", oid));
    }
    let data = client
        .get("/api/v6/dex/post-transaction/orders", &query)
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/pre-transaction/supported/chain
async fn chains(ctx: &Context) -> Result<()> {
    let client = ctx.client()?;
    let data = client
        .get("/api/v6/dex/pre-transaction/supported/chain", &[])
        .await?;
    output::success(data);
    Ok(())
}
