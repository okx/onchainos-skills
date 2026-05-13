pub mod a2a_pay;
pub mod addr;
pub mod dispatcher;
pub mod payment_flow;

// Re-export the dispatcher entry-points so external call sites read
// `crate::commands::payment::PaymentCommand` instead of `payment::dispatcher::PaymentCommand`.
pub use dispatcher::{execute, DefaultAction, PaymentCommand, SessionCommand};
