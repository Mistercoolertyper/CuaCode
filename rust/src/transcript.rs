//! The conversation as rows, folded from the worker's events.
//!
//! Kept out of the drawing on purpose. The model can then be built and asserted
//! in a test with no terminal, no worker and no model call -- which is what the
//! fixture is for -- and a GUI frontend wanting the same shape does not have to
//! re-derive it from the wire.

use serde_json::Value;

use crate::wire::Event;

/// One thing in the transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// What the user said. `images` are names only: the payloads never come
    /// back on a replay, by design.
    User { text: String, images: Vec<String> },
    /// Runtime text the worker put in front of the model: recall pointers,
    /// project documentation, a finished background job, a skill body.
    /// Runtime text put in front of the model. A tagged block -- recall
    /// pointers, project docs, a skill body, the interrupt note -- keeps only
    /// its head line: the payload is for the model, not for the reader.
    Notice { head: String, hidden: usize },
    /// Two agents talking: a message that arrived from a peer, or one that went out
    /// to one. Its own row because neither half is the person's: a peer's words drawn
    /// as the user's line would be a lie, and folded into a notice they would read as
    /// another note about this machine.
    Peer { head: String, text: String },
    /// Streamed text, and which assistant message it belongs to. The block is
    /// what stops a later answer being appended to an earlier one: two
    /// different blocks are two different rows, however little separates them
    /// on screen.
    Thinking { text: String, block: u64, name: Option<String> },
    Content { text: String, block: u64 },
    /// A call, and the result that came back against it. `result` is a summary,
    /// never a body: a shell result is kilobytes and a row is a line.
    ///
    /// `args` is a summary too, one key of it, and `call` is the arguments exactly
    /// as the wire sent them. Both are here on purpose: a frontend that draws a
    /// line wants the summary, and one that draws a call shape -- `write(path)` --
    /// or opens the call in full needs what the summary clipped away.
    Tool {
        name: String,
        args: String,
        call: Value,
        result: Option<String>,
        index: Option<i64>,
    },
    Error(String),
}

impl Row {
    /// A notice, collapsed to the line that says what it is.
    ///
    /// The worker prefixes a user turn with runtime text that is addressed to
    /// the model: recall pointers, project documentation, a skill body, the
    /// note left by an interrupt. All of it arrives as one blob wrapped in a
    /// `<tag>` block, and a chat log that prints the blob whole is a chat log
    /// nobody can read. The head line names the thing; a count says how much is
    /// behind it. The text itself is on the session's records, so nothing is
    /// lost by not drawing it.
    fn notice(text: String) -> Row {
        let tagged = text.lines().any(|l| {
            let t = l.trim();
            t.starts_with('<') && t.ends_with('>') && t.len() > 2
        });
        let mut lines = text.lines();
        let head = lines.next().unwrap_or("").trim().to_string();
        let rest = lines.filter(|l| !l.trim().is_empty()).count();
        if tagged && rest > 0 {
            Row::Notice { head, hidden: rest }
        } else {
            Row::Notice { head: text.trim().to_string(), hidden: 0 }
        }
    }

    /// A peer row, from the head line the worker sent and whatever it hid.
    ///
    /// The head is the whole point of it -- "agent x to y" -- so it is taken as
    /// written rather than re-derived here, and the body is whatever the worker put
    /// after the first newline.
    fn peer(text: String) -> Row {
        let mut lines = text.lines();
        let head = lines.next().unwrap_or("").trim().to_string();
        let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
        Row::Peer { head, text: body }
    }
}

#[derive(Debug, Default)]
pub struct Transcript {
    pub rows: Vec<Row>,
    /// The assistant message the next streamed text belongs to. Bumped when the
    /// model is asked again -- by a tool call, or by a turn ending -- so an
    /// answer can never be glued onto the one before it.
    block: u64,
    /// The row that message is still being written into. Its own identity rather
    /// than "the last row", because rows arrive under it while it streams: a line
    /// typed mid-turn, a peer's message, a notice. Text that is still arriving
    /// belongs to the row it started in, wherever that row now sits.
    open: Option<usize>,
}

impl Transcript {
    /// Fold one event in. Returns whether it drew anything.
    pub fn apply(&mut self, ev: &Event) -> bool {
        match (ev.env.kind.as_str(), ev.state()) {
            ("token", "user") => {
                let images = ev.payload.images.iter().filter_map(|i| i.name.clone()).collect();
                self.next_block();
                self.rows.push(Row::User { text: ev.token_text(), images });
                true
            }
            ("token", "notice") => {
                self.rows.push(Row::notice(ev.token_text()));
                true
            }
            ("token", "peer") => {
                self.rows.push(Row::peer(ev.token_text()));
                true
            }
            ("token", "thinking") => {
                self.push_text(ev.token_text(), true);
                true
            }
            ("token", "content") => {
                self.push_text(ev.token_text(), false);
                true
            }
            ("token", "tool_calls") => {
                // A new assistant message: text after this belongs to a
                // different block, whatever sits between them on screen.
                self.next_block();
                self.calls(ev.token_text());
                true
            }
            // A turn that ended is the seam the next answer cannot cross. The
            // user's own line used to be it, and that cut a thought in half the
            // moment it was typed mid-turn.
            ("token", "done" | "cancelled") => {
                self.next_block();
                false
            }
            ("token", "tool_output") => {
                self.result(ev);
                true
            }
            // A name for a step, from the worker's small model. It arrives after
            // the step it names, which is why it carries the thought's opening
            // words rather than an id: the row is found by the words.
            ("token", "step_name") => {
                self.name_step(ev);
                true
            }
            ("token", "error") | ("status", "error") => {
                let text = ev.payload.error.clone().unwrap_or_else(|| ev.token_text());
                self.next_block();
                self.rows.push(Row::Error(text));
                true
            }
            _ => false,
        }
    }

    /// Streamed text lands in the row it belongs to, extending it while that
    /// row is still the one being written and starting a new one when the message
    /// changed under it. A round that thinks, answers, thinks again is two thinking
    /// rows, not one.
    fn push_text(&mut self, text: String, thinking: bool) {
        if text.is_empty() {
            return;
        }
        if let Some(t) = self.stream(thinking) {
            t.push_str(&text);
            return;
        }
        let block = self.block;
        self.rows.push(if thinking {
            Row::Thinking { text, block, name: None }
        } else {
            Row::Content { text, block }
        });
        self.open = Some(self.rows.len() - 1);
    }

    /// The row being written, if this token belongs to it: same kind, same message.
    fn stream(&mut self, thinking: bool) -> Option<&mut String> {
        let block = self.block;
        match self.rows.get_mut(self.open?)? {
            Row::Thinking { text, block: b, .. } if thinking && *b == block => Some(text),
            Row::Content { text, block: b } if !thinking && *b == block => Some(text),
            _ => None,
        }
    }

    /// Close the message being written and start the next one: whatever text
    /// comes now is a different answer, however little separates the two on screen.
    fn next_block(&mut self) {
        self.block += 1;
        self.open = None;
    }

    /// Put the worker's name on the step it names.
    ///
    /// The newest thought whose text starts with the words the event carries. A
    /// name that finds no row is dropped: the row is what it was for, and naming
    /// the wrong step is worse than leaving one unnamed.
    fn name_step(&mut self, ev: &Event) {
        let name = ev.token_text().trim().to_string();
        let of = ev.payload.of.clone().unwrap_or_default();
        if name.is_empty() {
            return;
        }
        let at = self.rows.iter().rposition(|row| match row {
            Row::Thinking { text, .. } => {
                of.trim().is_empty() || text.trim_start().starts_with(of.trim())
            }
            _ => false,
        });
        if let Some(i) = at {
            if let Row::Thinking { name: slot, .. } = &mut self.rows[i] {
                *slot = Some(name);
            }
        }
    }

    /// The row the worker is writing into right now, for a frontend drawing the
    /// live dot. Not the newest row: a line typed mid-turn sits below it.
    pub fn open_row(&self) -> Option<usize> {
        self.open
    }

    /// The user's own line, drawn by the frontend that sent it.
    ///
    /// The worker echoes a user turn when it *replays* a session and never when
    /// one is sent: a live turn's only answer to a chat is a `chat_received`
    /// acknowledgement that carries no text. So the line is the frontend's to
    /// draw, at the moment it is typed, or it does not appear at all.
    ///
    /// `images` is what the row says was attached to it: the names, never the
    /// payloads, because a replay has no pictures to draw and a row that carried
    /// megabytes to say "a picture was here" would be the wrong shape of thing.
    ///
    /// It does not close the message being written. A line typed while a turn is
    /// streaming is a message *to* that turn, not the end of it: bumping here
    /// stranded the thought in flight, which could no longer be extended and was
    /// redrawn as finished with a second one starting under it. The seam that
    /// stops two answers merging is the turn ending (`done`), and a replayed user
    /// turn still closes the message before it.
    pub fn say(&mut self, text: &str, images: Vec<String>) {
        self.rows.push(Row::User { text: text.to_string(), images });
    }

    /// The tool_calls token is the one payload that is not a string, and its
    /// shape is the provider's rather than the worker's: the canonical
    /// `{name, args}`, openai's and ollama's `{function: {name, arguments}}`,
    /// anthropic's `{type: tool_use, name, input}` block. All three reach a
    /// frontend, because the worker rebuilds a replayed round in whichever
    /// dialect recorded it.
    fn calls(&mut self, raw: String) {
        let calls = serde_json::from_str::<Vec<Value>>(&raw).unwrap_or_default();
        if calls.is_empty() {
            if !raw.trim().is_empty() {
                self.rows.push(Row::Tool {
                    name: "?".into(),
                    args: raw,
                    call: Value::Null,
                    result: None,
                    index: None,
                });
            }
            return;
        }
        for call in calls {
            let parsed = call_args(&call);
            let args = parsed.as_ref().map(describe_args).unwrap_or_default();
            self.rows.push(Row::Tool {
                name: call_name(&call),
                args,
                call: parsed.unwrap_or(Value::Null),
                result: None,
                index: None,
            });
        }
    }

    /// The result lands on the call it answers: the oldest unanswered one of
    /// the same name, which is what makes a replayed round read in the order it
    /// happened rather than as a pile of results at the end.
    fn result(&mut self, ev: &Event) {
        let name = ev.token_text();
        let index = ev.payload.index;
        let summary = ev.payload.result.as_ref().map(brief).unwrap_or_default();
        // Two passes, not a chained `or_else`: the second closure borrows the
        // rows again, and the first borrow would still be alive.
        let slot = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Tool { name: n, result: None, .. } if *n == name))
            .or_else(|| {
                self.rows
                    .iter()
                    .position(|row| matches!(row, Row::Tool { result: None, .. }))
            });
        match slot {
            Some(i) => {
                if let Row::Tool { result, index: slot_index, .. } = &mut self.rows[i] {
                    *result = Some(summary);
                    *slot_index = index;
                }
            }
            None => self.rows.push(Row::Tool {
                name,
                args: String::new(),
                call: Value::Null,
                result: Some(summary),
                index,
            }),
        }
    }

    pub fn last_result_index(&self) -> Option<i64> {
        self.rows.iter().rev().find_map(|r| match r {
            Row::Tool { index, .. } => *index,
            _ => None,
        })
    }
}

/// A call's name, whichever dialect spelled it.
fn call_name(call: &Value) -> String {
    if let Some(n) = call.get("name").and_then(Value::as_str) {
        return n.to_string();
    }
    call.get("function").and_then(|f| f.get("name")).and_then(Value::as_str).unwrap_or("?").to_string()
}

/// A call's arguments. openai and ollama hand them over as a JSON string
/// inside the call, which is the one shape that has to be parsed again.
fn call_args(call: &Value) -> Option<Value> {
    if let Some(v) = call.get("args").or_else(|| call.get("input")) {
        return Some(v.clone());
    }
    match call.get("function")?.get("arguments")? {
        Value::String(s) => Some(serde_json::from_str(s).unwrap_or(Value::String(s.clone()))),
        v => Some(v.clone()),
    }
}

/// The one argument that says what a call *was*, in a few words.
///
/// A row is a line, and every argument belongs to the reader a keystroke away
/// in `tool.detail`. Listing them all here turns a conversation into a request
/// log: `command=mdfind ... timeout=60` tells a person nothing that
/// `command=mdfind -name adb` does not.
pub fn describe_args(args: &Value) -> String {
    /// The keys that name the work, in the order they name it best.
    const SAYS_WHAT: [&str; 6] = ["command", "path", "name", "query", "action", "url"];
    let Value::Object(map) = args else {
        return short(args);
    };
    let pick = SAYS_WHAT
        .iter()
        .find_map(|key| map.get(*key).map(|value| (*key, value)))
        .or_else(|| map.iter().find(|(key, _)| !is_bulky(key)).map(|(key, value)| (key.as_str(), value)));
    match pick {
        Some((key, value)) => format!("{key}={}", short(value)),
        None => String::new(),
    }
}

/// A result, one line.
///
/// Every tool wraps what it returns one level down, as `{"result": {...}}`, so
/// the shape worth describing is inside that key and not the wrapper. A failing
/// command is quoted instead: its own words are the most useful thing a row can
/// carry. Bulky values are measured, never quoted.
fn brief(result: &Value) -> String {
    let Value::Object(map) = result else {
        return short(result);
    };
    if let Some(Value::String(e)) = map.get("error") {
        return format!("error: {}", truncate(&first_line(e), 70));
    }
    let inner = match map.get("result") {
        Some(Value::Object(inner)) => inner,
        _ => map,
    };
    if let Some(code) = inner.get("exit_code").and_then(Value::as_i64) {
        if code != 0 {
            if let Some(Value::String(err)) = inner.get("stderr") {
                if !err.trim().is_empty() {
                    return format!("exit {code}: {}", truncate(&first_line(err), 80));
                }
            }
            return format!("exit {code}");
        }
    }
    let mut parts = Vec::new();
    let mut skipped = 0;
    for (k, v) in inner {
        if parts.len() == 4 {
            skipped += 1;
            continue;
        }
        match phrase(k, v) {
            Some(t) => parts.push(t),
            None => skipped += 1,
        }
    }
    if skipped > 0 {
        parts.push(format!("+{skipped}"));
    }
    if parts.is_empty() {
        return format!("{{{}}}", inner.len());
    }
    truncate(&parts.join(" · "), 150)
}

/// One key of a result as a phrase. `None` means nothing worth saying.
fn phrase(key: &str, v: &Value) -> Option<String> {
    match key {
        "exit_code" => return None, // 0 is not news, and a failure is quoted above
        "duration" => return v.as_f64().map(|s| format!("{s:.1}s")),
        "images" => return v.as_array().map(|a| format!("{} image(s)", a.len())),
        _ => {}
    }
    match v {
        Value::String(s) => {
            if is_bulky(key) {
                if s.trim().is_empty() {
                    None
                } else {
                    Some(format!("{key}:{}B", s.len()))
                }
            } else {
                Some(format!("{key} {}", truncate(&first_line(s), 34)))
            }
        }
        Value::Bool(true) => Some(key.to_string()),
        Value::Bool(false) => Some(format!("{key}=false")),
        Value::Number(n) => Some(format!("{key} {n}")),
        Value::Array(a) => Some(format!("{key}:{}", a.len())),
        Value::Object(o) => Some(format!("{key}:{{{}}}", o.len())),
        Value::Null => None,
    }
}

/// Keys whose value is a document rather than a fact. Never printed, always
/// measured.
///
/// ponytail: a hand-listed set. A tool returning a new kind of body leaks a
/// line of it into a row; the fix then is another key here, not a new rule. A
/// diff is here because its first line is `--- a/name`, which is a file name and
/// not news; the diff itself is what a detail is for.
///
/// Public because the GUI's detail pane asks the same question of a result, and
/// a second copy of this list is how the two answers drift.
pub fn is_bulky(key: &str) -> bool {
    matches!(key,
        "stdout" | "stderr" | "output" | "log" | "text" | "instructions"
            | "description" | "image_base64" | "content" | "diff")
}

fn short(v: &Value) -> String {
    match v {
        Value::String(s) => truncate(&first_line(s), 40),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => format!("[{}]", a.len()),
        Value::Object(o) => format!("{{{}}}", o.len()),
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// Wrap one row to `width` columns, on words.
///
/// Here rather than in a widget, because it is what makes a scroll offset
/// exact: a wrapped row *is* a row, and the UI's offset counts rows. A word
/// longer than the row is split rather than allowed to run off the edge.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    text.split('\n').flat_map(|segment| wrap_one(segment, width)).collect()
}

fn wrap_one(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut line = String::new();
    let mut owe_space = false;
    for word in text.split(' ') {
        let extra = usize::from(owe_space);
        if !line.is_empty() && line.chars().count() + extra + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
            owe_space = false;
        }
        if owe_space && !line.is_empty() {
            line.push(' ');
        }
        for c in word.chars() {
            if line.chars().count() == width {
                out.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
        owe_space = true;
    }
    out.push(line);
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire;

    fn ev(line: &str) -> Event {
        wire::parse(line).unwrap()
    }

    /// A step name arrives after the step and is put on the thought it names. The
    /// handle is the thought's own opening words, because a block id belongs to the
    /// frontend's fold and the worker has never seen one.
    #[test]
    fn a_step_name_lands_on_the_thought_it_names() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"thinking","token":"The router needs pattern segments. Let me look at the app."}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"step_name","token":"Tracing router callers","of":"The router needs pattern"}}"#));
        match &t.rows[0] {
            Row::Thinking { name, .. } => assert_eq!(name.as_deref(), Some("Tracing router callers")),
            other => panic!("{other:?}"),
        }
        // A name that matches no row is dropped rather than hung on whichever
        // thought happens to be last: a wrong name is worse than none.
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"step_name","token":"Something else","of":"nothing said this"}}"#));
        match &t.rows[0] {
            Row::Thinking { name, .. } => assert_eq!(name.as_deref(), Some("Tracing router callers")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn streamed_text_extends_its_row_then_starts_a_new_one() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"content","token":"Hel"}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"content","token":"lo"}}"#));
        assert_eq!(t.rows, vec![Row::Content { text: "Hello".into(), block: 0 }]);
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"thinking","token":"hm"}}"#));
        assert_eq!(t.rows.len(), 2);
    }

    #[test]
    fn a_result_lands_on_the_call_it_answers() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"ls"}}]}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"pwd"}}]}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","index":0,"result":{"exit":0}}}"#));
        match &t.rows[0] {
            Row::Tool { args, result, index, .. } => {
                assert!(args.contains("command=ls"));
                assert!(result.is_some());
                assert_eq!(*index, Some(0));
            }
            other => panic!("expected a tool row, got {other:?}"),
        }
        assert!(matches!(&t.rows[1], Row::Tool { result: None, .. }));
    }

    #[test]
    fn a_bulky_result_is_measured_not_printed() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{}}]}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"exit":0,"stdout":"line\nline\nline"}}}"#));
        let Row::Tool { result: Some(r), .. } = &t.rows[0] else { panic!("no result") };
        assert!(r.contains("stdout:14B"), "{r}");
        assert!(!r.contains("line"), "the body must not reach a row: {r}");
    }

    #[test]
    fn an_error_row_carries_the_text_of_a_failed_turn() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"error","token":"boom","status":"error"}}"#));
        assert_eq!(t.rows, vec![Row::Error("boom".into())]);
    }

    #[test]
    fn a_runtime_block_collapses_to_its_head_line() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"notice","token":"recall: 3 memories outside this scope\n\n<recall-memories>\na\nb\nc\n</recall-memories>"}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"notice","token":"provider -> ollama"}}"#));
        assert_eq!(
            t.rows,
            vec![
                Row::Notice { head: "recall: 3 memories outside this scope".into(), hidden: 5 },
                Row::Notice { head: "provider -> ollama".into(), hidden: 0 },
            ]
        );
    }

    #[test]
    fn a_failing_command_is_quoted_rather_than_measured() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"adb devices"}}]}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":127,"stderr":"zsh:1: command not found: adb\n","stdout":"","duration":0.39}}}}"#));
        let Row::Tool { result: Some(r), .. } = &t.rows[0] else { panic!("no result") };
        assert_eq!(r, "exit 127: zsh:1: command not found: adb");
    }

    #[test]
    fn a_wrapped_result_describes_what_is_inside_the_wrapper() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"file","args":{}}]}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"file","result":{"result":{"created":true,"path":"/tmp/x.py","written":6542}}}}"#));
        let Row::Tool { result: Some(r), .. } = &t.rows[0] else { panic!("no result") };
        assert!(r.contains("created"), "{r}");
        assert!(r.contains("written 6542"), "{r}");
        assert!(r.contains("/tmp/x.py"), "{r}");
    }

    #[test]
    fn the_frontends_own_line_is_drawn_when_it_is_sent() {
        // A live turn is never echoed by the worker, so if the frontend does
        // not draw this, what was asked is simply absent from the screen.
        let mut t = Transcript::default();
        t.say("what is on my screen?", Vec::new());
        assert!(
            matches!(t.rows.first(), Some(Row::User { text, .. }) if text == "what is on my screen?")
        );
        // And the answer that follows is its own row, not an appendage.
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"content","token":"hi"}}"#));
        assert_eq!(t.rows.len(), 2);
        assert!(matches!(t.rows[1], Row::Content { .. }));
    }

    #[test]
    fn a_line_typed_mid_turn_does_not_cut_the_thought_in_half() {
        // The reported bug: say() bumped the block, so the next thinking token
        // could not extend the row already in flight and opened a second one
        // under the user's line -- the first stranded mid-sentence, still
        // counting tokens, and drawn as finished.
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"thinking","token":"the reason "}}"#));
        t.say("wait, check the tests too", Vec::new());
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"thinking","token":"is in the fold"}}"#));
        let thoughts: Vec<&str> = t
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Thinking { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(thoughts, vec!["the reason is in the fold"]);
        // And the dot stays on that row: it is still the one being written.
        assert_eq!(t.open_row(), Some(0));
    }

    #[test]
    fn a_turn_ending_is_what_separates_two_answers() {
        // Requirement the user's line used to carry, now carried by the seam that
        // is actually a seam: a second turn cannot be appended to the first's answer.
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"content","token":"first"}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"done","token":"done","status":"done"}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"content","token":"second"}}"#));
        let answers: Vec<&str> = t
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Content { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(answers, vec!["first", "second"]);
        assert_eq!(t.open_row(), Some(1));
    }

    #[test]
    fn a_second_answer_never_merges_into_the_first() {
        // The reported bug: a round that produced no thinking pushed its text
        // onto the previous round's, so two answers read as one paragraph.
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"content","token":"first"}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{}}]}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0}}}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"content","token":"second"}}"#));
        let answers: Vec<&str> = t
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Content { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(answers, vec!["first", "second"]);
    }

    #[test]
    fn an_answer_after_a_tool_result_is_its_own_row() {
        let mut t = Transcript::default();
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{}}]}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0}}}}"#));
        t.apply(&ev(r#"{"type":"token","id":"a","data":{"state":"content","token":"done"}}"#));
        assert!(matches!(t.rows.last(), Some(Row::Content { .. })));
    }
}
