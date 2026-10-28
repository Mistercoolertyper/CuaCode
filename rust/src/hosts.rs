//! The other machines a frontend can run a worker on.
//!
//! Read from `~/.cuacode/hosts.json`, a list, and nothing else. `ssh` is the
//! whole transport and `ssh` is anything `ssh` accepts -- `box`, `me@box`, an
//! alias out of `~/.ssh/config` -- so a plain key pair works exactly like a
//! tailnet name does, and no VPN, tunnel or provider is named anywhere in here.
//!
//! No file means no hosts, which is every install that never asked for this: the
//! frontend shows its own machine and nothing else.

use std::path::Path;

use serde::Deserialize;

/// One machine, as the file writes it.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Host {
    /// What a picker calls it. Also the identity: two entries with one name are
    /// the same machine as far as a session list is concerned.
    pub name: String,
    /// The `ssh` destination.
    pub ssh: String,
    /// Where `main.py` lives over there.
    pub dir: String,
    /// The interpreter there. `python3` when left out.
    #[serde(default)]
    pub python: String,
    /// Where a session starts. `dir` when left out.
    #[serde(default)]
    pub cwd: String,
    /// Where that box keeps its state root. `$HOME/.cuacode` when left out, because
    /// a worker over there is spawned with no `CUACODE_HOME` and takes the default.
    #[serde(default)]
    pub home: String,
}

impl Host {
    pub fn python(&self) -> &str {
        if self.python.is_empty() {
            "python3"
        } else {
            &self.python
        }
    }

    pub fn cwd(&self) -> &str {
        if self.cwd.is_empty() {
            &self.dir
        } else {
            &self.cwd
        }
    }

    /// The state root over there, as the remote shell reads it: `$HOME` is left to
    /// expand on that box, because this machine's `$HOME` is not its home.
    pub fn home(&self) -> &str {
        if self.home.is_empty() {
            "$HOME/.cuacode"
        } else {
            self.home.trim_end_matches('/')
        }
    }

    /// The worker script over there. Built rather than joined, because it is a
    /// path on another filesystem and `Path` would join it with this one's rules.
    pub fn worker(&self) -> String {
        format!("{}/main.py", self.dir.trim_end_matches('/'))
    }

    /// The shell command that prints one file out of that box's state root.
    ///
    /// A frontend drawing a box it cannot open reads through this: the disk is still
    /// the transcript, it is just on the other end of an `ssh`. `tail` when a byte
    /// count is given, because a transcript is drawn as its last rows and pulling
    /// megabytes over the wire to draw forty of them is the whole file for nothing.
    ///
    /// `None` when the id or the name is not something worth putting in a shell word.
    /// Both travel from a worker, and a path built from unvalidated text is how one
    /// box ends up reading another's files.
    pub fn read_command(&self, id: &str, name: &str, tail: Option<usize>) -> Option<String> {
        let plain = |s: &str, extra: &str| {
            !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
        };
        if !plain(id, "-") || !plain(name, "-._") {
            return None;
        }
        let path = format!("{}/sessions/{id}/{name}", self.home());
        Some(match tail {
            Some(n) => format!("tail -c {n} \"{path}\""),
            None => format!("cat \"{path}\""),
        })
    }

    // ---- a worker hosted on that box -------------------------------------------
    //
    // The wire is line-JSON on stdin and stdout, so a worker whose stdio belongs to
    // **tmux** instead of to an `ssh` is a worker that outlives this machine: quit the
    // frontend, ctrl-c it, shut the laptop, and the pane on that box is still there,
    // still generating. The frontend is a client of it: commands go in through
    // `send-keys` (the pane's pty is the worker's stdin) and events come back out of the
    // file `pipe-pane` writes, which is the one way to read a stream that is not this
    // process's stdout.

    /// The tmux session that hosts this box's worker. One per machine, because there is
    /// one live worker per machine: it holds one conversation at a time, and switching
    /// conversations is `session.load`, which is what the wire already has.
    pub fn pane(&self) -> &'static str {
        "cua"
    }

    /// Where that pane's output is appended. Inside the state root, so a box that is
    /// read by this window is also a box whose live stream is a file here.
    pub fn wire_log(&self) -> String {
        format!("{}/live/{}.log", self.home(), self.pane())
    }

    /// Whether a worker is already hosted there. Asked before every attach: starting a
    /// second one would be a second worker on one state root, and the one already
    /// running would lose its turn.
    pub fn has_pane_command(&self) -> String {
        format!("tmux has-session -t {} 2>/dev/null", self.pane())
    }

    /// Start one: a pane, its output piped to the log, no echo on the pty (or every
    /// command this end sends would come back down the log as its own event), and then
    /// the worker itself, with `--detached` so it never stops to ask a person who may
    /// not be there.
    ///
    /// The pane runs `sh -c` and not a login shell: an interactive shell paints a prompt
    /// and its own escape sequences into the same pty the wire is on, and this box's fish
    /// does that with OSC 3008 and bracketed-paste markers. The `sleep` before `exec` is
    /// what lets `pipe-pane` attach first, so the ready line is in the log rather than
    /// printed into a pane nobody is capturing yet.
    pub fn start_pane_command(&self) -> String {
        let log = q(&self.wire_log());
        format!(
            "mkdir -p {live} && tmux new-session -d -s {pane} -x 220 -y 50 {run} \
             ; tmux pipe-pane -t {pane} -O {pipe}",
            live = q(&format!("{}/live", self.home())),
            pane = self.pane(),
            run = crate::worker::sh(&format!(
                "sh -c 'stty -echo; cd {} && sleep 1 && exec {} {} --detached'",
                self.cwd(),
                self.python(),
                self.worker()
            )),
            pipe = crate::worker::sh(&format!("cat >> {log}"))
        )
    }

    /// One envelope into that pane's stdin, as the frontend would have written it to a
    /// pipe. `-l` because the line is JSON and tmux would otherwise read its keys.
    pub fn send_command(&self, line: &str) -> String {
        format!(
            "tmux send-keys -t {pane} -l {line} ; tmux send-keys -t {pane} Enter",
            pane = self.pane(),
            line = crate::worker::sh(line.trim_end_matches('\n'))
        )
    }

    /// How much of the log is already written, which is where a fresh attach starts
    /// reading: everything before it is the past, and the past is on the record.
    pub fn log_size_command(&self) -> String {
        format!("wc -c < {}", q(&self.wire_log()))
    }

    /// Everything appended to the log from byte `from` on, followed as it grows. The
    /// `-f` is what makes this a stream: one `ssh` for the whole attachment, and closing
    /// it closes the view and nothing else.
    pub fn tail_command(&self, from: u64) -> String {
        format!("tail -f -c +{} {}", from.max(1), q(&self.wire_log()))
    }
}

/// A path for a **remote** shell: double-quoted, so it is one word and `$HOME` still
/// expands there. `worker::sh` single-quotes, which is right for a path this machine
/// built and wrong for one that starts with `$HOME`.
fn q(path: &str) -> String {
    format!("\"{}\"", path.replace('"', "\\\""))
}

/// The hosts in `home`, skipping any entry that could not start a worker.
///
/// ponytail: an entry missing `name`, `ssh` or `dir` is skipped silently rather
/// than reported. A picker row that fails when chosen is a worse answer than not
/// offering it; say so in the picker if a bad file ever needs diagnosing.
pub fn load(home: &Path) -> Vec<Host> {
    let text = std::fs::read_to_string(home.join("hosts.json")).unwrap_or_default();
    let parsed: Vec<Host> = serde_json::from_str(&text).unwrap_or_default();
    parsed
        .into_iter()
        .filter(|h| !h.name.is_empty() && !h.ssh.is_empty() && !h.dir.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_skips() {
        let dir = std::env::temp_dir().join(format!("cuacode-hosts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hosts.json"),
            r#"[
                {"name":"box","ssh":"box","dir":"/opt/cuacode/","cwd":"/home/me/work"},
                {"name":"bare","ssh":"me@bare","dir":"/srv/cuacode"},
                {"name":"broken","ssh":"","dir":"/srv/cuacode"}
            ]"#,
        )
        .unwrap();

        let hosts = load(&dir);
        assert_eq!(hosts.len(), 2, "the entry with no ssh destination is skipped");
        assert_eq!(hosts[0].worker(), "/opt/cuacode/main.py", "no doubled slash");
        assert_eq!(hosts[0].cwd(), "/home/me/work");
        assert_eq!(hosts[1].python(), "python3", "an absent interpreter defaults");
        assert_eq!(hosts[1].cwd(), "/srv/cuacode", "cwd falls back to dir");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_file_is_no_hosts() {
        let dir = std::env::temp_dir().join("cuacode-hosts-absent");
        assert!(load(&dir).is_empty());
    }

    /// A frontend reads that box's disk through one command, and the path in it is
    /// built from an id that came off a worker: a shell word only once it is proven
    /// to be one.
    #[test]
    fn a_read_stays_inside_that_boxs_state_root() {
        let h = Host {
            name: "box".into(),
            ssh: "box".into(),
            dir: "/srv/cuacode".into(),
            ..Default::default()
        };
        assert_eq!(h.home(), "$HOME/.cuacode", "the root is that box's, not this one's");
        let id = "20260101-0000-abcd";
        assert_eq!(
            h.read_command(id, "messages.jsonl", Some(4096)).unwrap(),
            "tail -c 4096 \"$HOME/.cuacode/sessions/20260101-0000-abcd/messages.jsonl\""
        );
        assert_eq!(
            h.read_command(id, "todo.json", None).unwrap(),
            "cat \"$HOME/.cuacode/sessions/20260101-0000-abcd/todo.json\""
        );
        assert!(h.read_command("../../etc/passwd", "messages.jsonl", None).is_none());
        assert!(h.read_command(id, "../../keys", None).is_none());
        assert!(h.read_command("", "messages.jsonl", None).is_none());
    }

    #[test]
    fn an_entry_may_name_its_own_state_root() {
        let h = Host {
            name: "box".into(),
            ssh: "box".into(),
            dir: "/srv/cuacode".into(),
            home: "/home/me/state/".into(),
            ..Default::default()
        };
        assert_eq!(h.home(), "/home/me/state", "a named root, with no doubled slash");
        assert!(h.read_command("s-1", "meta.json", None).unwrap().contains("/home/me/state/sessions/s-1/meta.json"));
    }
}
