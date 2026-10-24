//! The version id this crate was built from.
//!
//! Not a number someone remembers to bump: the commit count and short sha of
//! the tree the build ran in, so two builds of the same source agree and any
//! other build does not. `src/version.rs` reads these with `env!`, and both
//! frontends get them from that one const rather than from their own manifests,
//! which is why `fastrs` and `cuacode` say the same thing about the same build.
//!
//! No `cargo:rerun-if-changed` on purpose: the default is "any file in this
//! package changed", which is a build that is happening anyway, and pinning it
//! to this file alone would leave the stamp on a commit nobody is running.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let out = Command::new("git").args(args).current_dir(dir).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn main() {
    // A build with no git - a tarball, a vendored copy - still has to be able to
    // say something, and what it can honestly say is the number in the manifest.
    let id = match (git(&["rev-list", "--count", "HEAD"]), git(&["rev-parse", "--short", "HEAD"])) {
        (Some(n), Some(sha)) => format!("{n}.{sha}"),
        _ => format!("{}+unknown", env!("CARGO_PKG_VERSION")),
    };
    println!("cargo:rustc-env=CUACODE_VERSION={id}");
    println!("cargo:rustc-env=CUACODE_COMMIT={}", git(&["rev-parse", "HEAD"]).unwrap_or_default());
}
