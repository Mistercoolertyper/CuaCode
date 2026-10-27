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

/// The state root: `CUACODE_HOME` when it is set, `~/.cuacode` otherwise. The
/// same resolution `handler/session/store.py` does, so the window, the doctor
/// and the worker all read one directory.
pub fn state_root() -> std::path::PathBuf {
    if let Some(root) = std::env::var_os("CUACODE_HOME") {
        return std::path::PathBuf::from(root);
    }
    home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".cuacode")
}

/// The user's home directory. A Windows shell sets no `HOME`, so `USERPROFILE`
/// is what answers there; Python's `expanduser` reads the same one.
pub fn home_dir() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(std::path::PathBuf::from)
    }
}
