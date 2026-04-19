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
use crate::wallet_api::{BroadcastResponse, WalletApiClient};
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
    tx_param: String,
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

async fn create_impl(args: &CreateArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let (_account_id, from_addr) = resolve_xlayer_signing_account(args.address.as_deref())?;
    let signing_seed = load_signing_seed()?;
    let session_cert = load_session_cert()?;
    let key_uuid = Uuid::new_v4().to_string();
    let session_signature = sign_key_uuid(&key_uuid, &signing_seed)?;
    let card = AgentCard {
        role: normalize_role(require_non_empty(args.role.as_deref(), "--role")?)?,
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
        "keyUuid": key_uuid,
        "sessionSignature": session_signature,
        "sessionCert": session_cert,
        "cardJson": serde_json::to_string(&card).context("failed to serialize cardJson")?,
    });
    eprintln!(
        "[agent-identity-console] create request prepared: base_url={}, access_token_len={}, access_token_prefix={}, body={}",
        option_env!("OKX_BASE_URL").unwrap_or(crate::client::DEFAULT_BASE_URL),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string_pretty(&body).unwrap_or_else(|_| "<serialize failed>".to_string())
    );
    let unsigned = parse_agent_unsigned(
        client
            .post_authed(
                "/priapi/v5/wallet/agentic/pre-transaction/createAgent",
                &access_token,
                &body,
            )
            .await?,
    )?;
    let tx_hash = sign_and_broadcast_agent_transaction(
        ctx,
        &access_token,
        &unsigned,
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

    let card = AgentCard {
        // Role cannot be modified via update (not exposed as a CLI flag by product
        // spec); always echo back the existing role.
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
    let unsigned = parse_agent_unsigned(
        client
            .post_authed(
                "/priapi/v5/wallet/agentic/pre-transaction/updateAgent",
                &access_token,
                &body,
            )
            .await?,
    )?;
    let tx_hash = sign_and_broadcast_agent_transaction(
        ctx,
        &access_token,
        &unsigned,
        None,
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

async fn get_impl(args: &GetArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let page = parse_u32_arg(args.page.as_deref(), "--page", 1, Some(1), None, false)?;
    let page_size = parse_u32_arg(
        args.page_size.as_deref(),
        "--page-size",
        20,
        Some(1),
        None,
        false,
    )?;

    // Product spec: agent-list identifies the user via JWT; `from` is never needed.
    let mut query = vec![
        ("chainIndex".to_string(), XLAYER_CHAIN_INDEX.to_string()),
        ("page".to_string(), page.to_string()),
        ("pageSize".to_string(), page_size.to_string()),
    ];
    push_optional_query(&mut query, "agentIds", args.agent_ids.as_deref());

    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let data = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent-list",
            &access_token,
            &query_refs,
        )
        .await?;
    Ok(normalize_singleton_object(data))
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
    client
        .post_authed(
            "/priapi/v5/wallet/agentic/agent-status",
            &access_token,
            &body,
        )
        .await
        .map_err(format_api_error)?;
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
    eprintln!(
        "[agent-identity-console] upload request prepared: base_url={} access_token_len={} access_token_prefix={} file_path={} filename={} bytes_len={} multipart_fields=[file]",
        option_env!("OKX_BASE_URL").unwrap_or(crate::client::DEFAULT_BASE_URL),
        access_token.len(),
        redact_token_for_debug(&access_token),
        file,
        filename,
        bytes.len(),
    );
    let part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
    let form = reqwest::multipart::Form::new().part("file", part);
    let data = client
        .post_authed_multipart(
            "/priapi/v5/wallet/agentic/pre-transaction/upload-picture",
            &access_token,
            form,
        )
        .await?;

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
    let page = parse_u32_arg(args.page.as_deref(), "--page", 1, Some(1), None, false)?;
    let page_size = parse_u32_arg(
        args.page_size.as_deref(),
        "--page-size",
        20,
        Some(1),
        Some(100),
        true,
    )?;

    let mut query = vec![
        ("query".to_string(), query_text.to_string()),
        ("page".to_string(), page.to_string()),
        ("pageSize".to_string(), page_size.to_string()),
    ];
    push_multi_query(&mut query, "feedback", &args.feedback);
    push_multi_query(&mut query, "agentInfo", &args.agent_info);
    push_multi_query(&mut query, "status", &args.status);
    push_multi_query(&mut query, "service", &args.service);

    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let data = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/cli-search",
            &access_token,
            &query_refs,
        )
        .await?;
    Ok(normalize_singleton_object(data))
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
    client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/services",
            &access_token,
            &query_refs,
        )
        .await
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
    let unsigned = parse_agent_unsigned(
        client
            .post_authed(
                "/priapi/v5/wallet/agentic/pre-transaction/createComment",
                &access_token,
                &body,
            )
            .await?,
    )?;
    let tx_hash = sign_and_broadcast_agent_transaction(
        ctx,
        &access_token,
        &unsigned,
        args.address.as_deref(),
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

async fn feedback_list_impl(args: &FeedbackListArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let page = parse_u32_arg(args.page.as_deref(), "--page", 1, Some(1), None, false)?;
    let page_size = parse_u32_arg(
        args.page_size.as_deref(),
        "--page-size",
        20,
        Some(1),
        Some(50),
        true,
    )?;
    let sort_by = match args.sort_by.as_deref().unwrap_or("time_desc") {
        "time_desc" => "time_desc",
        "score_desc" => "score_desc",
        other => bail!("invalid value for --sort-by: {other}"),
    };
    let query = vec![
        (
            "agentId".to_string(),
            resolve_agent_id(&args.agent_id, &args.agent_id_flag)?.to_string(),
        ),
        ("page".to_string(), page.to_string()),
        ("pageSize".to_string(), page_size.to_string()),
        ("sortBy".to_string(), sort_by.to_string()),
    ];
    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let data = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/reviews",
            &access_token,
            &query_refs,
        )
        .await?;
    Ok(normalize_singleton_object(data))
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
    if msg_for_sign.is_empty() {
        bail!("pre-transaction response missing signable hashes");
    }

    let mut extra_data = json!({
        "txType": 3,
        "checkBalance": true,
        "encoding": unsigned.encoding,
        "signType": unsigned.sign_type,
        "teeParam": unsigned.tx_param,
        "msgForSign": Value::Object(msg_for_sign),
    });
    if !unsigned.uop_hash.is_empty() {
        extra_data["uopHash"] = json!(unsigned.uop_hash);
    }

    let extra_data_str =
        serde_json::to_string(&extra_data).context("failed to serialize broadcast extraData")?;
    let resp: BroadcastResponse = client
        .broadcast_transaction(
            access_token,
            &account_id,
            &from_addr,
            XLAYER_CHAIN_INDEX,
            &extra_data_str,
            None,
        )
        .await
        .map_err(format_api_error)?;
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

fn sign_key_uuid(key_uuid: &str, signing_seed: &[u8; 32]) -> Result<String> {
    Ok(crate::crypto::ed25519_sign_eip191(
        key_uuid,
        signing_seed,
        "utf8",
    )?)
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
        "1" | "buyer" | "requestor" | "requester" => Ok("buyer".to_string()),
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
    let data = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent-list",
            access_token,
            &query_refs,
        )
        .await?;
    let data = normalize_singleton_object(data);
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use anyhow::Result;
    use hpke::{
        aead::AesGcm256, kdf::HkdfSha256, kem::X25519HkdfSha256, single_shot_seal, OpModeS,
    };

    use crate::commands::Context;
    use crate::config::AppConfig;
    use crate::home::TEST_ENV_MUTEX;
    use crate::{keyring_store, wallet_store};

    #[test]
    fn agent_create_and_create_task_parse_under_shared_namespace() {
        use clap::Parser;

        let cli = crate::Cli::try_parse_from([
            "onchainos",
            "agent",
            "create",
            "--name",
            "demo",
            "--role",
            "provider",
            "--description",
            "provider",
        ])
        .unwrap();
        match cli.command {
            crate::Commands::TaskSystem { command } => match command {
                crate::commands::agent_commerce::AgentCommand::Create(args) => {
                    assert_eq!(args.name.as_deref(), Some("demo"));
                    assert_eq!(args.role.as_deref(), Some("provider"));
                }
                _ => panic!("unexpected command"),
            },
            _ => panic!("unexpected top-level command"),
        }

        let cli = crate::Cli::try_parse_from([
            "onchainos",
            "agent",
            "create-task",
            "--description",
            "build it",
            "--budget",
            "10",
            "--currency",
            "USDT",
            "--deadline-open",
            "24h",
            "--deadline-submit",
            "48h",
            "--quality-standards",
            "good",
        ])
        .unwrap();
        match cli.command {
            crate::Commands::TaskSystem { command } => match command {
                crate::commands::agent_commerce::AgentCommand::CreateTask { .. } => {}
                _ => panic!("unexpected command"),
            },
            _ => panic!("unexpected top-level command"),
        }
    }

    #[test]
    fn parse_services_validates_a2mcp_requirements() {
        let err = parse_services(Some(
            r#"[{"ServiceName":"test","ServiceDescription":"desc","ServiceType":"A2MCP"}]"#,
        ))
        .unwrap_err();
        assert!(format!("{err}").contains("Fee"));

        let services = parse_services(Some(
            r#"[{"ServiceName":"test","ServiceDescription":"desc","ServiceType":"A2MCP","Fee":"1","Endpoint":"https://x"}]"#,
        ))
        .unwrap();
        assert_eq!(services[0].service_type, "A2MCP");
    }

    #[test]
    fn extract_existing_agent_card_reads_embedded_card_json() {
        let agent = json!({
            "agentId": "1001",
            "cardJson": "{\"role\":\"provider\",\"Name\":\"Alice\",\"ProfilePicture\":\"https://cdn/avatar.png\",\"ProfileDescription\":\"desc\",\"CommunicationAddress\":\"0xcomm\",\"Service\":[{\"id\":\"svc-1\",\"ServiceName\":\"trade\",\"ServiceDescription\":\"desc\",\"Fee\":\"1\",\"ServiceType\":\"A2MCP\",\"Endpoint\":\"https://svc\"}]}"
        });
        let card = extract_existing_agent_card(&agent).unwrap();
        assert_eq!(card.role.as_deref(), Some("provider"));
        assert_eq!(card.name.as_deref(), Some("Alice"));
        assert_eq!(card.communication_address.as_deref(), Some("0xcomm"));
        assert_eq!(
            card.services.as_ref().unwrap()[0].id.as_deref(),
            Some("svc-1")
        );
    }

    #[test]
    fn resolve_current_xlayer_address_prefers_selected_account() {
        let wallets = wallets_fixture();
        let (account_id, addr) = resolve_current_xlayer_address(&wallets).unwrap();
        assert_eq!(account_id, "acc-1");
        assert_eq!(addr.address, "0xabc");
    }

    #[tokio::test]
    async fn create_impl_signs_and_broadcasts_agent_transaction() -> Result<()> {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let temp = prepare_test_home("agent_create_impl")?;
        std::env::set_var("ONCHAINOS_HOME", &temp);
        keyring_store::clear_all().ok();

        let session_key = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        let signing_seed = [9u8; 32];
        let encrypted_session_sk = encrypt_session_seed(&session_key, &signing_seed)?;
        let wallets = wallets_fixture_from_current_home();
        let (_, expected_addr) = resolve_current_xlayer_address(&wallets)?;
        let expected_addr = expected_addr.address;

        keyring_store::store(&[
            (
                "access_token",
                &make_jwt(chrono::Utc::now().timestamp() + 3600),
            ),
            (
                "refresh_token",
                &make_jwt(chrono::Utc::now().timestamp() + 7200),
            ),
            ("session_key", &session_key),
        ])?;
        wallet_store::save_session(&wallet_store::SessionJson {
            encrypted_session_sk,
            session_key_expire_at: (chrono::Utc::now().timestamp() + 7200).to_string(),
            ..Default::default()
        })?;
        wallet_store::save_wallets(&wallets)?;

        let requests = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));
        let server = MockServer::start(
            vec![
                MockResponse::json(
                    "/priapi/v5/wallet/agentic/pre-transaction/createAgent",
                    json!({
                        "code": "0",
                        "msg": "success",
                        "data": [{
                            "hash": "0x11",
                            "authHashFor7702": "0x22",
                            "uopHash": "0x33",
                            "signType": "eip1559Tx",
                            "encoding": "hex",
                            "txParam": "tee-param"
                        }]
                    }),
                ),
                MockResponse::json(
                    "/priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction",
                    json!({
                        "code": "0",
                        "msg": "success",
                        "data": [{
                            "txHash": "0xtx"
                        }]
                    }),
                ),
            ],
            requests.clone(),
        )?;

        let ctx = test_context(Some(server.base_url()));
        let data = create_impl(
            &CreateArgs {
                name: Some("Demo".into()),
                role: Some("provider".into()),
                description: Some("Agent".into()),
                picture: Some("https://cdn/demo.png".into()),
                service: Some(
                    r#"[{"ServiceName":"quote","ServiceDescription":"desc","Fee":"1","ServiceType":"A2MCP","Endpoint":"https://svc"}]"#
                        .into(),
                ),
                address: None,
            },
            &ctx,
        )
        .await?;

        assert_eq!(data["txHash"], "0xtx");

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let create_body: Value = serde_json::from_slice(&requests[0].body)?;
        assert_eq!(create_body["fromAddr"], expected_addr);
        assert!(create_body["keyUuid"].as_str().unwrap().len() > 10);
        assert!(create_body["sessionSignature"].as_str().unwrap().len() > 10);
        assert!(create_body["sessionCert"].is_string());
        let card_json = create_body["cardJson"].as_str().unwrap();
        let card_value: Value = serde_json::from_str(card_json)?;
        assert_eq!(card_value["Role"], "provider");
        assert_eq!(card_value["Name"], "Demo");

        let broadcast_body: Value = serde_json::from_slice(&requests[1].body)?;
        let extra_data: Value =
            serde_json::from_str(broadcast_body["extraData"].as_str().unwrap())?;
        assert_eq!(broadcast_body["accountId"], "acc-1");
        assert_eq!(broadcast_body["address"], expected_addr);
        assert_eq!(extra_data["txType"], 3);
        assert_eq!(extra_data["teeParam"], "tee-param");
        assert_eq!(extra_data["uopHash"], "0x33");
        assert!(extra_data["msgForSign"]["signature"].is_string());
        assert!(extra_data["msgForSign"]["authSignatureFor7702"].is_string());

        drop(requests);
        server.shutdown();
        keyring_store::clear_all().ok();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&temp).ok();
        Ok(())
    }

    #[tokio::test]
    async fn update_impl_merges_existing_card_fields() -> Result<()> {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let temp = prepare_test_home("agent_update_impl")?;
        std::env::set_var("ONCHAINOS_HOME", &temp);
        keyring_store::clear_all().ok();

        let session_key = base64::engine::general_purpose::STANDARD.encode([8u8; 32]);
        let signing_seed = [5u8; 32];
        let encrypted_session_sk = encrypt_session_seed(&session_key, &signing_seed)?;

        keyring_store::store(&[
            (
                "access_token",
                &make_jwt(chrono::Utc::now().timestamp() + 3600),
            ),
            (
                "refresh_token",
                &make_jwt(chrono::Utc::now().timestamp() + 7200),
            ),
            ("session_key", &session_key),
        ])?;
        wallet_store::save_session(&wallet_store::SessionJson {
            encrypted_session_sk,
            session_key_expire_at: (chrono::Utc::now().timestamp() + 7200).to_string(),
            ..Default::default()
        })?;
        wallet_store::save_wallets(&wallets_fixture_from_current_home())?;

        let requests = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));
        let server = MockServer::start(
            vec![
                MockResponse::json(
                    "/priapi/v5/wallet/agentic/agent-list",
                    json!({
                        "code": "0",
                        "msg": "success",
                        "data": {
                            "list": [{
                                "agentId": "1001",
                                "name": "Current Name",
                                "profilePicture": "https://cdn/current.png",
                                "profileDescription": "Current Desc",
                                "cardJson": "{\"role\":\"provider\",\"CommunicationAddress\":\"0xcomm\",\"Service\":[{\"id\":\"svc-1\",\"ServiceName\":\"old\",\"ServiceDescription\":\"old desc\",\"Fee\":\"9\",\"ServiceType\":\"A2MCP\",\"Endpoint\":\"https://old\"}]}"
                            }]
                        }
                    }),
                ),
                MockResponse::json(
                    "/priapi/v5/wallet/agentic/pre-transaction/updateAgent",
                    json!({
                        "code": "0",
                        "msg": "success",
                        "data": [{
                            "hash": "0xaa",
                            "authHashFor7702": "0xbb",
                            "signType": "eip1559Tx",
                            "encoding": "hex",
                            "txParam": "tee-param"
                        }]
                    }),
                ),
                MockResponse::json(
                    "/priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction",
                    json!({
                        "code": "0",
                        "msg": "success",
                        "data": [{
                            "txHash": "0xupdated"
                        }]
                    }),
                ),
            ],
            requests.clone(),
        )?;

        let ctx = test_context(Some(server.base_url()));
        let data = update_impl(
            &UpdateArgs {
                agent_id: Some("1001".into()),
                agent_id_flag: None,
                name: Some("New Name".into()),
                description: None,
                picture: None,
                service: None,
            },
            &ctx,
        )
        .await?;
        assert_eq!(data["txHash"], "0xupdated");

        let requests = requests.lock().unwrap();
        let update_body: Value = serde_json::from_slice(&requests[1].body)?;
        assert!(update_body["sessionCert"].is_string());
        let card_value: Value = serde_json::from_str(update_body["cardJson"].as_str().unwrap())?;
        assert_eq!(card_value["Name"], "New Name");
        assert_eq!(card_value["Role"], "provider");
        assert_eq!(card_value["ProfileDescription"], "Current Desc");
        assert!(card_value["CommunicationAddress"].is_null());
        assert_eq!(card_value["Service"][0]["id"], "svc-1");

        drop(requests);
        server.shutdown();
        keyring_store::clear_all().ok();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&temp).ok();
        Ok(())
    }

    #[tokio::test]
    async fn get_impl_uses_xlayer_default_address_and_query_params() -> Result<()> {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let temp = prepare_test_home("agent_get_impl")?;
        std::env::set_var("ONCHAINOS_HOME", &temp);
        keyring_store::clear_all().ok();

        keyring_store::store(&[
            (
                "access_token",
                &make_jwt(chrono::Utc::now().timestamp() + 3600),
            ),
            (
                "refresh_token",
                &make_jwt(chrono::Utc::now().timestamp() + 7200),
            ),
        ])?;
        wallet_store::save_wallets(&wallets_fixture())?;

        let requests = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));
        let server = MockServer::start(
            vec![MockResponse::json(
                "/priapi/v5/wallet/agentic/agent-list",
                json!({
                    "code": "0",
                    "msg": "success",
                    "data": {
                        "total": 1,
                        "page": 2,
                        "pageSize": 5,
                        "list": [{"agentId": "1001"}]
                    }
                }),
            )],
            requests.clone(),
        )?;

        let ctx = test_context(Some(server.base_url()));
        let data = get_impl(
            &GetArgs {
                agent_ids: Some("1001".into()),
                page: Some("2".into()),
                page_size: Some("5".into()),
            },
            &ctx,
        )
        .await?;
        assert_eq!(data["page"], 2);
        assert_eq!(data["list"][0]["agentId"], "1001");

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].path.contains("from="));
        assert!(requests[0].path.contains("chainIndex=196"));
        assert!(requests[0].path.contains("agentIds=1001"));
        assert!(requests[0].path.contains("page=2"));
        assert!(requests[0].path.contains("pageSize=5"));

        drop(requests);
        server.shutdown();
        keyring_store::clear_all().ok();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&temp).ok();
        Ok(())
    }

    #[tokio::test]
    async fn feedback_submit_impl_builds_comment_and_broadcasts() -> Result<()> {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let temp = prepare_test_home("agent_feedback_submit_impl")?;
        std::env::set_var("ONCHAINOS_HOME", &temp);
        keyring_store::clear_all().ok();

        let session_key = base64::engine::general_purpose::STANDARD.encode([6u8; 32]);
        let signing_seed = [4u8; 32];
        let encrypted_session_sk = encrypt_session_seed(&session_key, &signing_seed)?;
        let wallets = wallets_fixture_from_current_home();
        let (_, expected_addr) = resolve_current_xlayer_address(&wallets)?;
        let expected_addr = expected_addr.address;

        keyring_store::store(&[
            (
                "access_token",
                &make_jwt(chrono::Utc::now().timestamp() + 3600),
            ),
            (
                "refresh_token",
                &make_jwt(chrono::Utc::now().timestamp() + 7200),
            ),
            ("session_key", &session_key),
        ])?;
        wallet_store::save_session(&wallet_store::SessionJson {
            encrypted_session_sk,
            session_key_expire_at: (chrono::Utc::now().timestamp() + 7200).to_string(),
            ..Default::default()
        })?;
        wallet_store::save_wallets(&wallets)?;

        let requests = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));
        let server = MockServer::start(
            vec![
                MockResponse::json(
                    "/priapi/v5/wallet/agentic/pre-transaction/createComment",
                    json!({
                        "code": "0",
                        "msg": "success",
                        "data": [{
                            "hash": "0x44",
                            "authHashFor7702": "0x55",
                            "signType": "eip1559Tx",
                            "encoding": "hex",
                            "txParam": "feedback-tee"
                        }]
                    }),
                ),
                MockResponse::json(
                    "/priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction",
                    json!({
                        "code": "0",
                        "msg": "success",
                        "data": [{
                            "txHash": "0xfeed"
                        }]
                    }),
                ),
            ],
            requests.clone(),
        )?;

        let ctx = test_context(Some(server.base_url()));
        let data = feedback_submit_impl(
            &FeedbackSubmitArgs {
                agent_id: Some("1001".into()),
                score: Some("95".into()),
                tags: Some("fast,accurate".into()),
                endpoint: Some("https://svc".into()),
                feedback_uri: Some("ipfs://cid".into()),
                feedback_hash: Some("0xhash".into()),
                description: Some("Great service".into()),
                address: None,
            },
            &ctx,
        )
        .await?;
        assert_eq!(data["txHash"], "0xfeed");

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let comment_body: Value = serde_json::from_slice(&requests[0].body)?;
        assert_eq!(comment_body["fromAddr"], expected_addr);
        let comment: Value = serde_json::from_str(comment_body["comment"].as_str().unwrap())?;
        assert_eq!(comment["agentId"], "1001");
        assert_eq!(comment["score"], "95");
        assert_eq!(comment["description"], "Great service");

        let broadcast_body: Value = serde_json::from_slice(&requests[1].body)?;
        let extra_data: Value =
            serde_json::from_str(broadcast_body["extraData"].as_str().unwrap())?;
        assert_eq!(extra_data["teeParam"], "feedback-tee");
        assert!(extra_data["msgForSign"]["signature"].is_string());

        drop(requests);
        server.shutdown();
        keyring_store::clear_all().ok();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&temp).ok();
        Ok(())
    }

    #[test]
    #[ignore = "manual real API debug for agent-identity-console logs"]
    fn agent_identity_console_real_api_invalid_token_logs_request_and_response() -> Result<()> {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let temp = prepare_test_home("agent_identity_console_real_api")?;
        std::env::set_var("ONCHAINOS_HOME", &temp);
        keyring_store::clear_all().ok();

        let session_key = base64::engine::general_purpose::STANDARD.encode([3u8; 32]);
        let signing_seed = [2u8; 32];
        let encrypted_session_sk = encrypt_session_seed(&session_key, &signing_seed)?;

        keyring_store::store(&[
            (
                "access_token",
                &make_jwt(chrono::Utc::now().timestamp() + 3600),
            ),
            (
                "refresh_token",
                &make_jwt(chrono::Utc::now().timestamp() + 7200),
            ),
            ("session_key", &session_key),
        ])?;
        wallet_store::save_session(&wallet_store::SessionJson {
            encrypted_session_sk,
            session_key_expire_at: (chrono::Utc::now().timestamp() + 7200).to_string(),
            ..Default::default()
        })?;
        wallet_store::save_wallets(&wallets_fixture())?;

        let binary = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("onchainos");
        let output = Command::new(binary)
            .env("ONCHAINOS_HOME", &temp)
            .args([
                "agent",
                "create",
                "--name",
                "name",
                "--chain",
                "196",
                "--role",
                "buyer",
                "--description",
                "description",
            ])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("STDOUT:\n{stdout}");
        println!("STDERR:\n{stderr}");

        let combined = format!("{stdout}\n{stderr}");
        assert!(combined.contains("agent-identity-console"));
        assert!(combined.contains("10008") || combined.contains("access token invalid"));

        keyring_store::clear_all().ok();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&temp).ok();
        Ok(())
    }

    fn test_context(base_url_override: Option<String>) -> Context {
        Context {
            config: AppConfig::default(),
            base_url_override,
            chain_override: None,
        }
    }

    fn wallets_fixture() -> WalletsJson {
        let mut accounts_map = HashMap::new();
        accounts_map.insert(
            "acc-1".to_string(),
            wallet_store::AccountMapEntry {
                address_list: vec![AddressInfo {
                    account_id: "acc-1".into(),
                    address: "0xabc".into(),
                    chain_index: XLAYER_CHAIN_INDEX.into(),
                    chain_name: XLAYER_CHAIN_NAME.into(),
                    address_type: "aa".into(),
                    chain_path: String::new(),
                }],
            },
        );
        WalletsJson {
            selected_account_id: "acc-1".into(),
            accounts_map,
            ..Default::default()
        }
    }

    fn wallets_fixture_from_current_home() -> WalletsJson {
        let path = match dirs::home_dir() {
            Some(home) => home.join(".onchainos").join("wallets.json"),
            None => return wallets_fixture(),
        };
        let Ok(data) = fs::read_to_string(path) else {
            return wallets_fixture();
        };
        let Ok(wallets) = serde_json::from_str::<WalletsJson>(&data) else {
            return wallets_fixture();
        };
        if wallets
            .accounts_map
            .values()
            .flat_map(|entry| entry.address_list.iter())
            .any(|addr| addr.chain_index == XLAYER_CHAIN_INDEX)
        {
            wallets
        } else {
            wallets_fixture()
        }
    }

    fn make_jwt(exp: i64) -> String {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"alg":"none","typ":"JWT"}"#);
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#));
        format!("{header}.{payload}.sig")
    }

    fn encrypt_session_seed(session_key_b64: &str, signing_seed: &[u8; 32]) -> Result<String> {
        use hpke::{Deserializable, Serializable};

        let session_key_bytes =
            base64::engine::general_purpose::STANDARD.decode(session_key_b64)?;
        let mut session_key = [0u8; 32];
        session_key.copy_from_slice(&session_key_bytes);
        let x25519_sk = x25519_dalek::StaticSecret::from(session_key);
        let x25519_pk = x25519_dalek::PublicKey::from(&x25519_sk);
        let receiver_pk =
            <X25519HkdfSha256 as hpke::Kem>::PublicKey::from_bytes(x25519_pk.as_bytes()).unwrap();
        let mut rng = rand::rngs::OsRng;
        let (encapped, ciphertext) =
            single_shot_seal::<AesGcm256, HkdfSha256, X25519HkdfSha256, _>(
                &OpModeS::Base,
                &receiver_pk,
                b"okx-tee-sign",
                signing_seed,
                &[],
                &mut rng,
            )
            .map_err(|e| anyhow!("hpke seal failed: {e}"))?;
        let mut out = encapped.to_bytes().to_vec();
        out.extend_from_slice(&ciphertext);
        Ok(base64::engine::general_purpose::STANDARD.encode(out))
    }

    fn prepare_test_home(name: &str) -> Result<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[derive(Clone, Debug)]
    struct MockResponse {
        path: String,
        body: Vec<u8>,
        content_type: &'static str,
    }

    impl MockResponse {
        fn json(path: &str, body: Value) -> Self {
            Self {
                path: path.to_string(),
                body: serde_json::to_vec(&body).unwrap(),
                content_type: "application/json",
            }
        }
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        path: String,
        body: Vec<u8>,
    }

    struct MockServer {
        addr: SocketAddr,
        join: Option<thread::JoinHandle<()>>,
    }

    impl MockServer {
        fn start(
            responses: Vec<MockResponse>,
            requests: Arc<Mutex<Vec<RecordedRequest>>>,
        ) -> Result<Self> {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let addr = listener.local_addr()?;
            let join = thread::spawn(move || {
                for response in responses {
                    let (mut stream, _) = listener.accept().unwrap();
                    let request = read_request(&mut stream).unwrap();
                    requests.lock().unwrap().push(request.clone());
                    let request_path = request.path.split('?').next().unwrap_or(&request.path);
                    assert_eq!(request_path, response.path);
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.content_type,
                        response.body.len()
                    );
                    stream.write_all(reply.as_bytes()).unwrap();
                    stream.write_all(&response.body).unwrap();
                    stream.flush().unwrap();
                }
            });
            Ok(Self {
                addr,
                join: Some(join),
            })
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn shutdown(mut self) {
            if let Some(join) = self.join.take() {
                join.join().unwrap();
            }
        }
    }

    fn read_request(stream: &mut std::net::TcpStream) -> Result<RecordedRequest> {
        let mut buf = Vec::new();
        let mut header_end = None;
        loop {
            let mut chunk = [0u8; 1024];
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = find_bytes(&buf, b"\r\n\r\n") {
                header_end = Some(pos + 4);
                break;
            }
        }
        let header_end = header_end.ok_or_else(|| anyhow!("missing request header terminator"))?;
        let headers = &buf[..header_end];
        let header_text = String::from_utf8_lossy(headers);
        let mut lines = header_text.lines();
        let request_line = lines
            .next()
            .ok_or_else(|| anyhow!("missing request line"))?;
        let path = request_line
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| anyhow!("missing request path"))?
            .to_string();

        let mut content_length = 0usize;
        for line in lines {
            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = value.trim().parse::<usize>()?;
            } else if let Some(value) = line.strip_prefix("content-length:") {
                content_length = value.trim().parse::<usize>()?;
            }
        }

        let mut body = buf[header_end..].to_vec();
        while body.len() < content_length {
            let mut chunk = vec![0u8; content_length - body.len()];
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }

        Ok(RecordedRequest { path, body })
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }
}
