use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
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
        /// Enable MEV protection (supported on Base and other EVM chains)
        #[arg(long, default_value_t = false)]
        mev_protection: bool,
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
    let mut client = ctx.client_async().await?;
    match cmd {
        GatewayCommand::Gas { chain } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            output::success(fetch_gas(&mut client, &chain_index).await?);
        }
        GatewayCommand::GasLimit {
            from,
            to,
            amount,
            data,
            chain,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            output::success(
                fetch_gas_limit(
                    &mut client,
                    &chain_index,
                    &from,
                    &to,
                    &amount,
                    data.as_deref(),
                )
                .await?,
            );
        }
        GatewayCommand::Simulate {
            from,
            to,
            amount,
            data,
            chain,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            output::success(
                fetch_simulate(&mut client, &chain_index, &from, &to, &amount, &data).await?,
            );
        }
        GatewayCommand::Broadcast {
            signed_tx,
            address,
            chain,
            mev_protection,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            output::success(
                fetch_broadcast(
                    &mut client,
                    &chain_index,
                    &signed_tx,
                    &address,
                    mev_protection,
                )
                .await?,
            );
        }
        GatewayCommand::Orders {
            address,
            chain,
            order_id,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            output::success(
                fetch_orders(&mut client, &chain_index, &address, order_id.as_deref()).await?,
            );
        }
        GatewayCommand::Chains => {
            output::success(fetch_chains(&mut client).await?);
        }
    }
    Ok(())
}

/// GET /api/v6/dex/pre-transaction/gas-price
pub async fn fetch_gas(client: &mut ApiClient, chain_index: &str) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/pre-transaction/gas-price",
            &[("chainIndex", chain_index)],
        )
        .await
}

/// POST /api/v6/dex/pre-transaction/gas-limit
pub async fn fetch_gas_limit(
    client: &mut ApiClient,
    chain_index: &str,
    from: &str,
    to: &str,
    amount: &str,
    data: Option<&str>,
) -> Result<Value> {
    let mut body = json!({
        "chainIndex": chain_index,
        "fromAddress": from,
        "toAddress": to,
        "txAmount": amount,
    });
    if let Some(input_data) = data {
        body["extJson"] = json!({ "inputData": input_data });
    }
    client
        .post("/api/v6/dex/pre-transaction/gas-limit", &body)
        .await
}

/// POST /api/v6/dex/pre-transaction/simulate
pub async fn fetch_simulate(
    client: &mut ApiClient,
    chain_index: &str,
    from: &str,
    to: &str,
    amount: &str,
    data: &str,
) -> Result<Value> {
    let body = json!({
        "chainIndex": chain_index,
        "fromAddress": from,
        "toAddress": to,
        "txAmount": amount,
        "extJson": { "inputData": data },
    });
    client
        .post("/api/v6/dex/pre-transaction/simulate", &body)
        .await
}

/// POST /api/v6/dex/pre-transaction/broadcast-transaction
pub async fn fetch_broadcast(
    client: &mut ApiClient,
    chain_index: &str,
    signed_tx: &str,
    address: &str,
    mev_protection: bool,
) -> Result<Value> {
    let mut body = json!({
        "signedTx": signed_tx,
        "chainIndex": chain_index,
        "address": address,
    });
    if mev_protection {
        body["extraData"] = json!("{\"enableMevProtection\":true}");
    }
    // Read swap trace ID from cache; attach trace headers if present
    let cached_tid = crate::wallet_store::get_swap_trace_id().ok().flatten();
    if let Some(ref tid) = cached_tid {
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][fetch_broadcast] trace headers: ok-client-tid={}, ok-client-timestamp={}",
                tid, timestamp
            );
        }
        let headers = [
            ("ok-client-tid", tid.as_str()),
            ("ok-client-timestamp", timestamp.as_str()),
        ];
        client
            .post_no_retry_with_headers(
                "/api/v6/dex/pre-transaction/broadcast-transaction",
                &body,
                Some(&headers),
            )
            .await
    } else {
        client
            .post_no_retry_with_headers(
                "/api/v6/dex/pre-transaction/broadcast-transaction",
                &body,
                None,
            )
            .await
    }
}

/// GET /api/v6/dex/post-transaction/orders
pub async fn fetch_orders(
    client: &mut ApiClient,
    chain_index: &str,
    address: &str,
    order_id: Option<&str>,
) -> Result<Value> {
    let mut query: Vec<(&str, &str)> = vec![("address", address), ("chainIndex", chain_index)];
    if let Some(oid) = order_id {
        query.push(("orderId", oid));
    }
    client
        .get("/api/v6/dex/post-transaction/orders", &query)
        .await
}

/// GET /api/v6/dex/pre-transaction/supported/chain
pub async fn fetch_chains(client: &mut ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/pre-transaction/supported/chain", &[])
        .await
}
