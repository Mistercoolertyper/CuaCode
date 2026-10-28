//! One conversation's observable state, folded from the worker's events.
//!
//! A port of the Go frontends' session layer: it owns the worker process, keeps
//! what the last event said, and sends. A UI reads the snapshot and nothing
//! else, which is the point -- the second frontend (a GUI, one day) should not
//! have to re-derive any of this from the wire.

use std::path::Path;
use std::sync::mpsc::Receiver;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::attach;
use crate::wire::{Envelope, Event, State};
use crate::worker::{self, Target, Worker};

/// Everything a status line draws, as of the last event.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub state: State,
    pub session_id: String,
    pub mode: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub effort: String,

    pub msgs: i64,
    pub turns: i64,
    pub last_token: String,
    pub error: String,

    /// Another agent's message is in this worker's mailbox and the round in flight
    /// has not carried it into the model yet, or `None` when nothing is waiting.
    ///
    /// The name the worker put on the knock, which may be empty when it named
    /// nobody: what is waiting is a fact either way. This is *not* the message.
    /// Nothing about the conversation changes here -- the words arrive later as the
    /// `peer` event a frontend already draws -- and this is only what a status line
    /// may say while the round that has it waits.
    pub peer_waiting: Option<String>,

    /// What a fold is doing, while it is doing one: "start" when the conversation
    /// is being summarized and "done" when it is over. A fold is a model call, so
    /// the screen would otherwise sit on "thinking" with nothing saying what the
    /// wait is for. Empty when no fold is running.
    pub compacting: String,
    /// Records the fold is replacing, and characters it frees: what the status line
    /// can say about the job in progress.
    pub compact_folded: i64,
    pub compact_freed: i64,

    pub context_left: i64,
    pub context_used: i64,
    pub context_max: i64,

    /// The round that is generating, or the last one that did. Live figures are
    /// estimated from characters and say so; the provider's own count replaces
    /// them when the round is billed.
    pub phase: String,
    pub out_tokens: i64,
    pub think_tokens: i64,
    pub reply_tokens: i64,
    pub think_est: bool,
    pub tps: f64,
    pub think_tps: f64,
    pub reply_tps: f64,
    pub tps_est: bool,
}

/// One row of `session.list`: what a picker draws and what it loads by.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionMeta {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub frontend: String,
    #[serde(default)]
    pub turns: i64,
    #[serde(default)]
    pub records: i64,
    #[serde(default)]
    pub pinned: Option<String>,
}

/// Where the frontend was launched from and what it is, sent to the worker the
/// moment it says it is ready.
///
/// The worker records the cwd on the session and reads `frontmost_app` for its
/// app-scoped memories, so this is not decoration: an empty probe makes a
/// session that cannot say where it happened.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TerminalInfo {
    pub term: String,
    #[serde(rename = "term_program", skip_serializing_if = "String::is_empty")]
    pub program: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tty: String,
    #[serde(rename = "frontmost_app", skip_serializing_if = "String::is_empty")]
    pub frontmost_app: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cwd: String,
}

impl TerminalInfo {
    pub fn is_empty(&self) -> bool {
        self.term.is_empty() && self.cwd.is_empty() && self.program.is_empty()
    }
}

pub struct Session {
    pub worker: Worker,
    pub snap: Snapshot,
    pub terminal: TerminalInfo,
    /// Sessions the worker last listed, newest first.
    pub sessions: Vec<SessionMeta>,
    /// Envelope types this frontend took no action on. Counted rather than
    /// silently dropped, so the UI can say what it is not drawing.
    pub unhandled: u64,
    seq: u64,
    events: Receiver<Event>,
}

/// What a `compacting` envelope does to the snapshot.
///
/// A fold typed at the prompt runs with no turn in flight. Without moving the
/// state to Running the frame has no reason to reserve a status row, so the fold
/// happens with nothing on the screen at all -- which is exactly the complaint
/// this exists to answer. And it goes back to Idle rather than Done, because a
/// fold is a step inside whatever is happening, not the end of a turn.
///
/// Free, rather than a method, so it can be asserted without a spawned worker.
pub fn compact_state(snap: &mut Snapshot, stage: &str, folded: i64, freed: i64) {
    snap.compacting = stage.to_string();
    snap.compact_folded = folded;
    snap.compact_freed = freed;
    match stage {
        "start" => snap.state = State::Running,
        "done" => snap.state = State::Idle,
        _ => {}
    }
}

impl Session {
    pub fn start(home: Option<&Path>, cwd: Option<&Path>) -> Result<Session, String> {
        Session::start_on(&Target::Local, home, cwd)
    }

    /// Start on one machine. `cwd` is a path on *that* machine and is ignored for
    /// a remote worker, whose own entry names where it starts; `home` is only the
    /// local one, for the same reason.
    pub fn start_on(
        target: &Target,
        home: Option<&Path>,
        cwd: Option<&Path>,
    ) -> Result<Session, String> {
        let script = worker::worker_path()?;
        let python = worker::python(&script)?;
        let (worker, events) = Worker::spawn(target, &python, &script, home, cwd)?;
        Ok(Session {
            worker,
            snap: Snapshot::default(),
            terminal: terminal_for(target),
            sessions: Vec::new(),
            unhandled: 0,
            seq: 0,
            events,
        })
    }

    /// Everything that has arrived since the last call. Returned as an owned
    /// list so the caller can hold the session mutably while it reads them.
    pub fn drain(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }

    /// Fold one event in, and answer it if it was a request: the worker asks
    /// for the terminal envelope whenever it comes up, echoing the id it asked
    /// under.
    pub fn on_event(&mut self, ev: &Event) {
        if matches!(ev.state(), "ready" | "startup") && !self.terminal.is_empty() {
            let data = serde_json::to_value(&self.terminal).unwrap_or(Value::Null);
            let _ = self.worker.send(&Envelope::new("terminal", ev.env.id.clone(), data));
        }
        self.apply(ev);
    }

    fn apply(&mut self, ev: &Event) {
        let p = &ev.payload;

        // The coarse state is `status` when the envelope carries one, and the
        // two that end a run otherwise. `tooling` is the same state as running,
        // named for what the run is doing; `State::from` settles that.
        let status = ev.status();
        if !status.is_empty() {
            self.snap.state = State::from(status);
        } else {
            match ev.state() {
                "error" => self.snap.state = State::Error,
                "done" => {
                    self.snap.state = State::Done;
                    self.snap.turns += 1;
                }
                _ => {}
            }
        }

        if let Some(s) = &p.session_id {
            self.snap.session_id = s.clone();
        }
        if let Some(s) = &p.mode {
            self.snap.mode = s.clone();
        }
        if let Some(s) = &p.title {
            self.snap.title = s.clone();
        }
        if let Some(s) = &p.effort {
            self.snap.effort = s.clone();
        }
        if let Some(s) = &p.provider {
            self.snap.provider = s.clone();
        }
        if let Some(s) = &p.model {
            self.snap.model = s.clone();
        }
        if let Some(n) = p.msg_count {
            self.snap.msgs = n;
        }
        if let Some(n) = p.context_left {
            self.snap.context_left = n;
        }
        if let Some(n) = p.context_used {
            self.snap.context_used = n;
        }
        if let Some(n) = p.context_max {
            self.snap.context_max = n;
        }
        if let Some(s) = &p.phase {
            self.snap.phase = s.clone();
        }
        if let Some(n) = p.out_tokens {
            self.snap.out_tokens = n;
        }
        if let Some(n) = p.think_tokens {
            self.snap.think_tokens = n;
            self.snap.think_est = p.think_est.unwrap_or(false);
        }
        if let Some(n) = p.reply_tokens {
            self.snap.reply_tokens = n;
        }
        if let Some(v) = p.tps {
            self.snap.tps = v;
            self.snap.tps_est = p.tps_est.unwrap_or(false);
        }
        if let Some(v) = p.think_tps {
            self.snap.think_tps = v;
        }
        if let Some(v) = p.reply_tps {
            self.snap.reply_tps = v;
        }

        // A knock from another agent. It says a message is waiting behind the round
        // in flight; it is not the message, so no row is added for it and the words
        // themselves still arrive as the `peer` event. Cleared the moment they land,
        // and at every end of a turn: a knock nobody delivered is not a reason for
        // the screen to go on announcing it.
        if ev.state() == "peer_waiting" {
            self.snap.peer_waiting = Some(p.peer_name.clone().unwrap_or_default());
        } else if ev.state() == "peer" {
            self.snap.peer_waiting = None;
        }
        if matches!(self.snap.state, State::Done | State::Cancelled | State::Error) {
            self.snap.peer_waiting = None;
        }

        // A fold in progress. Its own field rather than part of `state`, because a
        // fold is not the turn changing what it is doing: the turn is still running
        // and will carry on being one when the fold is over.
        if ev.state() == "compacting" {
            compact_state(&mut self.snap, p.stage.as_deref().unwrap_or(""),
                          p.folded.unwrap_or(0), p.freed.unwrap_or(0));
        }
        // Cleared by any state that says a turn ended, like the knock above: a
        // fold that never reported its end must not leave the screen saying it.
        if matches!(self.snap.state, State::Done | State::Cancelled | State::Error) {
            self.snap.compacting.clear();
        }

        let token = ev.token_text();
        if !token.is_empty() {
            self.snap.last_token = token;
        }
        // The error text of a failed turn arrives as the token on the events
        // that carry one, and as `error` on the ones that do not.
        if let Some(e) = &p.error {
            self.snap.error = e.clone();
        } else if ev.state() == "error" && !self.snap.last_token.is_empty() {
            self.snap.error = self.snap.last_token.clone();
        }

        // The two replies a UI draws from rather than merely displays.
        if ev.env.kind == "sessions" {
            if let Some(list) = ev.env.data.get("sessions") {
                self.sessions = serde_json::from_value::<Vec<SessionMeta>>(list.clone())
                    .unwrap_or_default();
            }
        }
        if ev.state() == "ready" {
            if let Some(p) = &p.provider {
                self.snap.provider = p.clone();
            }
            if let Some(m) = &p.model {
                self.snap.model = m.clone();
            }
        }
    }

    /// A session id, short enough to read on a status line.
    pub fn short_id(&self) -> String {
        self.snap.session_id.chars().take(15).collect()
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        format!("{prefix}-{}", self.seq)
    }

    /// Send a user message, with whatever pictures were attached to it.
    ///
    /// The worker takes them as `[{"name", "b64"}]` on the same envelope as the
    /// text (see `main.py`'s `chat` action): only the payload reaches the model,
    /// and the name is what `attachment_note` tells it the picture was called.
    pub fn chat(&mut self, text: &str, images: &[attach::Image]) -> Result<(), String> {
        // Only when this message starts a run. Sent into one already going, it
        // is a mid-turn message and the readouts belong to the round in flight:
        // clearing them there would blank a live rate every time you typed.
        //
        // The state set here is a claim, not a measurement: nothing here knows
        // what the text is, and a message that is only a command starts no loop
        // at all. The worker is the side that knows, and it takes the claim back
        // with an `idle` when there is nothing to run -- so a frontend draws a
        // spinner the instant it sends, and it goes away when it should.
        if !self.snap.state.busy() {
            self.snap.peer_waiting = None;
            self.snap.phase.clear();
            self.snap.tps = 0.0;
            self.snap.think_tps = 0.0;
            self.snap.reply_tps = 0.0;
            self.snap.out_tokens = 0;
            self.snap.think_tokens = 0;
            self.snap.reply_tokens = 0;
            self.snap.state = State::Running;
        }
        self.snap.msgs += 1;
        let id = self.next_id("msg");
        let attached: Vec<serde_json::Value> =
            images.iter().enumerate().map(|(i, image)| image.json(i + 1)).collect();
        self.worker.send(&Envelope::cmd(
            id,
            "chat",
            json!({ "text": text, "images": attached }),
        ))
    }

    /// Abandon the run in flight, leaving the worker alive. It lands wherever
    /// the run is: opening the request, between chunks, between tool calls, and
    /// inside one.
    pub fn cancel(&mut self) -> Result<(), String> {
        let id = self.next_id("cancel");
        self.worker.send(&Envelope::cmd(id, "cancel", json!({})))
    }

    /// Push the running call into the background. Discarded by the worker when
    /// nothing is in flight, so it is only worth offering while a tool runs.
    pub fn background(&mut self) -> Result<(), String> {
        let id = self.next_id("bg");
        self.worker.send(&Envelope::cmd(id, "background", json!({})))
    }

    /// Any other worker command: `session.list`, `session.load`, `session.new`,
    /// `provider.use`, `context.report`, `usage.report`.
    pub fn command(&mut self, action: &str, fields: Value) -> Result<(), String> {
        let id = self.next_id("cmd");
        self.worker.send(&Envelope::cmd(id, action, fields))
    }

    /// Answer a request the worker made of this frontend, echoing its id. The
    /// worker blocks until it arrives, which is why a permission prompt left
    /// unanswered stays open rather than being dropped.
    pub fn reply(&mut self, id: &str, kind: &str, fields: Value) -> Result<(), String> {
        self.worker.send(&Envelope::new(kind, id, fields))
    }

    /// Done -> Idle, for a UI that wants the spinner to stop after a while.
    pub fn mark_idle(&mut self) -> bool {
        if !matches!(self.snap.state, State::Done | State::Cancelled) {
            return false;
        }
        self.snap.state = State::Idle;
        true
    }

    pub fn close(&mut self) {
        let _ = self.worker.close();
    }
}

/// The terminal facts to send to a worker on `target`.
///
/// The cwd is where **this** machine was standing, and the worker stamps it on the
/// session it holds, which is the fact recall matches on. A worker on another box
/// holds a session whose folder is over there, so a path from here names nothing
/// when it arrives: it is dropped rather than written down, and that box's own
/// `hosts.json` entry says where the worker stands instead. The rest of the probe
/// (which terminal, which app was in front) is true from either end of the pipe.
pub fn terminal_for(target: &Target) -> TerminalInfo {
    let mut t = probe_terminal();
    if matches!(target, Target::Remote(_)) {
        t.cwd.clear();
    }
    t
}

/// The terminal facts the worker records on a session: where this was run from,
/// and which app was in front of it.
pub fn probe_terminal() -> TerminalInfo {
    let mut t = TerminalInfo {
        term: std::env::var("TERM").unwrap_or_default(),
        program: std::env::var("TERM_PROGRAM").unwrap_or_default(),
        ..Default::default()
    };
    t.tty = run("tty").unwrap_or_default();
    t.frontmost_app = frontmost_app();
    t.cwd = working_dir();
    t
}

/// The directory the user was standing in, which is not always this process's.
/// `CUACODE_CWD` wins: run.sh launches a frontend from the repo root so the
/// worker can be found, and the session should still say where you were.
pub fn working_dir() -> String {
    if let Ok(dir) = std::env::var("CUACODE_CWD") {
        if !dir.is_empty() {
            return dir;
        }
    }
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into())
}

fn frontmost_app() -> String {
    #[cfg(target_os = "macos")]
    {
        run_with("osascript", &["-e",
            "tell application \"System Events\" to get name of first process whose frontmost is true"])
        .unwrap_or_default()
    }
    #[cfg(not(target_os = "macos"))]
    {
        String::new()
    }
}

fn run(cmd: &str) -> Option<String> {
    run_with(cmd, &[])
}

fn run_with(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_probe_says_where_this_is() {
        let t = probe_terminal();
        assert!(!t.cwd.is_empty(), "a session with no cwd cannot be recalled by directory");
        assert!(!t.is_empty());
    }

    /// The bug this pins: a session opened on another box recorded this machine's
    /// directory, because the terminal envelope carried it and the worker stamped
    /// it. One of `fuzzy`'s sessions said `/Users/.../personal` while its folder was
    /// on that box, so the rail, the location and recall all pointed at a path that
    /// does not exist there.
    #[test]
    fn a_remote_worker_is_not_told_where_this_one_stands() {
        let host = crate::hosts::Host {
            name: "box".into(),
            ssh: "box".into(),
            dir: "/srv/cuacode".into(),
            ..Default::default()
        };
        let here = terminal_for(&Target::Local);
        let there = terminal_for(&Target::Remote(host));
        assert!(!here.cwd.is_empty(), "this machine's own worker is told where it stands");
        assert!(there.cwd.is_empty(), "a path from here names nothing over there");
        assert_eq!(there.term, here.term, "the rest of the probe is true either way");
    }

    /// A fold typed at the prompt is visible while it runs, and gone after.
    ///
    /// The bug this pins: the session was Idle during a manual fold, so the frame
    /// reserved no status row and the worker sat in a model call with a screen that
    /// looked hung.
    #[test]
    fn a_fold_makes_the_screen_busy_and_then_lets_it_go() {
        let mut snap = Snapshot::default();
        assert_eq!(snap.state, State::Idle);

        compact_state(&mut snap, "start", 24, 18_000);
        assert_eq!(snap.compacting, "start");
        assert_eq!(snap.state, State::Running, "a fold with nothing else running must still show");
        assert_eq!((snap.compact_folded, snap.compact_freed), (24, 18_000));

        compact_state(&mut snap, "done", 0, 0);
        assert_eq!(snap.compacting, "done");
        assert_eq!(snap.state, State::Idle, "a finished fold did not finish a turn");
    }

    /// A turn that was already running is not knocked to Idle by a fold ending:
    /// an automatic fold happens mid-round, and the round carries on.
    #[test]
    fn a_fold_inside_a_turn_leaves_the_turn_alone() {
        let mut snap = Snapshot { state: State::Tools, ..Snapshot::default() };
        compact_state(&mut snap, "start", 2, 100);
        compact_state(&mut snap, "done", 0, 0);
        assert_eq!(snap.state, State::Idle);
    }

    #[test]
    fn a_busy_state_is_running_or_tools() {
        assert!(State::from("running").busy());
        assert!(State::from("tooling").busy());
        assert!(!State::from("done").busy());
    }
}
