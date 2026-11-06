//! cuacode-gui — zeron's own viewport, built from the vendored tree at
//! `rust/zeron` (MIT, pinned there at the version in `zeron/Cargo.toml`).
//!
//! Nothing in this file draws. It resolves where the window keeps its state
//! and hands zeron's `run_app` the config; the window, the engine and every
//! pixel are zeron's. The wiring to CuaCode's own worker comes after the
//! window is proven to open.

use std::path::{Path, PathBuf};

fn main() {
    // The CuaCode harness spawns `main.py`. This app knows where its own tree
    // is, so it says so once rather than letting the driver guess from a
    // working directory.
    if let Ok(main) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../main.py")
        .canonicalize()
    {
        // SAFETY: nothing else runs yet; `run_app` starts the threads below.
        unsafe { std::env::set_var("CUACODE_MAIN", main) };
    }
    resolve_python();
    // zeron's engine embeds in process when the port is 0 (ARCHITECTURE §1),
    // so one launch is one process and no daemon is left behind.
    zeron_ui::run_app(zeron_ui::UiConfig {
        data_dir: data_dir(),
        ipc_port: 0,
        // Unused while `workos_client_id` is None: that resolves to zeron's
        // development scope, which never dials the edge (crates/cli/src/env.rs).
        edge_url: "https://edge.zeron.sh".into(),
        edge_token: None,
        org_id: None,
        workos_client_id: None,
        // CuaCode's own worker is the agent this app exists for; zeron's
        // native harnesses stay in the picker beside it.
        default_harness: zeron_ui::HarnessId::CuaCode,
        initial_url: None,
    });
}

/// `CUACODE_ZERON_DATA` wins, then a folder under the CuaCode state root, so
/// the window's store never mixes with a real zeron install in `~/Library`.
fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CUACODE_ZERON_DATA") {
        return PathBuf::from(dir);
    }
    let root = std::env::var_os("CUACODE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cuacode")))
        .unwrap_or_else(|| PathBuf::from("."));
    root.join("gui").join("zeron-data")
}

/// Finder hands an app launchd's `PATH` (`/usr/bin:/bin:...`), where `python3`
/// is the system stub with none of the worker's packages. The login shell is
/// the only thing that can answer which python the person actually installed,
/// so ask it once, before any run starts.
fn resolve_python() {
    if std::env::var_os("CUACODE_PYTHON").is_some() {
        return;
    }
    let Ok(output) = std::process::Command::new("/bin/zsh")
        .args(["-lic", "command -v python3"])
        .output()
    else {
        return;
    };
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !path.is_empty() {
        // SAFETY: still before any thread exists; `run_app` starts them below.
        unsafe { std::env::set_var("CUACODE_PYTHON", path) };
    }
}
