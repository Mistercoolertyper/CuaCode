//! CuaCode's frontend side, in Rust.
//!
//! The worker is Python and speaks line-delimited JSON; everything here is a
//! consumer of that. `wire` is the format, `worker` is the process, `session`
//! is the conversation's state, `transcript` is the conversation as rows,
//! `attach` is a picture on its way into a message, and `version` is which build
//! this is and whether origin has moved past it.

pub mod attach;
pub mod doctor;
pub mod session;
pub mod transcript;
pub mod version;
pub mod wire;
pub mod worker;
