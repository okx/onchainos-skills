//! Task marketplace search.
//!
//! Wraps `POST /priapi/v1/aieco/task/job/search` — the search-service passthrough endpoint
//! shared by Agent CLI + C-end Web. All filter fields are optional; when omitted the backend
//! returns the whole tasks pool paginated.
//!
//! **Auth**: requires a JWT (login via `onchainos wallet login`). Routed through
//! `post_with_identity` for consistency with other task endpoints — `agenticId` is sent as the
//! caller-provided agent ID header and `sessionCert` is auto-injected if present.

use anyhow::Result;
use clap::ValueEnum;
use serde_json::{json, Map, Value};

use super::network::task_api_client::TaskApiClient;

const SEARCH_PATH: &str = "/priapi/v1/aieco/task/job/search";

/// Allowed values for `task-search --order-by`. Backend accepts only these four enum strings;
/// CLI accepts the snake_case form (e.g. `amount_asc`) and serializes the SCREAMING_SNAKE form
/// (e.g. `AMOUNT_ASC`) to the backend.
#[derive(Clone, Debug, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum TaskSearchOrderBy {
    CreateTimeDesc,
    CreateTimeAsc,
    AmountDesc,
    AmountAsc,
}

impl TaskSearchOrderBy {
    fn to_backend(&self) -> &'static str {
        match self {
            Self::CreateTimeDesc => "CREATE_TIME_DESC",
            Self::CreateTimeAsc => "CREATE_TIME_ASC",
            Self::AmountDesc => "AMOUNT_DESC",
            Self::AmountAsc => "AMOUNT_ASC",
        }
    }
}

/// Search the task marketplace. Mirrors the backend spec at §5.3
/// `POST /priapi/v1/aieco/task/job/search`.
#[allow(clippy::too_many_arguments)]
pub async fn handle_task_search(
    client: &mut TaskApiClient,
    agent_id: &str,
    keyword: Option<&str>,
    amount_min: Option<f64>,
    amount_max: Option<f64>,
    status: &[i32],
    order_by: Option<&TaskSearchOrderBy>,
    create_time_start: Option<i64>,
    create_time_end: Option<i64>,
    page: u32,
    page_size: u32,
) -> Result<()> {
    let mut body = Map::new();
    if let Some(k) = keyword.filter(|s| !s.is_empty()) {
        body.insert("keyword".into(), json!(k));
    }
    if let Some(v) = amount_min {
        body.insert("currencyAmountMin".into(), json!(v));
    }
    if let Some(v) = amount_max {
        body.insert("currencyAmountMax".into(), json!(v));
    }
    if !status.is_empty() {
        body.insert("status".into(), json!(status));
    }
    if let Some(o) = order_by {
        body.insert("orderBy".into(), json!(o.to_backend()));
    }
    if let Some(t) = create_time_start {
        body.insert("createTimeStart".into(), json!(t));
    }
    if let Some(t) = create_time_end {
        body.insert("createTimeEnd".into(), json!(t));
    }
    body.insert("page".into(), json!(page));
    body.insert("pageSize".into(), json!(page_size));

    let data = client
        .post_with_identity(SEARCH_PATH, &Value::Object(body), agent_id)
        .await?;
    crate::output::success(data);
    Ok(())
}
