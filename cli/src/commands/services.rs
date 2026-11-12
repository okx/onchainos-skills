//! Service CLI entry point for NewsLiquid / 6551.
use super::sink::CodedError;
use crate::output;
use crate::services::newsliquid;
use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

#[derive(Subcommand)]
pub enum ServiceCommand {
    /// Call one service operation. Writes require the owning flow's confirmation.
    Call {
        #[arg(long)]
        service: String,
        #[arg(long)]
        operation: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long, default_value = "{}")]
        params: String,
        #[arg(long)]
        confirmed: bool,
    },
}

fn params(raw: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| CodedError::invalid_input("params", "Provide a JSON parameter object."))?;
    newsliquid::check_params(&value)?;
    Ok(value)
}

pub async fn execute(command: ServiceCommand) -> Result<()> {
    let result = match command {
        ServiceCommand::Call {
            service,
            operation,
            id,
            params: raw,
            confirmed,
        } => {
            let params = params(&raw)?;
            newsliquid::require_service(&service)?;
            let result = newsliquid::call(&operation, id, params, confirmed).await?;
            json!({"service":newsliquid::ID,"result":result})
        }
    };
    output::success(result);
    Ok(())
}
