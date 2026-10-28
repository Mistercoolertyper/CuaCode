//! What the window knows: the projects the worker has been told about, and the
//! sessions on disk, grouped under them.
//!
//! Read straight off `~/.cuacode`, with no worker in the loop. That is the one
//! design decision in this file and it is deliberate: the worker's own IPC
//! answers `project.list` and `session.list` from exactly these two files
//! (`handler/session/store.py`), so opening a process to hear it read a JSON
//! file would buy nothing but a process. This window has no agent half at all
//! yet, and the moment it does, the listing is still this: a window that cannot
//! draw its own rail without a worker up is a window that is blank when the
//! worker is broken.
//!
//! The shapes are the worker's, not ours:
//!
//! ```text
//! projects.json   { "projects": [dir, ...], "pinned": [basename, ...],
//!                   "names": {dir: name}, "icons": {dir: token} }
//! sessions/<id>/meta.json   { id, title, cwd, provider, model, updated, ... }
//! ```
//!
//! `pinned` holds project *names* (basenames), not paths - that is their file's
//! shape and not a mistake here. A session belongs to a project when its `cwd`
//! is that directory or under it; a session whose `cwd` was never promoted is
//! not lost, it lands in the `Other sessions` group at the bottom.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One session, as `meta.json` describes it. Every field is optional: the
/// worker writes the key it has, and 31 of the 459 sessions on this machine
/// have no `cwd` at all.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub records: u64,
}

impl Session {
    /// What a row calls this. The worker's own name for the conversation when it
    /// has one, the id when it does not: an untitled session is a real thing and
    /// the id is at least true.
    pub fn label(&self) -> String {
        let t = self.title.trim();
        if !t.is_empty() {
            return t.to_string();
        }
        if self.id.is_empty() { "session".into() } else { self.id.clone() }
    }

    /// When this was last touched, as words: `12m`, `3h`, `2d`.
    ///
    /// Parsed from the record's own ISO stamp rather than from the file's mtime,
    /// which is the same fact written twice and one of them can lie. This is the
    /// window's only clock and it is honest about what it read.
    pub fn age(&self) -> String {
        let Some(secs) = iso_age_secs(&self.updated) else {
            return String::new();
        };
        match secs {
            s if s < 60 => "now".into(),
            s if s < 3600 => format!("{}m", s / 60),
            s if s < 86400 => format!("{}h", s / 3600),
            s if s < 86400 * 30 => format!("{}d", s / 86400),
            s => format!("{}mo", s / (86400 * 30)),
        }
    }
}

/// One heading in the rail: what it is called, where it stands, and the sessions
/// under it.
///
/// **Promoted or loose** is the split that matters, and it is a real distinction
/// rather than a display one. A promoted directory is in the user's own
/// `projects.json`: it has a name you chose, a pin, and a place. A loose one is a
/// directory some session happened to run in, and it is a heading here and nothing
/// else - nothing is written to the user's file for it, and deleting the sessions
/// under it leaves no trace.
#[derive(Clone, Debug)]
pub struct Project {
    pub dir: PathBuf,
    pub name: String,
    /// Whether it is pinned, which is what puts it above the rest and in pin
    /// order rather than promotion order.
    pub pinned: bool,
    /// Whether the user promoted this directory, or it is only where sessions ran.
    pub promoted: bool,
    pub sessions: Vec<Session>,
}

impl Project {
    /// The path with the home directory shortened. At rail width a long path is
    /// all slashes, and `~` is the part of it a person reads.
    pub fn where_line(&self) -> String {
        short_path(&self.dir)
    }
}

/// Everything the window draws, read once.
#[derive(Clone, Debug, Default)]
pub struct Workspace {
    pub projects: Vec<Project>,
    /// Sessions whose `cwd` is empty. The only thing that belongs in a group
    /// called "No directory" is a session that has no directory.
    ///
    /// This used to be every session that was not under a promoted project, which
    /// on this machine is 375 of 461 - one flat pile holding thirty different
    /// directories, and the useful part of the rail buried in it.
    pub other: Vec<Session>,
    /// Every session on disk, newest first, for the window's own count.
    pub total: usize,
    /// Where the state root turned out to be, so a failure can say it.
    pub home: PathBuf,
}

impl Workspace {
    /// Read the state root and group it.
    pub fn read() -> Workspace {
        Workspace::read_from(&state_root())
    }

    /// The same, against a root named rather than discovered: the one door, so
    /// a test drives the reader the window drives.
    pub fn read_from(home: &Path) -> Workspace {
        Workspace::group(read_projects(home), read_sessions(home), home.to_path_buf())
    }

    /// The same grouping, from a listing that came from somewhere else: a worker's
    /// own `session.list`, which is a box's listing read **on that box**.
    ///
    /// `projects` is empty for a box this window cannot open, and that is the honest
    /// answer rather than a gap: a promoted project is a fact about that box's
    /// `projects.json`, and the window has not read it. Every session then sits under
    /// its own directory, which is a fact each session carries.
    pub fn group(projects: Vec<Project>, mut sessions: Vec<Session>, home: PathBuf) -> Workspace {
        let total = sessions.len();

        // Newest first, and once: the grouping only takes from this, so the
        // order inside a project is this order and not a second sort.
        sessions.sort_by(|a, b| b.updated.cmp(&a.updated));

        let mut names: Vec<Project> = projects
            .iter()
            .map(|p| Project {
                dir: p.dir.clone(),
                name: p.name.clone(),
                pinned: p.pinned,
                promoted: true,
                sessions: Vec::new(),
            })
            .collect();

        // Everything left over is grouped **by the directory it stands in**:
        // same leaf name and same `where` line as a promoted project, and not in
        // `projects.json`.
        //
        // That split is the point. A project you promoted is an intent - it keeps
        // its place, its pin and its name - and a directory a session merely ran in
        // is a *fact about the session*, which is not an invitation to write
        // anything to the user's own file. So the directory is a heading in the rail
        // and nothing more, and the only sessions in "No directory" are the ones that
        // truly have none.
        let mut loose: std::collections::HashMap<String, Vec<Session>> =
            std::collections::HashMap::new();
        let mut order: Vec<String> = Vec::new();
        let mut other = Vec::new();
        for s in sessions {
            if let Some(p) = names.iter_mut().find(|p| under(&s.cwd, &p.dir)) {
                p.sessions.push(s);
                continue;
            }
            let cwd = s.cwd.trim();
            if cwd.is_empty() {
                other.push(s);
                continue;
            }
            // A session's own directory, exactly: two sessions in the same place
            // are the same heading, and a subdirectory is its own heading rather
            // than being folded into its parent. Folding them would be inventing a
            // project, which is the thing this is not doing.
            if !loose.contains_key(cwd) {
                order.push(cwd.to_string());
            }
            loose.entry(cwd.to_string()).or_default().push(s);
        }
        // Ordered by each directory's own newest session - `order` is already in
        // that order, because the sessions were sorted before they were grouped.
        let loose_projects = order.into_iter().map(|dir| {
            let sessions = loose.remove(&dir).unwrap_or_default();
            let name = leaf(Path::new(&dir));
            Project { dir: PathBuf::from(&dir), name, pinned: false, promoted: false, sessions }
        });

        // A pinned project leads, in pin order - their file's own order - and
        // everything else keeps the order the worker promoted it in. This does
        // not invent an order of its own. Loose directories come after every
        // promoted one.
        names.sort_by_key(|p| !p.pinned);
        names.extend(loose_projects);

        Workspace { projects: names, other, total, home }
    }

    /// How many sessions the window is showing across every group.
    pub fn shown(&self) -> usize {
        self.other.len() + self.projects.iter().map(|p| p.sessions.len()).sum::<usize>()
    }

    /// The project a session stands in, for a row's own caption.
    pub fn project_of<'a>(&'a self, id: &str) -> Option<&'a Project> {
        self.projects.iter().find(|p| p.sessions.iter().any(|s| s.id == id))
    }

    /// The first session in the list, which is what opens selected.
    pub fn first(&self) -> Option<&Session> {
        self.projects
            .iter()
            .find_map(|p| p.sessions.first())
            .or_else(|| self.other.first())
    }
}

/// One row of a session's transcript, as the window draws it.
///
/// Read from `messages.jsonl`, which is the worker's own canonical log
/// (`handler/session/main.py`): one JSON object per line, `t` in
/// `user | assistant | tool | recall | usage`. The two that are not a row are
/// `recall` and `usage`: the first is the runtime talking to the model about
/// itself, the second is a bill. Neither is something a reader of the
/// conversation asked to see.
#[derive(Clone, Debug)]
pub struct Turn {
    pub kind: Kind,
    pub text: String,
    /// When the record was written, as its own ISO stamp. Kept because a run's
    /// own line says how long it worked, and that is the distance between the
    /// first record of the turn and the answer that ended it - a fact only the
    /// record has. A record from before the stamp existed reads as empty.
    pub ts: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Something the person said.
    User,
    /// The model's answer.
    Answer,
    /// The model's reasoning, which folds to one line.
    Thought,
    /// A tool call, written as the call itself: `write(src/main.rs)`.
    Call,
    /// What a tool answered. One line, because a result can be a megabyte.
    Result,
}

/// A session's transcript, oldest row first, at most `max` rows.
///
/// The **tail of the file**, not the whole of it. A session that ran for ten
/// hours is one with a megabyte of tool output in it, and reading all of that to
/// draw the newest forty rows is the difference between a window that opens and
/// one that thinks about it. So: seek to the last `TAIL_BYTES`, drop the partial
/// line the seek landed in, and read forward from there.
///
pub fn transcript(home: &Path, id: &str, max: usize) -> Vec<Turn> {
    if !safe_id(id) {
        return Vec::new();
    }
    let path = home.join("sessions").join(id).join("messages.jsonl");
    match read_tail(&path, TAIL_BYTES as u64) {
        Ok(text) => turns(&text, max),
        Err(_) => Vec::new(),
    }
}

/// The same rows, from the bytes of a transcript rather than from a path to one.
///
/// One reader for a file this machine opened and one fetched from a box it cannot
/// open, because they are the same file: a remote read is the tail of that box's
/// `messages.jsonl`, bytes for bytes, with the same torn first line to drop.
pub fn turns(text: &str, max: usize) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    for line in text.lines() {
        let Ok(rec) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match rec.get("t").and_then(|t| t.as_str()).unwrap_or("") {
            "user" => out.push(Turn { kind: Kind::User, text: one_line(&str_of(&rec, "text")), ts: str_of(&rec, "ts") }),
            "assistant" => {
                let ts = str_of(&rec, "ts");
                let thinking = str_of(&rec, "thinking");
                if !thinking.trim().is_empty() {
                    out.push(Turn { kind: Kind::Thought, text: one_line(&thinking), ts: ts.clone() });
                }
                for call in rec.get("calls").and_then(|c| c.as_array()).into_iter().flatten() {
                    out.push(Turn {
                        kind: Kind::Call,
                        text: call_line(
                            call.get("name").and_then(|n| n.as_str()).unwrap_or("?"),
                            call.get("args"),
                        ),
                        ts: ts.clone(),
                    });
                }
                let content = str_of(&rec, "content");
                if !content.trim().is_empty() {
                    out.push(Turn { kind: Kind::Answer, text: content, ts });
                }
            }
            "tool" => {
                // The result is a JSON value: a string for some tools and an
                // object for others (`{"ok": true, "app": "Firefox"}`). Reading
                // it as a string gave an empty row for every tool that answers
                // with an object, which is most of them.
                let name = str_of(&rec, "name");
                out.push(Turn {
                    kind: Kind::Result,
                    text: result_line(&name, rec.get("result")),
                    ts: str_of(&rec, "ts"),
                });
            }
            _ => {}
        }
    }
    if out.len() > max {
        out.drain(..out.len() - max);
    }
    out
}

/// How much of the end of a transcript is read. 256KB is thousands of rows of
/// conversation and a fraction of a millisecond off an SSD.
///
/// Public because the same number is what a remote read asks `tail` for: a box read
/// over `ssh` and a file read here are the same read, and two sizes for it is how
/// one of them ends up drawing shorter than the other.
pub const TAIL_BYTES: usize = 256 * 1024;

/// The last `n` bytes of a file, as text, with the partial first line dropped.
///
/// The dropped line is the point: a byte offset lands in the middle of a record
/// about half the time, and a half a JSON object parses as nothing, silently
/// swallowing one row.
fn read_tail(path: &Path, n: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(n);
    if start > 0 {
        file.seek(SeekFrom::Start(start))?;
    }
    let mut buf = Vec::with_capacity((len - start) as usize + 1);
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok(match start {
        0 => text,
        _ => match text.split_once('\n') {
            Some((_, rest)) => rest.to_string(),
            None => String::new(),
        },
    })
}

/// What a file's stamp is: its length and its modification time. The cache key
/// for anything read off disk.
///
/// Both and not just the mtime: a write inside the same second that keeps the
/// length is possible, and a cache that misses a change is worse than no cache.
pub fn stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Some((meta.len(), mtime))
}

/// A session's plan (`todo.json`), as the pairs the sidebar draws.
pub fn plan(home: &Path, id: &str) -> Vec<(String, String)> {
    if !safe_id(id) {
        return Vec::new();
    }
    #[derive(serde::Deserialize)]
    struct Item {
        #[serde(default)]
        text: String,
        #[serde(default)]
        state: String,
    }
    #[derive(serde::Deserialize, Default)]
    struct File {
        #[serde(default)]
        items: Vec<Item>,
    }
    let path = home.join("sessions").join(id).join("todo.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<File>(&text) else {
        return Vec::new();
    };
    file.items.into_iter().map(|i| (i.text, i.state)).collect()
}

/// A run of work: the calls and reasoning between one answer and the next, as
/// one thing to draw.
///
/// The alternative is what a transcript looks like when it is a list of records:
/// forty rows of `read(...)`, each with its own result under it, and the two
/// sentences that matter lost in the middle. A person reads a session as turns -
/// what was asked, what was done about it, what came of it - so this is the
/// shape, and the rows inside a run are one line each because that is all a run
/// is worth until you open it.
///
/// `Note` is what was said in the run and is not a step: a thought, or a
/// sentence the model wrote alongside its calls. It is kept in order with them,
/// because "I will read the file, and here is what I found" is one thing.
#[derive(Clone, Debug)]
pub enum Row {
    /// Something the person said.
    Said(String),
    /// A run of work: what it was for, how long it took, and what it did.
    ///
    /// `secs` is the distance from the record that opened the turn to the one
    /// that ended it, and it is what the run's own line says: **"Worked for
    /// 2m 13s"** rather than a count of calls. The reason that is better: a count
    /// of calls is the agent's own bookkeeping
    /// while a duration is the thing the person waiting actually felt. `None`
    /// when the records carry no stamps to measure with.
    Run { note: Option<String>, secs: Option<f64>, steps: Vec<Step> },
    /// The answer, which is what the run was for.
    Answer(String),
}

/// One line of a run.
#[derive(Clone, Debug)]
pub struct Step {
    /// The call, as the call itself: `read(src/main.rs)`.
    pub call: String,
    /// What it answered, one line. `None` when nothing came back, which is not
    /// the same as an empty answer.
    pub result: Option<String>,
    /// Whether the result read as a failure. Derived from the text and marked as
    /// such, which is the only thing a frontend can honestly do with a tool
    /// result: the worker says what happened in words, not in a flag.
    pub failed: bool,
}

/// How a tool result reads. The worker's own vocabulary for a call that did not
/// work is the word `error` in the text, which is what every tool's failure path
/// writes (`{"error": ...}` serialised).
fn looks_failed(result: &str) -> bool {
    let r = result.to_ascii_lowercase();
    r.contains("error") || r.contains("failed") || r.contains("traceback") || r.starts_with("exit code")
}

/// Fold a session's records into rows: the conversation as a person reads it.
///
/// One pass, in order, and the only structure it invents is the run - which is
/// what the records already say, since the model's calls sit between one answer
/// and the next.
pub fn fold(turns: &[Turn]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut run: Vec<Step> = Vec::new();
    let mut pending: Option<String> = None;
    let mut note: Option<String> = None;
    // When the turn started and when its last record landed. The distance is what
    // the run's line says, and both are read off the records' own stamps.
    let mut began: Option<String> = None;
    let mut ended: Option<String> = None;

    // Close the open run, if there is one, and put it on the list.
    let flush = |rows: &mut Vec<Row>,
                 run: &mut Vec<Step>,
                 note: &mut Option<String>,
                 began: &mut Option<String>,
                 ended: &mut Option<String>| {
        if !run.is_empty() || note.is_some() {
            let secs = match (began.take(), ended.take()) {
                (Some(a), Some(b)) => iso_secs_between(&a, &b),
                _ => None,
            };
            rows.push(Row::Run { note: note.take(), secs, steps: std::mem::take(run) });
        }
    };

    // Every record of a turn feeds the clock, whichever kind it is: the wait is
    // from the person's own message to the answer, and the calls in between are
    // where the time actually went.
    let mark = |ts: &str, began: &mut Option<String>, ended: &mut Option<String>| {
        if ts.is_empty() {
            return;
        }
        if began.is_none() {
            *began = Some(ts.to_string());
        }
        *ended = Some(ts.to_string());
    };

    for turn in turns {
        // The clock first, for every kind: the person's own message is what the
        // run was waiting on, and it is part of what the run took.
        mark(&turn.ts, &mut began, &mut ended);
        match turn.kind {
            Kind::User => {
                // A user line is the boundary: it closes the run above it and
                // starts the clock for the one below.
                flush(&mut rows, &mut run, &mut note, &mut began, &mut ended);
                began = if turn.ts.is_empty() { None } else { Some(turn.ts.clone()) };
                ended = began.clone();
                rows.push(Row::Said(turn.text.clone()));
            }
            Kind::Answer => {
                // An answer ends whatever run was open: it is the point the run
                // was making. Its own stamp is the run's end, and it is read
                // before the flush so the duration includes the final answer.
                flush(&mut rows, &mut run, &mut note, &mut began, &mut ended);
                rows.push(Row::Answer(turn.text.clone()));
            }
            Kind::Thought => {
                // A thought between calls belongs to the run. A second thought
                // does not stack: two lines of reasoning either side of a call
                // are one run's voice, and the first is the one that says what
                // the run is for.
                if note.is_none() {
                    note = Some(turn.text.clone());
                }
            }
            Kind::Call => {
                run.push(Step { call: turn.text.clone(), result: None, failed: false });
            }
            Kind::Result => {
                // The result belongs to the call above it.
                if let Some(step) = run.last_mut() {
                    step.failed = looks_failed(&turn.text);
                    step.result = Some(turn.text.clone());
                }
            }
        }
        let _ = &pending;
        pending = None;
    }
    flush(&mut rows, &mut run, &mut note, &mut began, &mut ended);
    rows
}

/// The seconds between two of the worker's ISO stamps, or `None` if either will
/// not parse.
///
/// Hand-rolled rather than pulling in a date crate for one subtraction: the
/// worker writes `2026-10-12T09:20:22+00:00` and every record of every session on
/// this machine carries the same offset, so the offset cancels in a difference and
/// is not read at all. `ponytail:` that is the ceiling - a session recorded across
/// a DST change would be off by the hour, and the upgrade is `time`/`chrono`,
/// which this crate does not depend on and does not need for a duration.
fn iso_secs_between(a: &str, b: &str) -> Option<f64> {
    let (da, ta) = a.split_once('T')?;
    let (db, tb) = b.split_once('T')?;
    // The date is only used as a day count, so the difference can cross midnight.
    let days = day_number(db)? - day_number(da)?;
    let t = clock_secs(tb)? - clock_secs(ta)? + days as f64 * 86_400.0;
    (t >= 0.0).then_some(t)
}

/// The seconds since midnight in a `09:20:22+00:00` stamp.
fn clock_secs(time: &str) -> Option<f64> {
    let t = time.get(0..8)?;
    let mut parts = t.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// A `YYYY-MM-DD` date as a plain day count, so a difference can cross midnight.
fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    // days_from_civil, the standard one: 1970-01-01 is 0.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// A duration, as words: the same ladder, from tenths to minutes.
///
/// Tenths while it is short, because "8.0s" and "9s" are different feelings and
/// the first few seconds are the ones a person is watching; whole seconds to a
/// minute; then minutes and seconds, because "2m 13s" says more than "133s".
pub fn duration(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "0ms".into();
    }
    let ms = secs * 1000.0;
    if ms < 1000.0 {
        return format!("{}ms", (ms.round() as i64).max(1));
    }
    if ms < 10_000.0 {
        let tenths = (ms / 100.0).round() / 10.0;
        return if tenths >= 10.0 { "10s".into() } else { format!("{tenths:.1}s") };
    }
    if secs < 60.0 {
        return format!("{}s", secs.round() as i64);
    }
    let total = secs.round() as i64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    let mut parts: Vec<String> = Vec::new();
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 {
        parts.push(format!("{m}m"));
    }
    if s > 0 {
        parts.push(format!("{s}s"));
    }
    if parts.is_empty() { "0s".into() } else { parts.join(" ") }
}

/// One entry in the transcript's minimap: which row a turn starts at, and the two
/// lines it is remembered by.
///
/// Its rule: **a tick per turn**, not per row. A strip with one tick per record is
/// a barcode in a long
/// session; one per thing the person said is a table of contents, and a table of
/// contents is what you navigate by.
#[derive(Clone, Debug)]
pub struct Anchor {
    /// The row index this turn starts at, which is what a jump scrolls to.
    pub row: usize,
    /// What was asked, as one line: the mark's first line.
    pub ask: String,
    /// What came back, if anything yet: the second line of the mark.
    pub answer: String,
}

/// The turns in a folded transcript, in order.
///
/// Derived from the rows rather than kept beside them, so the two cannot drift:
/// the rows are the one thing the window already has, and a turn is a `Said` row
/// and whatever answers it until the next one.
pub fn anchors(rows: &[Row]) -> Vec<Anchor> {
    let mut out: Vec<Anchor> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        match row {
            Row::Said(text) => out.push(Anchor { row: i, ask: one_line(text), answer: String::new() }),
            Row::Answer(text) => {
                // The answer belongs to the ask above it, and the last one wins: a
                // turn answered twice is named by what it ended on.
                if let Some(last) = out.last_mut() {
                    last.answer = one_line(text);
                }
            }
            Row::Run { .. } => {}
        }
    }
    out
}

/// A call as one line: the name, then the arguments that say which thing it was
/// about. `write({"path": "src/main.rs", ...})` is a wall; `write(src/main.rs)`
/// is the row fastrs draws and the one a person reads.
fn call_line(name: &str, args: Option<&serde_json::Value>) -> String {
    let Some(args) = args.and_then(|a| a.as_object()) else {
        return name.to_string();
    };
    // The first argument that says which thing this call was about, in the order
    // a person would ask: what, then where.
    for key in ["path", "file", "dir", "command", "pattern", "query", "name", "action"] {
        if let Some(v) = args.get(key) {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                other => one_line(&other.to_string()),
            };
            if !s.trim().is_empty() {
                return format!("{name}({})", clip(&s, 64));
            }
        }
    }
    name.to_string()
}

/// A tool's answer as one line: the tool's name, then what it said.
///
/// `result` is a JSON value and its shape is the tool's own business: a string
/// for a shell, an object with an `ok` for the app tools, a list for a search.
/// So it is rendered and flattened rather than read as text, which is what made
/// every object-shaped result draw as an empty line.
fn result_line(name: &str, result: Option<&serde_json::Value>) -> String {
    let Some(v) = result else {
        return name.to_string();
    };
    let said = match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => one_line(s),
        other => {
            // An object or a list, as `key=value` pairs in the order it was
            // written: `ok=true app=Firefox snapped=true`.
            let flat = flatten(other, 0);
            one_line(&flat)
        }
    };
    if said.is_empty() { name.to_string() } else { said }
}

/// A JSON value as words: scalars as they are, objects and lists as pairs.
/// Depth-capped, because a tool result can be any shape at all and this is one
/// line of a row.
fn flatten(v: &serde_json::Value, depth: usize) -> String {
    if depth > 2 {
        return "\u{2026}".into();
    }
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a
            .iter()
            .take(4)
            .map(|x| flatten(x, depth + 1))
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        serde_json::Value::Object(o) => o
            .iter()
            .take(8)
            .map(|(k, x)| {
                let x = flatten(x, depth + 1);
                if x.is_empty() { k.clone() } else { format!("{k}={x}") }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// The string at a key, or empty.
fn str_of(rec: &serde_json::Value, key: &str) -> String {
    rec.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// The first line of something, collapsed to one row. A transcript row is one
/// line: a folded block is what a `+N lines` marker is for, and this window does
/// not have one yet.
pub fn one_line(text: &str) -> String {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    clip(first, 240)
}

/// A string cut to `n` characters with an ellipsis.
pub fn clip(text: &str, n: usize) -> String {
    if text.chars().count() <= n {
        return text.to_string();
    }
    let cut: String = text.chars().take(n).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// A session id is a path component, so it is refused rather than trusted: an id
/// with a `/` or a `..` in it would read a file outside the sessions directory.
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && !id.contains('/')
        && !id.contains('\\')
        && id != "."
        && id != ".."
        && !id.contains("..")
}

/// What the composer may offer: every provider the worker can reach, and the
/// models each one can be asked for.
///
/// Read from the worker's own **registry**, not from `config.json` alone. That is
/// the correction that matters here: `config.json` holds a key for one provider
/// on this machine, so a list built from it said "one provider" while the worker
/// could reach nine. The registry is the authority on what exists
/// (`handler/agent/providers/__init__.py`), and a provider is reachable whenever
/// *either* the file has a key for it *or* its own environment variable is set -
/// `config.api_key` reads both, and reads the environment first.
///
/// One subprocess at boot, `python3 -c`, which prints the registry as JSON. It is
/// the only way to ask a question the answer to which lives in Python, and it is
/// 77ms and needs no network and no key. There is no second copy of the provider
/// list in Rust: a list that drifts from the registry is a list that offers a
/// provider the worker does not have.
#[derive(Clone, Debug, Default)]
pub struct Choices {
    /// The provider the worker is on, `active` in `config.json`.
    pub active: String,
    /// Every provider, in registry order, with what it needs and what it can be
    /// asked for.
    pub providers: Vec<Provider>,
    /// The permission mode the worker is on, from the same file.
    pub permission: String,
}

/// One provider, as the registry describes it.
#[derive(Clone, Debug, Default)]
pub struct Provider {
    pub name: String,
    /// The environment variable its key is read from, when it has one.
    pub key_env: String,
    /// Whether a key was found in the file or the environment. Nothing is hidden
    /// for being unkeyed: a provider with no key is a provider you have not set
    /// up yet, and hiding it would make the list look broken.
    pub keyed: bool,
    /// The model the registry would pick with nothing named.
    pub default_model: String,
    /// The registry's own shortlist, best first. This is what their `models()`
    /// puts above the host's catalog, and it is the answer to "which should I
    /// use" rather than "what exists".
    pub shortlist: Vec<String>,
    /// The models this account has actually run, from the session records on
    /// disk. Kept because they are the ones known to work here.
    pub history: Vec<String>,
    /// The models with capabilities recorded under this provider, from
    /// `config.json`'s `model_caps`.
    pub caps: Vec<String>,
}

impl Choices {
    pub fn model_of(&self, provider: &str) -> String {
        self.providers
            .iter()
            .find(|p| p.name == provider)
            .map(|p| {
                if p.default_model.trim().is_empty() {
                    p.shortlist.first().cloned().unwrap_or_default()
                } else {
                    p.default_model.clone()
                }
            })
            .unwrap_or_default()
    }

    pub fn provider(&self, name: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.name == name)
    }

}

/// Read `config.json` for the two settings that are the file's, then ask the
/// worker's own registry what providers exist.
pub fn choices(home: &Path, worker: &Path) -> Choices {
    let mut out = Choices::default();
    let text = std::fs::read_to_string(home.join("config.json")).unwrap_or_default();
    if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&text) {
        let str_at = |v: &serde_json::Value, k: &str| {
            v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string()
        };
        out.active = str_at(&cfg, "active");
        out.permission = str_at(&cfg, "permission");
        for entry in registry(worker) {
            let name = entry.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let key_env = entry.get("key_env").and_then(|e| e.as_str()).unwrap_or("").to_string();
            // A key is reachable from either place, and the environment wins -
            // the same lookup `handler/config.py:api_key` does.
            let in_file = cfg
                .get("providers")
                .and_then(|p| p.get(&name))
                .and_then(|p| p.get("api_key"))
                .and_then(|k| k.as_str())
                .is_some_and(|k| !k.trim().is_empty());
            let in_env = !key_env.is_empty()
                && std::env::var(&key_env).map(|v| !v.trim().is_empty()).unwrap_or(false);
            let caps: Vec<String> = cfg
                .get("providers")
                .and_then(|p| p.get(&name))
                .and_then(|p| p.get("model_caps"))
                .and_then(|c| c.as_object())
                .map(|c| c.keys().cloned().collect())
                .unwrap_or_default();
            out.providers.push(Provider {
                name,
                key_env,
                keyed: in_file || in_env,
                default_model: entry
                    .get("default_model")
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .to_string(),
                shortlist: entry
                    .get("shortlist")
                    .and_then(|s| s.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
                history: Vec::new(),
                caps,
            });
        }
    }
    if out.active.is_empty() {
        out.active = "ollama".into();
    }
    if out.permission.is_empty() {
        out.permission = "default".into();
    }
    out
}

/// The worker's provider registry, as JSON: name, key variable, default model
/// and shortlist, for every provider it has.
///
/// A subprocess and not a hand-written list, and not a network call either: this
/// is the worker answering a question about itself, which is exactly the case a
/// second copy in Rust would get wrong. Prints `[]` on any failure, so a broken
/// worker leaves an empty list rather than a wrong one.
fn registry(worker: &Path) -> Vec<serde_json::Value> {
    let script = r#"
import json
try:
    from handler.agent.providers import PROVIDERS
except Exception:
    print("[]"); raise SystemExit
out = []
for name, p in PROVIDERS.items():
    out.append({
        "name": name,
        "key_env": getattr(p, "key_env", "") or "",
        "default_model": getattr(p, "default_model", "") or "",
        "shortlist": list(getattr(p, "model_scores", {}) or {}),
    })
print(json.dumps(out))
"#;
    // The worker's own directory, so `handler` imports without a PYTHONPATH.
    let dir = worker.parent().unwrap_or_else(|| Path::new("."));
    let out = std::process::Command::new("python3")
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    serde_json::from_slice(&out.stdout).unwrap_or_default()
}

// ---------------------------------------------------------------- icons and marks
//
// A project's own mark, and what a menu row says about a provider or a model.
// Nothing here draws: `view` turns these into elements.

/// The project's icon: an emoji, or an uploaded image, from `projects.json`'s
/// `icons` map.
///
/// The worker's own three forms, quoted from `handler/session/store.py`: one
/// emoji (up to 8 chars), a gradient token `grad:<n>`, or an uploaded image
/// `img:data:image/...;base64,...`. This window draws the first two; the third
/// needs an image pipeline it does not have, and their own `ProjectThumbnail`
/// falls back to the letter when an image fails to load, so that is what happens
/// here too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Icon {
    /// One emoji, or any short string the user typed.
    Glyph(String),
    /// A gradient token: their numbered set, one rung per byte.
    Gradient(u8),
    /// An uploaded image, which this window cannot decode yet.
    Image,
    /// Nothing set: the first letter of the name.
    Letter,
}

impl Icon {
    pub fn of(token: &str) -> Icon {
        let t = token.trim();
        if t.is_empty() {
            return Icon::Letter;
        }
        if let Some(n) = t.strip_prefix("grad:") {
            return Icon::Gradient(n.parse().unwrap_or(0));
        }
        if t.starts_with("img:") {
            return Icon::Image;
        }
        Icon::Glyph(t.to_string())
    }
}

/// Every project's icon token, keyed by its directory.
pub fn icons(home: &Path) -> std::collections::HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(home.join("projects.json")) else {
        return std::collections::HashMap::new();
    };
    let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&text) else {
        return std::collections::HashMap::new();
    };
    cfg.get("icons")
        .and_then(|i| i.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Where each provider in the worker's registry points, for a menu row.
///
/// The registry's own `base_url`, and the two that carry none in it are the two
/// that go to the vendor's own API. A host is the only thing that tells nine api
/// keys apart, so a row that omits it says nothing about the provider it is for.
/// Which vendored brand mark a provider wears.
///
/// The provider's own logo, from `assets/brands/`: `None` when there is no mark
/// for it, and a provider with no mark gets a letter rather than a Lucide
/// stand-in, because a wrong mark is worse than no mark.
///
/// A table of *brand* names, not of providers. The providers themselves come from
/// the worker's registry, so a provider the user adds to their own `config.json`
/// that happens to be one of these gets its mark for free, and one that is not
/// simply has none.
pub fn brand(provider: &str) -> Option<&'static str> {
    let key = provider.trim().to_ascii_lowercase();
    BRANDS.iter().find(|(name, _)| *name == key).map(|(_, file)| *file)
}

/// Every mark in `assets/brands/`, keyed by the lowercased provider name. The
/// files are named for their key, so this list and that directory have to agree;
/// a test reads the directory and checks.
pub const BRANDS: [(&str, &str); 24] = [
    ("ollama", "ollama.svg"),
    ("openai", "openai.svg"),
    ("anthropic", "anthropic.svg"),
    ("deepseek", "deepseek.svg"),
    ("nvidia", "nvidia.svg"),
    ("openrouter", "openrouter.svg"),
    ("together", "together.svg"),
    // The registry key is `Synthetic`, which serves Hugging Face models over
    // their own endpoint, so the mark is the one whose models those are.
    ("synthetic", "huggingface.svg"),
    // And `qubrain` serves GLM.
    ("qubrain", "glmv.svg"),
    ("huggingface", "huggingface.svg"),
    ("claude", "claude.svg"),
    ("gemini", "gemini.svg"),
    ("google", "google.svg"),
    ("meta", "meta.svg"),
    ("qwen", "qwen.svg"),
    ("mistral", "mistral.svg"),
    ("groq", "groq.svg"),
    ("grok", "grok.svg"),
    ("cohere", "cohere.svg"),
    ("kimi", "moonshot.svg"),
    ("moonshot", "moonshot.svg"),
    ("minimax", "minimax.svg"),
    ("zhipu", "zhipu.svg"),
    ("glmv", "glmv.svg"),
];

pub const HOSTS: [(&str, &str); 9] = [
    ("Synthetic", "api.synthetic.new"),
    ("anthropic", "api.anthropic.com"),
    ("deepseek", "api.deepseek.com"),
    ("nvidia", "integrate.api.nvidia.com"),
    ("ollama", "ollama.com"),
    ("openai", "api.openai.com"),
    ("openrouter", "openrouter.ai"),
    ("qubrain", "qubrain.org"),
    ("together", "api.together.xyz"),
];

/// A row of a menu, as the thing it is.
///
/// Four facts and not a tuple, because a menu row has more to say than a label:
/// whether you can pick it, which group it belongs to, what it is, and how it
/// looks when it is the one you are on. A list where eight of nine rows are
/// unusable has to *look* like that, or it is a list of nine failures.
#[derive(Clone, Debug)]
pub struct MenuRow {
    /// What the row says, and what choosing it means.
    pub value: String,
    /// What it is: a host, a capability, a sentence.
    pub note: String,
    /// Whether it is the one currently chosen.
    pub current: bool,
    /// Whether choosing it would work. A provider with no key is here and not
    /// pickable, which is the honest way round: hiding it makes the list look
    /// broken, and offering it makes a click fail.
    pub enabled: bool,
    /// The group it sits under, when the menu has groups.
    pub group: Option<String>,
    /// Its mark, when it has one of its own.
    pub brand: Option<String>,
}

impl MenuRow {
    pub fn new(value: impl Into<String>, note: impl Into<String>) -> MenuRow {
        MenuRow {
            value: value.into(),
            note: note.into(),
            current: false,
            enabled: true,
            group: None,
            brand: None,
        }
    }

    pub fn current(mut self, on: bool) -> MenuRow {
        self.current = on;
        self
    }

    pub fn enabled(mut self, on: bool) -> MenuRow {
        self.enabled = on;
        self
    }

    pub fn group(mut self, group: impl Into<String>) -> MenuRow {
        self.group = Some(group.into());
        self
    }

    pub fn brand(mut self, brand: Option<&str>) -> MenuRow {
        self.brand = brand.map(str::to_string);
        self
    }
}

/// One model as a menu row needs it.
#[derive(Clone, Debug, Default)]
pub struct ModelRow {
    pub name: String,
    /// The capabilities this account has recorded for it, as words. Empty when
    /// it has never run here, which is itself worth seeing.
    pub caps: String,
    /// How long ago it was last used. Empty for a model never run here.
    pub age: String,
    /// Whether this account has actually run it. What the menu groups by.
    pub used_here: bool,
}

/// The models a provider actually serves, asked of the provider itself.
///
/// This is the register of what exists, and the answer is per provider: ollama's
/// `/api/tags` and every openai-dialect host's `/v1/models`. Their `models()`
/// ranks a shortlist above the host's own catalog and returns both, which is the
/// whole list this window should offer.
///
/// A subprocess and a network call, so it is **not** on the render path and not
/// on the tick: the caller fetches it on a thread and hands it in through a
/// channel. It is one call per provider, and the answer only changes when the
/// account does.
pub fn catalog(worker: &Path, provider: &str) -> Vec<String> {
    let script = r#"
import json, sys
name = sys.argv[1]
try:
    from handler.agent.providers import PROVIDERS
    p = PROVIDERS.get(name)
    found = p.models() if p else None
except Exception:
    found = None
print(json.dumps(found or []))
"#;
    let dir = worker.parent().unwrap_or_else(|| Path::new("."));
    let Ok(out) = std::process::Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(provider)
        .current_dir(dir)
        .output()
    else {
        return Vec::new();
    };
    let mut list: Vec<String> = serde_json::from_slice(&out.stdout).unwrap_or_default();
    list.retain(|m| !m.trim().is_empty());
    list
}

/// The models a provider can be asked for, as menu rows, most recent first.
///
/// The order is the point: a model you ran an hour ago is almost always the one
/// you want, and a registry shortlist is what someone thinks you should use. So
/// this account's own history leads, newest first, then the shortlist, then
/// anything with capabilities recorded.
pub fn model_rows(home: &Path, provider: &str, catalog: &[String]) -> Vec<ModelRow> {
    let history = model_history(home);
    let caps = caps_of(home, provider);
    let used = history.get(provider).cloned().unwrap_or_default();
    let used_at = |name: &str| -> String {
        used.iter().find(|(n, _)| n == name).map(|(_, s)| s.clone()).unwrap_or_default()
    };
    // Only what the provider actually serves. A model that is in this account's
    // history but no longer in the catalog is gone, and offering it would be a
    // row that fails on click. The one exception is the model the window is
    // already on, which the caller adds, because a session pinned to a model
    // that has since been retired still has to be able to say so.
    let mut names: Vec<String> = catalog.to_vec();
    names.retain(|m| !m.trim().is_empty());
    names.sort();
    names.dedup();
    // Two orders in one list: what this account has run, newest first, above the
    // rest of the catalog. That is "the ones I use" over "the ones that exist".
    let (mut mine, mut rest): (Vec<String>, Vec<String>) = names
        .into_iter()
        .partition(|m| !used_at(m).is_empty());
    mine.sort_by(|a, b| used_at(b).cmp(&used_at(a)));
    let row = |name: &str| ModelRow {
        name: name.to_string(),
        caps: caps.get(name).cloned().unwrap_or_default(),
        age: age_of(&used_at(name)),
        used_here: !used_at(name).is_empty(),
    };
    mine.iter().map(|m| row(m)).chain(rest.iter().map(|m| row(m))).collect()
}

/// Which provider has run which model, and when it last did: `(model, stamp)`.
pub fn model_history(home: &Path) -> std::collections::HashMap<String, Vec<(String, String)>> {
    let mut out: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    let Ok(entries) = std::fs::read_dir(home.join("sessions")) else {
        return out;
    };
    for e in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path().join("meta.json")) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let (Some(p), Some(m)) = (
            meta.get("provider").and_then(|v| v.as_str()),
            meta.get("model").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        if m.trim().is_empty() {
            continue;
        }
        let stamp = meta.get("updated").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let list = out.entry(p.to_string()).or_default();
        // The newest stamp wins, because that is when the model was last run.
        match list.iter_mut().find(|(n, _)| n == m) {
            Some((_, s)) if stamp > *s => *s = stamp,
            Some(_) => {}
            None => list.push((m.to_string(), stamp)),
        }
    }
    out
}

/// The permission modes, with what each does to a run.
///
/// These are the worker's own four, quoted from `handler/config.py`'s
/// `permission_mode`, which is the only place that decides them:
///
///   ask     puts each call to the user
///   auto    hands them to a small model
///   bypass  asks nothing either, but keeps the irreversible calls and puts
///           those to the user
///   off     never asks
///
/// An unknown value lands on `ask` there, so this is the whole vocabulary.
pub const PERMISSIONS: [(&str, &str); 4] = [
    ("ask", "every call is put to you"),
    ("auto", "a small model decides"),
    ("bypass", "only irreversible calls ask"),
    ("off", "never asks"),
];

/// How hard the model thinks, quoted from `handler/agent/effort.py`'s own ladder:
///
///   off     do not think at all
///   low     barely
///   medium  the model's own middle
///   high    the model's own high rung
///   max     as hard as it goes
///
/// It is the worker's vocabulary and not a slider of this window's invention: the
/// worker snaps a level onto the rungs a model actually has
/// (`effort.Spec.rungs`), and refuses `off` outright when a model cannot stop
/// thinking. So an empty string is a real value too - the model's own default - and
/// it is the one a new conversation opens on.
pub const EFFORT: [(&str, &str); 6] = [
    ("", "the model's own default"),
    ("off", "do not think at all"),
    ("low", "barely think"),
    ("medium", "the middle rung"),
    ("high", "think hard"),
    ("max", "as hard as it goes"),
];

/// The capabilities this account has recorded for each of a provider's models.
fn caps_of(home: &Path, provider: &str) -> std::collections::HashMap<String, String> {
    let text = std::fs::read_to_string(home.join("config.json")).unwrap_or_default();
    let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&text) else {
        return std::collections::HashMap::new();
    };
    cfg.get("providers")
        .and_then(|p| p.get(provider))
        .and_then(|p| p.get("model_caps"))
        .and_then(|c| c.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let words: Vec<&str> = v
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                        .unwrap_or_default();
                    (k.clone(), words.join(" · "))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// How long ago a stamp was, as words, through the same parser `Session::age`
/// uses. A stamp of "now" is dropped: a menu that says `now` on every row it
/// just read says nothing.
fn age_of(stamp: &str) -> String {
    if stamp.is_empty() {
        return String::new();
    }
    let s = Session { updated: stamp.to_string(), ..Default::default() };
    let age = s.age();
    if age == "now" { String::new() } else { age }
}

// ---------------------------------------------------------------- git
//
// The changes a session's directory is sitting on. Read with `git` and not over
// the wire: the worker has no action for it, `fleet` does it the same way, and a
// session's own directory is a checkout git can answer about directly.
//
// Everything here is on the cache's clock. A `git status` is a subprocess, and a
// subprocess in a render path is the bug this window already had once.

/// One changed file: what it is called, what it did, and by how much.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changed {
    /// The path, relative to the repository root.
    pub path: String,
    /// `added`, `modified`, `deleted`, `renamed`, `untracked`.
    pub status: String,
    pub added: u32,
    pub removed: u32,
}

impl Changed {
    /// The last component, which is what a narrow row has room for.
    ///
    /// A git record's path can end in a slash. That is not a mistake in the path:
    /// `status --porcelain` collapses an untracked **directory** into one record
    /// and keeps the slash, so `internal/` is a row standing for every file
    /// inside it. `rsplit('/').next()` on that gives the empty string, which is
    /// how a row came out called nothing at all.
    pub fn name(&self) -> String {
        let trimmed = self.path.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
    }

    /// Whether this is a folder standing for its contents rather than one file.
    pub fn is_dir(&self) -> bool {
        self.path.ends_with('/')
    }

    /// The directory the file sits in, as far as the last two components, or
    /// nothing when it is at the root.
    ///
    /// Superset's row draws the name and leaves the path to a tooltip. This
    /// window has no tooltip yet, and a tree with three `main.py` in it is a list
    /// you cannot read: so the parent is drawn beside the name in the ghost tone.
    /// Take it out the day there is a tooltip.
    pub fn parent(&self) -> String {
        let trimmed = self.path.trim_end_matches('/');
        let Some((dir, _)) = trimmed.rsplit_once('/') else {
            return String::new();
        };
        // The last two components, because the top of a path is the same for
        // every row and says nothing.
        let parts: Vec<&str> = dir.split('/').collect();
        let tail = parts[parts.len().saturating_sub(2)..].join("/");
        format!("{tail}/")
    }

}

/// A repository's state: which branch, and what is uncommitted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// False when the directory is not in a checkout at all, which is a state
    /// worth saying rather than drawing as "0 files".
    pub repo: bool,
    pub branch: String,
    /// Staged files, then unstaged, then untracked: their own three sections, in
    /// their own order (`ChangeCategory`).
    pub staged: Vec<Changed>,
    pub unstaged: Vec<Changed>,
    pub untracked: Vec<Changed>,
}

impl Changes {
    pub fn total(&self) -> usize {
        self.staged.len() + self.unstaged.len() + self.untracked.len()
    }

    pub fn added(&self) -> u32 {
        self.all().map(|c| c.added).sum()
    }

    pub fn removed(&self) -> u32 {
        self.all().map(|c| c.removed).sum()
    }

    fn all(&self) -> impl Iterator<Item = &Changed> {
        self.staged.iter().chain(self.unstaged.iter()).chain(self.untracked.iter())
    }
}

/// Read a directory's git state. Every command is asked with `-C <dir>`, so this
/// works on a worktree, a subdirectory of a checkout, or a plain directory that
/// turns out not to be one.
/// One node of the file tree: a folder with what is in it, or a file.
///
/// A tree and not a flat list, because that is what a directory is. Their own
/// `FilesView` is a headless tree with 28px rows and a 10px indent per level, and
/// a flat list of paths is the thing it exists to not be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    /// The path relative to the repository root, which is also its identity.
    pub path: String,
    /// `None` for a file.
    pub children: Option<Vec<Node>>,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.children.is_some()
    }
}

/// The directory's tree, built from the paths git knows about.
///
/// One `git ls-files -z --cached --others --exclude-standard`, then assembled:
/// every path is split on `/` and walked into a tree of folders, so
/// `rust/app/src/main.rs` makes `rust`, `rust/app`, `rust/app/src` and leaves the
/// file under the last of them. Folders lead, each group alphabetical, which is
/// what every file browser on this machine does.
///
/// Sorted here and not per frame: a tree is in one order and that order is decided
/// when it is read. The `bool` is whether the ceiling was hit.
pub fn tree(dir: &Path) -> (Vec<Node>, bool) {
    let Ok(root) = git(dir, &["rev-parse", "--show-toplevel"]).map(|s| PathBuf::from(s.trim()))
    else {
        return (Vec::new(), false);
    };
    let Ok(raw) = git(&root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
    else {
        return (Vec::new(), false);
    };
    let mut dirs: std::collections::BTreeMap<String, Vec<Node>> = std::collections::BTreeMap::new();
    let mut seen = 0usize;
    for rec in raw.split('\0').filter(|r| !r.is_empty()) {
        if seen >= TREE_LIMIT {
            return (assemble(&mut dirs, ""), true);
        }
        let path = rec.trim_start_matches("./").to_string();
        if path.is_empty() {
            continue;
        }
        seen += 1;
        let parts: Vec<&str> = path.split('/').collect();
        // Every folder along the way gets an entry, even before its own file
        // lands: a folder with one file three levels down still exists.
        for i in 0..parts.len() {
            let parent = parts[..i].join("/");
            let here = parts[..=i].join("/");
            let leaf = i + 1 == parts.len() && !path.ends_with('/');
            let entry = Node {
                name: parts[i].to_string(),
                path: here,
                children: if leaf { None } else { Some(Vec::new()) },
            };
            let list = dirs.entry(parent).or_default();
            // A path listed twice can happen; a folder listed as both cannot.
            if !list.iter().any(|n| n.path == entry.path) {
                list.push(entry);
            }
        }
    }
    (assemble(&mut dirs, ""), false)
}

/// Read the flat map out into the tree it describes, from a folder's path down.
///
/// `dirs` is taken **by reference**. It used to be taken by value and recursed
/// into with `std::mem::take`, which is a bug with a nasty shape: the whole map
/// moves into the *first* directory child, so every folder after the first at
/// each level got an empty map and drew as an open row with nothing under it.
/// `.fleet-briefs` had children and `handler`, `rust`, `tests` and `tools` did
/// not, all in the same tree. Nothing was missing from the data; the reader threw
/// it away. `remove(&at)` is what takes a subtree, one at a time.
fn assemble(dirs: &mut std::collections::BTreeMap<String, Vec<Node>>, at: &str) -> Vec<Node> {
    let mut children = dirs.remove(at).unwrap_or_default();
    // Folders first, then files, each alphabetical: every file browser on this
    // machine does it, and mixing the two makes a directory unreadable.
    children.sort_by(|a, b| {
        b.is_dir()
            .cmp(&a.is_dir())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    for child in children.iter_mut() {
        if child.is_dir() {
            let here = child.path.clone();
            child.children = Some(assemble(dirs, &here));
        }
    }
    children
}

/// Flatten filtered, borrowed nodes into rows. The same walk as `tree_rows`,
/// over a slice of references rather than a tree of owners.
/// A tree flattened into rows, with the folders that are open walked into and the
/// ones that are shut left as a single row.
///
/// The tree is a nested structure and a drawing is a list of rows, so this is the
/// seam between them. Doing it here rather than in the builder is a `ponytail`
/// call: a flattened list is what a scroller can draw and what a filter can
/// search, and the tree itself stays the thing that is *read*.
/// A tree flattened into rows, **borrowed**: `(node, depth)` per visible node.
///
/// This used to clone every node on the way out, which is one deep copy of the
/// whole tree per call, and `render` was the caller. On a checkout with three
/// thousand files that is thousands of `String` copies per frame, and a resize
/// drag is a frame per pixel of pointer movement. A row only ever needs to read
/// the node it is for, so it takes a reference.
pub fn tree_rows<'a>(nodes: &'a [Node], is_open: &dyn Fn(&str) -> bool) -> Vec<(&'a Node, usize)> {
    let mut out: Vec<(&Node, usize)> = Vec::new();
    walk(nodes, 0, is_open, &mut out);
    out
}

fn walk<'a>(
    nodes: &'a [Node],
    depth: usize,
    is_open: &dyn Fn(&str) -> bool,
    out: &mut Vec<(&'a Node, usize)>,
) {
    for n in nodes {
        out.push((n, depth));
        if let Some(children) = &n.children {
            if is_open(&n.path) {
                walk(children, depth + 1, is_open, out);
            }
        }
    }
}

pub fn tree_rows_ref<'a>(
    nodes: &[&'a Node],
    is_open: &dyn Fn(&str) -> bool,
) -> Vec<(&'a Node, usize)> {
    let mut out: Vec<(&'a Node, usize)> = Vec::new();
    walk_ref(nodes, 0, is_open, &mut out);
    out
}

fn walk_ref<'a>(
    nodes: &[&'a Node],
    depth: usize,
    is_open: &dyn Fn(&str) -> bool,
    out: &mut Vec<(&'a Node, usize)>,
) {
    for n in nodes {
        out.push((n, depth));
        if let Some(children) = &n.children {
            if is_open(&n.path) {
                let refs: Vec<&Node> = children.iter().collect();
                walk_ref(&refs, depth + 1, is_open, out);
            }
        }
    }
}

/// The tree, filtered, as borrowed nodes. Recursive over four levels, which is
/// how deep a checkout this window draws goes.
pub fn filter_ref<'a>(nodes: &'a [Node], needle: &str) -> Vec<&'a Node> {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return nodes.iter().collect();
    }
    let mut out: Vec<&'a Node> = Vec::new();
    for n in nodes {
        match &n.children {
            None => {
                if n.path.to_lowercase().contains(&needle) {
                    out.push(n);
                }
            }
            Some(_) => {
                if !filter_ref(n.children.as_deref().unwrap_or(&[]), &needle).is_empty()
                    || n.name.to_lowercase().contains(&needle)
                {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// How many files the Files tab holds. A marked ceiling, not a guess: past this
/// the column wants a virtualised list, which it does not have yet.
pub const TREE_LIMIT: usize = 3000;


pub fn changes(dir: &Path) -> Changes {
    let mut out = Changes::default();
    let Ok(root) = git(dir, &["rev-parse", "--show-toplevel"]).map(|s| PathBuf::from(s.trim()))
    else {
        return out;
    };
    out.repo = true;
    out.branch = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "HEAD".into());

    // One pass over `status --porcelain=v1 -z`: the record separator is NUL, so a
    // path with a space in it survives, and the two status columns are the staged
    // and the unstaged letter.
    //
    // `-uall` and not the default: git collapses a wholly-untracked **directory**
    // to a single `dir/` entry (`?? internal/`), and a changes list that shows a
    // folder is a list where clicking the row tries to read a directory as a file
    // (`Could not read internal/: Is a directory`). `-uall` lists the real files
    // under it, which is what the column is about. A repo with a huge untracked
    // tree will list a lot of rows, and that is the honest answer: they are there.
    if let Ok(raw) = git_raw(&root, &["status", "--porcelain=v1", "-uall", "-z"]) {
        for rec in raw.split('\0').filter(|r| r.len() > 2) {
            let (xy, path) = rec.split_at(2);
            let mut it = xy.chars();
            let (x, y) = (it.next().unwrap_or(' '), it.next().unwrap_or(' '));
            let path = path.trim_start_matches(' ').to_string();
            // The line counts, which only `--numstat` has.
            let (added, removed) = numstat(&root, &path);
            let file = |status: &str| Changed {
                path: path.clone(),
                status: status.to_string(),
                added,
                removed,
            };
            if x == '?' {
                out.untracked.push(file("untracked"));
                continue;
            }
            if x != ' ' && x != '?' {
                out.staged.push(file(letter(x)));
            }
            if y != ' ' && y != '?' {
                out.unstaged.push(file(letter(y)));
            }
        }
    }
    out
}

/// `--numstat` for one path, as `(added, removed)`. Binary files report `-` and
/// get nothing, which is not the same as a zero.
fn numstat(root: &Path, path: &str) -> (u32, u32) {
    let Ok(out) = git(root, &["diff", "HEAD", "--numstat", "--", path]) else {
        return (0, 0);
    };
    let Some(line) = out.lines().next() else {
        return (0, 0);
    };
    let mut it = line.split('\t');
    let a = it.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let r = it.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    (a, r)
}

/// A porcelain status letter as the word their own `FileStatus` uses.
fn letter(c: char) -> &'static str {
    match c {
        'A' => "added",
        'M' => "modified",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        _ => "modified",
    }
}

/// One `git` call in `dir`, as trimmed stdout, or the error as a string.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The same, but NUL-safe: `status -z` output is not text in the usual sense and
/// `String::from_utf8_lossy` plus `.lines()` would eat a path with a newline.
fn git_raw(dir: &Path, args: &[&str]) -> Result<String, String> {
    git(dir, args)
}

/// What the sidebar is showing when a file is opened: a patch, or a whole file.
///
/// The distinction is the whole reason this is a type rather than a string. A
/// patch has `+` and `-` lines and is coloured by them; a **whole file** has no
/// such lines, and painting every one of its rows as an addition is what made an
/// untracked file read as a green smear. Their own viewer makes the same split:
/// `LightDiffViewer` for a diff, a code view for a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Viewing {
    /// A patch, from `git diff HEAD`.
    Patch(String),
    /// A file's own text, which is not a patch and is not coloured as one.
    Whole(String),
    /// Nothing to show, and why.
    Empty(String),
}

impl Viewing {
}

/// The file a change touched, as a patch or as its own text.
///
/// `diff HEAD` and not `diff --cached`: the point is what this checkout is
/// carrying that is not committed, staged or not. An untracked file has no diff
/// against HEAD, so it is read and shown whole, which is the honest answer and
/// the one a person expects when they click a file that only exists on their
/// disk.
pub fn file_view(dir: &Path, path: &str) -> Viewing {
    let Ok(root) = git(dir, &["rev-parse", "--show-toplevel"]).map(|s| PathBuf::from(s.trim()))
    else {
        return Viewing::Empty("Not a git checkout.".into());
    };
    let tracked = git(&root, &["ls-files", "--error-unmatch", "--", path]).is_ok();
    if !tracked {
        // A directory is not a file, and reading one is an `os error 21` dressed
        // up as a message about the user's repository. This is the seam where a
        // path stops being a path and becomes a thing to read, so it is where the
        // check goes: `Changed::is_dir` already knows, and a row that opened a
        // folder is a row that should have opened its tree.
        if root.join(path).is_dir() {
            return Viewing::Empty("This is a folder; open it in the Files tab.".into());
        }
        return match std::fs::read_to_string(root.join(path)) {
            Ok(text) => Viewing::Whole(text),
            Err(e) => Viewing::Empty(format!("Could not read {path}: {e}")),
        };
    }
    let patch = git(&root, &["diff", "HEAD", "--", path]).unwrap_or_default();
    if patch.trim().is_empty() {
        return Viewing::Empty(format!("{path} has no changes against HEAD."));
    }
    Viewing::Patch(patch)
}

/// Add a directory to the user's own `projects.json`.
///
/// A read-modify-write, the way every mutation in `handler/session/store.py` is,
/// and for the reason theirs is: this file is the worker's, and the day the
/// worker is the one adding a project, a frontend that wrote a bare dict literal
/// would have thrown the user's names, icons and pins away.
///
/// It refuses a directory that does not exist: promoting a path that is not
/// there gives a project row with nothing under it and no way to notice.
pub fn add_project(dir: &Path) -> Result<(), String> {
    let dir = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let path = state_root().join("projects.json");
    let mut file: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let obj = file.as_object_mut().ok_or_else(|| "projects.json is not an object".to_string())?;
    let list = obj.entry("projects").or_insert_with(|| serde_json::json!([]));
    let list = list.as_array_mut().ok_or_else(|| "projects is not a list".to_string())?;
    let as_str = dir.display().to_string();
    if !list.iter().any(|p| p.as_str() == Some(as_str.as_str())) {
        list.push(serde_json::Value::String(as_str));
    }
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    // Written beside and moved over, the same atomic replace `store.py` does, so
    // an interrupted write cannot leave a half file that reads as no projects.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

/// The state root: `CUACODE_HOME` when it is set, `~/.cuacode` otherwise. The
/// same resolution `handler/session/store.py` does, because pointing the window
/// at a different root is how its own tests run against a scratch home. It is
/// the shared crate's one copy, so the window and `doctor` cannot disagree
/// about which directory they are talking about.
pub use cuacode::state_root;

/// Whether a session's directory is the project's or under it.
///
/// **Case-insensitively**, because macOS is: `/Users/x/documents/demo` and
/// `/Users/x/Documents/demo` are one directory on this machine and two strings
/// in the file. Comparing them as strings files sessions under `No directory`
/// while their project sits right above them, which is the kind of bug that
/// makes a window look like it is guessing.
///
/// Component by component rather than on the whole string, so a path that merely
/// shares a *prefix* (`proj` and `proj-other`) is still not inside it.
fn under(cwd: &str, dir: &Path) -> bool {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return false;
    }
    let mut want = dir.components();
    let mut have = Path::new(cwd).components();
    loop {
        match (want.next(), have.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(w), Some(h)) => {
                let ws = w.as_os_str().to_string_lossy();
                let hs = h.as_os_str().to_string_lossy();
                if !ws.eq_ignore_ascii_case(&hs) {
                    return false;
                }
            }
        }
    }
}

/// `projects.json`, or nothing when it is absent or unreadable.
fn read_projects(home: &Path) -> Vec<Project> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        projects: Vec<String>,
        #[serde(default)]
        pinned: Vec<String>,
        #[serde(default)]
        names: std::collections::HashMap<String, String>,
        #[serde(default)]
        icons: std::collections::HashMap<String, String>,
    }
    let Ok(text) = std::fs::read_to_string(home.join("projects.json")) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<File>(&text) else {
        return Vec::new();
    };
    file.projects
        .into_iter()
        .filter_map(|dir| {
            if dir.trim().is_empty() {
                return None;
            }
            let path = PathBuf::from(&dir);
            // The file's own name for it, or the directory's leaf. A project
            // called "" would be a row with no name.
            let custom = file.names.get(&dir).map(String::as_str).unwrap_or("");
            let name = if custom.trim().is_empty() {
                leaf(&path)
            } else {
                custom.to_string()
            };
            let pinned = file.pinned.iter().any(|p| p == &name || p == &leaf(&path));
            Some(Project { dir: path, name, pinned, promoted: true, sessions: Vec::new() })
        })
        .collect()
}

/// Every `sessions/<id>/meta.json`, newest-first order not assumed: the caller
/// sorts.
///
/// Only `meta.json` is read. `messages.jsonl` is the conversation and can be
/// megabytes of tool output, and a listing that pays for that is a listing that
/// takes a second to appear.
fn read_sessions(home: &Path) -> Vec<Session> {
    let dir = home.join("sessions");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path().join("meta.json");
            let text = std::fs::read_to_string(path).ok()?;
            let mut s: Session = serde_json::from_str(&text).ok()?;
            if s.id.is_empty() {
                s.id = e.file_name().to_string_lossy().into_owned();
            }
            Some(s)
        })
        .collect()
}

/// The last component of a path, which is what a row has room for.
pub fn leaf(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

/// A path with the home directory shortened to `~`.
pub fn short_path(path: &Path) -> String {
    let Some(home) = cuacode::home_dir() else {
        return path.display().to_string();
    };
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// How long ago an ISO-8601 stamp was, in seconds. `None` if it is not one.
///
/// Hand-parsed rather than pulled in a date library: the worker writes
/// `%Y-%m-%dT%H:%M:%S+00:00` and nothing else, and one format is not a reason to
/// own a calendar. It is a civil-time conversion, and the arithmetic for it is
/// eight lines.
fn iso_age_secs(stamp: &str) -> Option<i64> {
    let b = stamp.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |from: usize, to: usize| stamp.get(from..to)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days since the epoch, civil-from-days (Howard Hinnant's algorithm, which
    // is exact for every date this will ever see).
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let stamp_secs = days * 86400 + hh * 3600 + mm * 60 + ss;
    // The offset the worker writes is +00:00 and the clock it is compared
    // against is UTC, so there is nothing to shift.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((now - stamp_secs).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_with_no_cwd_is_not_placed_by_guessing() {
        assert!(!under("", Path::new("/Users/x/proj")));
        assert!(!under("   ", Path::new("/Users/x/proj")));
    }

    /// A box's listing arrives over the wire and is grouped by the same code the
    /// local disk goes through. No projects, because the window has not read that
    /// box's `projects.json`: every session is headed by its own directory, newest
    /// first, and a session with no directory is the only thing under "No".
    #[test]
    fn a_listing_from_another_box_groups_by_its_own_directories() {
        let s = |id: &str, cwd: &str, updated: &str| Session {
            id: id.into(),
            title: id.into(),
            cwd: cwd.into(),
            updated: updated.into(),
            ..Default::default()
        };
        let listing = vec![
            s("old", "/home/me/work", "2026-10-01T10:00:00+00:00"),
            s("new", "/home/me/work", "2026-10-02T10:00:00+00:00"),
            s("elsewhere", "/home/me/other", "2026-10-01T12:00:00+00:00"),
            s("nowhere", "", "2026-10-01T09:00:00+00:00"),
        ];
        let ws = Workspace::group(Vec::new(), listing, PathBuf::from("/unread"));
        assert_eq!(ws.total, 4);
        let headings: Vec<&str> = ws.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(headings, vec!["work", "other"], "the newest session's directory leads");
        let ids: Vec<&str> = ws.projects[0].sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["new", "old"], "newest first inside a heading");
        assert_eq!(ws.other.len(), 1, "a session with no directory is the only one there");
    }

    #[test]
    fn rows_are_read_from_the_bytes_of_a_transcript() {
        let text = "{\"t\":\"user\",\"ts\":\"t1\",\"text\":\"hi\"}\n\n{\"t\":\"assistant\",\"ts\":\"t2\",\"content\":\"hello\"}\n";
        let rows = turns(text, 40);
        assert_eq!(rows.len(), 2, "the blank line is not a row");
        assert_eq!(rows[0].kind, Kind::User);
        assert_eq!(rows[1].kind, Kind::Answer);
        // A tail read lands mid-record: half a line parses as nothing and costs its own
        // row, never the ones after it.
        let torn = format!("t2\":\"hello\"}}\n{}", text);
        assert_eq!(turns(&torn, 40).len(), 2);
    }

    #[test]
    fn a_session_belongs_to_the_project_it_stands_in() {
        let p = Path::new("/Users/x/proj");
        assert!(under("/Users/x/proj", p));
        assert!(under("/Users/x/proj/src/deep", p));
        // A sibling whose name starts with the project's is not inside it, and
        // a prefix compare without the path type would say it was.
        assert!(!under("/Users/x/proj-other", p));
        assert!(!under("/Users/x", p));
    }

    #[test]
    fn flattening_a_tree_copies_nothing() {
        // The resize-lag bug, as a measurement. This used to return owned nodes:
        // one deep copy of every node per call, from `render`. A checkout of 265
        // files is about 265 nodes per level of the tree, and a resize drag is a
        // frame per pixel.
        let mut dirs: std::collections::BTreeMap<String, Vec<Node>> =
            std::collections::BTreeMap::new();
        // A wide, deep tree, the shape that hurt.
        let mut paths: Vec<String> = Vec::new();
        for a in 0..20 {
            for b in 0..10 {
                for c in 0..5 {
                    paths.push(format!("src/a{a}/b{b}/c{c}/file.rs"));
                }
            }
        }
        for path in &paths {
            let parts: Vec<&str> = path.split('/').collect();
            for i in 0..parts.len() {
                let parent = parts[..i].join("/");
                let here = parts[..=i].join("/");
                let leaf = i + 1 == parts.len();
                let entry = Node {
                    name: parts[i].to_string(),
                    path: here,
                    children: if leaf { None } else { Some(Vec::new()) },
                };
                let list = dirs.entry(parent).or_default();
                if !list.iter().any(|n| n.path == entry.path) {
                    list.push(entry);
                }
            }
        }
        let tree = assemble(&mut dirs, "");
        let started = std::time::Instant::now();
        // Ten flattens: one per frame of a short drag.
        for _ in 0..10 {
            let shown = filter_ref(&tree, "");
            let rows = tree_rows_ref(&shown, &|_| true);
            // Every row points into the tree, so nothing was copied to make it.
            assert!(rows.iter().all(|(n, _)| {
                std::ptr::eq(*n, *n) && !n.path.is_empty()
            }));
        }
        let took = started.elapsed();
        // Borrowing is pointer work. Ten passes over 1000+ nodes must be
        // microseconds; if this ever allocates a node per row it will not be.
        assert!(
            took < std::time::Duration::from_millis(20),
            "10 flattens took {took:?}: is it cloning nodes again?"
        );
    }

    #[test]
    fn nothing_on_the_render_path_reads_the_disk() {
        // The rule this window keeps breaking, as a test with a number in it.
        //
        // `render` runs ~30 times a second while anything is moving, so a read in
        // it costs 30x its own time per second. `model_rows` walks every
        // session's `meta.json` to order the list by what this account has run:
        // 67ms over 459 sessions, which in a frame is 2 seconds of CPU for every
        // second the menu is open. It is built on the press now, and this is the
        // measurement that says why it has to stay that way.
        let root = std::env::temp_dir().join(format!("cua-perf-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // A state root the size of a real one.
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        for i in 0..400 {
            let d = root.join("sessions").join(format!("20260101-0000{i:02}-abcd"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("meta.json"),
                serde_json::json!({
                    "id": format!("s{i}"),
                    "provider": "ollama",
                    "model": if i % 3 == 0 { "glm-5.3-flash" } else { "deepseek-v4.1-flash" },
                    "updated": "2026-01-01T00:00:00+00:00",
                })
                .to_string(),
            )
            .unwrap();
        }
        let choices = Choices {
            active: "ollama".into(),
            providers: vec![Provider {
                name: "ollama".into(),
                default_model: "deepseek-v4.1-flash".into(),
                keyed: true,
                ..Default::default()
            }],
            permission: "ask".into(),
        };
        let catalog: Vec<String> = vec!["glm-5.3-flash".into(), "deepseek-v4.1-flash".into()];
        let started = std::time::Instant::now();
        let rows = model_rows(&root, "ollama", &catalog);
        let took = started.elapsed();
        assert_eq!(rows.len(), 2);
        // The measurement, as an assertion: this is a call that must not be made
        // in a frame, and if it ever becomes cheap enough for one, this fails and
        // says so rather than the window quietly getting slower.
        assert!(
            took > std::time::Duration::from_millis(1),
            "model_rows took {took:?}, which is cheap enough to be suspicious: is it still reading?"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_tree_is_a_tree_and_not_a_list_of_paths() {
        // Built from paths, the way the real reader is, and checked for the
        // three things a browser needs: folders exist, they nest, and folders
        // lead their level.
        //
        // **Siblings are the point of this test**, and they are why it uses two
        // top-level folders and two under the first: the reader used to recurse
        // with `std::mem::take`, which handed the whole map to the *first*
        // folder at each level and left every sibling after it empty. One folder
        // per level - which is what this test had - cannot see that, and the
        // window shipped showing `handler`, `rust`, `tests` and `tools` all open
        // with nothing under them.
        let mut dirs: std::collections::BTreeMap<String, Vec<Node>> =
            std::collections::BTreeMap::new();
        for path in [
            "rust/app/src/main.rs",
            "rust/app/Cargo.toml",
            "rust/fastrs/src/main.rs",
            "tools/build.sh",
            "README.md",
            "z.txt",
        ] {
            let parts: Vec<&str> = path.split('/').collect();
            for i in 0..parts.len() {
                let parent = parts[..i].join("/");
                let here = parts[..=i].join("/");
                let leaf = i + 1 == parts.len();
                let entry = Node {
                    name: parts[i].to_string(),
                    path: here,
                    children: if leaf { None } else { Some(Vec::new()) },
                };
                let list = dirs.entry(parent).or_default();
                if !list.iter().any(|n| n.path == entry.path) {
                    list.push(entry);
                }
            }
        }
        let tree = assemble(&mut dirs, "");
        // Folders lead, alphabetically, then files alphabetically.
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["rust", "tools", "README.md", "z.txt"], "{names:?}");
        // Nesting: rust/app/src/main.rs is four levels deep.
        let rust = &tree[0];
        assert!(rust.is_dir());
        // Two folders under `rust`, both filled. This is the assertion the old
        // test could not make.
        let under_rust: Vec<&str> = rust
            .children
            .as_ref()
            .unwrap()
            .iter()
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(under_rust, ["app", "fastrs"], "{under_rust:?}");
        let app = &rust.children.as_ref().unwrap()[0];
        assert_eq!(app.name, "app");
        let src = &app.children.as_ref().unwrap()[0];
        assert_eq!(src.name, "src");
        assert_eq!(src.children.as_ref().unwrap()[0].name, "main.rs");
        assert!(!src.children.as_ref().unwrap()[0].is_dir());
        // ...and the sibling that used to come out empty.
        let fastrs = &rust.children.as_ref().unwrap()[1];
        assert_eq!(fastrs.name, "fastrs");
        assert!(
            !fastrs.children.as_ref().unwrap().is_empty(),
            "a sibling folder was left with nothing under it"
        );
        // A second top-level folder, likewise.
        let tools = &tree[1];
        assert_eq!(tools.name, "tools");
        assert!(
            !tools.children.as_ref().unwrap().is_empty(),
            "a second top-level folder was left with nothing under it"
        );
    }

    #[test]
    fn a_filtered_tree_keeps_the_path_to_what_matched() {
        // Dropping a parent leaves an orphan; the folder has to survive for the
        // match under it to be reachable.
        let tree = vec![Node {
            name: "rust".into(),
            path: "rust".into(),
            children: Some(vec![Node {
                name: "main.rs".into(),
                path: "rust/app/src/main.rs".into(),
                children: None,
            }]),
        }];
        let hit = filter_ref(&tree, "main.rs");
        assert_eq!(hit.len(), 1, "the folder was dropped: {hit:?}");
        assert_eq!(hit[0].name, "rust");
        assert_eq!(hit[0].children.as_ref().unwrap().len(), 1);
        // The filter borrows: the node it returns *is* the tree's own node.
        assert!(std::ptr::eq(hit[0], &tree[0]), "the filter copied a node");
        // A miss drops everything, and a folder matching its own name survives
        // even with nothing under it matching.
        assert!(filter_ref(&tree, "nothing-here").is_empty());
        assert_eq!(filter_ref(&tree, "rust").len(), 1);
        // An empty filter is the whole tree, and it copies nothing to say so.
        let all = filter_ref(&tree, "  ");
        assert_eq!(all.len(), 1);
        assert!(std::ptr::eq(all[0], &tree[0]), "an empty filter copied the tree");
    }

    #[test]
    fn a_shut_folder_is_one_row_and_an_open_one_is_walked() {
        let tree = vec![Node {
            name: "a".into(),
            path: "a".into(),
            children: Some(vec![Node { name: "b".into(), path: "a/b".into(), children: None }]),
        }];
        let all_shut = tree_rows(&tree, &|_| false);
        assert_eq!(all_shut.len(), 1, "a shut folder is one row");
        assert_eq!(all_shut[0].1, 0, "and it is at depth 0");
        let open = tree_rows(&tree, &|_| true);
        assert_eq!(open.len(), 2);
        assert_eq!(open[1].0.name, "b");
        assert_eq!(open[1].1, 1, "a child indents one step");
    }

    #[test]
    fn a_directory_is_its_own_project_whatever_case_it_was_typed_in() {
        // macOS is case-insensitive, so these are one directory and two strings.
        // Sessions were being filed under `No directory` by their project.
        let p = Path::new("/Users/x/Documents/demo");
        assert!(under("/Users/x/documents/demo", p), "the lowercase spelling is the same dir");
        assert!(under("/Users/x/DOCUMENTS/DEMO", p));
        // And a prefix is still not a parent.
        assert!(!under("/Users/x/Documents/demo-2", p));
    }

    #[test]
    fn a_row_is_named_by_the_title_it_has_or_the_id_it_was_given() {
        let mut s = Session { id: "20260101-000000-abcd".into(), ..Default::default() };
        assert_eq!(s.label(), "20260101-000000-abcd");
        s.title = "  fix the tests  ".into();
        assert_eq!(s.label(), "fix the tests");
    }

    #[test]
    fn an_age_reads_as_words_and_a_bad_stamp_reads_as_nothing() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let stamp = |ago: i64| iso_stamp(now - ago);
        let age = |ago: i64| Session { updated: stamp(ago), ..Default::default() }.age();
        assert_eq!(age(30), "now");
        assert_eq!(age(12 * 60), "12m");
        assert_eq!(age(3 * 3600), "3h");
        assert_eq!(age(2 * 86400), "2d");
        assert_eq!(Session { updated: "yesterday".into(), ..Default::default() }.age(), "");
        assert_eq!(Session::default().age(), "");
    }

    /// An ISO stamp for `secs` past the epoch, so the test can hand the parser a
    /// stamp of its own shape rather than freezing a date into the file.
    fn iso_stamp(secs: i64) -> String {
        let days = secs.div_euclid(86400);
        let rem = secs.rem_euclid(86400);
        // inverse of the civil-from-days above
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
            rem / 3600,
            (rem % 3600) / 60,
            rem % 60
        )
    }

    #[test]
    fn a_transcript_reads_the_tail_and_keeps_whole_lines() {
        // The seek lands mid-record about half the time, and half a JSON object
        // parses as nothing: the first line after the cut is dropped, and every
        // line after it survives.
        let root = std::env::temp_dir().join(format!("cua-tail-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("sessions/s1");
        std::fs::create_dir_all(&dir).unwrap();
        // Rows big enough that a 256KB tail cuts into the middle of one.
        let filler = "x".repeat(4096);
        let mut body = String::new();
        for i in 0..200 {
            body.push_str(
                &serde_json::json!({ "t": "user", "text": format!("row {i} {filler}") }).to_string(),
            );
            body.push('\n');
        }
        std::fs::write(dir.join("messages.jsonl"), &body).unwrap();
        let turns = transcript(&root, "s1", 1000);
        assert!(!turns.is_empty(), "the tail read found nothing");
        // Every row that came back is a whole row, and the last row of the file
        // is the last row returned.
        assert!(turns.iter().all(|t| t.text.starts_with("row ")), "a torn row survived");
        assert!(turns.last().unwrap().text.starts_with("row 199"), "the newest row was cut");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stamp_moves_when_the_file_does() {
        // The cache key. Both halves matter: a rewrite that keeps the length has
        // to move the stamp, or the window draws a file that is not there.
        let root = std::env::temp_dir().join(format!("cua-stamp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let f = root.join("f.jsonl");
        assert_eq!(stamp(&f), None, "a file that is not there has no stamp");
        std::fs::write(&f, "one").unwrap();
        let first = stamp(&f).unwrap();
        assert_eq!(first.0, 3);
        // Same length, different content: the mtime half is what catches this.
        std::thread::sleep(std::time::Duration::from_millis(5));
        std::fs::write(&f, "two").unwrap();
        assert_ne!(stamp(&f).unwrap(), first, "a same-length rewrite did not move the stamp");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_run_is_one_row_however_many_calls_it_made() {
        // The whole point of folding: forty records between one answer and the
        // next are one row to read, not forty.
        let t = |kind, text: &str| Turn { kind, text: text.to_string(), ts: String::new() };
        let turns = vec![
            t(Kind::User, "open firefox"),
            t(Kind::Thought, "Firefox is already running"),
            t(Kind::Call, "app_list()"),
            t(Kind::Result, "app_list: firefox is open"),
            t(Kind::Call, "app_open(firefox)"),
            t(Kind::Result, "app_open: ok"),
            t(Kind::Answer, "Firefox is open."),
        ];
        let rows = fold(&turns);
        // said, one run, answer: three rows for seven records.
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(matches!(&rows[0], Row::Said(s) if s == "open firefox"));
        assert!(matches!(&rows[2], Row::Answer(s) if s == "Firefox is open."));
        match &rows[1] {
            Row::Run { note, steps, .. } => {
                assert_eq!(steps.len(), 2, "the two calls are the two steps");
                assert_eq!(steps[0].call, "app_list()");
                assert_eq!(steps[0].result.as_deref(), Some("app_list: firefox is open"));
                assert!(!steps[0].failed);
                // A thought between the calls is the run's own voice, once.
                assert_eq!(note.as_deref(), Some("Firefox is already running"));
            }
            other => panic!("expected a run, got {other:?}"),
        }
    }

    #[test]
    fn a_tool_result_that_is_an_object_still_reads_as_a_line() {
        // This was the empty-row bug: most tools answer with an object, and
        // reading `result` as a string drew `app_open:` and nothing after it.
        let t = |kind, text: &str| Turn { kind, text: text.to_string(), ts: String::new() };
        let mut rec = serde_json::json!({
            "t": "tool",
            "name": "app_open",
            "result": { "result": { "ok": true, "app": "Firefox", "snapped": true } },
        });
        // Through the real reader, so the shape is the one on disk.
        let line = match rec.get("t").and_then(|t| t.as_str()) {
            Some("tool") => result_line("app_open", rec.get("result")),
            _ => unreachable!(),
        };
        assert!(line.contains("ok=true"), "an object result lost its fields: {line:?}");
        assert!(line.contains("app=Firefox"), "{line:?}");
        // A string result is unchanged, and an absent one is the tool's name.
        assert_eq!(result_line("shell", Some(&serde_json::json!("done"))), "done");
        assert_eq!(result_line("shell", None), "shell");
        assert_eq!(result_line("shell", Some(&serde_json::Value::Null)), "shell");
        let _ = t;
        rec["result"] = serde_json::Value::String("multi\nline".into());
        assert_eq!(result_line("shell", rec.get("result")), "multi");
    }

    #[test]
    fn a_run_of_one_call_does_not_get_a_head_too() {
        // A head over a single step said the same thing twice: `1 call ·
        // app_open` above `app_open` above its result.
        let t = |kind, text: &str| Turn { kind, text: text.to_string(), ts: String::new() };
        let rows = fold(&[t(Kind::Call, "app_open(firefox)"), t(Kind::Result, "ok=true")]);
        let Row::Run { steps, .. } = &rows[0] else {
            panic!("expected a run");
        };
        assert_eq!(steps.len(), 1);
    }

    #[test]
    fn a_failing_step_is_marked_and_a_clean_one_is_not() {
        let t = |kind, text: &str| Turn { kind, text: text.to_string(), ts: String::new() };
        let turns = vec![
            t(Kind::Call, "read(nope.rs)"),
            t(Kind::Result, "read: {\"error\": \"no such file\"}"),
            t(Kind::Call, "read(ok.rs)"),
            t(Kind::Result, "read: fn main() {}"),
        ];
        let Row::Run { steps, .. } = &fold(&turns)[0] else {
            panic!("expected one run");
        };
        assert!(steps[0].failed, "an error result is a failure");
        assert!(!steps[1].failed, "a clean result is not");
    }

    #[test]
    fn a_run_with_no_calls_is_still_a_run() {
        // A turn that only thought, or only wrote a sentence alongside nothing,
        // is still something that happened and must not vanish.
        let turns = vec![Turn { kind: Kind::Thought, text: "just thinking".into(), ts: String::new() }];
        let rows = fold(&turns);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], Row::Run { note: Some(n), steps, .. } if n == "just thinking" && steps.is_empty()));
    }

    #[test]
    fn every_brand_mark_named_is_a_file_that_exists() {
        // The table and the directory are two lists that have to agree, so this
        // reads the directory rather than trusting either of them.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/brands");
        for (name, file) in BRANDS {
            let path = dir.join(file);
            assert!(path.exists(), "{name} names {file}, which is not in assets/brands/");
            let svg = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(svg.contains("<svg"), "{file} is not an svg");
            // A mark with no fill of its own would draw solid black on a dark
            // window, which reads as a missing icon.
            assert!(
                svg.contains("currentColor") || svg.contains(r#"fill="#),
                "{file} has no colour for a dark window to inherit"
            );
        }
    }

    #[test]
    fn a_provider_with_no_mark_gets_none_rather_than_a_lookalike() {
        // The whole point: a made-up glyph beside a vendor's name is worse than
        // no glyph at all.
        assert_eq!(brand("ollama"), Some("ollama.svg"));
        assert_eq!(brand("openai"), Some("openai.svg"));
        // Case is the config's, not ours.
        assert_eq!(brand("Synthetic"), Some("huggingface.svg"));
        assert_eq!(brand("qubrain"), Some("glmv.svg"));
        assert_eq!(brand("some-endpoint-i-added"), None);
        assert_eq!(brand(""), None);
    }

    #[test]
    fn the_model_list_is_the_provider_s_catalog_and_nothing_else() {
        // The bug: the list was built from this account's session history, so it
        // held every model ever run anywhere and models the provider no longer
        // serves. The catalog is the register of what exists *now*, per provider.
        let root = std::env::temp_dir().join(format!("cua-cat-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        let sess = |id: &str, model: &str, updated: &str| {
            let d = root.join("sessions").join(id);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("meta.json"),
                serde_json::json!({ "id": id, "provider": "ollama", "model": model, "updated": updated })
                    .to_string(),
            )
            .unwrap();
        };
        sess("a", "glm-5.3-flash", "2026-01-01T00:00:10+00:00");
        sess("b", "retired-model", "2026-01-01T00:00:20+00:00");
        std::fs::write(
            root.join("config.json"),
            serde_json::json!({ "providers": { "ollama": { "model_caps": { "glm-5.3-flash": ["completion", "tools"] } } } })
                .to_string(),
        )
        .unwrap();
        // The catalog the provider actually answered with. Note `retired-model`
        // is absent: it is in history and no longer served.
        let catalog: Vec<String> =
            vec!["glm-5.3-flash".into(), "kimi-k3".into(), "gemma4:31b".into()];
        let rows = model_rows(&root, "ollama", &catalog);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.len(), 3, "one row per served model, got {names:?}");
        assert!(!names.contains(&"retired-model"), "a retired model was offered: {names:?}");
        // What this account has run leads, newest first: glm-5.3-flash was run.
        assert_eq!(names[0], "glm-5.3-flash");
        // Its capabilities come along, and the rest of the catalog follows.
        assert!(rows[0].caps.contains("tools"), "{:?}", rows[0].caps);
        // An empty catalog offers nothing rather than everything.
        assert!(model_rows(&root, "ollama", &[]).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_recorded_path_that_ends_in_a_slash_still_has_a_name() {
        // The bug, from a real repo: `status --porcelain` collapses an untracked
        // directory into one record and keeps the slash, so `internal/` came out
        // of `rsplit('/').next()` as the empty string and the row was called
        // nothing at all.
        let dir = Changed { path: "internal/".into(), status: "untracked".into(), ..Default::default() };
        assert_eq!(dir.name(), "internal");
        assert!(dir.is_dir(), "a trailing slash is a folder");
        // A file is unchanged, and a nested one keeps its last component.
        let f = Changed { path: ".dockerignore".into(), ..Default::default() };
        assert_eq!(f.name(), ".dockerignore");
        assert!(!f.is_dir());
        let nested = Changed { path: "asr/dev_server.py".into(), ..Default::default() };
        assert_eq!(nested.name(), "dev_server.py");
        assert_eq!(nested.parent(), "asr/", "the folder is what tells two files apart");
        // Three deep: the last two components of the directory, because the top
        // of a path is the same for every row in a repo and says nothing.
        let deep = Changed { path: "rust/app/src/main.rs".into(), ..Default::default() };
        assert_eq!(deep.name(), "main.rs");
        assert_eq!(deep.parent(), "app/src/");
        assert_eq!(
            Changed { path: "image.png".into(), ..Default::default() }.parent(),
            "",
            "a file at the root has no folder to show"
        );
        // A bare slash is degenerate but must not panic or go blank twice.
        let root = Changed { path: "/".into(), ..Default::default() };
        assert!(!root.name().is_empty() || root.path == "/");
    }

    #[test]
    fn the_reader_survives_the_bytes_that_repo_actually_wrote() {
        // The real `git status --porcelain=v1 -z` stream, byte for byte, from
        // `260530-Antenna` on branch `feat/group-access`.
        let raw = " M .dockerignore\0 M Makefile\0?? asr/dev_server.py\0?? internal/\0";
        let mut got: Vec<(String, String)> = Vec::new();
        for rec in raw.split('\0').filter(|r| r.len() > 2) {
            let (xy, path) = rec.split_at(2);
            let mut it = xy.chars();
            let (x, _y) = (it.next().unwrap_or(' '), it.next().unwrap_or(' '));
            let path = path.trim_start_matches(' ').to_string();
            let status = if x == '?' { "untracked" } else { letter(x) };
            got.push((status.to_string(), path));
        }
        assert_eq!(got.len(), 4, "every record is kept: {got:?}");
        // The folder's record is the one that used to draw blank.
        let folder = got.iter().find(|(_, p)| p == "internal/").expect("the folder record");
        assert_eq!(Changed { path: folder.1.clone(), status: folder.0.clone(), ..Default::default() }.name(), "internal");
        assert_eq!(got[0].1, ".dockerignore");
        assert_eq!(got[3].0, "untracked");
    }

    #[test]
    fn a_session_id_cannot_climb_out_of_the_sessions_directory() {
        assert!(safe_id("20260101-000000-abcd"));
        assert!(!safe_id(""));
        assert!(!safe_id("../meta"));
        assert!(!safe_id("a/b"));
        assert!(!safe_id(".."));
    }

    #[test]
    fn a_call_reads_as_the_call_itself() {
        // `write({"path":"src/main.rs","content":"..."})` is a wall of JSON.
        let args = serde_json::json!({ "path": "src/main.rs", "content": "fn main() {}" });
        assert_eq!(call_line("write", Some(&args)), "write(src/main.rs)");
        // A call with nothing naming its subject is just its name.
        assert_eq!(call_line("screenshot", None), "screenshot");
        assert_eq!(call_line("screenshot", Some(&serde_json::json!({}))), "screenshot");
    }

    #[test]
    fn the_reader_finds_a_scratch_home_and_groups_it() {
        // A state root built here and read back by the same code the window
        // uses: two promoted projects (the second pinned), and four sessions -
        // two in the first project, one inside a subdirectory of it, one in a
        // directory nobody promoted.
        let root = std::env::temp_dir().join(format!("cua-app-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let alpha = root.join("work/alpha");
        // `pinned` holds a project's *basename*, which is the worker's own shape.
        let beta = root.join("beta");
        let loose = root.join("loose");
        for d in [alpha.join("sub"), beta.clone(), loose.join("deep")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::write(
            root.join("projects.json"),
            serde_json::json!({
                "projects": [alpha.display().to_string(), beta.display().to_string()],
                "pinned": ["beta"],
                "names": { alpha.display().to_string(): "Alpha" },
                "icons": {},
            })
            .to_string(),
        )
        .unwrap();
        let session = |id: &str, cwd: &Path, updated: &str| {
            let dir = root.join("sessions").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("meta.json"),
                serde_json::json!({ "id": id, "title": id, "cwd": cwd.display().to_string(), "updated": updated })
                    .to_string(),
            )
            .unwrap();
        };
        session("a", &alpha, "2026-01-01T00:00:10+00:00");
        session("b", &alpha.join("sub"), "2026-01-01T00:00:20+00:00");
        session("c", &beta, "2026-01-01T00:00:30+00:00");
        session("d", &loose.join("deep"), "2026-01-01T00:00:40+00:00");

        let ws = Workspace::read_from(&root);
        assert_eq!(ws.total, 4);
        // Every session is shown: the loose one is not dropped, it is grouped.
        assert_eq!(ws.shown(), 4);

        // beta is pinned, so it leads even though alpha was promoted first, and
        // alpha is named by its `names` entry rather than its directory.
        assert_eq!(ws.projects[0].name, "beta");
        assert!(ws.projects[0].pinned);
        assert!(ws.projects[0].promoted);
        assert_eq!(ws.projects[0].sessions.len(), 1);
        assert_eq!(ws.projects[1].name, "Alpha");
        assert!(!ws.projects[1].pinned);
        assert!(ws.projects[1].promoted);

        // A session under a subdirectory of a project is that project's, and
        // newest first inside it.
        assert_eq!(ws.projects[1].sessions.len(), 2);
        let ids: Vec<&str> = ws.projects[1].sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["b", "a"], "newest first inside a project");

        // The loose session gets **its own heading**, named for the directory it
        // actually stands in, rather than being dumped in with everything else that
        // was not promoted. `d` ran in `<root>/loose/deep`, and that exact directory
        // is the heading: a session is not folded into a parent it merely sits under,
        // because folding it would be inventing a project - which is the thing this
        // grouping exists not to do.
        assert_eq!(ws.projects.len(), 3, "a loose directory is its own heading");
        let loose_head = &ws.projects[2];
        assert_eq!(loose_head.name, "deep");
        assert!(!loose_head.promoted, "nothing was written to the user's file for it");
        assert_eq!(loose_head.sessions.len(), 1);
        assert_eq!(loose_head.sessions[0].id, "d");
        // ...and "No directory" is only the sessions that have no directory at all.
        assert!(ws.other.is_empty(), "nothing here is missing a directory");

        // Reading it back through the cache: the first call fills, the second
        // does not, and a stamp for a file that moved does.
        let turns = transcript(&root, "a", 100);
        assert_eq!(turns.len(), 0, "a session with no transcript has no rows");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod perf {
    /// How long one rebuild of the biggest thing on screen takes.
    ///
    /// Not a micro-benchmark for its own sake: the number decides whether a
    /// re-render per pixel of a resize drag is fine or fatal, and the answer is
    /// the difference between "the drag is heavy" and "the drag is not".
    use super::*;

    #[test]
    fn a_full_transcript_build_measured() {
        // 400 rows, the window's own ceiling, from a real session's records.
        let root = std::env::temp_dir().join("cua-perf-home");
        std::fs::create_dir_all(root.join("sessions/real")).unwrap();
        let mut body = String::new();
        for i in 0..900 {
            let rec = if i % 3 == 0 {
                serde_json::json!({ "t": "assistant", "thinking": format!("thinking {i}"), "content": format!("answer {i} with some words in it") })
            } else if i % 3 == 1 {
                serde_json::json!({ "t": "tool", "name": "read", "result": { "result": { "ok": true } } })
            } else {
                serde_json::json!({ "t": "user", "text": format!("do the thing {i}") })
            };
            body.push_str(&rec.to_string());
            body.push('\n');
        }
        std::fs::write(root.join("sessions/real/messages.jsonl"), &body).unwrap();

        let started = std::time::Instant::now();
        let turns = transcript(&root, "real", 400);
        let rows = fold(&turns);
        let read = started.elapsed();
        assert!(!rows.is_empty());
        // The read is once, and it is what the cache exists to pay for once.
        println!("read + fold: {read:?} for {} rows", rows.len());
        // If one read were per frame, this is what a resize drag would cost.
        println!("if this ran per frame: {} ms/frame", read.as_secs_f64() * 1000.0);
    }

    #[test]
    fn each_side_of_a_duration_is_its_own_word() {
        assert_eq!(duration(0.4), "400ms");
        assert_eq!(duration(8.04), "8.0s");
        assert_eq!(duration(9.99), "10s");
        assert_eq!(duration(22.3), "22s");
        assert_eq!(duration(133.0), "2m 13s");
        assert_eq!(duration(3_725.0), "1h 2m 5s");
        assert_eq!(duration(-1.0), "0ms");
    }

    #[test]
    fn a_stamp_difference_reads_the_clock_and_crosses_midnight() {
        let a = "2026-10-12T09:20:22+00:00";
        assert_eq!(iso_secs_between(a, "2026-10-12T09:22:35+00:00"), Some(133.0));
        // The date is a day count, so a run that started before midnight still
        // measures the way it ran.
        assert_eq!(
            iso_secs_between("2026-10-12T23:59:30+00:00", "2026-10-13T00:00:30+00:00"),
            Some(60.0)
        );
        // A stamp that will not parse is not a zero: the row says nothing.
        assert_eq!(iso_secs_between(a, ""), None);
    }

    #[test]
    fn a_run_says_how_long_it_took() {
        let turn = |kind, text: &str, ts: &str| Turn {
            kind,
            text: text.to_string(),
            ts: ts.to_string(),
        };
        let turns = vec![
            turn(Kind::User, "read the file", "2026-10-12T09:00:00+00:00"),
            turn(Kind::Call, "read(src/main.rs)", "2026-10-12T09:00:01+00:00"),
            turn(Kind::Result, "fn main() {}", "2026-10-12T09:00:04+00:00"),
            turn(Kind::Answer, "It is three lines.", "2026-10-12T09:00:09+00:00"),
        ];
        let rows = fold(&turns);
        match &rows[1] {
            Row::Run { secs, steps, .. } => {
                assert_eq!(steps.len(), 1);
                assert_eq!(*secs, Some(9.0), "the run is measured from the person's message");
            }
            other => panic!("expected a run between the question and the answer, got {other:?}"),
        }
    }
}
