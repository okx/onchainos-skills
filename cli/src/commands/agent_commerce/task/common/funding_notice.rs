use anyhow::{bail, Result};
use clap::{Args, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, ValueEnum)]
pub enum FundingNoticeFormat {
    Json,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum FundingNoticeReason {
    #[value(name = "task-payment")]
    TaskPayment,
    #[value(name = "payment-402", alias = "payment402")]
    Payment402,
    #[value(name = "dispute-bond")]
    DisputeBond,
    #[value(name = "subscription")]
    Subscription,
}

#[derive(Args, Debug)]
pub struct FundingNoticeArgs {
    #[arg(long)]
    pub chain: String,
    #[arg(long)]
    pub currency: String,
    #[arg(long)]
    pub shortfall: String,
    #[arg(long = "deposit-address")]
    pub deposit_address: String,
    #[arg(long)]
    pub required: Option<String>,
    #[arg(long)]
    pub available: Option<String>,
    #[arg(long = "deposit-chain")]
    pub deposit_chain: Option<String>,
    #[arg(long, value_enum, default_value_t = FundingNoticeReason::TaskPayment)]
    pub reason: FundingNoticeReason,
    #[arg(long, value_enum, default_value_t = FundingNoticeFormat::Json)]
    pub format: FundingNoticeFormat,
    #[arg(long = "notify-user")]
    pub notify_user: bool,
    /// Already-localized content to send with --notify-user.
    #[arg(long)]
    pub content: Option<String>,
    #[arg(long = "image-dir", hide = true)]
    pub image_dir: Option<PathBuf>,
}

struct FundingNoticeInput {
    chain: String,
    currency: String,
    shortfall: String,
    deposit_address: String,
    required: Option<String>,
    available: Option<String>,
    deposit_chain: String,
    reason: FundingNoticeReason,
    image_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FundingNoticeOutput {
    content_canonical: String,
    fallback_content_canonical: String,
    image_path: Option<String>,
    markdown_image: Option<String>,
    terminal_qr: Option<String>,
    display_mode: String,
    deposit_address: String,
    chain: String,
    deposit_chain: String,
    currency: String,
    shortfall: String,
    required: Option<String>,
    available: Option<String>,
    reason: String,
    must_localize: bool,
    must_notify_with_image_path: bool,
    must_run_notify_command: bool,
    must_render_markdown_image_below_first_option: bool,
    must_repeat_in_final_response: bool,
    forbid_funding_summary: bool,
    display_policy: String,
    end_turn: bool,
    notify_command: Option<String>,
    notify_command_args: Option<Vec<String>>,
}

pub fn funding_display_mode() -> &'static str {
    crate::qr::display_mode()
}

pub fn funding_notice_command(warning: &serde_json::Value, reason: &str) -> Option<String> {
    let chain = warning["chain"].as_str().unwrap_or("XLayer");
    let currency = warning["currency"].as_str()?;
    let shortfall = warning["shortfall"].as_str()?;
    let deposit_address = warning["depositAddress"].as_str()?;
    let mut parts = vec![
        "onchainos".to_string(),
        "agent".to_string(),
        "funding-notice".to_string(),
        "--chain".to_string(),
        chain.to_string(),
        "--currency".to_string(),
        currency.to_string(),
        "--shortfall".to_string(),
        shortfall.to_string(),
        "--deposit-address".to_string(),
        deposit_address.to_string(),
    ];
    for (flag, key) in [
        ("--available", "available"),
        ("--required", "required"),
        ("--deposit-chain", "depositChain"),
    ] {
        if let Some(value) = warning[key].as_str().filter(|value| !value.is_empty()) {
            parts.push(flag.to_string());
            parts.push(value.to_string());
        }
    }
    parts.extend([
        "--reason".to_string(),
        reason.to_string(),
        "--format".to_string(),
        "json".to_string(),
    ]);
    Some(parts.join(" "))
}

pub fn funding_blocked_envelope(
    warning: &serde_json::Value,
    reason: &str,
    action: &str,
) -> serde_json::Value {
    let funding_notice_command = funding_notice_command(warning, reason);
    let must_run_funding_notice = funding_notice_command.is_some();
    let funding_display_mode = funding_display_mode();
    let image_notify = must_run_funding_notice && funding_display_mode == "image-notify";
    serde_json::json!({
        "blocked": true,
        "blockedReason": "insufficient-balance",
        "submitted": false,
        "balanceWarning": warning.clone(),
        "mustRunFundingNotice": must_run_funding_notice,
        "fundingNoticeCommand": funding_notice_command,
        "fundingDisplayMode": funding_display_mode,
        "mustRunNotifyCommand": image_notify,
        "mustRenderMarkdownImageBelowFirstOption": image_notify,
        "mustRepeatInFinalResponse": true,
        "forbidFundingSummary": true,
        "finalResponsePolicy": "Final response must repeat the full localized funding notice with all four funding options; put markdownImage under option 1 when present; never summarize.",
        "platformPolicy": if !must_run_funding_notice {
            "Funding notice unavailable: show balanceWarning, explain deposit address is missing, then end turn."
        } else if image_notify {
            "Non-TTY: run fundingNoticeCommand, then notifyCommandArgs for PNG QR, then put markdownImage under option 1 in final."
        } else {
            "TTY: run fundingNoticeCommand, show terminalQr and full notice; do not claim PNG was sent."
        },
        "resumeAction": "After the user says topped up, re-enter the owning Reference and run its fresh read-only balance or prepare check. Never rerun a saved write command directly.",
        "guidance": if must_run_funding_notice {
            format!("{action} was blocked by insufficient balance. Save the current business context. Run fundingNoticeCommand, then follow its displayMode. End turn.")
        } else {
            format!("{action} was blocked by insufficient balance. Save the current business context. Show balanceWarning and missing deposit address. End turn.")
        },
    })
}

pub fn execute(args: FundingNoticeArgs) -> Result<()> {
    let format = args.format.clone();
    let notify_user = args.notify_user;
    let content = args.content.clone();
    let input = FundingNoticeInput::try_from(args)?;
    let (notice, image_path) = build_funding_notice(input)?;

    if notify_user {
        let content = content.as_deref().expect("checked by input conversion");
        let image_path = image_path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("--notify-user requires an image-notify display path")
        })?;
        super::okx_a2a::user_notify(content, Some(image_path), true)?;
        return Ok(());
    }

    match format {
        FundingNoticeFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true,
                    "data": notice,
                }))?
            );
        }
    }
    Ok(())
}

fn build_funding_notice(
    input: FundingNoticeInput,
) -> Result<(FundingNoticeOutput, Option<PathBuf>)> {
    // Delegate QR rendering, display-mode resolution, PNG write, markdown, and
    // notify argv to the Common QR module (spec Appendix B). `build_qr_output`
    // never fails: on any encode/write error it degrades to an address-only
    // `QrOutput` carrying no QR fields (FR-6), so the notice is always produced.
    let qr = crate::qr::build_qr_output(&input.deposit_address, input.image_dir.as_deref());
    Ok(build_funding_notice_from_qr(input, qr))
}

/// Map a Common QR [`crate::qr::QrOutput`] onto the Agent-Commerce funding notice.
///
/// The QR-derived fields (`terminal_qr`, `image_path`, `markdown_image`,
/// `notify_command_args`, `display_mode`) come straight from `qr`; every other
/// field is funding-notice business copy or the notify-command protocol owned
/// here (`notify_command` shell string, `must_*` flags, `display_policy`). Whether
/// the notice runs the image-notify protocol is gated on a PNG actually being
/// written (`qr.image_path`), so the FR-6 degrade case (image mode, encode failed)
/// coherently falls back to the address-only text path with no notify command.
fn build_funding_notice_from_qr(
    input: FundingNoticeInput,
    qr: crate::qr::QrOutput,
) -> (FundingNoticeOutput, Option<PathBuf>) {
    let image_path = qr.image_path.as_deref().map(PathBuf::from);
    let image_notify = image_path.is_some();
    let content_canonical = render_content(&input);
    let fallback_content_canonical = render_fallback_content(&input);
    let notify_command = qr.image_path.as_ref().map(|path| {
        format!(
            "onchainos agent user-notify --content \"$ONCHAINOS_FUNDING_NOTICE_CONTENT\" --image-path {}",
            shell_quote(path)
        )
    });

    let notice = FundingNoticeOutput {
        content_canonical,
        fallback_content_canonical,
        image_path: qr.image_path,
        markdown_image: qr.markdown_image,
        terminal_qr: qr.terminal_qr,
        display_mode: qr.display_mode,
        deposit_address: input.deposit_address,
        chain: input.chain,
        deposit_chain: input.deposit_chain,
        currency: input.currency,
        shortfall: input.shortfall,
        required: input.required,
        available: input.available,
        reason: reason_name(&input.reason).to_string(),
        must_localize: true,
        must_notify_with_image_path: image_notify,
        must_run_notify_command: image_notify,
        must_render_markdown_image_below_first_option: image_notify,
        must_repeat_in_final_response: true,
        forbid_funding_summary: true,
        display_policy: if image_notify {
            "Non-TTY: run notifyCommandArgs for PNG QR, put markdownImage under option 1, then repeat the full localized notice in final; never summarize."
        } else {
            "TTY: show terminalQr and the full localized notice; never summarize or claim PNG was sent."
        }
        .to_string(),
        end_turn: true,
        notify_command,
        notify_command_args: qr.notify_command_args,
    };
    (notice, image_path)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl TryFrom<FundingNoticeArgs> for FundingNoticeInput {
    type Error = anyhow::Error;

    fn try_from(args: FundingNoticeArgs) -> Result<Self> {
        let chain = required_arg("--chain", args.chain)?;
        let currency = required_arg("--currency", args.currency)?;
        let shortfall = required_arg("--shortfall", args.shortfall)?;
        let deposit_address = required_arg("--deposit-address", args.deposit_address)?;
        if args.notify_user
            && args
                .content
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
        {
            bail!("--notify-user requires --content with already-localized text");
        }
        Ok(Self {
            deposit_chain: args
                .deposit_chain
                .map(|value| required_arg("--deposit-chain", value))
                .transpose()?
                .unwrap_or_else(|| chain.clone()),
            chain,
            currency,
            shortfall,
            deposit_address,
            required: args.required,
            available: args.available,
            reason: args.reason,
            image_dir: args.image_dir,
        })
    }
}

fn required_arg(name: &str, value: String) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{name} must not be empty");
    }
    Ok(trimmed.to_string())
}

fn render_content(input: &FundingNoticeInput) -> String {
    let mut lines = vec![format!(
        "Insufficient {} balance on {}: shortfall {} {}.",
        input.currency, input.chain, input.shortfall, input.currency
    )];
    if let Some(available) = &input.available {
        lines.push(format!("Available: {available} {}.", input.currency));
    }
    if let Some(required) = &input.required {
        lines.push(format!("Required: {required} {}.", input.currency));
    }
    lines.extend([
        String::new(),
        format!("Deposit address: {}", input.deposit_address),
        format!("Deposit network: {}", input.deposit_chain),
        String::new(),
        "Funding options:".to_string(),
        format!(
            "1. Scan and deposit — send {} directly to the address above on {}.",
            input.currency, input.deposit_chain
        ),
        format!(
            "2. Swap — swap <token> to {} {} on {}.",
            input.shortfall, input.currency, input.chain
        ),
        format!(
            "3. Bridge — bridge {} {} from <chain> to {}.",
            input.shortfall, input.currency, input.chain
        ),
        format!(
            "4. Withdraw from OKX — withdraw {} to the address above using the {} network.",
            input.currency, input.deposit_chain
        ),
        String::new(),
        gas_line(input),
        String::new(),
        "After topping up, tell me \"I topped up\".".to_string(),
    ]);
    lines.join("\n")
}

fn render_fallback_content(input: &FundingNoticeInput) -> String {
    format!(
        "QR image could not be attached. Deposit {} to {} on {}. After topping up, tell me \"I topped up\".",
        input.currency, input.deposit_address, input.deposit_chain
    )
}

fn gas_line(input: &FundingNoticeInput) -> String {
    if is_x_layer(&input.chain) || is_x_layer(&input.deposit_chain) {
        "Gas is paid by the platform; no OKB or other native token is required.".to_string()
    } else {
        "Ensure the wallet meets the network gas requirements.".to_string()
    }
}

/// Chain labels come from multiple surfaces (`XLayer`, `X Layer`, `x-layer`).
/// Normalize separators before applying the X Layer gas-sponsorship rule.
fn is_x_layer(chain: &str) -> bool {
    chain
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .eq("xlayer".chars())
}

fn reason_name(reason: &FundingNoticeReason) -> &'static str {
    match reason {
        FundingNoticeReason::TaskPayment => "task-payment",
        FundingNoticeReason::Payment402 => "payment-402",
        FundingNoticeReason::DisputeBond => "dispute-bond",
        FundingNoticeReason::Subscription => "subscription",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_input() -> FundingNoticeInput {
        FundingNoticeInput {
            chain: "XLayer".to_string(),
            currency: "USDT".to_string(),
            shortfall: "0.01".to_string(),
            deposit_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
            required: Some("0.01".to_string()),
            available: Some("0".to_string()),
            deposit_chain: "XLayer".to_string(),
            reason: FundingNoticeReason::TaskPayment,
            image_dir: None,
        }
    }

    #[test]
    fn x_layer_gas_sponsorship_accepts_display_name_variants() {
        for chain in ["XLayer", "X Layer", "x-layer", "x_layer"] {
            let mut input = test_input();
            input.chain = chain.to_string();
            input.deposit_chain = chain.to_string();
            assert_eq!(
                gas_line(&input),
                "Gas is paid by the platform; no OKB or other native token is required."
            );
        }
    }

    #[test]
    fn non_x_layer_keeps_generic_gas_requirement() {
        let mut input = test_input();
        input.chain = "Ethereum".to_string();
        input.deposit_chain = "Ethereum".to_string();
        assert_eq!(
            gas_line(&input),
            "Ensure the wallet meets the network gas requirements."
        );
    }

    /// A terminal-unicode `QrOutput` exactly as `qr::build_qr_output` produces in
    /// TTY mode: a real Unicode block, no image fields.
    fn terminal_qr_output(address: &str) -> crate::qr::QrOutput {
        crate::qr::QrOutput {
            requested_format: "auto".to_string(),
            resolved_format: Some("unicode".to_string()),
            display_mode: "terminal-unicode".to_string(),
            terminal_qr: Some(
                crate::qr::render_address_qr_unicode(address).expect("unicode render"),
            ),
            image_path: None,
            mime_type: None,
            markdown_image: None,
            notify_command_args: None,
        }
    }

    /// An image-notify `QrOutput` mirroring `qr::build_qr_output`'s image-mode
    /// shape (a written PNG at `png_path`, markdown + notify argv populated).
    fn image_qr_output(png_path: &str) -> crate::qr::QrOutput {
        crate::qr::QrOutput {
            requested_format: "auto".to_string(),
            resolved_format: Some("png".to_string()),
            display_mode: "image-notify".to_string(),
            terminal_qr: None,
            image_path: Some(png_path.to_string()),
            mime_type: Some("image/png".to_string()),
            markdown_image: Some(format!("![QR Code](<{png_path}>)")),
            notify_command_args: Some(vec![
                "onchainos".to_string(),
                "agent".to_string(),
                "user-notify".to_string(),
                "--content".to_string(),
                "<localized content>".to_string(),
                "--image-path".to_string(),
                png_path.to_string(),
            ]),
        }
    }

    // Terminal-unicode QrOutput → terminal_qr carried, image fields empty, no
    // notify command; business copy + policy intact.
    #[test]
    fn terminal_unicode_qr_maps_to_funding_output() {
        let input = test_input();
        let addr = input.deposit_address.clone();
        let (notice, image_path) = build_funding_notice_from_qr(input, terminal_qr_output(&addr));

        assert!(image_path.is_none());
        assert_eq!(notice.display_mode, "terminal-unicode");
        assert!(notice.image_path.is_none());
        assert!(notice.markdown_image.is_none());
        assert!(
            notice
                .terminal_qr
                .as_deref()
                .unwrap_or_default()
                .contains('█')
        );
        assert!(!notice.must_notify_with_image_path);
        assert!(!notice.must_run_notify_command);
        assert!(!notice.must_render_markdown_image_below_first_option);
        assert!(notice.notify_command.is_none());
        assert!(notice.notify_command_args.is_none());
        // Business copy / notify protocol preserved.
        assert!(notice.content_canonical.contains(&addr));
        assert!(notice.must_repeat_in_final_response);
        assert!(notice.forbid_funding_summary);
        assert!(notice.display_policy.contains("show terminalQr"));
    }

    // Image-notify QrOutput → image_path/markdownImage/notifyCommandArgs carried,
    // must_* flags + display_policy business fields unchanged, notify command shell
    // string rebuilt from the QR image path.
    #[test]
    fn image_notify_qr_maps_to_funding_output() {
        let input = test_input();
        let addr = input.deposit_address.clone();
        let png_path = "/tmp/onchainos-funding-qr-42-99.png";
        let (notice, image_path) = build_funding_notice_from_qr(input, image_qr_output(png_path));

        assert_eq!(notice.display_mode, "image-notify");
        assert_eq!(image_path.as_deref(), Some(std::path::Path::new(png_path)));
        assert_eq!(notice.image_path.as_deref(), Some(png_path));
        assert!(notice.terminal_qr.is_none());
        assert!(notice.must_notify_with_image_path);
        assert!(notice.must_run_notify_command);
        assert!(notice.must_render_markdown_image_below_first_option);
        assert!(notice.must_repeat_in_final_response);
        assert!(notice.forbid_funding_summary);
        assert!(notice
            .markdown_image
            .as_deref()
            .is_some_and(|value| value.contains("onchainos-funding-qr-")));
        assert!(notice
            .notify_command_args
            .as_ref()
            .is_some_and(|args| args.iter().any(|a| a == "--image-path")));
        assert!(
            notice
                .display_policy
                .contains("put markdownImage under option 1")
        );
        assert!(
            notice
                .notify_command
                .as_deref()
                .unwrap_or_default()
                .contains("--image-path")
        );
        // Business copy preserved.
        assert!(notice.content_canonical.contains(&addr));
    }

    // FR-6: an over-capacity deposit address makes the QR encoder fail; the Common
    // QR module degrades silently, so `build_funding_notice` still returns Ok with
    // the address-only notice and no QR fields — no Err bubbled from the QR step.
    #[test]
    fn qr_encode_failure_still_produces_notice() {
        let mut input = test_input();
        input.deposit_address = format!("0x{}", "a".repeat(8000));
        let addr = input.deposit_address.clone();

        let (notice, image_path) = build_funding_notice(input).expect("notice still produced");

        assert!(image_path.is_none());
        assert!(notice.terminal_qr.is_none());
        assert!(notice.image_path.is_none());
        assert!(notice.markdown_image.is_none());
        assert!(notice.notify_command.is_none());
        assert!(notice.notify_command_args.is_none());
        assert!(!notice.must_notify_with_image_path);
        assert!(!notice.must_run_notify_command);
        // Address text is still present so the user can fund manually.
        assert!(notice.content_canonical.contains(&addr));
    }

    #[test]
    fn blocked_envelope_carries_requested_reason() {
        let warning = serde_json::json!({
            "sufficient": false,
            "chain": "XLayer",
            "currency": "USDT",
            "shortfall": "0.5",
            "available": "0",
            "required": "0.5",
            "depositAddress": "0x1234567890abcdef1234567890abcdef12345678",
            "depositChain": "XLayer"
        });

        let envelope = funding_blocked_envelope(&warning, "dispute-bond", "Dispute bond");
        assert_eq!(envelope["submitted"], serde_json::json!(false));
        assert!(
            envelope["fundingNoticeCommand"]
                .as_str()
                .unwrap()
                .contains("--reason dispute-bond")
        );
        assert_eq!(
            envelope["mustRepeatInFinalResponse"],
            serde_json::json!(true)
        );
        assert!(envelope["mustRenderMarkdownImageBelowFirstOption"].is_boolean());
        assert_eq!(envelope["forbidFundingSummary"], serde_json::json!(true));
        assert!(
            envelope["finalResponsePolicy"]
                .as_str()
                .unwrap()
                .contains("all four funding options")
        );
    }

}
