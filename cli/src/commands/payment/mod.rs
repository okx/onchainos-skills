pub mod a2a_pay;
pub mod addr;
pub mod decode_receipt;
pub mod dispatcher;
pub mod http_carrier;
pub mod payment_flow;
pub mod quote;
pub mod session_state;
pub mod state;
pub mod subscription;

// Re-export the dispatcher entry-points so external call sites read
// `crate::commands::payment::PaymentCommand` instead of `payment::dispatcher::PaymentCommand`.
pub use dispatcher::{execute, DefaultAction, PaymentCommand, SessionCommand};

// Re-export the two-phase quote/pay MCP entry points so
// `cli/src/mcp/mod.rs` calls `commands::payment::fetch_*` without reaching into
// submodules.
pub use decode_receipt::fetch_decode_receipt;
pub use payment_flow::{fetch_pay, fetch_session, SessionParams};
pub use quote::fetch_quote;
