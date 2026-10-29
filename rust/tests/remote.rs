//! A worker on another machine, proved without one.
//!
//! The whole feature is one `ssh` process whose stdin and stdout carry the wire
//! the local worker already speaks, so there are only two things that can be
//! wrong: the arguments `ssh` is given, and the command line `ssh` runs over
//! there. Both are asserted here. The second one is also *run*, through `sh`,
//! because a command line that only looks right is the bug this file exists for.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use cuacode::hosts::Host;
use cuacode::wire;
use cuacode::worker;

fn host(dir: &str) -> Host {
    Host {
        name: "box".into(),
        ssh: "me@box".into(),
        dir: dir.into(),
        python: String::new(),
        cwd: String::new(),
        home: String::new(),
    }
}

/// The repo root, which is where `main.py` actually is.
fn root() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/ has a parent")
        .display()
        .to_string()
}

#[test]
fn ssh_cannot_stop_to_ask_a_question() {
    let argv = worker::ssh_argv(&host("/srv/cuacode"));
    assert_eq!(argv[argv.len() - 2], "me@box", "the destination is penultimate");
    assert!(
        argv.iter().any(|a| a == "BatchMode=yes"),
        "the wire is on stdin; an ssh that prompted would eat it and hang"
    );
    assert!(argv.iter().any(|a| a == "-T"), "no pty in front of the wire");
}

#[test]
fn the_remote_line_starts_a_worker() {
    let dir = root();
    let cmd = worker::remote_command(&host(&dir));

    // `sh -c` is exactly what a remote sshd does with this string, so this is the
    // remote half of the spawn without a remote. A scratch home keeps the test out
    // of the real state root.
    let home = std::env::temp_dir().join(format!("cuacode-remote-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .env("CUACODE_HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sh starts");

    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).expect("the worker says something first");
    let ev = wire::parse(line.trim()).expect("a remote worker speaks the same wire");

    assert_eq!(ev.env.kind, "status", "the first line is the ready report");
    assert_eq!(ev.state(), "ready");
    assert!(
        ev.env.data.get("session_id").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()),
        "ready carries the id, which is how a session is tied to its folder"
    );

    // Closing stdin is the polite stop; kill covers a worker that ignores it.
    drop(child.stdin.take());
    let _ = child.kill();
    let _ = child.wait();
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn a_path_with_a_space_survives_the_shell() {
    let cmd = worker::remote_command(&host("/home/me/my code/cuacode"));
    assert!(
        cmd.contains("'/home/me/my code/cuacode'"),
        "the directory is quoted, not split into two arguments: {cmd}"
    );
}

/// The line that starts the pane's worker is a shell string built out of `hosts.json`, and
/// it is the one place a path with a space in it would silently start nothing: `cd` fails,
/// `&&` stops, and the pane dies without saying why. Run it, the way tmux does.
#[test]
fn a_pane_starts_the_worker_from_a_path_with_a_space() {
    let dir = std::env::temp_dir().join(format!("cuacode pane {}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.py"), "import sys\nprint('started', *sys.argv[1:])\n")
        .unwrap();
    let h = host(&dir.display().to_string());

    let line = h.start_pane_command();
    assert!(
        line.contains("-icanon"),
        "a canonical pty drops any line past 4095 bytes, pictures included: {line}"
    );
    assert!(
        line.contains(": > \"$HOME/.cuacode/live/cua.log\""),
        "the log is emptied first, or a fresh attach replays the last pane's: {line}"
    );

    let out =
        Command::new("sh").arg("-c").arg(h.pane_run()).output().expect("sh starts");
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        said.contains("started --detached"),
        "the worker runs out of a directory with a space in it: {said:?} {:?}",
        String::from_utf8_lossy(&out.stderr)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// A read of that box's disk is the same transport: no pty, nothing can prompt, and
/// the command it runs is the one the caller asked for rather than a worker.
#[test]
fn a_read_rides_the_same_ssh_as_the_worker() {
    let h = host("/srv/cuacode");
    let read = h.read_command("20260101-0000-abcd", "messages.jsonl", Some(4096)).unwrap();
    let argv = worker::ssh_argv_for(&h, &read);
    assert_eq!(argv[argv.len() - 2], "me@box");
    assert_eq!(argv[argv.len() - 1], read, "the command is the read, not a worker");
    assert!(argv.iter().any(|a| a == "BatchMode=yes"), "nothing may stop to ask");
    assert!(!read.contains("main.py"), "a read does not start anything");
}
