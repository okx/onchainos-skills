//! Write-side agent commands:
//! - `agent create`     → pre-transaction/createAgent + broadcast
//! - `agent update`     → pre-transaction/updateAgent + broadcast
//! - `agent activate` / `agent deactivate` → agent-status (no broadcast)
//! - `agent upload`     → pre-transaction/upload-picture (multipart)
//! - `agent feedback-submit` → pre-transaction/createComment + broadcast
//! - `agent xmtp-sign`  → pre-transaction/sign-msg (no broadcast)
//!
//! Each `*_impl` builds the request body / multipart, posts, and (for
//! broadcast-bearing commands) threads into `signing::sign_and_broadcast_agent_transaction`.
//! Shared helpers live in `utils.rs`; wire signing + Erc8004Payload assembly
//! lives in `signing.rs`.

use std::fs;

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::Context;
use crate::output;

use super::args::{
    AgentStatusArgs, CreateArgs, FeedbackSubmitArgs, UpdateArgs, UploadArgs, XmtpSignArgs,
};
use super::models::{AgentCard, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_INDEX_NUM};
use super::queries::fetch_agent_for_update;
use super::signing::{
    build_erc8004_overlay, load_session_cert, load_signing_seed, resolve_xlayer_signing_account,
    sign_and_broadcast_agent_transaction, sign_key_uuid,
};
use super::utils::{
    ensure_provider_has_service, normalize_role, parse_agent_unsigned, parse_services,
    parse_u32_arg, reconstruct_post_url_for_log, redact_token_for_debug, require_non_empty,
    resolve_agent_id, resolve_optional_update_string, resolve_update_services,
    resolve_update_string, trim_or_empty, wallet_client,
};

// ─── Public command entry points ──────────────────────────────────────────

pub async fn create(args: CreateArgs, ctx: &Context) -> Result<()> {
    output::success(create_impl(&args, ctx).await?);
    Ok(())
}

pub async fn update(args: UpdateArgs, ctx: &Context) -> Result<()> {
    output::success(update_impl(&args, ctx).await?);
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

pub async fn feedback_submit(args: FeedbackSubmitArgs, ctx: &Context) -> Result<()> {
    output::success(feedback_submit_impl(&args, ctx).await?);
    Ok(())
}

pub async fn xmtp_sign(args: XmtpSignArgs, ctx: &Context) -> Result<()> {
    output::success(xmtp_sign_impl(&args, ctx).await?);
    Ok(())
}

// ─── `agent create` ───────────────────────────────────────────────────────

async fn create_impl(args: &CreateArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let (_account_id, addr_info) = resolve_xlayer_signing_account(args.address.as_deref())?;
    let from_addr = addr_info.address.clone();
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
        "sessionSignature": session_signature,
        "sessionCert": session_cert,
        "cardJson": serde_json::to_string(&card).context("failed to serialize cardJson")?,
    });
    eprintln!(
        "[agent-identity] create request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/createAgent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string())
    );
    let response = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/createAgent",
            &access_token,
            &body,
        )
        .await?;
    eprintln!(
        "[agent-identity] create response: {}",
        serde_json::to_string(&response)
            .unwrap_or_else(|_| "<serialize failed>".to_string())
    );
    let unsigned = parse_agent_unsigned(response)?;
    // erc8004Msg 三个字段：communicationAddress 由后端 pre-transaction 返回；
    // role / keyUuid 是本次 create 客户端持有的值。sessionSignature 已不再
    // 进 erc8004Msg（保留在请求体里）。
    let communication_address = unsigned
        .extra_data
        .get("communicationAddress")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let overlay = build_erc8004_overlay(&communication_address, &normalized_role, &key_uuid);
    let tx_hash = sign_and_broadcast_agent_transaction(
        &access_token,
        &unsigned,
        overlay,
        args.address.as_deref(),
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

// ─── `agent update` ───────────────────────────────────────────────────────

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
    eprintln!(
        "[agent-identity] update request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/updateAgent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );
    let update_result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/updateAgent",
            &access_token,
            &body,
        )
        .await;
    match &update_result {
        Ok(data) => eprintln!(
            "[agent-identity] update response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] update response err: {:#}", e),
    }
    let response = update_result?;
    let unsigned = parse_agent_unsigned(response)?;
    // 产品规范：update 客户端没有 erc8004Msg 子字段（communicationAddress 不可
    // 修改，role / keyUuid / sessionSignature 也不在 update 请求体里），所以
    // 整个 erc8004Msg 不写入广播 extraData。
    let tx_hash = sign_and_broadcast_agent_transaction(&access_token, &unsigned, None, None).await?;
    Ok(json!({ "txHash": tx_hash }))
}

// ─── `agent activate` / `agent deactivate` ────────────────────────────────

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

    eprintln!(
        "[agent-identity] agent-status request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent-status"),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/agent-status",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] agent-status response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] agent-status response err: {:#}", e),
    }

    result.map_err(format_api_error)?;
    Ok(())
}

// ─── `agent upload` ───────────────────────────────────────────────────────

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
    eprintln!(
        "[agent-identity] upload request: url={} access_token_len={} access_token_prefix={} file_path={} filename={} bytes_len={}",
        upload_url,
        access_token.len(),
        redact_token_for_debug(&access_token),
        file,
        filename,
        bytes.len(),
    );
    let part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
    let form = reqwest::multipart::Form::new().part("file", part);
    let result = client
        .post_authed_multipart(
            "/priapi/v5/wallet/agentic/pre-transaction/upload-picture",
            &access_token,
            form,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] upload response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] upload response err: {:#}", e),
    }

    let data = result?;

    let url = if let Some(url) = data.get("url").and_then(Value::as_str) {
        url.to_string()
    } else if let Some(first) = data.as_array().and_then(|arr| arr.first()) {
        if let Some(url) = first.get("url").and_then(Value::as_str) {
            url.to_string()
        } else if let Some(url) = first.as_str() {
            url.to_string()
        } else {
            bail!("upload response missing url");
        }
    } else {
        bail!("upload response missing url");
    };
    Ok(json!({ "url": url }))
}

// ─── `agent feedback-submit` ──────────────────────────────────────────────

async fn feedback_submit_impl(args: &FeedbackSubmitArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let (_account_id, addr_info) = resolve_xlayer_signing_account(args.address.as_deref())?;
    let from_addr = addr_info.address.clone();
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

    eprintln!(
        "[agent-identity] feedback-submit request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/createComment",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/createComment",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] feedback-submit response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] feedback-submit response err: {:#}", e),
    }

    let response = result?;
    let unsigned = parse_agent_unsigned(response)?;
    // 产品规范：feedback-submit 客户端没有 erc8004Msg 子字段（role 不用，
    // keyUuid / sessionSignature 也不在 feedback 请求体里），整体不写入广播
    // extraData。
    let tx_hash = sign_and_broadcast_agent_transaction(
        &access_token,
        &unsigned,
        None,
        args.address.as_deref(),
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

// ─── `agent xmtp-sign` ────────────────────────────────────────────────────

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

    eprintln!(
        "[agent-identity] xmtp-sign request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] xmtp-sign response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] xmtp-sign response err: {:#}", e),
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
