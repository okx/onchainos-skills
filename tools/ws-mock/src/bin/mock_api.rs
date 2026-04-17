#![allow(dead_code)]
//! mock-api: 任务后端 HTTP mock 服务器
//!
//! 模拟真实后端 REST API，供 openclaw buyer AI 和 mock-seller/mock-buyer 工具调用。
//! mock 直接在内存更新状态（跳过链上 + Kafka 流程）。
//!
//! 默认监听 http://127.0.0.1:9001
//!
//! 接口：
//!   POST /api/v1/task/create                  — 创建任务
//!   POST /api/v1/task/{jobId}/apply           — Provider 报名
//!   POST /api/v1/task/{jobId}/accept          — Client 接受 Provider + 注资
//!   POST /api/v1/task/{jobId}/submit          — Provider 提交交付
//!   POST /api/v1/task/{jobId}/complete        — Client 验收通过
//!   POST /api/v1/task/{jobId}/refuse          — Client 拒绝验收
//!   POST /api/v1/task/{jobId}/close           — Client 关闭任务
//!   POST /api/v1/task/{jobId}/setVisibility   — 转为公开
//!   POST /api/v1/task/{jobId}/dispute         — Provider 申请仲裁
//!   POST /api/v1/task/{jobId}/match           — 推荐 Provider 列表
//!   GET  /api/v1/task/{jobId}                 — 任务详情
//!   GET  /api/v1/task/list                    — 公开任务列表
//!   GET  /api/v1/tasks/my                     — 我的任务
//!   GET  /api/v1/task/{jobId}/providerConfirmStatus — Provider 报名状态
//!   GET  /api/v1/task/hasInProgress           — 是否有进行中任务

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const API_PORT: u16 = 9001;

// ─── 任务状态 ────────────────────────────────────────────────────────────────

const S_INIT: i32 = -1;
const S_OPEN: i32 = 0;
const S_ACCEPTED: i32 = 1;
const S_SUBMITTED: i32 = 2;
const S_REFUSED: i32 = 3;
const S_DISPUTED: i32 = 4;
const S_COMPLETE: i32 = 5;
#[allow(dead_code)]
const S_REJECTED: i32 = 6;
const S_CLOSE: i32 = 7;
#[allow(dead_code)]
const S_EXPIRED: i32 = 8;

fn status_str(s: i32) -> &'static str {
    match s {
        S_INIT     => "init",
        S_OPEN     => "open",
        S_ACCEPTED => "accepted",
        S_SUBMITTED => "submitted",
        S_REFUSED  => "refused",
        S_DISPUTED => "disputed",
        S_COMPLETE => "complete",
        6          => "rejected",
        S_CLOSE    => "close",
        8          => "expired",
        _          => "unknown",
    }
}

// ─── 数据模型 ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub job_id: String,
    pub title: String,
    pub description: String,
    pub description_summary: String,
    pub token_address: String,
    pub token_amount: String,
    /// 0=escrow, 1=non_escrow, 2=x402, null=未设置
    pub payment_type: Option<i32>,
    /// 0=Private, 1=Public
    pub open_type: i32,
    /// -1..8
    pub status: i32,
    pub status_str: String,
    pub chain_id: i32,
    pub min_credit_score: Option<f64>,
    pub designated_provider: Option<String>,
    pub buyer_agent_address: String,
    pub buyer_agent_id: String,
    pub provider_agent_address: Option<String>,
    pub provider_agent_id: Option<String>,
    pub group_id: Option<String>,
    pub evaluator_address: Option<String>,
    pub expire_config: serde_json::Value,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderConfirm {
    provider_address: String,
    provider_agent_id: String,
    token_address: String,
    token_amount: String,
}

#[derive(Clone)]
struct AppState {
    tasks: Arc<DashMap<String, TaskRecord>>,
    /// job_id → 已报名的 providers
    confirms: Arc<DashMap<String, Vec<ProviderConfirm>>>,
    /// Path to the JSON file used for task persistence
    persist_path: Arc<String>,
}

// ─── 磁盘持久化 ───────────────────────────────────────────────────────────────

/// Load tasks from `path` into `tasks`. Returns the number of tasks loaded.
fn load_tasks_from_disk(path: &str, tasks: &DashMap<String, TaskRecord>) -> usize {
    match std::fs::read_to_string(path) {
        Ok(json) => {
            match serde_json::from_str::<std::collections::HashMap<String, TaskRecord>>(&json) {
                Ok(map) => {
                    let count = map.len();
                    for (k, v) in map { tasks.insert(k, v); }
                    count
                }
                Err(e) => {
                    eprintln!("[{}][mock-api] ⚠  failed to parse {}: {}", now_log(), path, e);
                    0
                }
            }
        }
        Err(_) => 0, // file doesn't exist yet — normal on first run
    }
}

/// Serialize all tasks to `path` (best-effort, logs on error).
fn save_tasks_to_disk(path: &str, tasks: &DashMap<String, TaskRecord>) {
    let map: std::collections::HashMap<String, TaskRecord> = tasks
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect();
    match serde_json::to_string_pretty(&map) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                eprintln!("[{}][mock-api] ⚠  failed to save tasks to {}: {}", now_log(), path, e);
            }
        }
        Err(e) => eprintln!("[{}][mock-api] ⚠  failed to serialize tasks: {}", now_log(), e),
    }
}

// ─── 请求/响应结构 ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTaskReq {
    title: String,
    description: String,
    #[serde(default)]
    description_summary: String,
    #[serde(default = "default_token_addr")]
    payment_token_address: String,
    #[serde(default = "default_amount")]
    payment_token_amount: String,
    #[serde(default = "default_chain")]
    chain_id: i32,
    #[serde(default = "default_expire_config")]
    expire_config: serde_json::Value,
    payment_type: Option<i32>,
    #[serde(default)]
    visibility: i32,
    min_credit_score: Option<f64>,
    designated_provider: Option<String>,
    // mock 字段（真实后端从 JWT 取）
    #[serde(default = "default_buyer_addr")]
    buyer_agent_address: String,
    #[serde(default = "default_buyer_id")]
    buyer_agent_id: String,
}

fn default_token_addr() -> String { "0x779ded0c9e1022225f8e0630b35a9b54be713736".into() }
fn default_amount()     -> String { "100".into() }
fn default_chain()      -> i32    { 196 }
fn default_buyer_addr() -> String { "0xMockBuyer00000000000000000000000000001".into() }
fn default_buyer_id()   -> String { "mock-buyer-agent-001".into() }
fn default_expire_config() -> serde_json::Value {
    serde_json::json!({ "openExpireSec": 86400, "acceptedExpireSec": 259200 })
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ApplyReq {
    #[serde(default)]
    token_amount: String,
    #[serde(default)]
    token_symbol: String,
    // mock 字段
    #[serde(default = "default_seller_addr")]
    provider_address: String,
    #[serde(default = "default_seller_id")]
    provider_agent_id: String,
}
fn default_seller_addr() -> String { "0xSeller000000000000000000000000000000001".into() }
fn default_seller_id()   -> String { "mock-seller-agent-001".into() }

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AcceptReq {
    #[serde(default = "default_seller_addr")]
    provider_address: String,
    #[serde(default = "default_seller_id")]
    provider_agent_id: String,
    token_symbol: Option<String>,
    token_amount: Option<String>,
    group_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct VisibilityReq {
    #[serde(default)]
    visibility: i32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DisputeReq {
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskListQuery {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    token_address: Option<String>,
    #[serde(default)]
    min_amount: Option<String>,
    #[serde(default)]
    max_amount: Option<String>,
    #[serde(default)]
    sort_by: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
}
fn default_page()      -> u32 { 1 }
fn default_page_size() -> u32 { 20 }

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MyTasksQuery {
    role: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
    // mock：真实后端从 JWT 取，这里从 query 传
    #[serde(default = "default_buyer_addr")]
    agent_address: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderConfirmQuery {
    provider_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HasInProgressQuery {
    #[serde(default)]
    agent_ids: Option<String>,
    #[serde(default = "default_buyer_addr")]
    agent_address: String,
}

// ─── 工具函数 ─────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Returns `HH:MM:SS` (UTC) for log prefixes.
fn now_log() -> String {
    let s = now_secs();
    let sod = s % 86400;
    format!("{:02}:{:02}:{:02}", sod / 3600, (sod % 3600) / 60, sod % 60)
}

fn now_iso() -> String {
    // 近似 ISO8601，mock 精度足够
    let s = now_secs();
    let days_since_epoch = s / 86400;
    let secs_of_day = s % 86400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let sec = secs_of_day % 60;
    // 简单推算年月日（2026-01-01 = 20454 days since 1970-01-01）
    let year = 1970 + days_since_epoch / 365;
    let day_of_year = days_since_epoch % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{sec:02}Z")
}

static JOB_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1000);

fn gen_job_id() -> String {
    let n = JOB_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("0x{n:x}")
}

fn ok(data: serde_json::Value) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "code": 0, "data": data }))
}

fn err(code: i32, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    let status = if code == 2001 { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
    (status, Json(serde_json::json!({ "code": code, "msg": msg, "data": null })))
}

fn mock_uop_hash() -> String {
    format!("0x{:x}", now_secs())
}

fn mock_calldata() -> String {
    format!("0x{:0>128}", now_secs())
}

fn set_status(task: &mut TaskRecord, s: i32) {
    task.status = s;
    task.status_str = status_str(s).to_string();
    task.update_time = now_iso();
}

// ─── 路由处理 ─────────────────────────────────────────────────────────────────

/// POST /api/v1/task/create
async fn create_task(
    State(st): State<AppState>,
    Json(req): Json<CreateTaskReq>,
) -> impl IntoResponse {
    if req.title.is_empty() || req.title.len() > 256 {
        return err(1001, "title required, max 256 chars").into_response();
    }
    if req.description.is_empty() {
        return err(1001, "description required").into_response();
    }

    let job_id = gen_job_id();
    let task = TaskRecord {
        job_id:                job_id.clone(),
        title:                 req.title,
        description:           req.description.clone(),
        description_summary:   if req.description_summary.is_empty() {
                                   req.description.chars().take(200).collect()
                               } else { req.description_summary },
        token_address:         req.payment_token_address,
        token_amount:          req.payment_token_amount,
        payment_type:          req.payment_type,
        open_type:             req.visibility,
        status:                S_OPEN,
        status_str:            "open".into(),
        chain_id:              req.chain_id,
        min_credit_score:      req.min_credit_score,
        designated_provider:   req.designated_provider,
        buyer_agent_address:   req.buyer_agent_address,
        buyer_agent_id:        req.buyer_agent_id,
        provider_agent_address: None,
        provider_agent_id:     None,
        group_id:              None,
        evaluator_address:     None,
        expire_config:         req.expire_config,
        create_time:           now_iso(),
        update_time:           now_iso(),
    };

    println!("[{}][mock-api] ✓ task created: {} \"{}\"", now_log(), job_id, task.title);
    st.tasks.insert(job_id.clone(), task);
    save_tasks_to_disk(&st.persist_path, &st.tasks);

    ok(serde_json::json!({ "jobId": job_id, "uopHash": mock_uop_hash() })).into_response()
}

/// POST /api/v1/task/{jobId}/apply  — Provider 报名
async fn apply_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
    body: Option<Json<ApplyReq>>,
) -> impl IntoResponse {
    let req = body.map(|b| b.0).unwrap_or_default();
    match st.tasks.get(&job_id) {
        None => return err(2001, "task not found").into_response(),
        Some(t) if t.status != S_OPEN => return err(2002, "task status must be OPEN").into_response(),
        _ => {}
    }
    let confirm = ProviderConfirm {
        provider_address: req.provider_address.clone(),
        provider_agent_id: req.provider_agent_id.clone(),
        token_address: default_token_addr(),
        token_amount: if req.token_amount.is_empty() { "100".into() } else { req.token_amount },
    };
    println!("[{}][mock-api] ✓ provider applied: job={} provider={}", now_log(), job_id, req.provider_address);
    st.confirms.entry(job_id).or_default().push(confirm);
    ok(serde_json::json!({ "uopHash": mock_uop_hash() })).into_response()
}

/// POST /api/v1/task/{jobId}/accept  — Client 接受 Provider，注资
async fn accept_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
    body: Option<Json<AcceptReq>>,
) -> impl IntoResponse {
    let req = body.map(|b| b.0).unwrap_or_default();
    match st.tasks.get_mut(&job_id) {
        None => return err(2001, "task not found").into_response(),
        Some(mut t) => {
            if t.status != S_OPEN { return err(2002, "task status must be OPEN").into_response(); }
            t.provider_agent_address = Some(req.provider_address.clone());
            t.provider_agent_id = Some(req.provider_agent_id.clone());
            if let Some(gid) = req.group_id { t.group_id = Some(gid); }
            set_status(&mut t, S_ACCEPTED);
            println!("[{}][mock-api] ✓ task accepted: job={} provider={}", now_log(), job_id, req.provider_address);
        }
    }
    ok(serde_json::json!({ "calldata": mock_calldata() })).into_response()
}

/// POST /api/v1/task/{jobId}/submit  — Provider 提交交付
async fn submit_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    match st.tasks.get_mut(&job_id) {
        None => return err(2001, "task not found").into_response(),
        Some(mut t) => {
            if t.status != S_ACCEPTED { return err(2002, "task status must be ACCEPTED").into_response(); }
            set_status(&mut t, S_SUBMITTED);
            println!("[{}][mock-api] ✓ task submitted: job={}", now_log(), job_id);
        }
    }
    ok(serde_json::json!({ "uopHash": mock_uop_hash() })).into_response()
}

/// POST /api/v1/task/{jobId}/complete  — Client 验收通过
async fn complete_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    match st.tasks.get_mut(&job_id) {
        None => return err(2001, "task not found").into_response(),
        Some(mut t) => {
            if t.status != S_SUBMITTED && t.status != S_ACCEPTED {
                return err(2002, "task status must be SUBMITTED or ACCEPTED").into_response();
            }
            set_status(&mut t, S_COMPLETE);
            println!("[{}][mock-api] ✓ task completed: job={}", now_log(), job_id);
        }
    }
    ok(serde_json::json!({ "calldata": mock_calldata() })).into_response()
}

/// POST /api/v1/task/{jobId}/refuse  — Client 拒绝验收
async fn refuse_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    match st.tasks.get_mut(&job_id) {
        None => return err(2001, "task not found").into_response(),
        Some(mut t) => {
            if t.status != S_SUBMITTED { return err(2002, "task status must be SUBMITTED").into_response(); }
            set_status(&mut t, S_REFUSED);
            println!("[{}][mock-api] ✓ task refused: job={}", now_log(), job_id);
        }
    }
    ok(serde_json::json!({ "calldata": mock_calldata() })).into_response()
}

/// POST /api/v1/task/{jobId}/close  — Client 主动关闭
async fn close_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    match st.tasks.get_mut(&job_id) {
        None => return err(2001, "task not found").into_response(),
        Some(mut t) => {
            if t.status != S_OPEN { return err(2002, "task status must be OPEN").into_response(); }
            set_status(&mut t, S_CLOSE);
            println!("[{}][mock-api] ✓ task closed: job={}", now_log(), job_id);
        }
    }
    ok(serde_json::json!({ "uop": mock_uop_hash() })).into_response()
}

/// POST /api/v1/task/{jobId}/setVisibility  — 转为公开
async fn set_visibility(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
    body: Option<Json<VisibilityReq>>,
) -> impl IntoResponse {
    let visibility = body.map(|b| b.0.visibility).unwrap_or(1);
    match st.tasks.get_mut(&job_id) {
        None => return err(2001, "task not found").into_response(),
        Some(mut t) => {
            if t.status != S_OPEN { return err(2002, "task status must be OPEN").into_response(); }
            t.open_type = visibility;
            t.update_time = now_iso();
            println!("[{}][mock-api] ✓ visibility set: job={} open_type={}", now_log(), job_id, visibility);
        }
    }
    ok(serde_json::json!({ "uop": mock_uop_hash() })).into_response()
}

/// POST /api/v1/task/{jobId}/dispute  — Provider 申请仲裁
async fn dispute_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
    body: Option<Json<DisputeReq>>,
) -> impl IntoResponse {
    let reason = body.map(|b| b.0.reason).unwrap_or_default();
    match st.tasks.get_mut(&job_id) {
        None => return err(2001, "task not found").into_response(),
        Some(mut t) => {
            if t.status != S_REFUSED { return err(2002, "task status must be REFUSED").into_response(); }
            set_status(&mut t, S_DISPUTED);
            println!("[{}][mock-api] ✓ task disputed: job={} reason={}", now_log(), job_id, reason);
        }
    }
    ok(serde_json::json!({ "uopHash": mock_uop_hash() })).into_response()
}

/// POST /api/v1/task/{jobId}/match  — 推荐 Provider 列表
async fn match_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    if st.tasks.get(&job_id).is_none() {
        return err(2001, "task not found").into_response();
    }
    let recommendations = serde_json::json!([
        {
            "providerAddress": "0xSeller000000000000000000000000000000001",
            "providerAgentId": "mock-seller-agent-001",
            "matchScore": 92.5,
            "creditScore": 88,
            "capabilitySummary": "专注 Solidity 审计和 DeFi 协议开发，完成率 96%",
            "completedTaskCount": 42
        },
        {
            "providerAddress": "0xSeller000000000000000000000000000000002",
            "providerAgentId": "mock-seller-agent-002",
            "matchScore": 85.0,
            "creditScore": 79,
            "capabilitySummary": "全栈区块链开发，擅长 Rust 和 EVM 合约",
            "completedTaskCount": 18
        }
    ]);
    ok(serde_json::json!({ "recommendations": recommendations })).into_response()
}

/// GET /api/v1/task/{jobId}  — 任务详情
async fn get_task(
    Path(job_id): Path<String>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    // 支持 task-001 / task-002 这类 mock 友好 ID
    match st.tasks.get(&job_id) {
        Some(t) => ok(serde_json::json!({ "task": t.clone() })).into_response(),
        None => err(2001, "task not found").into_response(),
    }
}

/// GET /api/v1/task/list  — 公开任务列表
async fn list_tasks(
    Query(q): Query<TaskListQuery>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    let mut tasks: Vec<TaskRecord> = st.tasks.iter()
        .filter(|e| {
            let t = e.value();
            // 只返回公开任务
            if t.open_type != 1 { return false; }
            if let Some(ref s) = q.status {
                if t.status_str != *s { return false; }
            }
            if let Some(ref addr) = q.token_address {
                if t.token_address != *addr { return false; }
            }
            true
        })
        .map(|e| e.value().clone())
        .collect();

    // 排序
    match q.sort_by.as_deref() {
        Some("amount_asc")  => tasks.sort_by(|a, b| a.token_amount.cmp(&b.token_amount)),
        Some("amount_desc") => tasks.sort_by(|a, b| b.token_amount.cmp(&a.token_amount)),
        _                   => tasks.sort_by(|a, b| b.create_time.cmp(&a.create_time)),
    }

    let total = tasks.len() as u32;
    let page = q.page.max(1);
    let page_size = q.page_size.min(100).max(1);
    let start = ((page - 1) * page_size) as usize;
    let list: Vec<_> = tasks.into_iter().skip(start).take(page_size as usize).collect();

    ok(serde_json::json!({ "total": total, "page": page, "pageSize": page_size, "list": list }))
        .into_response()
}

/// GET /api/v1/tasks/my  — 我的任务
async fn my_tasks(
    Query(q): Query<MyTasksQuery>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    let role = q.role.as_str();
    if role != "client" && role != "provider" {
        return err(1001, "role must be client or provider").into_response();
    }

    let mut tasks: Vec<TaskRecord> = st.tasks.iter()
        .filter(|e| {
            let t = e.value();
            let addr_match = if role == "client" {
                t.buyer_agent_address == q.agent_address
            } else {
                t.provider_agent_address.as_deref() == Some(q.agent_address.as_str())
            };
            if !addr_match { return false; }
            if let Some(ref s) = q.status {
                if t.status_str != *s { return false; }
            }
            true
        })
        .map(|e| e.value().clone())
        .collect();

    tasks.sort_by(|a, b| b.update_time.cmp(&a.update_time));
    let total = tasks.len() as u32;
    let page = q.page.max(1);
    let page_size = q.page_size.min(100).max(1);
    let start = ((page - 1) * page_size) as usize;
    let list: Vec<_> = tasks.into_iter().skip(start).take(page_size as usize).collect();

    ok(serde_json::json!({ "total": total, "page": page, "pageSize": page_size, "list": list }))
        .into_response()
}

/// GET /api/v1/task/{jobId}/providerConfirmStatus
async fn provider_confirm_status(
    Path(job_id): Path<String>,
    Query(q): Query<ProviderConfirmQuery>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    if st.tasks.get(&job_id).is_none() {
        return err(2001, "task not found").into_response();
    }
    let confirms = st.confirms.get(&job_id);
    let found = confirms.as_ref().and_then(|cs| {
        if let Some(ref agent_id) = q.provider_agent_id {
            cs.iter().find(|c| &c.provider_agent_id == agent_id).cloned()
        } else {
            cs.first().cloned()
        }
    });
    match found {
        Some(c) => ok(serde_json::json!({
            "confirmed": true,
            "providerAddress": c.provider_address,
            "providerAgentId": c.provider_agent_id,
            "tokenAddress": c.token_address,
            "tokenAmount": c.token_amount
        })).into_response(),
        None => ok(serde_json::json!({
            "confirmed": false,
            "providerAddress": null,
            "providerAgentId": null,
            "tokenAddress": null,
            "tokenAmount": null
        })).into_response(),
    }
}

/// GET /api/v1/task/hasInProgress
async fn has_in_progress(
    Query(q): Query<HasInProgressQuery>,
    State(st): State<AppState>,
) -> impl IntoResponse {
    let in_progress = st.tasks.iter().any(|e| {
        let t = e.value();
        let is_me = t.buyer_agent_address == q.agent_address
            || t.provider_agent_address.as_deref() == Some(q.agent_address.as_str());
        let active = (S_OPEN..=S_DISPUTED).contains(&t.status);
        is_me && active
    });
    ok(serde_json::json!({ "hasInProgress": in_progress })).into_response()
}

// ─── 预置数据 ─────────────────────────────────────────────────────────────────

fn seed_tasks(tasks: &DashMap<String, TaskRecord>) {
    let seeds = vec![
        TaskRecord {
            job_id: "task-001".into(),
            title: "Solidity 合约安全审计".into(),
            description: "审计目标合约地址 0xABC123...，重点检查重入攻击（reentrancy）、权限控制（access control）和整数溢出漏洞。要求提交详细的审计报告，包含风险评级和修复建议。".into(),
            description_summary: "EVM 合约安全审计，重点重入攻击和权限控制检查".into(),
            token_address: "0x779ded0c9e1022225f8e0630b35a9b54be713736".into(),
            token_amount: "500".into(),
            payment_type: Some(0),
            open_type: 1,
            status: S_OPEN,
            status_str: "open".into(),
            chain_id: 196,
            min_credit_score: Some(70.0),
            designated_provider: None,
            buyer_agent_address: "0xMockBuyer00000000000000000000000000001".into(),
            buyer_agent_id: "mock-buyer-agent-001".into(),
            provider_agent_address: None,
            provider_agent_id: None,
            group_id: None,
            evaluator_address: None,
            expire_config: serde_json::json!({ "openExpireSec": 86400, "acceptedExpireSec": 259200 }),
            create_time: "2026-04-15T08:00:00Z".into(),
            update_time: "2026-04-15T08:00:00Z".into(),
        },
        TaskRecord {
            job_id: "task-002".into(),
            title: "DEX 套利机器人开发".into(),
            description: "开发跨链 DEX 套利机器人，支持 Uniswap V3 和 PancakeSwap，使用 Rust 实现。要求完整的回测报告、单元测试和部署文档。".into(),
            description_summary: "Rust DEX 套利机器人，支持 Uni V3 和 PCS".into(),
            token_address: "0x779ded0c9e1022225f8e0630b35a9b54be713736".into(),
            token_amount: "2000".into(),
            payment_type: Some(0),
            open_type: 1,
            status: S_OPEN,
            status_str: "open".into(),
            chain_id: 196,
            min_credit_score: Some(80.0),
            designated_provider: None,
            buyer_agent_address: "0xMockBuyer00000000000000000000000000001".into(),
            buyer_agent_id: "mock-buyer-agent-001".into(),
            provider_agent_address: None,
            provider_agent_id: None,
            group_id: None,
            evaluator_address: None,
            expire_config: serde_json::json!({ "openExpireSec": 172800, "acceptedExpireSec": 604800 }),
            create_time: "2026-04-15T09:00:00Z".into(),
            update_time: "2026-04-15T09:00:00Z".into(),
        },
        TaskRecord {
            job_id: "task-003".into(),
            title: "XLayer 链上数据索引服务".into(),
            description: "为 XLayer 构建一个链上事件索引服务，监听指定合约的 Transfer/Swap 事件，写入 PostgreSQL，并提供 REST API 查询接口。要求支持断线重连、历史区块回扫、以及 OpenAPI 文档。".into(),
            description_summary: "XLayer 事件索引 + REST API，支持历史回扫".into(),
            token_address: "0x779ded0c9e1022225f8e0630b35a9b54be713736".into(),
            token_amount: "800".into(),
            payment_type: Some(0),
            open_type: 1,
            status: S_OPEN,
            status_str: "open".into(),
            chain_id: 196,
            min_credit_score: Some(60.0),
            designated_provider: None,
            buyer_agent_address: "0xMockBuyer00000000000000000000000000002".into(),
            buyer_agent_id: "mock-buyer-agent-002".into(),
            provider_agent_address: None,
            provider_agent_id: None,
            group_id: None,
            evaluator_address: None,
            expire_config: serde_json::json!({ "openExpireSec": 259200, "acceptedExpireSec": 432000 }),
            create_time: "2026-04-15T10:00:00Z".into(),
            update_time: "2026-04-15T10:00:00Z".into(),
        },
    ];
    for t in seeds {
        // Don't overwrite tasks loaded from disk.
        tasks.entry(t.job_id.clone()).or_insert(t);
    }
}

// ─── 入口 ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let persist_path = std::env::var("MOCK_API_DB")
        .unwrap_or_else(|_| "./mock-tasks.json".to_string());

    let tasks: Arc<DashMap<String, TaskRecord>> = Arc::new(DashMap::new());

    // Load persisted tasks first, then seed any missing built-in tasks.
    let loaded = load_tasks_from_disk(&persist_path, &tasks);
    if loaded > 0 {
        println!("[{}][mock-api] loaded {} task(s) from {}", now_log(), loaded, persist_path);
    }
    seed_tasks(&tasks);  // no-ops for IDs already present

    let state = AppState {
        tasks,
        confirms: Arc::new(DashMap::new()),
        persist_path: Arc::new(persist_path),
    };

    let addr = format!("127.0.0.1:{API_PORT}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    let ts = now_log();
    println!("[{ts}][mock-api] HTTP server listening on http://{addr}");
    println!("[{ts}][mock-api] task db: {}", state.persist_path);
    println!("[{ts}][mock-api] 写接口 (POST):");
    println!("[{ts}][mock-api]   /api/v1/task/create");
    println!("[{ts}][mock-api]   /api/v1/task/{{jobId}}/apply | accept | submit | complete | refuse | close | dispute | match");
    println!("[{ts}][mock-api] 读接口 (GET):");
    println!("[{ts}][mock-api]   /api/v1/task/{{jobId}}");
    println!("[{ts}][mock-api]   /api/v1/task/list");
    println!("[{ts}][mock-api]   /api/v1/tasks/my?role=client&agent_address=0x...");
    println!("[{ts}][mock-api]   /api/v1/task/hasInProgress?agent_address=0x...");
    println!("[{ts}][mock-api] 已预置示例任务: task-001 (合约审计), task-002 (套利机器人), task-003 (链上索引)");

    let app = Router::new()
        // 写接口（calldata generation）
        .route("/api/v1/task/create",                post(create_task))
        .route("/api/v1/task/:job_id/apply",         post(apply_task))
        .route("/api/v1/task/:job_id/accept",        post(accept_task))
        .route("/api/v1/task/:job_id/submit",        post(submit_task))
        .route("/api/v1/task/:job_id/complete",      post(complete_task))
        .route("/api/v1/task/:job_id/refuse",        post(refuse_task))
        .route("/api/v1/task/:job_id/close",         post(close_task))
        .route("/api/v1/task/:job_id/setVisibility", post(set_visibility))
        .route("/api/v1/task/:job_id/dispute",       post(dispute_task))
        .route("/api/v1/task/:job_id/match",         post(match_task))
        // 读接口
        .route("/api/v1/task/list",                  get(list_tasks))
        .route("/api/v1/task/hasInProgress",         get(has_in_progress))
        .route("/api/v1/tasks/my",                   get(my_tasks))
        .route("/api/v1/task/:job_id",               get(get_task))
        .route("/api/v1/task/:job_id/providerConfirmStatus", get(provider_confirm_status))
        .with_state(state);

    axum::serve(listener, app).await.unwrap();
}
