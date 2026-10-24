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

use crate::wire::{self, Envelope, Event};

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
            for name in ["bin/python3", "bin/python"] {
                let candidate = root.join(venv).join(name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    for name in ["python3", "python"] {
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
}

impl Worker {
    /// Spawn the worker and start streaming its stdout.
    ///
    /// `home` is `CUACODE_HOME`: the state root to run against. `None` leaves
    /// the worker on its own default, which is `~/.cuacode`.
    pub fn spawn(
        python: &Path,
        worker: &Path,
        home: Option<&Path>,
        cwd: Option<&Path>,
    ) -> Result<(Worker, Receiver<Event>), String> {
        let mut cmd = Command::new(python);
        cmd.arg(worker)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        if let Some(home) = home {
            cmd.env("CUACODE_HOME", home);
        }

        let mut child = cmd.spawn().map_err(|e| format!("start {}: {e}", python.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin pipe")?;
        let stdout = child.stdout.take().ok_or("no stdout pipe")?;
        let stderr = child.stderr.take().ok_or("no stderr pipe")?;

        let errors = Arc::new(Mutex::new(Vec::new()));
        let closing = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        read_stderr(stderr, errors.clone());
        let worker = Worker { child, stdin: Some(stdin), errors, closing };
        read_stdout(stdout, tx, worker.closing.clone(), worker.errors.clone(), worker.child.id());
        Ok((worker, rx))
    }

    /// Write one envelope to the worker's stdin.
    pub fn send(&mut self, env: &Envelope) -> Result<(), String> {
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
    pub fn close(&mut self) -> Result<(), String> {
        self.closing.store(true, Ordering::SeqCst);
        let _ = self.send(&Envelope::cmd("quit", "stop", serde_json::json!({})));
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // The worker holds a real process group and a session folder; it must
        // not outlive the thing that spawned it.
        self.closing.store(true, Ordering::SeqCst);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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
