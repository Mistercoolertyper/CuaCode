//! CuaCode's frontend side, in Rust.
//!
//! The worker is Python and speaks line-delimited JSON; everything here is a
//! consumer of that. `wire` is the format, `worker` is the process, `session`
//! is the conversation's state, `transcript` is the conversation as rows, and
//! `attach` is a picture on its way into a message.

pub mod attach;
pub mod session;
pub mod transcript;
pub mod wire;
pub mod worker;
