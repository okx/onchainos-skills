//! Shared wallet order history query and response handling.

mod query;
mod response;

pub(super) use query::cmd_query_history;
