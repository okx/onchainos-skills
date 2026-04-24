//! CLI `Args` definitions for every `onchainos agent ...` subcommand under
//! the identity module. Only clap structs live here — no business logic.

use clap::Args;

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
    /// 必填：被评价的 agent id，进 create-comment 请求体 `comment.agentid`。
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// 必填：评价发起方的 agent id，进广播 `extraData.erc8004Msg.feedBackAgentId`。
    #[arg(long = "creator-id")]
    pub creator_id: Option<String>,
    /// 必填：0-100 的整数分数，进 create-comment 请求体 `comment.value`。
    #[arg(long)]
    pub score: Option<String>,
    /// 选填：文字评价，进 create-comment 请求体 `comment.comment`。
    #[arg(long)]
    pub description: Option<String>,
    /// 选填：taskId，进广播 `extraData.erc8004Msg.taskId`；为空则不写入。
    #[arg(long = "task-id")]
    pub task_id: Option<String>,
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
