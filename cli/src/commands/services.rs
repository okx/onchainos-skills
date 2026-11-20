//! Service login and wallet-scoped credential access.
use crate::{output, services::newsliquid};
use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

#[derive(Subcommand)]
pub enum ServiceCommand {
    /// Log in with the selected wallet and store the service credential.
    Login {
        #[arg(long)]
        service: String,
        /// Obtain a fresh login even when a cached credential has not expired.
        #[arg(long)]
        refresh: bool,
        /// Accept only a wallet warning already shown to and accepted by the user.
        #[arg(long, requires = "wallet")]
        confirm_wallet_warning: bool,
        /// Expected current wallet; required when continuing a wallet warning.
        #[arg(long)]
        wallet: Option<String>,
    },
    /// Read the current wallet's valid service token for use inside a script.
    Token {
        #[arg(long)]
        service: String,
        /// Stop if the selected wallet differs from the script's intended wallet.
        #[arg(long)]
        wallet: Option<String>,
    },
}

pub async fn execute(command: ServiceCommand) -> Result<()> {
    let result = match command {
        ServiceCommand::Login {
            service,
            refresh,
            confirm_wallet_warning,
            wallet,
        } => {
            newsliquid::require_service(&service)?;
            newsliquid::login(refresh, confirm_wallet_warning, wallet.as_deref()).await?
        }
        ServiceCommand::Token { service, wallet } => {
            newsliquid::require_service(&service)?;
            newsliquid::token(wallet.as_deref())?
        }
    };
    output::success(json!({"service":newsliquid::ID,"result":result}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: ServiceCommand,
    }

    #[test]
    fn credential_commands_parse_and_removed_proxy_commands_are_rejected() {
        let parsed =
            TestCli::try_parse_from(["service", "login", "--service", "newsliquid", "--refresh"])
                .unwrap();
        assert!(matches!(
            parsed.command,
            ServiceCommand::Login { refresh: true, .. }
        ));
        assert!(TestCli::try_parse_from([
            "service",
            "token",
            "--service",
            "newsliquid",
            "--wallet",
            "expected-wallet",
        ])
        .is_ok());
        for command in ["request", "status", "stream"] {
            assert!(
                TestCli::try_parse_from(["service", command, "--service", "newsliquid"]).is_err()
            );
        }
        assert!(TestCli::try_parse_from([
            "service",
            "login",
            "--service",
            "newsliquid",
            "--confirm-wallet-warning",
        ])
        .is_err());
    }
}
