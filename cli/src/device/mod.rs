//! Device identity headers attached to every outbound request in
//! `ApiClient::anonymous_headers()`:
//!
//! - [`id`] — `device-id`, a stable per-machine key (ASCII).
//! - [`name`] — `device-name`, a display name (raw UTF-8).
//!
//! Both are best-effort: an unresolvable value is omitted rather than failing
//! the request. Both are memoized for the process lifetime. Neither is
//! percent-encoded, so the backend must read them as UTF-8.

pub mod id;
pub mod name;
