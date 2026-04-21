use std::fs;

use anyhow::{anyhow, bail, Context as _, Result};
use base64::Engine;
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::commands::agentic_wallet::auth::{
    ensure_tokens_refreshed, format_api_error,
};
use crate::commands::Context;
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store::{self, WalletsJson};
use crate::{keyring_store, wallet_store::AddressInfo};

const XLAYER_CHAIN_INDEX: &str = "196";
const XLAYER_CHAIN_INDEX_NUM: u64 = 196;
const XLAYER_CHAIN_NAME: &str = "XLayer";

#[derive(Args, Clone, Debug)]
pub struct CreateArgs {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub role: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub picture: Option<String>,
    #[arg(long)]
    pub service: Option<String>,
    #[arg(long)]
    pub address: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct UpdateArgs {
    #[arg(value_name = "agentId")]
    pub agent_id: Option<String>,
    #[arg(long = "agent-id", hide = true)]
    pub agent_id_flag: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub picture: Option<String>,
    #[arg(long)]
    pub service: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct GetArgs {
    #[arg(long = "agent-ids")]
    pub agent_ids: Option<String>,
    #[arg(long)]
    pub page: Option<String>,
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct AgentStatusArgs {
    #[arg(value_name = "agentId")]
    pub agent_id: Option<String>,
    #[arg(long = "agent-id", hide = true)]
    pub agent_id_flag: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct UploadArgs {
    #[arg(value_name = "file")]
    pub file: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct SearchArgs {
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub feedback: Vec<String>,
    #[arg(long = "agent-info", value_delimiter = ',')]
    pub agent_info: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub status: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub service: Vec<String>,
    #[arg(long)]
    pub page: Option<String>,
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct ServiceListArgs {
    #[arg(value_name = "agentId")]
    pub agent_id: Option<String>,
    #[arg(long = "agent-id", hide = true)]
    pub agent_id_flag: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct FeedbackSubmitArgs {
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    #[arg(long)]
    pub score: Option<String>,
    #[arg(long)]
    pub tags: Option<String>,
    #[arg(long)]
    pub endpoint: Option<String>,
    #[arg(long = "feedback-uri")]
    pub feedback_uri: Option<String>,
    #[arg(long = "feedback-hash")]
    pub feedback_hash: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub address: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct FeedbackListArgs {
    #[arg(value_name = "agentId")]
    pub agent_id: Option<String>,
    #[arg(long = "agent-id", hide = true)]
    pub agent_id_flag: Option<String>,
    #[arg(long)]
    pub page: Option<String>,
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
    #[arg(long = "sort-by")]
    pub sort_by: Option<String>,
}

/// `onchainos agent xmtp-sign` 用户使用本地 signing_seed 对任意 message 做代签。
/// 不走广播，直接 POST 到 pre-transaction/sign-msg 拿后端返回的 signature。
#[derive(Args, Clone, Debug)]
pub struct XmtpSignArgs {
    /// keyUuid：之前 create 时生成过的那个 UUID，用户可通过 agent get 查出来
    #[arg(long = "key-uuid")]
    pub key_uuid: Option<String>,
    /// 要签名的消息，原样传给后端
    #[arg(long)]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct AgentService {
    #[serde(
        rename = "id",
        default,
        alias = "id",
        skip_serializing_if = "Option::is_none"
    )]
    id: Option<String>,
    #[serde(
        rename = "ServiceDescription",
        alias = "ServiceDescription",
        alias = "serviceDescription"
    )]
    service_description: String,
    #[serde(rename = "ServiceName", alias = "ServiceName", alias = "serviceName")]
    service_name: String,
    #[serde(rename = "Fee", default, alias = "Fee", alias = "fee")]
    fee: String,
    #[serde(rename = "ServiceType", alias = "ServiceType", alias = "serviceType")]
    service_type: String,
    #[serde(
        rename = "Endpoint",
        default,
        alias = "Endpoint",
        alias = "endpoint",
        skip_serializing_if = "Option::is_none"
    )]
    endpoint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct AgentCard {
    #[serde(rename = "Role")]
    role: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "ProfilePicture")]
    profile_picture: String,
    #[serde(rename = "ProfileDescription")]
    profile_description: String,
    #[serde(
        rename = "CommunicationAddress",
        skip_serializing_if = "Option::is_none"
    )]
    communication_address: Option<String>,
    #[serde(rename = "Service")]
    services: Vec<AgentService>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ExistingAgentCard {
    role: Option<String>,
    name: Option<String>,
    profile_picture: Option<String>,
    profile_description: Option<String>,
    communication_address: Option<String>,
    services: Option<Vec<AgentService>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentUnsignedTx {
    #[serde(default, deserialize_with = "null_string")]
    hash: String,
    #[serde(default, deserialize_with = "null_string")]
    auth_hash_for7702: String,
    #[serde(default, deserialize_with = "null_string")]
    uop_hash: String,
    #[serde(default, deserialize_with = "null_string")]
    sign_type: String,
    #[serde(default, deserialize_with = "null_string")]
    encoding: String,
    #[serde(default, deserialize_with = "null_string")]
    unsigned_tx_hash: String,
    #[serde(default, deserialize_with = "null_string")]
    unsigned_tx: String,
    #[serde(default)]
    extra_data: Value,
}

/// 广播阶段 erc8004Msg 子对象的内容。按产品规范：
/// - create：首次注册，4 个子字段全部携带（建立 agent 身份）
/// - update / feedback-submit：不允许修改 communicationAddress / role / keyUuid /
///   sessionSignature 任何一个，所以 erc8004Msg 是空对象 `{}`，避免后端误改 K1
///   或 agent 通信地址等持久身份。
enum Erc8004Payload {
    Create {
        communication_address: String,
        role: String,
        key_uuid: String,
        session_signature: String,
    },
    Empty,
}

pub async fn create(args: CreateArgs, ctx: &Context) -> Result<()> {
    output::success(create_impl(&args, ctx).await?);
    Ok(())
}

pub async fn update(args: UpdateArgs, ctx: &Context) -> Result<()> {
    output::success(update_impl(&args, ctx).await?);
    Ok(())
}

pub async fn get(args: GetArgs, ctx: &Context) -> Result<()> {
    output::success(get_impl(&args, ctx).await?);
    Ok(())
}

pub async fn activate(args: AgentStatusArgs, ctx: &Context) -> Result<()> {
    activate_impl(&args, ctx).await?;
    output::success_empty();
    Ok(())
}

pub async fn deactivate(args: AgentStatusArgs, ctx: &Context) -> Result<()> {
    deactivate_impl(&args, ctx).await?;
    output::success_empty();
    Ok(())
}

pub async fn upload(args: UploadArgs, ctx: &Context) -> Result<()> {
    output::success(upload_impl(&args, ctx).await?);
    Ok(())
}

pub async fn search(args: SearchArgs, ctx: &Context) -> Result<()> {
    output::success(search_impl(&args, ctx).await?);
    Ok(())
}

pub async fn service_list(args: ServiceListArgs, ctx: &Context) -> Result<()> {
    output::success(service_list_impl(&args, ctx).await?);
    Ok(())
}

pub async fn feedback_submit(args: FeedbackSubmitArgs, ctx: &Context) -> Result<()> {
    output::success(feedback_submit_impl(&args, ctx).await?);
    Ok(())
}

pub async fn feedback_list(args: FeedbackListArgs, ctx: &Context) -> Result<()> {
    output::success(feedback_list_impl(&args, ctx).await?);
    Ok(())
}

pub async fn xmtp_sign(args: XmtpSignArgs, ctx: &Context) -> Result<()> {
    output::success(xmtp_sign_impl(&args, ctx).await?);
    Ok(())
}

async fn create_impl(args: &CreateArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let (_account_id, from_addr) = resolve_xlayer_signing_account(args.address.as_deref())?;
    let signing_seed = load_signing_seed()?;
    let session_cert = load_session_cert()?;
    let key_uuid = Uuid::new_v4().to_string();
    let session_signature = sign_key_uuid(&key_uuid, &signing_seed)?;
    let normalized_role = normalize_role(require_non_empty(args.role.as_deref(), "--role")?)?;
    let card = AgentCard {
        role: normalized_role.clone(),
        name: require_non_empty(args.name.as_deref(), "--name")?.to_string(),
        profile_picture: trim_or_empty(args.picture.as_deref()),
        profile_description: require_non_empty(args.description.as_deref(), "--description")?
            .to_string(),
        communication_address: None,
        services: parse_services(args.service.as_deref())?,
    };
    ensure_provider_has_service(&card)?;
    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "fromAddr": from_addr,
        "keyUuid": key_uuid.clone(),
        "sessionSignature": session_signature.clone(),
        "sessionCert": session_cert,
        "cardJson": serde_json::to_string(&card).context("failed to serialize cardJson")?,
    });
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] create request: url={} access_token_len={} access_token_prefix={} body={}",
            reconstruct_post_url_for_log(
                ctx,
                "/priapi/v5/wallet/agentic/pre-transaction/createAgent",
            ),
            access_token.len(),
            redact_token_for_debug(&access_token),
            serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string())
        );
    }
    let response = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/createAgent",
            &access_token,
            &body,
        )
        .await?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] create response: {}",
            serde_json::to_string(&response)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        );
    }
    let unsigned = parse_agent_unsigned(response)?;
    // erc8004Msg 全 4 字段：首次注册 agent 身份必须建立。
    // communicationAddress 由后端 pre-transaction 返回。
    let communication_address = unsigned
        .extra_data
        .get("communicationAddress")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let erc8004 = Erc8004Payload::Create {
        communication_address,
        role: normalized_role,
        key_uuid,
        session_signature,
    };
    let tx_hash = sign_and_broadcast_agent_transaction(
        ctx,
        &access_token,
        &unsigned,
        &erc8004,
        args.address.as_deref(),
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

async fn update_impl(args: &UpdateArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let agent_id = resolve_agent_id(&args.agent_id, &args.agent_id_flag)?;
    let session_cert = load_session_cert()?;

    // Product spec: update is full overwrite — fields not passed must be echoed back
    // from the existing agent, so always fetch current state first.
    let current = fetch_agent_for_update(ctx, &access_token, agent_id).await?;

    // Role cannot be modified via update (not exposed as a CLI flag by product
    // spec); always echo back the existing role into cardJson only.
    let card = AgentCard {
        role: normalize_role(
            current
                .role
                .as_deref()
                .ok_or_else(|| anyhow!("existing agent has no role"))?,
        )?,
        name: resolve_update_string(args.name.as_deref(), current.name.as_deref(), "--name")?,
        profile_picture: resolve_optional_update_string(
            args.picture.as_deref(),
            current.profile_picture.as_deref(),
        ),
        profile_description: resolve_update_string(
            args.description.as_deref(),
            current.profile_description.as_deref(),
            "--description",
        )?,
        // Product spec: update is not allowed to modify CommunicationAddress, so the
        // field is intentionally omitted from cardJson (skip_serializing_if = is_none).
        // fromAddr is likewise handled server-side by agentId and not sent here.
        communication_address: None,
        services: resolve_update_services(args.service.as_deref(), current.services.as_ref())?,
    };
    ensure_provider_has_service(&card)?;
    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "agentId": agent_id,
        "sessionCert": session_cert,
        "cardJson": serde_json::to_string(&card).context("failed to serialize cardJson")?,
    });
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] update request: url={} access_token_len={} access_token_prefix={} body={}",
            reconstruct_post_url_for_log(
                ctx,
                "/priapi/v5/wallet/agentic/pre-transaction/updateAgent",
            ),
            access_token.len(),
            redact_token_for_debug(&access_token),
            serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
        );
    }
    let update_result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/updateAgent",
            &access_token,
            &body,
        )
        .await;
    if cfg!(feature = "debug-log") {
        match &update_result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] update response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] update response err: {:#}", e),
        }
    }
    let response = update_result?;
    let unsigned = parse_agent_unsigned(response)?;
    // 产品规范：update 不允许修改 communicationAddress / role / keyUuid /
    // sessionSignature 任何一个，erc8004Msg 一律空 `{}`，避免覆盖 agent 持久身份。
    let tx_hash = sign_and_broadcast_agent_transaction(
        ctx,
        &access_token,
        &unsigned,
        &Erc8004Payload::Empty,
        None,
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

async fn get_impl(args: &GetArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;

    // Product spec: agent-list identifies the user via JWT; `from` is never needed.
    // page / pageSize 是选填——用户没传就不塞 query，让后端使用自身默认。
    let mut query = vec![("chainIndex".to_string(), XLAYER_CHAIN_INDEX.to_string())];
    push_optional_query(&mut query, "agentIds", args.agent_ids.as_deref());
    if let Some(page_raw) = args.page.as_deref() {
        let page = parse_u32_arg(Some(page_raw), "--page", 1, Some(1), None, false)?;
        query.push(("page".to_string(), page.to_string()));
    }
    if let Some(page_size_raw) = args.page_size.as_deref() {
        let page_size = parse_u32_arg(
            Some(page_size_raw),
            "--page-size",
            20,
            Some(1),
            None,
            false,
        )?;
        query.push(("pageSize".to_string(), page_size.to_string()));
    }

    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] get request: url={} access_token_len={} access_token_prefix={} query={:?}",
            reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent-list", &query_refs),
            access_token.len(),
            redact_token_for_debug(&access_token),
            query_refs,
        );
    }

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent-list",
            &access_token,
            &query_refs,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] get response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] get response err: {:#}", e),
        }
    }

    Ok(normalize_singleton_object(result?))
}

async fn activate_impl(args: &AgentStatusArgs, ctx: &Context) -> Result<()> {
    agent_status_impl(args, 1, ctx).await
}

async fn deactivate_impl(args: &AgentStatusArgs, ctx: &Context) -> Result<()> {
    agent_status_impl(args, 2, ctx).await
}

async fn agent_status_impl(args: &AgentStatusArgs, status: u32, ctx: &Context) -> Result<()> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let agent_id = resolve_agent_id(&args.agent_id, &args.agent_id_flag)?;
    let body = json!({
        "agentId": agent_id,
        "chainIndex": XLAYER_CHAIN_INDEX,
        "status": status,
    });

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] agent-status request: url={} access_token_len={} access_token_prefix={} body={}",
            reconstruct_post_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent-status"),
            access_token.len(),
            redact_token_for_debug(&access_token),
            serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
        );
    }

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/agent-status",
            &access_token,
            &body,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] agent-status response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] agent-status response err: {:#}", e),
        }
    }

    result.map_err(format_api_error)?;
    Ok(())
}

async fn upload_impl(args: &UploadArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let file = require_non_empty(args.file.as_deref(), "[file]")?;
    let bytes = fs::read(file).with_context(|| format!("failed to read file: {file}"))?;
    let filename = std::path::Path::new(file)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("upload.bin")
        .to_string();
    let upload_url = reconstruct_post_url_for_log(
        ctx,
        "/priapi/v5/wallet/agentic/pre-transaction/upload-picture",
    );
    // 无条件打印 URL，联调时方便直接复制粘贴对比后端。
    eprintln!("[agent-identity] upload request url={}", upload_url);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] upload request: url={} access_token_len={} access_token_prefix={} file_path={} filename={} bytes_len={}",
            upload_url,
            access_token.len(),
            redact_token_for_debug(&access_token),
            file,
            filename,
            bytes.len(),
        );
    }
    let part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
    let form = reqwest::multipart::Form::new().part("file", part);
    let result = client
        .post_authed_multipart(
            "/priapi/v5/wallet/agentic/pre-transaction/upload-picture",
            &access_token,
            form,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] upload response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] upload response err: {:#}", e),
        }
    }

    let data = result?;

    let url = if let Some(url) = data.get("url").and_then(Value::as_str) {
        url.to_string()
    } else if let Some(first) = data.as_array().and_then(|arr| arr.first()) {
        first
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("upload response missing url"))?
    } else {
        bail!("upload response missing url");
    };
    Ok(json!({ "url": url }))
}

async fn search_impl(args: &SearchArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let query_text = require_non_empty(args.query.as_deref(), "--query")?;

    // query 必填；page / pageSize / 多值过滤字段按文档都是选填，用户没传就不塞
    let mut query = vec![("query".to_string(), query_text.to_string())];
    if let Some(page_raw) = args.page.as_deref() {
        let page = parse_u32_arg(Some(page_raw), "--page", 1, Some(1), None, false)?;
        query.push(("page".to_string(), page.to_string()));
    }
    if let Some(page_size_raw) = args.page_size.as_deref() {
        let page_size = parse_u32_arg(
            Some(page_size_raw),
            "--page-size",
            20,
            Some(1),
            Some(100),
            true,
        )?;
        query.push(("pageSize".to_string(), page_size.to_string()));
    }
    push_multi_query(&mut query, "feedback", &args.feedback);
    push_multi_query(&mut query, "agentInfo", &args.agent_info);
    push_multi_query(&mut query, "status", &args.status);
    push_multi_query(&mut query, "service", &args.service);

    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] search request: url={} access_token_len={} access_token_prefix={} query={:?}",
            reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/cli-search", &query_refs),
            access_token.len(),
            redact_token_for_debug(&access_token),
            query_refs,
        );
    }

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/cli-search",
            &access_token,
            &query_refs,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] search response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] search response err: {:#}", e),
        }
    }

    Ok(normalize_singleton_object(result?))
}

async fn service_list_impl(args: &ServiceListArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let agent_id = resolve_agent_id(&args.agent_id, &args.agent_id_flag)?;
    let query = vec![("agentId".to_string(), agent_id.to_string())];
    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] service-list request: url={} access_token_len={} access_token_prefix={} query={:?}",
            reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/services", &query_refs),
            access_token.len(),
            redact_token_for_debug(&access_token),
            query_refs,
        );
    }

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/services",
            &access_token,
            &query_refs,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] service-list response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] service-list response err: {:#}", e),
        }
    }

    result
}

async fn feedback_submit_impl(args: &FeedbackSubmitArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let (_account_id, from_addr) = resolve_xlayer_signing_account(args.address.as_deref())?;
    let score = parse_u32_arg(
        Some(require_non_empty(args.score.as_deref(), "--score")?),
        "--score",
        0,
        Some(0),
        Some(100),
        false,
    )?;
    let comment = json!({
        "agentId": require_non_empty(args.agent_id.as_deref(), "--agent-id")?,
        "score": score.to_string(),
        "tags": trim_or_empty(args.tags.as_deref()),
        "endpoint": trim_or_empty(args.endpoint.as_deref()),
        "feedbackURI": trim_or_empty(args.feedback_uri.as_deref()),
        "feedbackHash": trim_or_empty(args.feedback_hash.as_deref()),
        "description": trim_or_empty(args.description.as_deref()),
    });
    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "fromAddr": from_addr,
        "comment": serde_json::to_string(&comment).context("failed to serialize comment")?,
    });

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] feedback-submit request: url={} access_token_len={} access_token_prefix={} body={}",
            reconstruct_post_url_for_log(
                ctx,
                "/priapi/v5/wallet/agentic/pre-transaction/createComment",
            ),
            access_token.len(),
            redact_token_for_debug(&access_token),
            serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
        );
    }

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/createComment",
            &access_token,
            &body,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] feedback-submit response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] feedback-submit response err: {:#}", e),
        }
    }

    let response = result?;
    let unsigned = parse_agent_unsigned(response)?;
    // 产品规范：feedback-submit 不允许修改 communicationAddress / role / keyUuid /
    // sessionSignature 任何一个，erc8004Msg 一律空 `{}`。
    let tx_hash = sign_and_broadcast_agent_transaction(
        ctx,
        &access_token,
        &unsigned,
        &Erc8004Payload::Empty,
        args.address.as_deref(),
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

async fn feedback_list_impl(args: &FeedbackListArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;

    // agentId 必填；page / pageSize / sortBy 按文档都是选填，用户没传就不塞，让后端用自身默认
    let mut query = vec![(
        "agentId".to_string(),
        resolve_agent_id(&args.agent_id, &args.agent_id_flag)?.to_string(),
    )];
    if let Some(page_raw) = args.page.as_deref() {
        let page = parse_u32_arg(Some(page_raw), "--page", 1, Some(1), None, false)?;
        query.push(("page".to_string(), page.to_string()));
    }
    if let Some(page_size_raw) = args.page_size.as_deref() {
        let page_size = parse_u32_arg(
            Some(page_size_raw),
            "--page-size",
            20,
            Some(1),
            Some(50),
            true,
        )?;
        query.push(("pageSize".to_string(), page_size.to_string()));
    }
    if let Some(sort_by_raw) = args.sort_by.as_deref() {
        let sort_by = match sort_by_raw {
            "time_desc" | "score_desc" => sort_by_raw,
            other => bail!("invalid value for --sort-by: {other}"),
        };
        query.push(("sortBy".to_string(), sort_by.to_string()));
    }
    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] feedback-list request: url={} access_token_len={} access_token_prefix={} query={:?}",
            reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/reviews", &query_refs),
            access_token.len(),
            redact_token_for_debug(&access_token),
            query_refs,
        );
    }

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/reviews",
            &access_token,
            &query_refs,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] feedback-list response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] feedback-list response err: {:#}", e),
        }
    }

    Ok(normalize_singleton_object(result?))
}

/// `onchainos agent xmtp-sign`：用本地 signing_seed 对 keyUuid 现场签一次，
/// 连同 CLI 传入的 message + 本地 sessionCert 一起 POST 到后端的 sign-msg 接口，
/// 后端返回 signature 后透传给用户。不走广播。
async fn xmtp_sign_impl(args: &XmtpSignArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;

    let key_uuid = require_non_empty(args.key_uuid.as_deref(), "--key-uuid")?;
    let message = require_non_empty(args.message.as_deref(), "--message")?;

    let signing_seed = load_signing_seed()?;
    let session_cert = load_session_cert()?;
    // Same signing algorithm as create / update's erc8004Msg: Ed25519 over the
    // raw UTF-8 bytes of keyUuid — see sign_key_uuid.
    let session_signature = sign_key_uuid(key_uuid, &signing_seed)?;

    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX,
        "sessionCert": session_cert,
        "sessionSignature": session_signature,
        "signType": "aiagentsign",
        "keyUuid": key_uuid,
        "message": message,
    });

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] xmtp-sign request: url={} access_token_len={} access_token_prefix={} body={}",
            reconstruct_post_url_for_log(
                ctx,
                "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            ),
            access_token.len(),
            redact_token_for_debug(&access_token),
            serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
        );
    }

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &body,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] xmtp-sign response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] xmtp-sign response err: {:#}", e),
        }
    }

    let data = result.map_err(format_api_error)?;
    let first = data
        .as_array()
        .and_then(|arr| arr.first())
        .cloned()
        .ok_or_else(|| anyhow!("xmtp-sign response is empty"))?;
    if first
        .get("signature")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        bail!("xmtp-sign response missing signature");
    }
    Ok(first)
}

fn wallet_client(_ctx: &Context) -> Result<WalletApiClient> {
    WalletApiClient::new()
}

fn redact_token_for_debug(token: &str) -> String {
    if token.len() <= 16 {
        return format!("{token}***");
    }
    format!("{}***{}", &token[..8], &token[token.len() - 6..])
}

// Log-only helpers. Precedence mirrors WalletApiClient::with_base_url:
// compile-time OKX_BASE_URL > ctx.base_url_override > DEFAULT_BASE_URL.
// Note: reconstruct_get_url_for_log does NOT percent-encode values, so the
// logged URL may diverge from the actual wire URL when values contain
// characters that wallet_api::build_query_string would escape.
fn resolve_base_url_for_log(ctx: &Context) -> String {
    option_env!("OKX_BASE_URL")
        .map(str::to_string)
        .or_else(|| ctx.base_url_override.clone())
        .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string())
}

fn reconstruct_post_url_for_log(ctx: &Context, path: &str) -> String {
    format!("{}{}", resolve_base_url_for_log(ctx), path)
}

fn reconstruct_get_url_for_log(ctx: &Context, path: &str, query: &[(&str, &str)]) -> String {
    let base = resolve_base_url_for_log(ctx);
    let filtered: Vec<&(&str, &str)> = query.iter().filter(|(_, v)| !v.is_empty()).collect();
    if filtered.is_empty() {
        return format!("{base}{path}");
    }
    let pairs: Vec<String> = filtered
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    format!("{base}{path}?{}", pairs.join("&"))
}

fn ed25519_pubkey_hex(signing_seed: &[u8; 32]) -> String {
    use ed25519_dalek::SigningKey;
    let sk = SigningKey::from_bytes(signing_seed);
    hex::encode(sk.verifying_key().to_bytes())
}

fn push_optional_query(query: &mut Vec<(String, String)>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        query.push((key.to_string(), value.trim().to_string()));
    }
}

fn push_multi_query(query: &mut Vec<(String, String)>, key: &str, values: &[String]) {
    for value in values {
        if !value.trim().is_empty() {
            query.push((key.to_string(), value.trim().to_string()));
        }
    }
}

fn normalize_singleton_object(data: Value) -> Value {
    match data {
        Value::Array(mut arr) if arr.len() == 1 && arr[0].is_object() => arr.remove(0),
        other => other,
    }
}

fn parse_agent_unsigned(data: Value) -> Result<AgentUnsignedTx> {
    let item = data
        .as_array()
        .and_then(|arr| arr.first())
        .cloned()
        .ok_or_else(|| anyhow!("pre-transaction response is empty"))?;
    serde_json::from_value(item).context("failed to parse pre-transaction response")
}

async fn sign_and_broadcast_agent_transaction(
    ctx: &Context,
    access_token: &str,
    unsigned: &AgentUnsignedTx,
    erc8004: &Erc8004Payload,
    address_override: Option<&str>,
) -> Result<String> {
    let client = wallet_client(ctx)?;
    let (account_id, from_addr) = resolve_xlayer_signing_account(address_override)?;
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);
    let session_cert = session.session_cert;
    if session_cert.is_empty() {
        bail!("session cert missing, please login again: onchainos wallet login");
    }
    if unsigned.hash.is_empty()
        && unsigned.auth_hash_for7702.is_empty()
        && unsigned.unsigned_tx_hash.is_empty()
    {
        bail!("pre-transaction response missing signable hashes");
    }

    // msgForSign follows transfer.rs's conditional-insert rules: each hash field
    // is only signed+populated when present, sessionCert is always included.
    let mut msg_for_sign = Map::new();
    if !unsigned.hash.is_empty() {
        msg_for_sign.insert(
            "signature".to_string(),
            json!(crate::crypto::ed25519_sign_eip191(
                &unsigned.hash,
                &signing_seed,
                "hex"
            )?),
        );
    }
    if !unsigned.auth_hash_for7702.is_empty() {
        msg_for_sign.insert(
            "authSignatureFor7702".to_string(),
            json!(crate::crypto::ed25519_sign_hex(
                &unsigned.auth_hash_for7702,
                &signing_seed_b64
            )?),
        );
    }
    if !unsigned.unsigned_tx_hash.is_empty() {
        msg_for_sign.insert(
            "unsignedTxHash".to_string(),
            json!(unsigned.unsigned_tx_hash),
        );
        msg_for_sign.insert(
            "sessionSignature".to_string(),
            json!(crate::crypto::ed25519_sign_encoded(
                &unsigned.unsigned_tx_hash,
                &signing_seed_b64,
                &unsigned.encoding,
            )?),
        );
    }
    if !unsigned.unsigned_tx.is_empty() {
        msg_for_sign.insert("unsignedTx".to_string(), json!(unsigned.unsigned_tx));
    }
    msg_for_sign.insert("sessionCert".to_string(), json!(session_cert));

    let mut extra_data = if unsigned.extra_data.is_object() {
        unsigned.extra_data.clone()
    } else {
        json!({})
    };
    extra_data["txType"] = json!(3);
    extra_data["syncWaitOnChain"] = json!(true);
    extra_data["checkBalance"] = json!(true);
    extra_data["uopHash"] = json!(unsigned.uop_hash);
    if !unsigned.encoding.is_empty() {
        extra_data["encoding"] = json!(unsigned.encoding);
    }
    if !unsigned.sign_type.is_empty() {
        extra_data["signType"] = json!(unsigned.sign_type);
    }
    extra_data["msgForSign"] = Value::Object(msg_for_sign);
    // erc8004Msg：按 Erc8004Payload 决定塞什么。
    // - Create：4 个子字段全部携带（首次注册 agent 身份）
    // - Empty：空 `{}`，update / feedback-submit 都用这个，避免覆盖 agent 持久身份
    extra_data["erc8004Msg"] = match erc8004 {
        Erc8004Payload::Create {
            communication_address,
            role,
            key_uuid,
            session_signature,
        } => json!({
            "communicationAddress": communication_address,
            "role": role,
            "keyUuid": key_uuid,
            "sessionSignature": session_signature,
        }),
        Erc8004Payload::Empty => json!({}),
    };

    let extra_data_str =
        serde_json::to_string(&extra_data).context("failed to serialize broadcast extraData")?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] broadcast request prepared: \
             url={} access_token_len={} access_token_prefix={} \
             accountId={} address={} chainIndex={} extraData={}",
            reconstruct_post_url_for_log(
                ctx,
                "/priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction",
            ),
            access_token.len(),
            redact_token_for_debug(access_token),
            account_id,
            from_addr,
            XLAYER_CHAIN_INDEX,
            extra_data_str,
        );
    }

    let resp_result = client
        .broadcast_transaction(
            access_token,
            &account_id,
            &from_addr,
            XLAYER_CHAIN_INDEX,
            &extra_data_str,
            None,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &resp_result {
            Ok(r) => eprintln!(
                "[DEBUG][agent-identity] broadcast response ok: txHash={} pkgId={} orderId={} orderType={}",
                r.tx_hash, r.pkg_id, r.order_id, r.order_type
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] broadcast response err: {:#}", e),
        }
    }

    let resp = resp_result.map_err(format_api_error)?;
    if resp.tx_hash.is_empty() {
        bail!("broadcast response missing txHash");
    }
    Ok(resp.tx_hash)
}

fn resolve_xlayer_signing_account(address: Option<&str>) -> Result<(String, String)> {
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow!("no XLayer address found in current account"))?;
    if let Some(address) = address.filter(|value| !value.trim().is_empty()) {
        for (account_id, entry) in &wallets.accounts_map {
            for addr in &entry.address_list {
                if is_xlayer_address(addr) && addr.address.eq_ignore_ascii_case(address.trim()) {
                    return Ok((account_id.clone(), addr.address.clone()));
                }
            }
        }
        bail!("no XLayer address found in current account");
    }

    let (account_id, addr_info) = resolve_current_xlayer_address(&wallets)?;
    Ok((account_id, addr_info.address))
}

fn resolve_current_xlayer_address(wallets: &WalletsJson) -> Result<(String, AddressInfo)> {
    let account_id = wallets.selected_account_id.trim();
    if account_id.is_empty() {
        bail!("no XLayer address found in current account");
    }
    let entry = wallets
        .accounts_map
        .get(account_id)
        .ok_or_else(|| anyhow!("no XLayer address found in current account"))?;
    let addr = entry
        .address_list
        .iter()
        .find(|addr| is_xlayer_address(addr))
        .cloned()
        .ok_or_else(|| anyhow!("no XLayer address found in current account"))?;
    Ok((account_id.to_string(), addr))
}

fn is_xlayer_address(addr: &AddressInfo) -> bool {
    addr.chain_index == XLAYER_CHAIN_INDEX
        || addr.chain_name.eq_ignore_ascii_case(XLAYER_CHAIN_NAME)
}

fn load_signing_seed() -> Result<[u8; 32]> {
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!("session expired, please login again: onchainos wallet login"))?;
    crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)
}

fn load_session_cert() -> Result<String> {
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!("session expired, please login again: onchainos wallet login"))?;
    Ok(session.session_cert)
}

/// 用 signing_seed 对 keyUuid 做 Ed25519 签名，作为 sessionSignature。
///
/// 按产品规范：不套 EIP-191 前缀，不做 Keccak-256 预哈希——直接把 keyUuid 的
/// UTF-8 字节喂给 Ed25519 签名算法。后端验签等价于：
///   VerifyKey(pubkey).verify(keyUuid.encode("utf-8"), base64_decode(sig))
///
/// 注意：`crypto::ed25519_sign_eip191` 是 agentic wallet（transfer.rs）签
/// EVM tx hash 用的协议路径，这里不复用，避免和 identity 的签名语义混淆。
fn sign_key_uuid(key_uuid: &str, signing_seed: &[u8; 32]) -> Result<String> {
    let sig_bytes = crate::crypto::ed25519_sign(signing_seed, key_uuid.as_bytes())?;
    let signature = base64::engine::general_purpose::STANDARD.encode(&sig_bytes);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] sign_key_uuid: keyUuid={} keyUuid_utf8_bytes_hex={} signed_bytes_len={} signing_pubkey_hex={}",
            key_uuid,
            hex::encode(key_uuid.as_bytes()),
            key_uuid.as_bytes().len(),
            ed25519_pubkey_hex(signing_seed),
        );
    }
    Ok(signature)
}

fn parse_services(raw: Option<&str>) -> Result<Vec<AgentService>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let services: Vec<AgentService> =
        serde_json::from_str(raw).context("failed to parse --service as JSON array")?;
    services
        .into_iter()
        .map(normalize_service)
        .collect::<Result<Vec<_>>>()
}

fn normalize_service(mut service: AgentService) -> Result<AgentService> {
    if service
        .id
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        service.id = None;
    } else {
        service.id = Some(service.id.unwrap().trim().to_string());
    }
    service.service_name = service.service_name.trim().to_string();
    service.service_description = service.service_description.trim().to_string();
    service.fee = service.fee.trim().to_string();
    service.service_type = service.service_type.trim().to_ascii_uppercase();
    service.endpoint = service
        .endpoint
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if service.service_name.is_empty() {
        bail!("missing required field in --service: ServiceName");
    }
    if service.service_description.is_empty() {
        bail!("missing required field in --service: ServiceDescription");
    }
    match service.service_type.as_str() {
        "A2A" => {
            // Product spec: A2A services do not have an Endpoint field.
            service.endpoint = None;
        }
        "A2MCP" => {
            if service.fee.is_empty() {
                bail!("missing required field in --service for A2MCP: Fee");
            }
            if service.endpoint.is_none() {
                bail!("missing required field in --service for A2MCP: Endpoint");
            }
        }
        other => bail!("invalid ServiceType in --service: {other}"),
    }

    Ok(service)
}

fn normalize_role(role: &str) -> Result<String> {
    match role.trim().to_ascii_lowercase().as_str() {
        "1" | "buyer" | "requestor" | "requester" => Ok("requester".to_string()),
        "2" | "provider" => Ok("provider".to_string()),
        "3" | "evaluator" => Ok("evaluator".to_string()),
        other => bail!("invalid value for --role: {other}"),
    }
}

fn resolve_agent_id<'a>(
    agent_id: &'a Option<String>,
    agent_id_flag: &'a Option<String>,
) -> Result<&'a str> {
    if let Some(agent_id) = agent_id.as_deref().filter(|value| !value.trim().is_empty()) {
        return Ok(agent_id.trim());
    }
    if let Some(agent_id) = agent_id_flag
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(agent_id.trim());
    }
    bail!("missing required parameter: agentId")
}

fn require_non_empty<'a>(value: Option<&'a str>, flag: &str) -> Result<&'a str> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Ok(value),
        None => bail!("missing required parameter: {flag}"),
    }
}

fn trim_or_empty(value: Option<&str>) -> String {
    value.unwrap_or("").trim().to_string()
}

fn resolve_update_string(
    new_value: Option<&str>,
    current_value: Option<&str>,
    flag: &str,
) -> Result<String> {
    if let Some(value) = new_value {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            bail!("missing required parameter: {flag}");
        }
        return Ok(trimmed.to_string());
    }
    if let Some(value) = current_value {
        return Ok(value.to_string());
    }
    bail!("missing required parameter: {flag}")
}

fn resolve_optional_update_string(new_value: Option<&str>, current_value: Option<&str>) -> String {
    if let Some(value) = new_value {
        value.trim().to_string()
    } else {
        current_value.unwrap_or("").to_string()
    }
}

fn ensure_provider_has_service(card: &AgentCard) -> Result<()> {
    if card.role == "provider" && card.services.is_empty() {
        bail!("provider agents require at least one service; provide --service");
    }
    Ok(())
}

fn resolve_update_services(
    new_value: Option<&str>,
    current_value: Option<&Vec<AgentService>>,
) -> Result<Vec<AgentService>> {
    if new_value.is_some() {
        return parse_services(new_value);
    }
    Ok(current_value.cloned().unwrap_or_default())
}

async fn fetch_agent_for_update(
    ctx: &Context,
    access_token: &str,
    agent_id: &str,
) -> Result<ExistingAgentCard> {
    let client = wallet_client(ctx)?;
    // Product spec: agent-list identifies the user via JWT and returns all agents
    // owned by that user; `from` is never needed even when filtering by agentIds.
    let query = vec![
        ("chainIndex".to_string(), XLAYER_CHAIN_INDEX.to_string()),
        ("agentIds".to_string(), agent_id.to_string()),
        ("page".to_string(), "1".to_string()),
        ("pageSize".to_string(), "20".to_string()),
    ];

    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] update.fetch-agent request: url={} access_token_len={} access_token_prefix={} query={:?}",
            reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent-list", &query_refs),
            access_token.len(),
            redact_token_for_debug(access_token),
            query_refs,
        );
    }

    let fetch_result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent-list",
            access_token,
            &query_refs,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &fetch_result {
            Ok(data) => eprintln!(
                "[DEBUG][agent-identity] update.fetch-agent response: {}",
                serde_json::to_string(data)
                    .unwrap_or_else(|_| "<serialize failed>".to_string())
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] update.fetch-agent response err: {:#}", e),
        }
    }

    let data = normalize_singleton_object(fetch_result?);
    let list = data["list"]
        .as_array()
        .ok_or_else(|| anyhow!("agent get response missing list"))?;
    let item = list
        .iter()
        .find(|item| json_to_string(item.get("agentId")) == Some(agent_id.to_string()))
        .cloned()
        .ok_or_else(|| anyhow!("agent not found: {agent_id}"))?;
    extract_existing_agent_card(&item)
}

fn extract_existing_agent_card(item: &Value) -> Result<ExistingAgentCard> {
    let card_json = extract_embedded_card_json(item);

    let role = first_string_field(&[card_json.as_ref(), Some(item)], &[&["Role"], &["role"]]);
    let name = first_string_field(&[card_json.as_ref(), Some(item)], &[&["Name"], &["name"]]);
    let profile_picture = first_string_field(
        &[card_json.as_ref(), Some(item)],
        &[&["ProfilePicture"], &["profilePicture"]],
    );
    let profile_description = first_string_field(
        &[card_json.as_ref(), Some(item)],
        &[&["ProfileDescription"], &["profileDescription"]],
    );
    let communication_address = first_string_field(
        &[card_json.as_ref(), Some(item)],
        &[&["CommunicationAddress"], &["communicationAddress"]],
    );

    let services = first_value_field(
        &[card_json.as_ref(), Some(item)],
        &[&["Service"], &["service"], &["services"]],
    )
    .map(parse_services_value)
    .transpose()?;

    Ok(ExistingAgentCard {
        role,
        name,
        profile_picture,
        profile_description,
        communication_address,
        services,
    })
}

fn extract_embedded_card_json(item: &Value) -> Option<Value> {
    first_value_field(
        &[Some(item)],
        &[&["cardJson"], &["cardJSON"], &["card"], &["agentCard"]],
    )
    .and_then(|value| match value {
        Value::String(text) => serde_json::from_str::<Value>(text).ok(),
        Value::Object(_) => Some(value.clone()),
        _ => None,
    })
}

fn first_string_field(sources: &[Option<&Value>], keys: &[&[&str]]) -> Option<String> {
    for source in sources {
        let Some(source) = source else {
            continue;
        };
        for group in keys {
            for key in *group {
                if let Some(value) = source.get(*key) {
                    if let Some(value) = json_to_string(Some(value)) {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

fn first_value_field<'a>(sources: &[Option<&'a Value>], keys: &[&[&str]]) -> Option<&'a Value> {
    for source in sources {
        let Some(source) = source else {
            continue;
        };
        for group in keys {
            for key in *group {
                if let Some(value) = source.get(*key) {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn parse_services_value(value: &Value) -> Result<Vec<AgentService>> {
    let services = match value {
        Value::Null => Vec::new(),
        Value::Array(_) => serde_json::from_value::<Vec<AgentService>>(value.clone())
            .context("failed to parse current agent services")?,
        _ => bail!("current agent services are not an array"),
    };
    services
        .into_iter()
        .map(normalize_service)
        .collect::<Result<Vec<_>>>()
}

fn json_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(if *boolean { "true" } else { "false" }.to_string()),
        _ => None,
    }
}

fn parse_u32_arg(
    value: Option<&str>,
    flag: &str,
    default: u32,
    min: Option<u32>,
    max: Option<u32>,
    clamp_max: bool,
) -> Result<u32> {
    let Some(value) = value else {
        return Ok(default);
    };
    let parsed = value
        .trim()
        .parse::<u32>()
        .map_err(|_| anyhow!("invalid value for {flag}: expected integer"))?;
    if let Some(min) = min {
        if parsed < min {
            bail!("invalid value for {flag}: must be >= {min}");
        }
    }
    if let Some(max) = max {
        if parsed > max {
            if clamp_max {
                return Ok(max);
            }
            bail!("invalid value for {flag}: must be <= {max}");
        }
    }
    Ok(parsed)
}

fn null_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(String::new()),
        Value::String(text) => Ok(text),
        Value::Number(number) => Ok(number.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or null, got {other}"
        ))),
    }
}

