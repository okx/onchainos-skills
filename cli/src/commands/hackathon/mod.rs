mod register;

pub(crate) use register::require_uid_for_cefi;
pub use register::{execute, register, resolve_registration_evm_address, HackathonCommand};
