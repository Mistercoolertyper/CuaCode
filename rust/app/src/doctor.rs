//! `cuacode doctor`: which build this is, where it came from, and whether origin
//! has moved past it.
//!
//! The one way to ask that question without opening a window, and it is the
//! command both frontends point at when they have news about an update.
//! `doctor --upgrade` is the same answer acted on: pull, rebuild, restart, and
//! it prints every step rather than doing them quietly, because a pull and a
//! build that fail are things to read.

use cuacode::version::{self, News};
use cuacode::worker;

use crate::state;

pub fn run(args: &[String]) -> i32 {
    let upgrade = args.iter().any(|a| a == "--upgrade");
    let root = version::root();

    println!("{:<8} {}", "cuacode", version::ID);
    println!(
        "{:<8} {}",
        "commit",
        if version::COMMIT.is_empty() { "unknown, this build had no git" } else { version::COMMIT }
    );
    match &root {
        Some(dir) => {
            let branch = version::git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
            println!("{:<8} {}  branch {branch}", "tree", dir.display());
            println!(
                "{:<8} {}",
                "origin",
                version::git(dir, &["remote", "get-url", "origin"]).unwrap_or_else(|_| "no remote".into())
            );
        }
        None => println!("{:<8} not inside a checkout, so updates cannot be asked for", "tree"),
    }
    match version::check() {
        News::Behind(n) => println!("{:<8} behind {n} {}  ·  run cuacode doctor --upgrade", "updates", if n == 1 { "update" } else { "updates" }),
        News::Current => println!("{:<8} up to date with origin", "updates"),
        News::Unknown(why) => println!("{:<8} cannot say: {why}", "updates"),
    }
    match worker::worker_path() {
        Ok(path) => println!(
            "{:<8} {} via {}",
            "worker",
            path.display(),
            worker::python(&path).map(|p| p.display().to_string()).unwrap_or_else(|e| e)
        ),
        Err(why) => println!("{:<8} {why}", "worker"),
    }
    println!("{:<8} {}", "state", state::state_root().display());

    if !upgrade {
        return 0;
    }
    let Some(root) = root else {
        eprintln!("cuacode: --upgrade needs a checkout; this binary is not inside one");
        return 1;
    };
    println!();
    // `git pull --ff-only` rather than a merge or a rebase: a machine holding
    // this much uncommitted work must be told so, not merged into.
    if let Err(code) = step(&root, "git", &["pull", "--ff-only"]) {
        return code;
    }
    // The build is `build.sh`, the tree's own: it reads the frontends out of the
    // manifests, so a new one is not a thing this command has to learn.
    let build = root.join("build.sh");
    if !build.is_file() {
        eprintln!("cuacode: {} is missing, so nothing was rebuilt", build.display());
        return 1;
    }
    if let Err(code) = step(&root, "./build.sh", &[]) {
        return code;
    }
    println!("\ndone - restart the app to run the new build");
    0
}

/// Run one step of the upgrade in the tree, with its own output going straight to
/// the terminal: a build that is quiet until it fails is a build nobody trusts.
fn step(dir: &std::path::Path, program: &str, args: &[&str]) -> Result<(), i32> {
    println!("$ {program} {}", args.join(" "));
    match std::process::Command::new(program).args(args).current_dir(dir).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            eprintln!("cuacode: {program} exited {status}, stopping here");
            Err(1)
        }
        Err(e) => {
            eprintln!("cuacode: could not run {program}: {e}");
            Err(1)
        }
    }
}
