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
use super::signing::{
    build_erc8004_overlay, load_agent_signing_session, load_session_cert, load_signing_seed,
    sign_and_broadcast_agent_transaction, sign_key_uuid,
};
use super::utils::{
    ensure_provider_has_service, normalize_role, parse_agent_unsigned, parse_services,
    parse_u32_arg, poll_tx_agent_status, reconstruct_post_url_for_log, redact_token_for_debug,
    require_non_empty, trim_or_empty, wallet_client,
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
    let mut client = wallet_client(ctx)?;
    // --address 已从 CLI 去掉；广播走默认 XLayer 地址（当前选中账号）。
    let signing_session = load_agent_signing_session(None)?;
    let from_addr = signing_session.addr_info.address.clone();
    let key_uuid = Uuid::new_v4().to_string();
    let session_signature = sign_key_uuid(&key_uuid, &signing_session.signing_seed)?;
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
        "sessionCert": &signing_session.session_cert,
        "cardJson": serde_json::to_string(&card).context("failed to serialize cardJson")?,
    });
    eprintln!(
        "[agent-identity] create request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/create-agent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string())
    );
    let response = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/create-agent",
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
    let overlay = build_erc8004_overlay(&[
        ("communicationAddress", &communication_address),
        ("role", &normalized_role),
        ("keyUuid", &key_uuid),
    ]);
    let tx_hash = sign_and_broadcast_agent_transaction(
        &access_token,
        &unsigned,
        overlay,
        &signing_session,
    )
    .await?;
    let agent_info = poll_tx_agent_status(&mut client, &access_token, &tx_hash).await;
    match agent_info {
        Some(agent) => Ok(json!({ "txHash": tx_hash, "agent": agent })),
        None => Ok(json!({ "txHash": tx_hash })),
    }
}

// ─── `agent update` ───────────────────────────────────────────────────────

async fn update_impl(args: &UpdateArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?;
    let signing_session = load_agent_signing_session(None)?;

    // 产品规范：update 不允许修改 role / CommunicationAddress，所以都不写进
    // cardJson。其它字段只写用户本次传进来的——没传就不带；是否保留旧值由
    // 服务端决定。CLI 不再预先 GET /agent/agent-list 回填，那一步由上层 skill
    // 按需指引用户完成。
    //
    // agentId 放进 cardJson（后端按 cardJson.AgentId 识别目标），请求体顶层
    // 不带。AgentId 保持 PascalCase 不在本次 spec 改名范围；name / image /
    // services 走新 lowercase schema；ProfileDescription 亦未在 spec 中点名，
    // 故保留原 PascalCase。
    let mut card = serde_json::Map::new();
    card.insert("AgentId".into(), json!(agent_id));
    let name = trim_or_empty(args.name.as_deref());
    if !name.is_empty() {
        card.insert("name".into(), json!(name));
    }
    let description = trim_or_empty(args.description.as_deref());
    if !description.is_empty() {
        card.insert("ProfileDescription".into(), json!(description));
    }
    let picture = trim_or_empty(args.picture.as_deref());
    if !picture.is_empty() {
        card.insert("image".into(), json!(picture));
    }
    if args.service.is_some() {
        let services = parse_services(args.service.as_deref())?;
        card.insert(
            "services".into(),
            serde_json::to_value(&services).context("failed to serialize services list")?,
        );
    }

    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "sessionCert": &signing_session.session_cert,
        "cardJson": serde_json::to_string(&Value::Object(card))
            .context("failed to serialize cardJson")?,
    });
    eprintln!(
        "[agent-identity] update request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/update-agent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );
    let update_result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/update-agent",
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
    let tx_hash =
        sign_and_broadcast_agent_transaction(&access_token, &unsigned, None, &signing_session)
            .await?;
    let agent_info = poll_tx_agent_status(&mut client, &access_token, &tx_hash).await;
    match agent_info {
        Some(agent) => Ok(json!({ "txHash": tx_hash, "agent": agent })),
        None => Ok(json!({ "txHash": tx_hash })),
    }
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
    let mut client = wallet_client(ctx)?;
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?;
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
    let file = require_non_empty(args.file.as_deref(), "--file")?;
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
    let mut client = wallet_client(ctx)?;
    // --agent-id / --creator-id / --score 必填；--description / --task-id 选填。
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?.to_string();
    let creator_id = require_non_empty(args.creator_id.as_deref(), "--creator-id")?.to_string();
    let score = parse_u32_arg(
        Some(require_non_empty(args.score.as_deref(), "--score")?),
        "--score",
        0,
        Some(0),
        Some(100),
        false,
    )?;
    let feedback_desc = trim_or_empty(args.description.as_deref());
    let task_id = trim_or_empty(args.task_id.as_deref());
    let signing_session = load_agent_signing_session(None)?;

    // 请求体：create-comment 需要 chainIndex + sessionCert + feedBackAgentId +
    // comment（fromAddr 已不再带）。feedBackAgentId 是评价发起方的 agent id，与
    // 广播 extraData.erc8004Msg.feedBackAgentId 同源（均来自 --creator-id），
    // 但在请求体里放在顶层（和 chainIndex 同级），不进 comment 子对象。
    // 本地 XLayer 地址解析仍然要做，结果只给下一步广播用。
    // 注意：comment 子对象里的 "comment" 字段就是原来的 feedbackDesc，
    // 与外层 body.comment（序列化后的 JSON 字符串）同名但是不同层级，别混淆。
    let comment = json!({
        "agentid": agent_id,
        "value": score.to_string(),
        "comment": feedback_desc,
    });
    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "sessionCert": &signing_session.session_cert,
        "feedBackAgentId": creator_id,
        "comment": serde_json::to_string(&comment).context("failed to serialize comment")?,
    });

    eprintln!(
        "[agent-identity] feedback-submit request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/create-comment",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/create-comment",
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
    // erc8004Msg：feedBackAgentId 必填（来自 --creator-id），taskId 选填。空值
    // 由 build_erc8004_overlay 过滤，所以 taskId 空串不会写进 erc8004Msg。
    let overlay = build_erc8004_overlay(&[
        ("taskId", &task_id),
        ("feedBackAgentId", &creator_id),
    ]);
    // --address 已从 CLI 去掉；广播走默认 XLayer 地址（当前选中账号）。
    let tx_hash = sign_and_broadcast_agent_transaction(
        &access_token,
        &unsigned,
        overlay,
        &signing_session,
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
    let mut client = wallet_client(ctx)?;

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
