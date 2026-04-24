use anyhow::{anyhow, bail, Context, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use crate::commands::agentic_wallet::payment_flow;
use crate::output;

#[derive(Subcommand)]
pub enum PaymentCommand {
    /// Sign an x402 payment and return the payment proof
    X402Pay {
        /// JSON accepts array from the 402 response (decoded.accepts).
        /// The CLI selects the best scheme automatically
        /// (prefers "exact", falls back to "aggr_deferred", then first entry).
        #[arg(long)]
        accepts: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
    },
    /// Sign an EIP-3009 TransferWithAuthorization locally with a hex private key
    /// (reads EVM_PRIVATE_KEY env var). Accepts the same JSON accepts array as x402-pay;
    /// domain name/version are read from accepts[].extra.name / extra.version.
    Eip3009Sign {
        /// JSON accepts array from the 402 response (same format as x402-pay).
        /// domain name/version are extracted from the selected entry's `extra.name` / `extra.version`.
        #[arg(long)]
        accepts: String,
    },
    /// Manage the default payment asset used when the server offers multiple options.
    Default {
        #[command(subcommand)]
        action: DefaultAction,
    },
}

#[derive(Subcommand)]
pub enum DefaultAction {
    /// Save an asset + chain as the default; used first when matching `accepts`.
    Set {
        /// EVM token contract address, e.g. 0xUSDG
        #[arg(long)]
        asset: String,
        /// Numeric EVM chain id, e.g. "1" (Ethereum), "196" (X Layer), "8453" (Base)
        #[arg(long)]
        chain: String,
        /// Display name shown in notifications, e.g. "USDT"
        #[arg(long)]
        name: Option<String>,
        /// Tier the user just confirmed: `basic` or `premium`. Skills
        /// pass this from the OVER_QUOTA `notifications[].data.tier` so
        /// only the acknowledged tier advances to `ChargingConfirmed`.
        /// Omit for manual invocations that don't act on a prompt.
        #[arg(long)]
        tier: Option<String>,
    },
    /// Show the saved default payment asset (if any).
    Get,
    /// Clear the saved default payment asset.
    Unset,
}

pub async fn execute(cmd: PaymentCommand) -> Result<()> {
    match cmd {
        PaymentCommand::X402Pay { accepts, from } => cmd_pay(&accepts, from.as_deref()).await,
        PaymentCommand::Eip3009Sign { accepts } => {
            let accepts_val: Value =
                serde_json::from_str(&accepts).context("--accepts must be a valid JSON array")?;
            let (proof, _entry) = payment_flow::sign_payment_local(&accepts_val, None).await?;
            output::success(json!({
                "signature": proof.signature,
                "authorization": proof.authorization,
            }));
            Ok(())
        }
        PaymentCommand::Default { action } => cmd_default(action),
    }
}

/// Convert a numeric EVM chain id (e.g. `"196"`) to CAIP-2 form
/// (`"eip155:196"`) for storage. Only plain decimal integers are
/// accepted — chain names (`"xlayer"`) and pre-formed CAIP-2 strings
/// (`"eip155:196"`) are rejected. Non-EVM chain ids are rejected too
/// (x402 payments are EIP-712 signed).
fn chain_id_to_caip2(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("--chain must not be empty");
    }
    let n: u64 = trimmed.parse().with_context(|| {
        format!(
            "--chain must be a numeric chain id (e.g. \"1\" for Ethereum, \
             \"196\" for X Layer), got: {input}"
        )
    })?;
    if matches!(n, 195 | 501 | 607 | 784) {
        bail!("x402 payments are EVM-only; chain id {n} is not supported");
    }
    Ok(format!("eip155:{n}"))
}

/// Extract the numeric chain id from a CAIP-2 `eip155:<id>` string for
/// display. Returns an empty string if the prefix is missing (never
/// happens for values written by `chain_id_to_caip2`).
fn caip2_to_chain_id(caip2: &str) -> String {
    caip2.strip_prefix("eip155:").unwrap_or(caip2).to_string()
}

fn cmd_default(action: DefaultAction) -> Result<()> {
    use crate::commands::agentic_wallet::payment_flow::PaymentTier;
    use crate::payment_cache::{PaymentCache, PaymentDefault};
    use crate::payment_notify::TierState;

    match action {
        DefaultAction::Set {
            asset,
            chain,
            name,
            tier,
        } => {
            let asset = asset.trim().to_string();
            if !is_valid_evm_address(&asset) {
                bail!("--asset must be a valid EVM address (0x + 40 hex chars)");
            }
            let chain = chain.trim().to_string();
            let network = chain_id_to_caip2(&chain)?;
            let name = name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let tier = match tier.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Some(
                    PaymentTier::from_server_str(s)
                        .ok_or_else(|| anyhow!("--tier must be `basic` or `premium`"))?,
                ),
                None => None,
            };

            let mut cache = PaymentCache::load().unwrap_or_default();
            cache.default_asset = Some(PaymentDefault {
                asset: asset.clone(),
                network,
                name: name.clone(),
            });
            // Explicit consent promotes only the tier the user just
            // confirmed — untagged calls (manual use) never change
            // state, so a pending prompt on another tier still fires.
            if let Some(t) = tier {
                let slot = match t {
                    PaymentTier::Basic => &mut cache.basic_state,
                    PaymentTier::Premium => &mut cache.premium_state,
                };
                if *slot == TierState::ChargingUnconfirmed {
                    *slot = TierState::ChargingConfirmed;
                }
            }
            cache.save().context("failed to save payment cache")?;
            output::success(json!({
                "asset": asset,
                "chain": chain,
                "name": name,
            }));
            Ok(())
        }
        DefaultAction::Get => {
            let cache = PaymentCache::load().unwrap_or_default();
            match cache.default_asset {
                Some(d) => output::success(json!({
                    "asset": d.asset,
                    "chain": caip2_to_chain_id(&d.network),
                    "name": d.name,
                })),
                None => output::success_empty(),
            }
            Ok(())
        }
        DefaultAction::Unset => {
            let mut cache = PaymentCache::load().unwrap_or_default();
            cache.default_asset = None;
            cache.save().context("failed to save payment cache")?;
            output::success_empty();
            Ok(())
        }
    }
}

/// Validate common payment inputs: amount, pay_to, asset.
/// Returns the parsed amount as u128.
fn validate_payment_inputs(amount: &str, pay_to: &str, asset: &str) -> Result<u128> {
    if amount.is_empty() {
        bail!("--amount must not be empty");
    }
    let parsed_amount = amount
        .parse::<u128>()
        .context("--amount must be a non-negative integer in minimal units")?;
    if parsed_amount == 0 {
        bail!("--amount must be greater than zero");
    }
    if !is_valid_evm_address(pay_to) {
        bail!("--pay-to must be a valid EVM address (0x + 40 hex chars)");
    }
    if !is_valid_evm_address(asset) {
        bail!("--asset must be a valid EVM contract address (0x + 40 hex chars)");
    }
    Ok(parsed_amount)
}

/// Sign an x402 payment authorization and print the proof as JSON.
/// All crypto happens in `payment_flow::sign_payment_with_preference`. Passes
/// `None` for the preference so the user's saved default asset does NOT
/// influence which accepts entry gets signed — this command signs exactly
/// what the caller supplied via `--accepts`.
async fn cmd_pay(accepts_json: &str, from: Option<&str>) -> Result<()> {
    let accepts: Value =
        serde_json::from_str(accepts_json).context("--accepts must be a valid JSON array")?;
    let (proof, _entry) =
        payment_flow::sign_payment_with_preference(&accepts, from, None, None).await?;
    let mut out = json!({
        "signature": proof.signature,
        "authorization": proof.authorization,
    });
    if let Some(cert) = proof.session_cert {
        out["sessionCert"] = json!(cert);
    }
    output::success(out);
    Ok(())
}

fn is_valid_evm_address(addr: &str) -> bool {
    addr.starts_with("0x") && addr.len() == 42 && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ── parse_eip155_chain_id ─────────────────────────────────────────

    #[test]
    fn parse_eip155_base() {
        assert_eq!(
            payment_flow::parse_eip155_chain_id("eip155:8453").unwrap(),
            8453
        );
    }

    #[test]
    fn parse_eip155_ethereum() {
        assert_eq!(payment_flow::parse_eip155_chain_id("eip155:1").unwrap(), 1);
    }

    #[test]
    fn parse_eip155_xlayer() {
        assert_eq!(
            payment_flow::parse_eip155_chain_id("eip155:196").unwrap(),
            196
        );
    }

    #[test]
    fn parse_eip155_missing_prefix() {
        let err = payment_flow::parse_eip155_chain_id("8453").unwrap_err();
        assert!(err.to_string().contains("eip155:"));
    }

    #[test]
    fn parse_eip155_wrong_prefix() {
        let err = payment_flow::parse_eip155_chain_id("solana:101").unwrap_err();
        assert!(err.to_string().contains("eip155:"));
    }

    #[test]
    fn parse_eip155_empty() {
        assert!(payment_flow::parse_eip155_chain_id("").is_err());
    }

    #[test]
    fn parse_eip155_non_numeric() {
        let err = payment_flow::parse_eip155_chain_id("eip155:abc").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    #[test]
    fn parse_eip155_negative() {
        let err = payment_flow::parse_eip155_chain_id("eip155:-1").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    #[test]
    fn parse_eip155_overflow() {
        let err = payment_flow::parse_eip155_chain_id("eip155:99999999999999999999").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    // ── CLI argument parsing ──────────────────────────────────────────

    /// Wrapper so clap can parse PaymentCommand as a top-level subcommand.
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: PaymentCommand,
    }

    // ── CLI argument parsing ──────────────────────────────────────────

    #[test]
    fn cli_x402_pay_accepts_and_from() {
        let json = r#"[{"scheme":"aggr_deferred","network":"eip155:196","amount":"1000","payTo":"0xA","asset":"0xB"}]"#;
        let cli = TestCli::parse_from(["test", "x402-pay", "--accepts", json, "--from", "0xPayer"]);
        match cli.command {
            PaymentCommand::X402Pay { accepts, from } => {
                assert_eq!(accepts, json);
                assert_eq!(from.as_deref(), Some("0xPayer"));
            }
            _ => panic!("expected X402Pay"),
        }
    }

    #[test]
    fn cli_x402_pay_accepts_only() {
        let json = r#"[{"network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}]"#;
        let cli = TestCli::parse_from(["test", "x402-pay", "--accepts", json]);
        match cli.command {
            PaymentCommand::X402Pay { accepts, from } => {
                assert_eq!(accepts, json);
                assert_eq!(from, None);
            }
            _ => panic!("expected X402Pay"),
        }
    }

    #[test]
    fn cli_x402_pay_missing_accepts() {
        let result = TestCli::try_parse_from(["test", "x402-pay"]);
        assert!(result.is_err());
    }

    // ── eip3009-sign CLI parsing ─────────────────────────────────────

    #[test]
    fn cli_eip3009_sign_accepts_and_from() {
        let json = r#"[{"scheme":"exact","network":"eip155:8453","amount":"1000000","payTo":"0xA","asset":"0xB","extra":{"name":"USD Coin","version":"2"}}]"#;
        let cli = TestCli::parse_from(["test", "eip3009-sign", "--accepts", json]);
        match cli.command {
            PaymentCommand::Eip3009Sign { accepts } => {
                assert_eq!(accepts, json);
            }
            _ => panic!("expected Eip3009Sign"),
        }
    }

    #[test]
    fn cli_eip3009_sign_no_from_required() {
        let json = r#"[{"network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}]"#;
        let result = TestCli::try_parse_from(["test", "eip3009-sign", "--accepts", json]);
        assert!(result.is_ok(), "eip3009-sign should parse without --from");
    }

    #[test]
    fn cli_eip3009_sign_missing_accepts() {
        let result = TestCli::try_parse_from(["test", "eip3009-sign", "--from", "0xPayer"]);
        assert!(result.is_err());
    }

    // ── default subcommand CLI parsing ────────────────────────────────

    #[test]
    fn cli_default_set_passes_numeric_chain_through() {
        let cli = TestCli::parse_from([
            "test",
            "default",
            "set",
            "--asset",
            "0x1234567890123456789012345678901234567890",
            "--chain",
            "196",
            "--name",
            "USDG",
        ]);
        match cli.command {
            PaymentCommand::Default {
                action:
                    DefaultAction::Set {
                        asset,
                        chain,
                        name,
                        tier,
                    },
            } => {
                assert_eq!(asset, "0x1234567890123456789012345678901234567890");
                assert_eq!(chain, "196");
                assert_eq!(name.as_deref(), Some("USDG"));
                assert_eq!(tier, None);
            }
            _ => panic!("expected Default::Set"),
        }
    }

    #[test]
    fn cli_default_get_and_unset_parse() {
        let cli = TestCli::parse_from(["test", "default", "get"]);
        assert!(matches!(
            cli.command,
            PaymentCommand::Default {
                action: DefaultAction::Get
            }
        ));
        let cli = TestCli::parse_from(["test", "default", "unset"]);
        assert!(matches!(
            cli.command,
            PaymentCommand::Default {
                action: DefaultAction::Unset
            }
        ));
    }

    // ── chain_id_to_caip2 / caip2_to_chain_id ─────────────────────────

    #[test]
    fn chain_id_to_caip2_accepts_numeric_evm_ids() {
        assert_eq!(chain_id_to_caip2("196").unwrap(), "eip155:196");
        assert_eq!(chain_id_to_caip2("1").unwrap(), "eip155:1");
        assert_eq!(chain_id_to_caip2("  8453  ").unwrap(), "eip155:8453");
    }

    #[test]
    fn chain_id_to_caip2_rejects_non_numeric_inputs() {
        assert!(chain_id_to_caip2("xlayer").is_err());
        assert!(chain_id_to_caip2("ethereum").is_err());
        // Pre-formed CAIP-2 is rejected: only plain chain id is accepted.
        assert!(chain_id_to_caip2("eip155:196").is_err());
    }

    #[test]
    fn chain_id_to_caip2_rejects_non_evm_chains() {
        assert!(chain_id_to_caip2("195").is_err()); // TRON
        assert!(chain_id_to_caip2("501").is_err()); // Solana
        assert!(chain_id_to_caip2("607").is_err()); // TON
        assert!(chain_id_to_caip2("784").is_err()); // SUI
    }

    #[test]
    fn chain_id_to_caip2_rejects_empty_and_negative() {
        assert!(chain_id_to_caip2("").is_err());
        assert!(chain_id_to_caip2("   ").is_err());
        assert!(chain_id_to_caip2("-1").is_err());
    }

    #[test]
    fn caip2_to_chain_id_strips_prefix() {
        assert_eq!(caip2_to_chain_id("eip155:196"), "196");
        assert_eq!(caip2_to_chain_id("eip155:1"), "1");
    }

    // ── default set advances pending tiers to confirmed ──────────────

    fn tmp_home(sub: &str) -> std::path::PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(sub);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).ok();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_set_with_tier_basic_promotes_only_basic() {
        use crate::payment_cache::PaymentCache;
        use crate::payment_notify::TierState;

        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_default_set_tier_basic");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let seed = PaymentCache {
            basic_state: TierState::ChargingUnconfirmed,
            premium_state: TierState::ChargingUnconfirmed,
            ..Default::default()
        };
        seed.save().unwrap();

        cmd_default(DefaultAction::Set {
            asset: "0x1234567890123456789012345678901234567890".into(),
            chain: "196".into(),
            name: Some("USDG".into()),
            tier: Some("basic".into()),
        })
        .expect("cmd_default set succeeds");

        let loaded = PaymentCache::load().expect("cache written");
        assert_eq!(loaded.basic_state, TierState::ChargingConfirmed);
        assert_eq!(
            loaded.premium_state,
            TierState::ChargingUnconfirmed,
            "premium must stay Unconfirmed so its prompt still fires"
        );

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn default_set_without_tier_leaves_all_states_untouched() {
        use crate::payment_cache::PaymentCache;
        use crate::payment_notify::TierState;

        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_default_set_no_tier");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let seed = PaymentCache {
            basic_state: TierState::ChargingUnconfirmed,
            premium_state: TierState::ChargingUnconfirmed,
            ..Default::default()
        };
        seed.save().unwrap();

        cmd_default(DefaultAction::Set {
            asset: "0x1234567890123456789012345678901234567890".into(),
            chain: "196".into(),
            name: None,
            tier: None,
        })
        .expect("cmd_default set succeeds");

        let loaded = PaymentCache::load().expect("cache written");
        assert_eq!(loaded.basic_state, TierState::ChargingUnconfirmed);
        assert_eq!(loaded.premium_state, TierState::ChargingUnconfirmed);

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn default_set_rejects_unknown_tier() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_default_set_bad_tier");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let err = cmd_default(DefaultAction::Set {
            asset: "0x1234567890123456789012345678901234567890".into(),
            chain: "196".into(),
            name: None,
            tier: Some("gold".into()),
        })
        .unwrap_err();
        assert!(err.to_string().contains("basic"));

        std::env::remove_var("ONCHAINOS_HOME");
    }
}
