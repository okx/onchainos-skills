use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use super::Context;
use crate::output;

#[derive(Subcommand)]
pub enum PredictCommand {
    /// Browse prediction markets
    Markets {
        /// Search query
        #[arg(long)]
        search: Option<String>,
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
        /// Max results
        #[arg(long, default_value = "10")]
        limit: u32,
    },
    /// Browse events
    Events,
    /// Get token price in prediction market
    Price {
        /// Token ID
        token_id: String,
    },
    /// Get order book
    Book {
        /// Token ID
        token_id: String,
    },
    /// Place an order
    Order {
        /// Token ID
        #[arg(long)]
        token: String,
        /// Side: buy or sell
        #[arg(long)]
        side: String,
        /// Price
        #[arg(long)]
        price: String,
        /// Size
        #[arg(long)]
        size: String,
    },
    /// Cancel an order
    Cancel {
        /// Order ID
        order_id: String,
    },
    /// View positions
    Positions,
    /// Redeem a resolved position
    Redeem {
        /// Condition ID
        #[arg(long)]
        condition: String,
    },
}

pub async fn execute(_ctx: &Context, _cmd: PredictCommand) -> Result<()> {
    output::success(json!({"message": "Coming soon"}));
    Ok(())
}
