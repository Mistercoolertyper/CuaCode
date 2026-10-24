//! The agent half: one worker process, and what it is doing.
//!
//! This is the seam the base app was missing. Everything else in this window
//! reads the state root and draws it; this runs a turn. It links the shared
//! `cuacode` crate rather than re-deriving the wire, because the wire is a
//! contract with the Python side and the terminal frontends already hold up
//! their end of it - a second parser here would be a second thing to break.
//!
//! ## The disk is the transcript; the stream is liveness
//!
//! The conversation is read from `messages.jsonl`, exactly as before, by the
//! `Cache` that already reads its tail. The worker commits a round to that file
//! as the round ends, so the answer lands in the window on the app's own clock
//! and reopening the app shows the same conversation as the one that was live.
//!
//! What a file cannot tell you is what is happening *now*: that a run is in
//! flight, that it failed, and that the worker is blocked on a question only a
//! person can answer. That is what this holds, and it is deliberately small -
//! the whole of it is six fields, and the window draws them as a pulse, a
//! sentence, and a prompt.
//!
//! ## One worker, and the rail can point it anywhere
//!
//! A worker process holds one session. A send from the middle of the rail is a
//! conversation the worker is not holding, so it is loaded into the worker first
//! and sent behind it - one live conversation at a time, and the rail says which
//! one by what is selected.

use std::path::{Path, PathBuf};

use cuacode::session::Session;

/// What the window needs to know about the worker, without holding it.
#[derive(Default, Clone)]
pub struct Live {
    /// Whether a worker process is up.
    pub up: bool,
    /// Whether a turn is in flight, which is what the send control draws as Stop.
    pub busy: bool,
    /// The id of the session the worker is holding, once it has said.
    pub session_id: String,
    /// What the worker is doing, in its own words: `running`, `tools`.
    pub phase: String,
    /// How hard this conversation is set to think, in the worker's own words:
    /// one of `handler/agent/effort.py`'s ladder (`off low medium high max`), or
    /// empty for the model's own default. It travels on the same status the
    /// provider and model do, so the window never keeps a second copy of it.
    pub effort: String,
    /// Set when the worker is blocked on a question only a person can answer.
    pub asking: Option<Ask>,
    /// The last error the worker reported, if any. Cleared when a new one lands.
    pub error: String,
}

/// A permission question, which blocks the worker until it is answered.
///
/// The answer goes back to the *id the worker asked under*, so the id travels
/// with the question rather than being kept beside it.
#[derive(Clone)]
pub struct Ask {
    pub id: String,
    pub name: String,
    /// One line saying what would happen: the summary the worker computed.
    pub summary: String,
}

/// The worker, and what it is doing.
pub struct Agent {
    sess: Session,
    live: Live,
    /// The session the worker was last *told* to hold, so a send into the rail's
    /// selection loads it once rather than on every frame that draws.
    asked_for: String,
}

impl Agent {
    /// Start a worker. `home` is `CUACODE_HOME`; `cwd` is where the boot session
    /// stands, which is a real directory rather than wherever the app was run
    /// from.
    pub fn start(home: &Path, cwd: Option<&Path>) -> Result<Agent, String> {
        let sess = Session::start(Some(home), cwd)?;
        Ok(Agent { sess, live: Live { up: true, ..Default::default() }, asked_for: String::new() })
    }

    pub fn up(&self) -> bool {
        self.live.up
    }

    /// A copy of what is being drawn. A copy and not a borrow: the window holds
    /// its own state mutably while it reads this, and six fields is six fields.
    pub fn live(&self) -> Live {
        self.live.clone()
    }

    /// Take whatever the worker has said since the last look.
    ///
    /// Returns whether anything *drawn* moved, which is what tells the tick to
    /// keep painting.
    pub fn poll(&mut self) -> bool {
        let mut moved = false;
        for ev in self.sess.drain() {
            moved |= self.on(ev);
        }
        moved
    }

    fn on(&mut self, ev: cuacode::wire::Event) -> bool {
        // A question blocks the worker dead, so it is answered by a person and is
        // not a row. It is the one event that stops the loop until it is.
        if ev.env.kind == "permission" {
            let name = ev
                .env
                .data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("a call")
                .to_string();
            let summary = ev
                .env
                .data
                .get("preview")
                .and_then(|p| p.get("summary"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            self.live.asking = Some(Ask { id: ev.env.id.clone(), name, summary });
            return true;
        }
        // The session the worker is holding: `session.new` and `session.load` both
        // answer with one, and so does a chat reply.
        let mut moved = false;
        if let Some(s) = ev.env.data.get("session_id").and_then(|v| v.as_str()) {
            if !s.is_empty() && s != self.live.session_id {
                self.live.session_id = s.to_string();
                moved = true;
            }
        }
        // The worker's own fold keeps the running state and the error, and the
        // shared half is the one owner of that. The effort rides on its snapshot
        // too, so it is read from there rather than parsed again here.
        let before = (self.live.busy, self.live.phase.clone(), self.live.error.clone());
        self.sess.on_event(&ev);
        self.live.busy = self.sess.snap.state.busy();
        self.live.phase = self.sess.snap.state.as_str().to_string();
        self.live.error = self.sess.snap.error.clone();
        if !self.sess.snap.effort.is_empty() {
            self.live.effort = self.sess.snap.effort.clone();
        }
        if (self.live.busy, self.live.phase.clone(), self.live.error.clone()) != before {
            moved = true;
        }
        moved
    }

    /// Point the worker at a directory, so the next thing it runs stands there.
    ///
    /// This is the `terminal` envelope the worker stamps the session with: it
    /// records the cwd on the session and reads it for recall, so a conversation
    /// that cannot say where it happened is a conversation memory cannot find
    /// again. Sent rather than assumed because the composer's project chip is
    /// the thing that decides it, and it can change between sessions.
    fn set_cwd(&mut self, dir: Option<&Path>) -> Result<(), String> {
        let Some(dir) = dir else { return Ok(()) };
        self.sess.terminal.cwd = dir.display().to_string();
        let data = serde_json::to_value(&self.sess.terminal).unwrap_or(serde_json::Value::Null);
        self.sess.reply("app-cwd", "terminal", data)
    }

    /// Begin a fresh conversation standing in `cwd`.
    ///
    /// This is "new session": the worker commits the session it was holding,
    /// drops it if it was empty, and creates one. Nothing is written to disk
    /// until the first message, so a new session opened and left alone leaves no
    /// folder behind.
    pub fn new_session(&mut self, cwd: Option<&Path>) -> Result<(), String> {
        self.set_cwd(cwd)?;
        self.live.error.clear();
        self.live.asking = None;
        self.asked_for.clear();
        self.live.session_id.clear();
        self.sess.command("session.new", serde_json::json!({}))
    }

    /// Point the worker at a session that already exists on disk.
    pub fn load(&mut self, id: &str) -> Result<(), String> {
        self.live.error.clear();
        self.live.asking = None;
        self.asked_for = id.to_string();
        self.sess.command("session.load", serde_json::json!({ "id": id }))
    }

    /// Send a message. A message starts a run, so the worker answers with `busy`
    /// and the window draws that until the turn lands on disk.
    pub fn chat(&mut self, text: &str, cwd: Option<&Path>) -> Result<(), String> {
        self.set_cwd(cwd)?;
        self.sess.chat(text, &[])
    }

    /// The session the worker is holding.
    pub fn holding(&self) -> &str {
        &self.live.session_id
    }

    /// The session the window last asked the worker to hold.
    pub fn asked_for(&self) -> &str {
        &self.asked_for
    }

    /// Answer the question the worker is blocked on, and let it go.
    pub fn answer(&mut self, allow: bool) -> Result<(), String> {
        let Some(ask) = self.live.asking.take() else {
            return Ok(());
        };
        self.sess.reply(&ask.id, "permission", serde_json::json!({ "allow": allow }))
    }

    /// Any worker action that is not one of the four above: the settings, the
    /// permission mode. One door, so a caller cannot invent a second spelling.
    pub fn command(&mut self, action: &str, fields: serde_json::Value) -> Result<(), String> {
        self.sess.command(action, fields)
    }

    /// Show a setting the window just asked for, before the worker confirms it.
    ///
    /// Only for a value the window is the one *asking* for: the chip should answer
    /// on this frame rather than on the tick the confirmation lands. The worker's own
    /// reply still overwrites this, so a level it refuses (an `off` on a model that
    /// cannot stop) settles back to what is actually in force.
    pub fn set_effort_local(&mut self, level: &str) {
        self.live.effort = level.to_string();
    }

    /// Abandon the run in flight. The worker stays up and the conversation stays
    /// open, which is what the Stop on the send control is.
    pub fn cancel(&mut self) -> Result<(), String> {
        self.sess.cancel()
    }

    /// The worker's own main.py, so a caller can say which tree it is in.
    #[allow(dead_code)]
    pub fn worker_path() -> Result<PathBuf, String> {
        cuacode::worker::worker_path()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        // A worker holds a session folder and a process group; it does not
        // outlive the window. `Session::close` asks it to stop first, which is
        // what lets it commit the round in flight rather than losing it.
        self.sess.close();
    }
}
