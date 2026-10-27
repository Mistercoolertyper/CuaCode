//! `cuacode doctor`: which build this is, where it came from, and whether origin
//! has moved past it.
//!
//! This lives in the shared crate rather than the GUI so Windows can answer the
//! question without building or opening the GUI, and so every frontend and the
//! launcher use one implementation. It never starts the worker or a session.
//! `doctor --upgrade` is the same answer acted on: pull, rebuild, restart, and
//! print every step because a pull and a build that fail are things to read.

use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use crate::version::{self, News};
use crate::worker;

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    upgrade: bool,
    help: bool,
}

/// Print the build, checkout, update, worker, and state-root report.
///
/// A leading `doctor` is accepted so a launcher can forward its whole command
/// line without having to remove the subcommand first. `--upgrade` pulls with
/// `--ff-only` and then rebuilds the platform's frontends.
pub fn run(args: &[String]) -> i32 {
    let options = match parse_args(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("cuacode doctor: {message}");
            eprintln!("run `cuacode doctor --help` for usage");
            return 2;
        }
    };
    if options.help {
        print_help();
        return 0;
    }

    let root = version::root();
    println!("{:<8} {}", "cuacode", version::ID);
    println!(
        "{:<8} {}",
        "commit",
        if version::COMMIT.is_empty() {
            "unknown, this build had no git"
        } else {
            version::COMMIT
        }
    );
    match &root {
        Some(dir) => {
            let branch =
                version::git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
            println!("{:<8} {}  branch {branch}", "tree", dir.display());
            println!(
                "{:<8} {}",
                "origin",
                version::git(dir, &["remote", "get-url", "origin"])
                    .unwrap_or_else(|_| "no remote".into())
            );
        }
        None => println!(
            "{:<8} not inside a checkout, so updates cannot be asked for",
            "tree"
        ),
    }
    match version::check() {
        News::Behind(n) => println!(
            "{:<8} behind {n} {} - run cuacode doctor --upgrade",
            "updates",
            if n == 1 { "update" } else { "updates" }
        ),
        News::Current => println!("{:<8} up to date with origin", "updates"),
        News::Unknown(why) => println!("{:<8} cannot say: {why}", "updates"),
    }
    match worker::worker_path() {
        Ok(path) => println!(
            "{:<8} {} via {}",
            "worker",
            path.display(),
            worker::python(&path)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|e| e)
        ),
        Err(why) => println!("{:<8} {why}", "worker"),
    }
    println!("{:<8} {}", "state", crate::state_root().display());

    if !options.upgrade {
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
    if let Err(code) = rebuild(&root) {
        return code;
    }
    println!("\ndone - restart the app to run the new build");
    0
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args;
    if args
        .first()
        .is_some_and(|arg| arg.eq_ignore_ascii_case("doctor"))
    {
        args = &args[1..];
    }
    for arg in args {
        match arg.as_str() {
            "--upgrade" => options.upgrade = true,
            "--help" | "-h" => options.help = true,
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(options)
}

fn print_help() {
    println!(
        "CuaCode doctor\n\
         \n\
         Usage: cuacode doctor [--upgrade]\n\
         \n\
         Print the build, checkout, update, worker, and state-root report.\n\
         --upgrade pulls with --ff-only and rebuilds the frontends.\n\
         -h, --help shows this help."
    );
}

/// Rebuild the frontends this platform can run. Unix keeps the tree's own
/// `build.sh`; Windows calls Cargo directly so neither WSL nor Git Bash is a
/// dependency.
fn rebuild(root: &Path) -> Result<(), i32> {
    #[cfg(windows)]
    {
        rebuild_windows(root)
    }
    #[cfg(not(windows))]
    {
        let build = root.join("build.sh");
        if !build.is_file() {
            eprintln!(
                "cuacode: {} is missing, so nothing was rebuilt",
                build.display()
            );
            return Err(1);
        }
        step(root, "./build.sh", &[])
    }
}

#[cfg(windows)]
fn rebuild_windows(root: &Path) -> Result<(), i32> {
    let rust = root.join("rust");
    if !rust.is_dir() {
        eprintln!(
            "cuacode: {} is missing, so nothing was rebuilt",
            rust.display()
        );
        return Err(1);
    }
    let Some(cargo) = find_cargo() else {
        eprintln!("cuacode: cargo was not found on PATH or in ~/.cargo/bin");
        return Err(1);
    };
    let dev_cmd = find_vs_dev_cmd();

    // Doctor first, in a target directory of its own: cargo may not write the
    // image of the process running this function on Windows, and the launcher
    // swaps the staged copy in on the next launch. Doing it before the
    // frontends matters - a frontend build that fails must not be the reason a
    // tree stays stuck on an old doctor, since doctor is what reports the
    // failure.
    if rust.join("src/bin/cuacode-doctor.rs").is_file() {
        let target = rust.join("target").join("doctor-next");
        cargo_build(
            &cargo,
            dev_cmd.as_deref(),
            &rust,
            &["--release", "-p", "cuacode-rs", "--bin", "cuacode-doctor"],
            Some(&target),
        )?;
        let built = target.join("release").join("cuacode-doctor.exe");
        let release = rust.join("target").join("release");
        let next = release.join("cuacode-doctor.next.exe");
        if let Err(e) = std::fs::create_dir_all(&release) {
            eprintln!("cuacode: could not create {}: {e}", release.display());
            return Err(1);
        }
        if let Err(e) = std::fs::copy(&built, &next) {
            eprintln!(
                "cuacode: could not stage {} as {}: {e}",
                built.display(),
                next.display()
            );
            return Err(1);
        }
        println!("staged the next doctor at {}", next.display());
    }

    // One package and one binary per call, which is the pairing every cargo
    // version agrees on: a `-p` list beside a `--bin` list can be read as "the
    // bins of the default package" and fail with "no bin target named `quik`".
    cargo_build(
        &cargo,
        dev_cmd.as_deref(),
        &rust,
        &["--release", "-p", "cuacode-quik", "--bin", "quik"],
        None,
    )?;
    cargo_build(
        &cargo,
        dev_cmd.as_deref(),
        &rust,
        &["--release", "-p", "cuacode-fastrs", "--bin", "fastrs"],
        None,
    )?;
    Ok(())
}

#[cfg(windows)]
fn cargo_build(
    cargo: &Path,
    dev_cmd: Option<&Path>,
    rust: &Path,
    args: &[&str],
    target_dir: Option<&Path>,
) -> Result<(), i32> {
    println!("$ cargo build {}", args.join(" "));

    // rustc finds link.exe, but it does not find the MSVC and Windows SDK
    // libraries unless the caller's environment names them. VsDevCmd supplies
    // LIB/INCLUDE, and vswhere finds the installation with the C++ tools. No
    // `-arch` is passed: VsDevCmd then takes the host it is running on, which
    // is the target cargo builds for by default, x64 or arm64 alike.
    let mut command = if let Some(dev_cmd) = dev_cmd {
        let mut command = std::process::Command::new("cmd.exe");
        // Pass the batch file as its own token. Putting the whole command in
        // one string makes Rust escape the embedded quotes for CreateProcess,
        // but cmd.exe does not understand backslash-escaped quotes.
        command
            .args(["/d", "/c", "call"])
            .arg(dev_cmd)
            .args([">nul", "&&"])
            .arg(cargo)
            .arg("build")
            .args(args);
        command
    } else {
        let mut command = std::process::Command::new(cargo);
        command.arg("build").args(args);
        command
    };
    if let Some(target_dir) = target_dir {
        command.env("CARGO_TARGET_DIR", target_dir);
    }
    report_status("cargo", command.current_dir(rust).status())
}

#[cfg(windows)]
fn find_cargo() -> Option<PathBuf> {
    for variable in ["CUACODE_CARGO", "CARGO"] {
        if let Some(path) = std::env::var_os(variable).map(PathBuf::from) {
            if path.is_file() {
                return Some(path);
            }
        }
    }
    if let Some(path) = on_path("cargo.exe") {
        return Some(path);
    }
    let path = crate::home_dir()?.join(".cargo").join("bin").join("cargo.exe");
    path.is_file().then_some(path)
}

#[cfg(windows)]
fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn find_vs_dev_cmd() -> Option<PathBuf> {
    let program_files_x86 = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let vswhere = program_files_x86
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if vswhere.is_file() {
        let output = std::process::Command::new(vswhere)
            .args([
                "-all",
                "-products",
                "*",
                "-requires",
                "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                "-property",
                "installationPath",
            ])
            .output()
            .ok()?;
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let candidate = PathBuf::from(line.trim())
                    .join("Common7")
                    .join("Tools")
                    .join("VsDevCmd.bat");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Run one step of the upgrade in the tree, with its output going straight to
/// the terminal: a build that is quiet until it fails is a build nobody trusts.
fn step(dir: &Path, program: &str, args: &[&str]) -> Result<(), i32> {
    println!("$ {program} {}", args.join(" "));
    let result = std::process::Command::new(program)
        .args(args)
        .current_dir(dir)
        .status();
    report_status(program, result)
}

fn report_status(
    program: &str,
    result: std::io::Result<std::process::ExitStatus>,
) -> Result<(), i32> {
    match result {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn a_launcher_can_forward_the_doctor_subcommand() {
        assert_eq!(
            parse_args(&args(&["doctor", "--upgrade"])).unwrap(),
            Options {
                upgrade: true,
                help: false
            }
        );
        assert_eq!(
            parse_args(&args(&["--help"])).unwrap(),
            Options {
                upgrade: false,
                help: true
            }
        );
        assert_eq!(
            parse_args(&args(&["DOCTOR", "--upgrade"])).unwrap(),
            Options {
                upgrade: true,
                help: false
            }
        );
    }

    #[test]
    fn a_prompt_that_is_not_doctor_is_not_silently_ignored() {
        assert!(parse_args(&args(&["explain this"])).is_err());
    }
}
