pub const ERR_NOT_LOGGED_IN: &str = "not logged in";

/// Shared error handler for API responses that may require user confirmation.
///
/// - code=81362 and !force → return CliConfirming (needs user confirmation)
/// - other ApiCodeError → extract msg as plain error
/// - non-ApiCodeError → pass through
pub(crate) fn handle_confirming_error(e: anyhow::Error, force: bool) -> anyhow::Error {
    match e.downcast::<crate::wallet_api::ApiCodeError>() {
        Ok(api_err) => {
            if !force && api_err.code == "81362" {
                crate::output::CliConfirming {
                    message: api_err.msg,
                    next: "If the user confirms, re-run the same command with --force flag appended to proceed.".to_string(),
                }
                .into()
            } else {
                anyhow::anyhow!("{}", api_err.msg)
            }
        }
        Err(e) => e,
    }
}
