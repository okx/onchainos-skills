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
use std::time::Duration;

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::Context;
use crate::output;
use crate::wallet_api::WalletApiClient;

use super::args::{
    AgentStatusArgs, CreateArgs, FeedbackSubmitArgs, SubmitApprovalArgs, UpdateArgs, UploadArgs,
    XmtpSignArgs,
};
use super::models::{AgentCard, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_INDEX_NUM};
use super::signing::{
    build_erc8004_overlay, load_agent_signing_session, load_session_cert, load_signing_seed,
    sign_and_broadcast_agent_transaction, sign_key_uuid,
};
use super::socket::{open_identity_subscription, IdentitySubscription};
use super::utils::{
    ensure_provider_has_service, identity_ws_url, normalize_bcp47, normalize_role,
    normalize_singleton_object, parse_agent_unsigned, parse_services, parse_stars_arg,
    reconstruct_post_url_for_log, redact_token_for_debug, require_non_empty, trim_or_empty,
    wallet_client,
};

const PUSH_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-page size for the post-broadcast agent-list pagination loop. 100 is
/// well above any real wallet's agent count and minimizes round trips; the
/// loop relies on the response's `total` to know when to stop.
const AGENT_LIST_PAGE_SIZE: usize = 100;
/// Safety cap so a buggy `total` cannot trap us in an infinite paging loop
/// — 100 × 20 = 2 000 agents, ample headroom over real-world counts.
const AGENT_LIST_MAX_PAGES: usize = 20;

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
    output::success(activate_impl(&args, ctx).await?);
    Ok(())
}

pub async fn deactivate(args: AgentStatusArgs, ctx: &Context) -> Result<()> {
    output::success(deactivate_impl(&args, ctx).await?);
    Ok(())
}

pub async fn submit_approval(args: SubmitApprovalArgs, ctx: &Context) -> Result<()> {
    output::success(submit_approval_impl(&args, ctx).await?);
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
    // Description 必填规则按 role 分支：
    //   - provider（卖家）：被搜索 / 匹配的核心字段 → 必填。
    //   - requester（买家）/ evaluator（验证者）：选填；未填则上链
    //     `ProfileDescription: ""`（与 picture 一致），skill 端渲染为
    //     `未填 / (not set)`，详见 references/register.md（按 role 分支收集字段）。
    let profile_description = if normalized_role == "provider" {
        require_non_empty(args.description.as_deref(), "--description")?.to_string()
    } else {
        trim_or_empty(args.description.as_deref())
    };
    let card = AgentCard {
        role: normalized_role.clone(),
        name: require_non_empty(args.name.as_deref(), "--name")?.to_string(),
        profile_picture: trim_or_empty(args.picture.as_deref()),
        profile_description,
        communication_address: None,
        services: parse_services(args.service.as_deref())?,
    };
    ensure_provider_has_service(&card)?;
    let mut body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "fromAddr": from_addr,
        "keyUuid": key_uuid.clone(),
        "sessionSignature": session_signature,
        "sessionCert": &signing_session.session_cert,
        "cardJson": serde_json::to_string(&card).context("failed to serialize cardJson")?,
    });
    if let Some(consent_key) = &args.consent_key {
        body["consentKey"] = json!(consent_key);
    }
    if args.agreed == Some(true) {
        body["agreed"] = json!(true);
    }
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
    // Consent intercept: first-time wallet address returns a non-null consent
    // object. Return the item directly so the skill layer can display the
    // terms, collect user agreement, and re-invoke with --consent-key + --agreed true.
    if let Some(item) = response.as_array().and_then(|a| a.first()) {
        let has_consent = item
            .get("consent")
            .map(|c| !c.is_null())
            .unwrap_or(false);
        if has_consent {
            return Ok(item.clone());
        }
    }
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

    // 广播前先建立 wallet-agentic-identity 订阅；任何环节失败都降级，不阻断后续广播。
    // identity_ws_url() 默认 WS_URL_PROD（wss://wsdex.okx.com:8443/ws/v5/private），
    // OKX_AGENTIC_WS_URL 环境变量可整 URL 覆盖（dev / pre / debug 用）。
    // push-platform login 用钱包地址作为 "token" 值（不再是 JWT）。
    let subscription =
        match open_identity_subscription(&from_addr, &identity_ws_url()).await {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!(
                "[agent-identity] ws subscribe failed, falling through to broadcast-only: {e:#}"
            );
            None
        }
    };

    let tx_hash = sign_and_broadcast_agent_transaction(
        &access_token,
        &unsigned,
        overlay,
        &signing_session,
    )
    .await?;

    let push = wait_for_identity_push(subscription, &tx_hash).await;
    let agent_list = fetch_agent_list(&mut client, &access_token).await;
    let new_agent_id = compute_new_agent_id(
        push.as_ref(),
        agent_list.as_ref(),
        &from_addr,
        args.known_agent_ids.as_deref(),
    );
    Ok(assemble_identity_envelope(
        tx_hash,
        push,
        agent_list,
        new_agent_id,
    ))
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

    // 广播前先建立 wallet-agentic-identity 订阅；任何环节失败都降级，不阻断后续广播。
    // identity_ws_url() 默认 WS_URL_PROD（wss://wsdex.okx.com:8443/ws/v5/private），
    // OKX_AGENTIC_WS_URL 环境变量可整 URL 覆盖（dev / pre / debug 用）。
    // push-platform login 用钱包地址作为 "token" 值（不再是 JWT）。
    let subscription = match open_identity_subscription(
        &signing_session.addr_info.address,
        &identity_ws_url(),
    )
    .await
    {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!(
                "[agent-identity] ws subscribe failed, falling through to broadcast-only: {e:#}"
            );
            None
        }
    };

    let tx_hash =
        sign_and_broadcast_agent_transaction(&access_token, &unsigned, None, &signing_session)
            .await?;

    let push = wait_for_identity_push(subscription, &tx_hash).await;
    let agent_list = fetch_agent_list(&mut client, &access_token).await;
    let new_agent_id = compute_new_agent_id(
        push.as_ref(),
        agent_list.as_ref(),
        &signing_session.addr_info.address,
        args.known_agent_ids.as_deref(),
    );
    Ok(assemble_identity_envelope(
        tx_hash,
        push,
        agent_list,
        new_agent_id,
    ))
}

// ─── `agent activate` / `agent deactivate` ────────────────────────────────

async fn activate_impl(args: &AgentStatusArgs, ctx: &Context) -> Result<Value> {
    agent_status_impl(args, 1, ctx).await
}

async fn deactivate_impl(args: &AgentStatusArgs, ctx: &Context) -> Result<Value> {
    agent_status_impl(args, 2, ctx).await
}

async fn agent_status_impl(args: &AgentStatusArgs, status: u32, ctx: &Context) -> Result<Value> {
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

    result.map_err(format_api_error)
}

async fn submit_approval_impl(args: &SubmitApprovalArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?;
    let mut body = json!({
        "agentId": agent_id,
        "chainIndex": XLAYER_CHAIN_INDEX,
    });
    if let Some(lang) = normalize_bcp47(args.preferred_language.as_deref()) {
        body["preferredLanguage"] = Value::String(lang);
    }

    eprintln!(
        "[agent-identity] submit-approval request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/submit-approval"),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/agent/submit-approval",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] submit-approval response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] submit-approval response err: {:#}", e),
    }

    result.map_err(format_api_error)
}

// ─── `agent upload` ───────────────────────────────────────────────────────

const MAX_UPLOAD_BYTES: usize = 1024 * 1024; // 1 MB

async fn upload_impl(args: &UploadArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let file = require_non_empty(args.file.as_deref(), "--file")?;
    let bytes = fs::read(file).with_context(|| format!("failed to read file: {file}"))?;
    if bytes.len() > MAX_UPLOAD_BYTES {
        bail!(
            "file size {} bytes exceeds the 1 MB limit — please downscale the image and retry",
            bytes.len()
        );
    }
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
    // --score is 0.00–5.00 stars (up to 2 decimal places, step 0.01); the
    // CLI multiplies by 20 with round-half-up to produce the 0–100 u32
    // wire value. Mapping + format validation live in
    // `utils::parse_stars_arg` (single source of truth). Earlier revisions
    // made stars→score the skill's responsibility, which was fragile —
    // skills are prompt-driven and would occasionally forget the
    // multiplication and corrupt the rating on chain. CLI is the
    // authoritative boundary now.
    let score = parse_stars_arg(
        require_non_empty(args.score.as_deref(), "--score")?,
        "--score",
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

// ─── Post-broadcast finalize helpers (create / update) ────────────────────

/// Drain the WS subscription waiting for the matching push. Any failure
/// (including timeout) collapses to `None`; the surrounding command then
/// emits `txHash` + `agentList` only.
async fn wait_for_identity_push(
    subscription: Option<IdentitySubscription>,
    tx_hash: &str,
) -> Option<Value> {
    let sub = subscription?;
    match sub.wait_for_match(tx_hash, PUSH_WAIT_TIMEOUT).await {
        Ok(opt) => opt,
        Err(e) => {
            eprintln!("[agent-identity] ws wait failed: {e:#}");
            None
        }
    }
}

/// Best-effort fetch of the caller's full XLayer agent list (struct C).
/// Pages through `/agent/agent-list?chainIndex=196` with `pageSize=100`
/// until `total` is satisfied or `AGENT_LIST_MAX_PAGES` is hit. Returns
/// `{ total, list: [...] }`. Note the backend's actual response shape
/// uses the field name `list` (not `items` — earlier docs claimed
/// `items`; that was a pre-existing doc bug confirmed empirically on
/// 2026-05-10).
///
/// Failure modes that short-circuit to `None` so the envelope omits the
/// field rather than emit a misleading partial:
///   - any HTTP error from the client during pagination
///   - page 1 missing or non-numeric `total` field (response shape
///     anomaly — cannot trust the dataset)
///   - any page missing or non-array `list` field (same)
///
/// An empty `list` on any page stops pagination gracefully and returns
/// the aggregated results so far (does not abort to `None`). The
/// legitimate empty case (`total == 0` and page 1 returned `list: []`)
/// returns `Some({total: 0, list: []})`.
async fn fetch_agent_list(client: &mut WalletApiClient, access_token: &str) -> Option<Value> {
    let mut all_items: Vec<Value> = Vec::new();
    let mut total: u64 = 0;
    let mut all_agent_count: u64 = 0;
    let mut page: usize = 1;

    while page <= AGENT_LIST_MAX_PAGES {
        let page_str = page.to_string();
        let page_size_str = AGENT_LIST_PAGE_SIZE.to_string();
        let raw = match client
            .get_authed(
                "/priapi/v5/wallet/agentic/agent/agent-list",
                access_token,
                &[
                    ("chainIndex", XLAYER_CHAIN_INDEX),
                    ("page", &page_str),
                    ("pageSize", &page_size_str),
                ],
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[agent-identity] agent-list page {page} fetch failed: {e:#}");
                return None;
            }
        };

        // Keep the raw response for debug logging on shape-mismatch aborts.
        // Without this, the abort logs only say "shape wrong" with no clue
        // what the backend actually returned — see the post-mortem on the
        // 2026-05-10 test where the abort fired but the actual response shape
        // was unknown.
        let raw_repr = raw.to_string();
        let normalized = normalize_singleton_object(raw);

        // Page 1: total must be present and numeric. Missing/wrong-shape => abort.
        if page == 1 {
            total = match normalized.get("total").and_then(Value::as_u64) {
                Some(t) => t,
                None => {
                    eprintln!(
                        "[agent-identity] agent-list page 1 missing or non-numeric `total` field — abort. raw={raw_repr} normalized={normalized}"
                    );
                    return None;
                }
            };
        }

        // `list` must be an array on every page. Missing/wrong-shape => abort.
        // Backend uses the field name `list` (not `items`); see the doc
        // comment above this function.
        let page_items: Vec<Value> = match normalized.get("list").and_then(Value::as_array) {
            Some(arr) => arr.clone(),
            None => {
                eprintln!(
                    "[agent-identity] agent-list page {page} missing or non-array `list` field — abort. raw={raw_repr} normalized={normalized}"
                );
                return None;
            }
        };
        let page_count = page_items.len();

        // Count agents in this page: each group carries agentList[].
        let page_agent_count: u64 = page_items
            .iter()
            .filter_map(|item| item.get("agentList").and_then(Value::as_array))
            .map(|a| a.len() as u64)
            .sum();
        all_agent_count += page_agent_count;
        all_items.extend(page_items);

        // Done: accumulated agent count satisfies backend's reported total.
        // Also covers the empty case: total == 0, page 1 list == [], 0 >= 0.
        if all_agent_count >= total {
            break;
        }

        // list == [] means the backend has no more data; stop paging.
        // total may be stale — treat an empty page as end-of-data rather
        // than an error so we don't keep incrementing page indefinitely.
        if page_count == 0 {
            eprintln!(
                "[agent-identity] agent-list page {page} returned empty list (total={} agents_accumulated={}) — stopping",
                total,
                all_agent_count,
            );
            break;
        }

        page += 1;
        if page > AGENT_LIST_MAX_PAGES {
            eprintln!(
                "[agent-identity] agent-list paging hit safety cap of {} pages × {} ({} agents accumulated, backend total={})",
                AGENT_LIST_MAX_PAGES,
                AGENT_LIST_PAGE_SIZE,
                all_agent_count,
                total,
            );
        }
    }

    Some(json!({
        "total": total,
        "list": all_items,
    }))
}

/// Assemble the `{ txHash, agent?, agentList?, newAgentId }` envelope.
/// `agent` and `agentList` are independent best-effort segments; either may
/// be missing without affecting the other. `newAgentId` is always present as
/// a top-level key (string id when resolvable, JSON `null` otherwise) so
/// callers can rely on the key existing.
fn assemble_identity_envelope(
    tx_hash: String,
    push: Option<Value>,
    agent_list: Option<Value>,
    new_agent_id: Option<String>,
) -> Value {
    let mut out = json!({ "txHash": tx_hash });
    if let Some(p) = push {
        out["agent"] = p;
    }
    if let Some(list) = agent_list {
        out["agentList"] = list;
    }
    out["newAgentId"] = match new_agent_id {
        Some(id) => Value::String(id),
        None => Value::Null,
    };
    out
}

/// Compute the top-level `newAgentId` for a create / update response.
///
/// Resolution order (additive, never errors):
///   1. WS push present with an `agentId` → use it (stringified). This is the
///      authoritative signal and ignores `--known-agent-ids` entirely.
///   2. WS push absent, `agentList` present, AND `known_agent_ids` provided →
///      double-layer diff: find the wrapper whose `ownerAddress` matches the
///      signing wallet, then within that wrapper's nested `agentList[*]` pick
///      the single agentId NOT in the known set. Exactly one new id → use it;
///      zero / more-than-one / no matching wrapper → `None`.
///   3. Otherwise → `None`.
fn compute_new_agent_id(
    push: Option<&Value>,
    agent_list: Option<&Value>,
    signing_address: &str,
    known_agent_ids: Option<&str>,
) -> Option<String> {
    // Rule 1: WS push wins.
    if let Some(p) = push {
        if let Some(id) = agent_id_to_string(p.get("agentId")) {
            return Some(id);
        }
    }

    // Rule 2: diff against the pre-write snapshot.
    let known_csv = known_agent_ids?;
    let agent_list = agent_list?;
    let known: std::collections::HashSet<String> = parse_known_agent_ids(known_csv);

    diff_new_agent_id(agent_list, signing_address, &known)
}

/// Parse the `--known-agent-ids` CSV into a set of normalized id strings.
/// Whitespace-trimmed, empty entries dropped. Ids are compared as strings
/// (after normalization) so `42` and `"42"` collide regardless of JSON type.
fn parse_known_agent_ids(csv: &str) -> std::collections::HashSet<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Normalize an agentId `Value` (string or integer) to its canonical string
/// form for set comparison. `None` for null / missing / unsupported types.
fn agent_id_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Diff the post-broadcast agent list against the pre-write `known` snapshot
/// to find the single newly-minted agent id for the signing wallet. Tolerates
/// BOTH envelope shapes the `/agent-list` endpoint has used:
///   • single-layer  `list[*]`              — the live backend today: each row
///     IS an agent and carries its own `ownerAddress`.
///   • double-layer   `list[*].agentList[*]` — older grouped schema: `list[*]`
///     is an accountName wrapper carrying `ownerAddress` + nested `agentList`.
/// Only rows whose owning wallet matches `signing_address` are considered (a
/// row/wrapper with no `ownerAddress` is treated as the caller's own, since the
/// list endpoint is JWT-scoped to the caller). Returns `Some(id)` only when
/// EXACTLY ONE matching agent id is absent from `known`.
fn diff_new_agent_id(
    agent_list: &Value,
    signing_address: &str,
    known: &std::collections::HashSet<String>,
) -> Option<String> {
    let items = agent_list.get("list").and_then(Value::as_array)?;
    let signing_lower = signing_address.trim().to_ascii_lowercase();

    // A row/wrapper belongs to the signing wallet when its `ownerAddress`
    // matches (case-insensitive). Missing `ownerAddress` → treat as owned
    // (the endpoint is JWT-scoped), never as a disqualifier.
    let owner_matches = |node: &Value| -> bool {
        match node.get("ownerAddress").and_then(Value::as_str) {
            Some(addr) => addr.trim().to_ascii_lowercase() == signing_lower,
            None => true,
        }
    };

    let mut candidates: Vec<String> = Vec::new();
    for item in items {
        match item.get("agentList").and_then(Value::as_array) {
            // Double-layer: `item` is an accountName wrapper; diff its rows
            // only when the wrapper belongs to the signing wallet.
            Some(rows) if owner_matches(item) => {
                candidates.extend(rows.iter().filter_map(|r| agent_id_to_string(r.get("agentId"))));
            }
            Some(_) => {}
            // Single-layer: `item` IS the agent row, carrying its ownerAddress.
            None => {
                if owner_matches(item) {
                    if let Some(id) = agent_id_to_string(item.get("agentId")) {
                        candidates.push(id);
                    }
                }
            }
        }
    }

    candidates.retain(|id| !known.contains(id));
    candidates.sort();
    candidates.dedup();

    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_account_envelope() -> Value {
        json!({
            "total": 2,
            "list": [
                {
                    "ownerAddress": "0xSIGNER",
                    "accountName": "wallet-1",
                    "agentList": [
                        { "agentId": 10, "name": "old-a" },
                        { "agentId": 11, "name": "old-b" },
                        { "agentId": 99, "name": "new-one" },
                    ],
                },
                {
                    // Another derived wallet — must NOT be conflated.
                    "ownerAddress": "0xOTHER",
                    "accountName": "wallet-2",
                    "agentList": [ { "agentId": 500, "name": "someone-else" } ],
                },
            ],
        })
    }

    /// Live `/agent-list` shape: single-layer `list[*]`, each row IS an agent
    /// carrying its own `ownerAddress` (no `agentList` wrapper). `total` =
    /// agent count.
    fn single_layer_envelope() -> Value {
        json!({
            "total": 3,
            "list": [
                { "agentId": 10, "name": "old-a", "ownerAddress": "0xSIGNER" },
                { "agentId": 11, "name": "old-b", "ownerAddress": "0xSIGNER" },
                { "agentId": 99, "name": "new-one", "ownerAddress": "0xSIGNER" },
            ],
        })
    }

    #[test]
    fn new_agent_id_diff_single_layer_one_new_row() {
        // The real backend shape: flat rows with per-row ownerAddress.
        let list = single_layer_envelope();
        let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10,11"));
        assert_eq!(got.as_deref(), Some("99"));
    }

    #[test]
    fn new_agent_id_diff_single_layer_case_insensitive_and_no_owner_ok() {
        // Case-insensitive owner match; a row without ownerAddress is treated
        // as the caller's own (endpoint is JWT-scoped).
        let list = json!({
            "total": 2,
            "list": [
                { "agentId": 10, "name": "old", "ownerAddress": "0xSIGNER" },
                { "agentId": 99, "name": "new" }, // no ownerAddress → owned
            ],
        });
        let got = compute_new_agent_id(None, Some(&list), "0xsigner", Some("10"));
        assert_eq!(got.as_deref(), Some("99"));
    }

    #[test]
    fn new_agent_id_diff_single_layer_zero_and_two_candidates_are_null() {
        let list = single_layer_envelope();
        // all known → none new → null.
        assert_eq!(
            compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10,11,99")),
            None
        );
        // only 10 known → 11 and 99 both new → ambiguous → null.
        assert_eq!(
            compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10")),
            None
        );
    }

    #[test]
    fn new_agent_id_prefers_ws_push() {
        let push = json!({ "agentId": "12345", "txHash": "0xabc" });
        let list = two_account_envelope();
        // Even with a known-ids snapshot present, the WS push wins.
        let got = compute_new_agent_id(
            Some(&push),
            Some(&list),
            "0xSIGNER",
            Some("10,11,99"),
        );
        assert_eq!(got.as_deref(), Some("12345"));
    }

    #[test]
    fn new_agent_id_ws_push_numeric_id_stringified() {
        let push = json!({ "agentId": 777 });
        let got = compute_new_agent_id(Some(&push), None, "0xSIGNER", None);
        assert_eq!(got.as_deref(), Some("777"));
    }

    #[test]
    fn new_agent_id_diff_one_new_row() {
        let list = two_account_envelope();
        let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10,11"));
        assert_eq!(got.as_deref(), Some("99"));
    }

    #[test]
    fn new_agent_id_diff_case_insensitive_owner_match() {
        let list = two_account_envelope();
        // Signing address differs in case from the wrapper's ownerAddress.
        let got = compute_new_agent_id(None, Some(&list), "0xsigner", Some("10,11"));
        assert_eq!(got.as_deref(), Some("99"));
    }

    #[test]
    fn new_agent_id_diff_zero_candidates_is_null() {
        let list = two_account_envelope();
        // All ids known → no new candidate.
        let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10,11,99"));
        assert_eq!(got, None);
    }

    #[test]
    fn new_agent_id_diff_two_candidates_is_null() {
        let list = two_account_envelope();
        // Only 10 known → 11 and 99 are both "new" → ambiguous → null.
        let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10"));
        assert_eq!(got, None);
    }

    #[test]
    fn new_agent_id_no_matching_wrapper_is_null() {
        let list = two_account_envelope();
        let got = compute_new_agent_id(None, Some(&list), "0xNOBODY", Some("10,11"));
        assert_eq!(got, None);
    }

    #[test]
    fn new_agent_id_without_known_ids_and_no_push_is_null() {
        let list = two_account_envelope();
        // Rule 2 requires --known-agent-ids; absent → null (never errors).
        let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", None);
        assert_eq!(got, None);
    }

    #[test]
    fn envelope_always_carries_new_agent_id_key() {
        // Present case.
        let env = assemble_identity_envelope(
            "0xhash".to_string(),
            None,
            None,
            Some("42".to_string()),
        );
        assert_eq!(env["newAgentId"], json!("42"));
        // Null case — key still present.
        let env = assemble_identity_envelope("0xhash".to_string(), None, None, None);
        assert_eq!(env["newAgentId"], Value::Null);
        assert!(env.as_object().unwrap().contains_key("newAgentId"));
    }
}
