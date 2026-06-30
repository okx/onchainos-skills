//! Task "in-progress" query — okx-ai-guide node 5a.
//!
//! Wraps `POST /priapi/v1/aieco/task/inProgress` — the query endpoint shared by
//! Agent CLI + C-end Web. Given up to 20 `agentIds`, the backend validates the
//! caller→agent binding (via the JWT uid) and returns in-progress work classified
//! by role: `buyerTasks` / `providerTasks` / `evaluatorDisputes`.
//!
//! **Auth**: requires a JWT (login via `onchainos wallet login`). Routed through
//! `post_with_identity` for consistency with the other task endpoints; the backend
//! keys the query off the JWT uid + the `agentIds` in the body.

use anyhow::{bail, Result};
use serde_json::json;

use super::network::task_api_client::TaskApiClient;

const IN_PROGRESS_PATH: &str = "/priapi/v1/aieco/task/inProgress";

/// Max `agentIds` per request. The backend rejects more with `param err` (code 1001).
const MAX_AGENT_IDS: usize = 20;

/// Query in-progress tasks & disputes for the given agent IDs. Mirrors the backend
/// spec at §5.3 `POST /priapi/v1/aieco/task/inProgress`.
pub async fn handle_in_progress(client: &mut TaskApiClient, agent_ids: &[String]) -> Result<()> {
    if agent_ids.is_empty() {
        bail!("at least one --agent-ids value is required");
    }
    if agent_ids.len() > MAX_AGENT_IDS {
        bail!(
            "at most {MAX_AGENT_IDS} agent IDs allowed per request (got {})",
            agent_ids.len()
        );
    }

    let body = json!({ "agentIds": agent_ids });
    // The query is keyed off the JWT uid + the body `agentIds`; the single-valued
    // `agenticId` header only needs a representative agent — use the first.
    let header_agent = agent_ids[0].as_str();
    let data = client
        .post_with_identity(IN_PROGRESS_PATH, &body, header_agent)
        .await?;
    crate::output::success(data);
    Ok(())
}
