//! Shared identity validation for subscription requests.
//!
//! Subscription commands exist on both the user and ASP sides. Keeping this
//! selector in `task::common` prevents either role module from depending on the
//! other role's implementation details.

use anyhow::{anyhow, Result};

pub(crate) fn select_subscription_agent_id(
    user_agent_id: &str,
    asp_agent_id: &str,
) -> Result<String> {
    for agent_id in [user_agent_id, asp_agent_id] {
        let agent_id = agent_id.trim();
        if !agent_id.is_empty() {
            return Ok(agent_id.to_string());
        }
    }
    Err(anyhow!("agenticId is required for subscription requests"))
}

#[cfg(test)]
mod tests {
    use super::select_subscription_agent_id;

    #[test]
    fn rejects_blank_user_and_asp_ids() {
        let error = select_subscription_agent_id("   ", "")
            .expect_err("subscription requests require a usable agenticId");

        assert!(error.to_string().contains("agenticId is required"));
    }

    #[test]
    fn falls_back_to_asp_identity() {
        let selected = select_subscription_agent_id("", "  asp-5254  ")
            .expect("ASP identity should authorize subscription requests");

        assert_eq!(selected, "asp-5254");
    }
}
