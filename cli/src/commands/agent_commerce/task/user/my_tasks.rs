//! Unified buyer-side listing for subscription and one-time tasks.

use anyhow::{anyhow, bail, Result};
use clap::ValueEnum;
use serde_json::{json, Map, Value};

use super::subscription_ops::enrich_buyer_subscription_page;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::query as common_query;
use crate::commands::agent_commerce::task::common::state_machine::Status;
use crate::commands::agent_commerce::task::common::AGENT_ROLE_USER;

const TASK_MY_PATH: &str = "/priapi/v1/aieco/task/my";
const SUBSCRIPTION_MY_PATH: &str = "/priapi/v1/aieco/task/subscribe/my";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum MyTaskType {
    #[default]
    All,
    Subscription,
    OneTime,
}

impl MyTaskType {
    fn includes(self, kind: TaskKind) -> bool {
        matches!(self, Self::All)
            || matches!(
                (self, kind),
                (Self::Subscription, TaskKind::Subscription) | (Self::OneTime, TaskKind::OneTime)
            )
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Subscription => "subscription",
            Self::OneTime => "one-time",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskKind {
    Subscription,
    OneTime,
}

#[derive(Debug)]
struct Page {
    list: Vec<Value>,
    total: u64,
    total_no_condition: u64,
    page: u32,
    page_size: u32,
    this_device_id: Option<Value>,
    this_device_name: Option<Value>,
}

impl Page {
    fn from_value(value: Value, kind: TaskKind) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| anyhow!("{} task page must be a JSON object", kind.label()))?;
        let total = object
            .get("total")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("{} task page is missing numeric total", kind.label()))?;
        let total_no_condition = object
            .get("totalNoCondition")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                anyhow!(
                    "{} task page is missing numeric totalNoCondition",
                    kind.label()
                )
            })?;
        let page = object
            .get("page")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| anyhow!("{} task page is missing numeric page", kind.label()))?;
        let page_size = object
            .get("pageSize")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| anyhow!("{} task page is missing numeric pageSize", kind.label()))?;
        let mut list = object
            .get("list")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| anyhow!("{} task page is missing list array", kind.label()))?;
        if kind == TaskKind::OneTime {
            enrich_one_time_status_names(&mut list);
        }
        let (this_device_id, this_device_name) = if kind == TaskKind::Subscription {
            (
                object.get("thisDeviceId").cloned(),
                object.get("thisDeviceName").cloned(),
            )
        } else {
            (None, None)
        };
        Ok(Self {
            list,
            total,
            total_no_condition,
            page,
            page_size,
            this_device_id,
            this_device_name,
        })
    }
}

fn enrich_one_time_status_names(list: &mut [Value]) {
    for row in list {
        let Some(object) = row.as_object_mut() else {
            continue;
        };
        let Some(status_code) = object.get("status").and_then(Value::as_i64) else {
            continue;
        };
        let status_name = match i32::try_from(status_code) {
            Ok(status_code) => Status::from_int(status_code).as_str().to_string(),
            Err(_) => format!("status_{status_code}"),
        };
        object.insert("statusName".to_string(), Value::String(status_name));
    }
}

impl TaskKind {
    fn label(self) -> &'static str {
        match self {
            Self::Subscription => "subscription",
            Self::OneTime => "one-time",
        }
    }
}

fn list_path(kind: TaskKind, page: u32, page_size: u32, status_type: u8) -> String {
    let base = match kind {
        TaskKind::Subscription => SUBSCRIPTION_MY_PATH,
        TaskKind::OneTime => TASK_MY_PATH,
    };
    let backend_status_type = if status_type == 0 { 1 } else { status_type };
    format!("{base}?page={page}&pageSize={page_size}&statusType={backend_status_type}")
}

fn split_summary_and_page(status_type: u8, page: Page) -> Result<(Value, Page, u8)> {
    match status_type {
        0 => Ok((
            json!({"all": page.total_no_condition, "active": page.total}),
            page,
            1,
        )),
        1 => Ok((json!({"active": page.total}), page, 1)),
        2 => Ok((json!({"ended": page.total}), page, 2)),
        _ => bail!("status-type must be 0, 1, or 2; got {status_type}"),
    }
}

fn page_section(kind: TaskKind, status_type: u8, page: Page) -> Value {
    let has_next = u64::from(page.page).saturating_mul(u64::from(page.page_size)) < page.total;
    let mut section = Map::new();
    section.insert("statusType".to_string(), json!(status_type));
    section.insert("page".to_string(), json!(page.page));
    section.insert("pageSize".to_string(), json!(page.page_size));
    section.insert("total".to_string(), json!(page.total));
    section.insert(
        "totalNoCondition".to_string(),
        json!(page.total_no_condition),
    );
    section.insert("hasNext".to_string(), json!(has_next));
    if kind == TaskKind::Subscription {
        section.insert(
            "thisDeviceId".to_string(),
            page.this_device_id.unwrap_or(Value::Null),
        );
        section.insert(
            "thisDeviceName".to_string(),
            page.this_device_name.unwrap_or(Value::Null),
        );
    }
    section.insert("list".to_string(), Value::Array(page.list));
    Value::Object(section)
}

fn compose_output(
    task_type: MyTaskType,
    status_type: u8,
    page: u32,
    page_size: u32,
    subscriptions: Option<Page>,
    one_time: Option<Page>,
) -> Result<Value> {
    if !(0..=2).contains(&status_type) {
        bail!("status-type must be 0, 1, or 2; got {status_type}");
    }
    let mut summary = Map::new();
    let mut output = Map::new();
    output.insert(
        "query".to_string(),
        json!({
            "taskType": task_type.as_str(),
            "statusType": status_type,
            "page": page,
            "pageSize": page_size,
        }),
    );

    if task_type.includes(TaskKind::Subscription) {
        let pages = subscriptions
            .ok_or_else(|| anyhow!("subscription results are required for this task type"))?;
        let (counts, displayed, displayed_status_type) =
            split_summary_and_page(status_type, pages)?;
        summary.insert("subscription".to_string(), counts);
        output.insert(
            "subscriptions".to_string(),
            page_section(TaskKind::Subscription, displayed_status_type, displayed),
        );
    } else if subscriptions.is_some() {
        bail!("subscription results were supplied for an unrequested task type");
    }

    if task_type.includes(TaskKind::OneTime) {
        let pages =
            one_time.ok_or_else(|| anyhow!("one-time results are required for this task type"))?;
        let (counts, displayed, displayed_status_type) =
            split_summary_and_page(status_type, pages)?;
        summary.insert("oneTime".to_string(), counts);
        output.insert(
            "oneTimeTasks".to_string(),
            page_section(TaskKind::OneTime, displayed_status_type, displayed),
        );
    } else if one_time.is_some() {
        bail!("one-time results were supplied for an unrequested task type");
    }

    output.insert("summary".to_string(), Value::Object(summary));
    Ok(Value::Object(output))
}

async fn fetch_page(
    client: &mut TaskApiClient,
    agent_id: &str,
    kind: TaskKind,
    status_type: u8,
    page: u32,
    page_size: u32,
) -> Result<Page> {
    let path = list_path(kind, page, page_size, status_type);
    let raw = match kind {
        TaskKind::Subscription => client
            .get_with_agent_id(&path, agent_id)
            .await
            .map_err(|e| anyhow!("failed to fetch subscription tasks: {e}"))?,
        TaskKind::OneTime => client
            .get_with_agent_id(&path, agent_id)
            .await
            .map_err(|e| anyhow!("failed to fetch one-time tasks: {e}"))?,
    };
    let prepared = match kind {
        TaskKind::Subscription => enrich_buyer_subscription_page(raw, agent_id)?,
        TaskKind::OneTime => raw,
    };
    Page::from_value(prepared, kind)
}

fn require_user_agent_id(agent_id: String) -> Result<String> {
    let agent_id = agent_id.trim();
    if agent_id.is_empty() {
        bail!(
            "no User identity found on this account; register a User identity before listing tasks"
        );
    }
    Ok(agent_id.to_string())
}

pub async fn handle_my_tasks(
    client: &mut TaskApiClient,
    task_type: MyTaskType,
    status_type: u8,
    page: u32,
    page_size: u32,
) -> Result<()> {
    if !(0..=2).contains(&status_type) {
        bail!("status-type must be 0, 1, or 2; got {status_type}");
    }
    let agent_id =
        require_user_agent_id(common_query::resolve_agent_id("", AGENT_ROLE_USER).await)?;
    let subscriptions = if task_type.includes(TaskKind::Subscription) {
        Some(
            fetch_page(
                client,
                &agent_id,
                TaskKind::Subscription,
                status_type,
                page,
                page_size,
            )
            .await?,
        )
    } else {
        None
    };
    let one_time = if task_type.includes(TaskKind::OneTime) {
        Some(
            fetch_page(
                client,
                &agent_id,
                TaskKind::OneTime,
                status_type,
                page,
                page_size,
            )
            .await?,
        )
    } else {
        None
    };
    let data = compose_output(
        task_type,
        status_type,
        page,
        page_size,
        subscriptions,
        one_time,
    )?;
    crate::output::success(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page(
        total: u64,
        total_no_condition: u64,
        page: u32,
        page_size: u32,
        list: Vec<serde_json::Value>,
    ) -> Page {
        Page {
            list,
            total,
            total_no_condition,
            page,
            page_size,
            this_device_id: None,
            this_device_name: None,
        }
    }

    #[test]
    fn paths_include_backend_group_and_pagination() {
        assert_eq!(
            list_path(TaskKind::OneTime, 2, 25, 1),
            "/priapi/v1/aieco/task/my?page=2&pageSize=25&statusType=1"
        );
        assert_eq!(
            list_path(TaskKind::Subscription, 2, 25, 2),
            "/priapi/v1/aieco/task/subscribe/my?page=2&pageSize=25&statusType=2"
        );
        assert_eq!(
            list_path(TaskKind::OneTime, 1, 20, 0),
            "/priapi/v1/aieco/task/my?page=1&pageSize=20&statusType=1"
        );
    }

    #[test]
    fn all_status_uses_all_totals_and_active_pages() {
        let active_subscription_page = Page::from_value(
            json!({
                "total": 3,
                "totalNoCondition": 12,
                "page": 1,
                "pageSize": 20,
                "list": [{"jobId": "sub-1"}]
            }),
            TaskKind::Subscription,
        )
        .unwrap();
        let active_one_time_page = Page::from_value(
            json!({
                "total": 2,
                "totalNoCondition": 8,
                "page": 1,
                "pageSize": 20,
                "list": [{"jobId": "task-1"}]
            }),
            TaskKind::OneTime,
        )
        .unwrap();
        let output = compose_output(
            MyTaskType::All,
            0,
            1,
            20,
            Some(active_subscription_page),
            Some(active_one_time_page),
        )
        .unwrap();

        assert_eq!(
            output["summary"]["subscription"],
            json!({"all": 12, "active": 3})
        );
        assert_eq!(output["summary"]["oneTime"], json!({"all": 8, "active": 2}));
        assert_eq!(output["subscriptions"]["statusType"], 1);
        assert_eq!(output["oneTimeTasks"]["statusType"], 1);
        assert_eq!(output["oneTimeTasks"]["list"][0]["jobId"], "task-1");
    }

    #[test]
    fn sections_preserve_backend_totals_and_pagination() {
        let backend_page = Page::from_value(
            json!({
                "total": 5,
                "totalNoCondition": 12,
                "page": 1,
                "pageSize": 2,
                "list": [{"jobId": "task-21"}]
            }),
            TaskKind::OneTime,
        )
        .unwrap();

        let output =
            compose_output(MyTaskType::OneTime, 1, 99, 99, None, Some(backend_page)).unwrap();

        assert_eq!(output["oneTimeTasks"]["total"], 5);
        assert_eq!(output["oneTimeTasks"]["totalNoCondition"], 12);
        assert_eq!(output["oneTimeTasks"]["page"], 1);
        assert_eq!(output["oneTimeTasks"]["pageSize"], 2);
        assert_eq!(output["oneTimeTasks"]["hasNext"], true);
    }

    #[test]
    fn active_status_reports_active_total_and_independent_has_next() {
        let output = compose_output(
            MyTaskType::All,
            1,
            2,
            2,
            Some(page(5, 9, 2, 2, vec![json!({"jobId": "sub-3"})])),
            Some(page(4, 7, 2, 2, vec![json!({"jobId": "task-3"})])),
        )
        .unwrap();

        assert_eq!(output["summary"]["subscription"], json!({"active": 5}));
        assert_eq!(output["summary"]["oneTime"], json!({"active": 4}));
        assert_eq!(output["subscriptions"]["hasNext"], true);
        assert_eq!(output["oneTimeTasks"]["hasNext"], false);
    }

    #[test]
    fn terminal_subscription_query_omits_one_time_sections() {
        let output = compose_output(
            MyTaskType::Subscription,
            2,
            1,
            20,
            Some(Page {
                list: vec![],
                total: 0,
                total_no_condition: 12,
                page: 1,
                page_size: 20,
                this_device_id: Some(json!("device-1")),
                this_device_name: Some(json!("MacBook Pro")),
            }),
            None,
        )
        .unwrap();

        assert_eq!(output["summary"]["subscription"], json!({"ended": 0}));
        assert!(output["summary"].get("oneTime").is_none());
        assert!(output.get("oneTimeTasks").is_none());
        assert_eq!(output["subscriptions"]["thisDeviceId"], "device-1");
        assert_eq!(output["subscriptions"]["thisDeviceName"], "MacBook Pro");
    }

    #[test]
    fn one_time_query_adds_canonical_status_names_and_preserves_backend_fields() {
        let backend_page = Page::from_value(
            json!({
                "total": 4,
                "totalNoCondition": 4,
                "page": 1,
                "pageSize": 20,
                "list": [
                    {"jobId": "task-init", "status": -1},
                    {"jobId": "task-completed", "status": 6},
                    {"jobId": "task-refunded", "status": 9},
                    {"jobId": "task-future", "status": 17, "futureField": {"kept": true}}
                ]
            }),
            TaskKind::OneTime,
        )
        .unwrap();
        let output =
            compose_output(MyTaskType::OneTime, 2, 1, 20, None, Some(backend_page)).unwrap();

        let rows = output["oneTimeTasks"]["list"].as_array().unwrap();
        assert_eq!(rows[0]["statusName"], "init");
        assert_eq!(rows[1]["statusName"], "completed");
        assert_eq!(rows[2]["statusName"], "failed");
        assert_eq!(rows[3]["statusName"], "status_17");
        assert_eq!(rows[3]["futureField"], json!({"kept": true}));
        assert!(output.get("subscriptions").is_none());
    }

    #[test]
    fn parse_page_requires_backend_total() {
        let error = Page::from_value(json!({"list": []}), TaskKind::OneTime)
            .unwrap_err()
            .to_string();
        assert!(error.contains("total"));
    }

    #[test]
    fn missing_user_identity_returns_an_actionable_error() {
        let error = require_user_agent_id("  ".to_string())
            .unwrap_err()
            .to_string();
        assert!(error.contains("User identity"));
        assert!(error.contains("register"));
    }
}
