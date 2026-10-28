//! The worker process: find it, start it, and turn its stdout into events.
//!
//! A port of the Go frontends' runner and protocol layers. Nothing here
//! knows what a conversation is; it moves lines, and reports the worker's death
//! through the same channel as its speech so a UI already drawing worker errors
//! draws that one too.
//!
//! ponytail: unix paths only (`venv/bin/python3`, no `Scripts\` variant) and no
//! SIGTERM handler. Add both if this ever ships off this Mac; `cuacode`
//! itself staying cross-platform is the Python side's job, not a TUI's.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::hosts::Host;
use crate::wire::{self, Envelope, Event};

/// Where the worker runs.
///
/// `Remote` is not a transport of its own: it is one `ssh` process, and the wire
/// on its stdin and stdout is the same wire as a local worker's. That is the
/// whole feature -- a worker on another machine is a worker whose parent process
/// happens to be `ssh`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Target {
    /// This machine.
    #[default]
    Local,
    /// Another one, out of `~/.cuacode/hosts.json`.
    Remote(Host),
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::Local => "this machine",
            Target::Remote(h) => &h.name,
        }
    }
}

/// Single-quote a word for a POSIX shell, embedded quotes included.
///
/// Public because a frontend that writes a command line for another machine needs
/// the same quoting the worker's own `ssh` line does: two spellings of "a shell
/// word" is how one of them ends up splitting a path with a space in it.
pub fn sh(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The command line the remote `ssh` runs.
///
/// `BatchMode` on the ssh side is not tidiness: the wire is on stdin, so an ssh
/// that stopped to ask for a password would eat the protocol and then hang with
/// nothing said. A host that needs a password fails loudly instead, which is what
/// the exit report is for.
pub fn remote_command(h: &Host) -> String {
    format!(
        "cd {} && exec {} {}",
        sh(h.cwd()),
        h.python(),
        sh(&h.worker())
    )
}

/// The arguments `ssh` itself is given. Split out from `spawn` so the shape that
/// matters -- the destination, and that nothing can stop and ask a question -- is
/// assertable without a network or a process.
pub fn ssh_argv(h: &Host) -> Vec<String> {
    ssh_argv_for(h, &remote_command(h))
}

/// The same arguments, carrying somebody else's command: a read out of that box's
/// state root is one command on the same transport, and it must not be able to stop
/// and ask a question either.
pub fn ssh_argv_for(h: &Host, command: &str) -> Vec<String> {
    let mut argv = vec![
        "-T".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=10".to_string(),
    ];
    argv.push(h.ssh.clone());
    argv.push(command.to_string());
    argv
}

/// The longest single stdout line accepted. A tool_calls payload can run past
/// bufio's 64 KiB default, and a line refused for length reads exactly like the
/// worker dying with nothing said about it.
const MAX_LINE: usize = 8 << 20;

/// How much of the worker's stderr is kept for the exit report. Only the tail
/// matters: a traceback ends with the line that explains it.
const STDERR_TAIL: usize = 8 << 10;

/// Find the Python worker: `$CUACODE_WORKER`, then `main.py` walking up from
/// here, then up from the executable.
pub fn worker_path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("CUACODE_WORKER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(format!("CUACODE_WORKER={} is not a file", p.display()));
    }

    let mut roots = vec![std::env::current_dir().map_err(|e| e.to_string())?];
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.canonicalize().unwrap_or(exe);
        roots.push(exe.parent().map(Path::to_path_buf).unwrap_or_default());
    }

    for root in roots {
        for dir in root.ancestors() {
            let candidate = dir.join("main.py");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err("main.py not found above cwd or executable (set CUACODE_WORKER)".into())
}

/// The interpreter to run the worker with: `$CUACODE_PYTHON`, then a venv
/// beside the worker script, then `python3`/`python` on PATH.
pub fn python(worker: &Path) -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("CUACODE_PYTHON") {
        return Ok(PathBuf::from(p));
    }
    if let Some(root) = worker.parent() {
        for venv in ["venv", ".venv"] {
            #[cfg(windows)]
            let names = ["Scripts/python.exe", "Scripts/python3.exe"];
            #[cfg(not(windows))]
            let names = ["bin/python3", "bin/python"];
            for name in names {
                let candidate = root.join(venv).join(name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    #[cfg(windows)]
    let names = ["python3.exe", "python.exe"];
    #[cfg(not(windows))]
    let names = ["python3", "python"];
    for name in names {
        if let Some(p) = on_path(name) {
            return Ok(p);
        }
    }
    Err("no python interpreter found on PATH (set CUACODE_PYTHON)".into())
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// A running worker and the events coming off it.
pub struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    errors: Arc<Mutex<Vec<u8>>>,
    closing: Arc<AtomicBool>,
    /// Set when this is a **hosted** worker: one on a box whose stdio belongs to a tmux
    /// pane there rather than to this process. `child` is then the `ssh` carrying the
    /// tail of that pane's log, and closing it closes the view and nothing else.
    hosted: Option<Host>,
}

impl Worker {
    /// Spawn the worker and start streaming its stdout.
    ///
    /// `home` is `CUACODE_HOME`: the state root to run against. `None` leaves
    /// the worker on its own default, which is `~/.cuacode`.
    pub fn spawn(
        target: &Target,
        python: &Path,
        worker: &Path,
        home: Option<&Path>,
        cwd: Option<&Path>,
    ) -> Result<(Worker, Receiver<Event>), String> {
        // ponytail: a remote worker keeps its own `~/.cuacode`. `home` is a path on
        // this machine, and handing it over would point the worker at a directory
        // that does not exist there. A per-host home is the upgrade, if a scratch
        // root on another machine is ever wanted.
        let mut cmd = match target {
            Target::Local => {
                let mut cmd = Command::new(python);
                cmd.arg(worker);
                if let Some(dir) = cwd {
                    cmd.current_dir(dir);
                }
                if let Some(home) = home {
                    cmd.env("CUACODE_HOME", home);
                }
                cmd
            }
            Target::Remote(h) => {
                let mut cmd = Command::new("ssh");
                cmd.args(ssh_argv(h));
                cmd
            }
        };
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            format!("start {}: {e}", target.name())
        })?;
        let stdin = child.stdin.take().ok_or("no stdin pipe")?;
        let stdout = child.stdout.take().ok_or("no stdout pipe")?;
        let stderr = child.stderr.take().ok_or("no stderr pipe")?;

        let errors = Arc::new(Mutex::new(Vec::new()));
        let closing = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        read_stderr(stderr, errors.clone());
        let worker = Worker { child, stdin: Some(stdin), errors, closing, hosted: None };
        read_stdout(stdout, tx, worker.closing.clone(), worker.errors.clone(), worker.child.id());
        Ok((worker, rx))
    }

    /// Attach to the worker hosted on `host`, starting one if that box has none.
    ///
    /// This is the detachable shape. The worker over there is a child of a tmux pane, so
    /// nothing here is its parent: shutting this machine down stops nothing, and the run
    /// in flight keeps going. Commands go into the pane with `send-keys` (a command per
    /// call, about a second, which is why a message is where you feel it) and events come
    /// off the end of the file `pipe-pane` appends, followed by one `tail -f` for the
    /// whole attachment.
    pub fn attach(h: &Host) -> Result<(Worker, Receiver<Event>), String> {
        let mut fresh = false;
        if !ssh_ok(h, &h.has_pane_command())? {
            ssh_run(h, &h.start_pane_command(), None)?;
            fresh = true;
            // The pane's shell has to start the worker: a moment, or the log is measured
            // before the ready line is in it and the first frame waits on a worker that
            // has already spoken.
            thread::sleep(std::time::Duration::from_millis(400));
        }
        // A pane this call started is read from the beginning, so the ready line reaches the
        // screen even if it was written a moment before the tail attached. One that was
        // already there is read from its end: everything before it is the past, and the past
        // is the session record, which `session.load` replays.
        let from = if fresh {
            1
        } else {
            ssh_run(h, &h.log_size_command(), None)?
                .trim()
                .parse::<u64>()
                .unwrap_or(0)
                + 1
        };
        let mut cmd = Command::new("ssh");
        cmd.args(ssh_argv_for(h, &h.tail_command(from)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("ssh {}: {e}", h.name))?;
        let stdout = child.stdout.take().ok_or("no stdout pipe")?;
        let stderr = child.stderr.take().ok_or("no stderr pipe")?;
        let errors = Arc::new(Mutex::new(Vec::new()));
        let closing = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        read_stderr(stderr, errors.clone());
        let worker = Worker {
            child,
            stdin: None,
            errors,
            closing: closing.clone(),
            hosted: Some(h.clone()),
        };
        read_hosted(stdout, tx, closing, h.name.clone());
        Ok((worker, rx))
    }

    /// Write one envelope to the worker's stdin.
    ///
    /// A hosted worker has no stdin here: the envelope is typed into the pane that owns
    /// it, which is the same line the pipe would have carried.
    pub fn send(&mut self, env: &Envelope) -> Result<(), String> {
        if let Some(h) = self.hosted.clone() {
            return ssh_run(&h, &h.send_command(&env.to_line()), None).map(|_| ());
        }
        let stdin = self.stdin.as_mut().ok_or("worker is closed")?;
        stdin.write_all(env.to_line().as_bytes()).map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())
    }

    /// Whatever the worker last wrote to stderr, tail-first. For a UI that
    /// wants to show more than the exit line carries.
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.errors.lock().unwrap()).trim().to_string()
    }

    /// Ask the worker to stop, then make sure it has.
    ///
    /// A hosted worker is **not** stopped by this: dropping the view is not ending the
    /// work, which is the whole reason it is hosted. Its tail is closed and the pane on
    /// that box carries on.
    pub fn close(&mut self) -> Result<(), String> {
        self.closing.store(true, Ordering::SeqCst);
        if self.hosted.is_none() {
            let _ = self.send(&Envelope::cmd("quit", "stop", serde_json::json!({})));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // The worker this process spawned holds a real process group and a session
        // folder, so it must not outlive the thing that spawned it. A **hosted** one is
        // the other case: what is dropped here is the tail, and the work stays.
        self.closing.store(true, Ordering::SeqCst);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One `ssh` into `host`, with the exit code as the answer: the shape a question like
/// "is a worker already hosted there" needs.
fn ssh_ok(h: &Host, command: &str) -> Result<bool, String> {
    let out = Command::new("ssh")
        .args(ssh_argv_for(h, command))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ssh {}: {e}", h.name))?;
    Ok(out.status.success())
}

/// One `ssh` into `host`, its stdout as the answer, and a failed exit as the error it
/// printed. `stdin` is written in and closed, for a command that reads a file's worth of
/// text.
fn ssh_run(h: &Host, command: &str, stdin: Option<&str>) -> Result<String, String> {
    use std::io::Write as _;
    let mut child = Command::new("ssh")
        .args(ssh_argv_for(h, command))
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh {}: {e}", h.name))?;
    if let (Some(text), Some(pipe)) = (stdin, child.stdin.as_mut()) {
        pipe.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if said.is_empty() { format!("ssh {} failed", h.name) } else { said });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Lines off a hosted worker's log, followed for as long as the tail lives.
///
/// A line that is not an envelope is dropped rather than reported: the pane's pty can
/// echo, tmux can print, and none of that is the worker speaking. A tail that ends is
/// the **connection** ending, and it says so, because the worker over there is still
/// running and a screen that read that as a dead worker would be lying.
fn read_hosted(stdout: impl Read + Send + 'static, tx: Sender<Event>, closing: Arc<AtomicBool>,
               name: String) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            let line = String::from_utf8_lossy(&buf);
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            let Ok(ev) = wire::parse(line) else { continue };
            if tx.send(ev).is_err() {
                return;
            }
        }
        if !closing.load(Ordering::SeqCst) {
            let _ = tx.send(wire::worker_exit(&format!(
                "lost the connection to {name}; the worker there is still running, ctrl-g to come back"
            )));
        }
    });
}

fn read_stdout(stdout: impl Read + Send + 'static, tx: Sender<Event>, closing: Arc<AtomicBool>,
               errors: Arc<Mutex<Vec<u8>>>, pid: u32) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    let _ = tx.send(wire::worker_exit(&format!("reading worker output: {e}")));
                    return;
                }
            }
            let line = String::from_utf8_lossy(&buf);
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            if line.len() > MAX_LINE {
                // Saying so beats the silent stop a byte-scanner would give:
                // over the limit, the rest of this line is not recoverable.
                let _ = tx.send(wire::worker_exit(&format!(
                    "worker sent a line over {} bytes", MAX_LINE)));
                break;
            }
            // A malformed line becomes an error event rather than a gap in the
            // stream: a frontend drawing worker errors draws this one too.
            let ev = match wire::parse(line) {
                Ok(ev) => ev,
                Err(e) => wire::worker_exit(&format!("worker sent a line that is not JSON: {e}")),
            };
            if tx.send(ev).is_err() {
                break; // nobody is listening; the UI is gone
            }
        }

        // Everything above ends with the pipe closed, which is also the only
        // safe point to reap the process.
        if !closing.load(Ordering::SeqCst) {
            let tail = String::from_utf8_lossy(&errors.lock().unwrap()).trim().to_string();
            let mut msg = format!("worker exited (pid {pid})");
            if !tail.is_empty() {
                msg.push('\n');
                msg.push_str(&tail);
            }
            let _ = tx.send(wire::worker_exit(&msg));
        }
    });
}

fn read_stderr(stderr: impl Read + Send + 'static, into: Arc<Mutex<Vec<u8>>>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut kept = into.lock().unwrap();
                    kept.extend_from_slice(&buf[..n]);
                    if kept.len() > STDERR_TAIL {
                        let drop = kept.len() - STDERR_TAIL;
                        kept.drain(..drop);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_worker_is_found_by_env_or_by_walking_up() {
        // The test binary lives under rust/target, so nothing above it is a
        // worker; CUACODE_WORKER is the path that must work.
        let worker = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../main.py");
        assert!(worker.is_file(), "the Python side is the sibling of this crate");
        // Set for this process only; the lookup is the thing under test.
        unsafe { std::env::set_var("CUACODE_WORKER", &worker) };
        assert_eq!(worker_path().unwrap(), worker);
        assert!(python(&worker).is_ok(), "a python is on PATH or in a venv");
    }
}
