//! The wire: one line-delimited JSON envelope per worker event.
//!
//! A port of the Go frontends' protocol, and deliberately two layers.
//! `Envelope` keeps the payload verbatim, so an event this code has never heard
//! of is still a valid event with everything it carried intact. `Event` is the
//! typed read of the keys a frontend acts on, and it is what the UI matches on.
//!
//! Absent counters are `Option`, where the Go used `-1`. The worker leaves a
//! number out when it has nothing to report, and to a frontend a zero reads as
//! a measurement rather than as a silence, so the absence has to survive the
//! parse as an absence.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One envelope, exactly as it crosses the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub data: Value,
}

impl Envelope {
    pub fn new(kind: impl Into<String>, id: impl Into<String>, data: Value) -> Self {
        Envelope { kind: kind.into(), id: id.into(), data }
    }

    /// A `cmd` envelope, which is what every action the frontend sends is.
    pub fn cmd(id: impl Into<String>, action: &str, fields: Value) -> Self {
        let mut data = match fields {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        data.insert("action".into(), Value::String(action.into()));
        Envelope::new("cmd", id, Value::Object(data))
    }

    /// The line to write to the worker's stdin.
    ///
    /// Cannot fail: an `Envelope` is three strings and a `Value`, and neither
    /// can hold something JSON cannot express.
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).expect("an envelope is always serializable");
        s.push('\n');
        s
    }
}

/// A streamed token: a JSON string for text, an array for tool calls.
///
/// A frontend that only wants words gets them from [`Token::text`]; one
/// drawing the calls keeps the array. Neither is dropped for being the other.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Text(String),
    Calls(Value),
}

impl Token {
    fn from_value(v: Value) -> Token {
        match v {
            Value::String(s) => Token::Text(s),
            other => Token::Calls(other),
        }
    }

    /// The text, or the JSON of whatever else it was.
    pub fn text(&self) -> String {
        match self {
            Token::Text(s) => s.clone(),
            Token::Calls(v) => v.to_string(),
        }
    }
}

/// The scalar payload keys, flattened. Everything the worker has ever sent is
/// optional, because it sends what it has and nothing else.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Payload {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub token: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub show: Option<String>,
    #[serde(default)]
    pub ts: Option<String>,
    #[serde(default)]
    pub index: Option<i64>,
    #[serde(rename = "msg_count", default)]
    pub msg_count: Option<i64>,
    #[serde(rename = "context_left", default)]
    pub context_left: Option<i64>,
    #[serde(rename = "context_used", default)]
    pub context_used: Option<i64>,
    #[serde(rename = "context_max", default)]
    pub context_max: Option<i64>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(rename = "out_tokens", default)]
    pub out_tokens: Option<i64>,
    #[serde(rename = "thinking_tokens", default)]
    pub think_tokens: Option<i64>,
    #[serde(rename = "reply_tokens", default)]
    pub reply_tokens: Option<i64>,
    #[serde(rename = "thinking_est", default)]
    pub think_est: Option<bool>,
    #[serde(default)]
    pub tps: Option<f64>,
    #[serde(rename = "think_tps", default)]
    pub think_tps: Option<f64>,
    #[serde(rename = "reply_tps", default)]
    pub reply_tps: Option<f64>,
    #[serde(rename = "tps_est", default)]
    pub tps_est: Option<bool>,
    #[serde(default)]
    pub images: Vec<ImageRef>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(rename = "session_id", default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Who a `peer_waiting` says is waiting. Absent when the worker carried no
    /// name, which stays absent rather than becoming an empty one here.
    #[serde(rename = "peer_name", default)]
    pub peer_name: Option<String>,
    /// The first words of the thought a `step_name` names.
    ///
    /// A block id is the frontend's own counter, so the worker cannot address a
    /// row with one. What both ends hold is the thought itself, and this is the
    /// handle they meet on: the row whose text starts with these words is the step.
    #[serde(default)]
    pub of: Option<String>,
    /// A fold in the conversation: "start" while the summarizer runs, "done" when
    /// it lands. `folded` and `freed` are what the status line can say about it.
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub folded: Option<i64>,
    #[serde(default)]
    pub freed: Option<i64>,
    #[serde(default)]
    pub result: Option<Value>,
}

/// A name for an attachment. The payloads never come back on a replay: a
/// frontend redrawing a conversation wants to say a picture was there, not to
/// be handed megabytes of it again.
#[derive(Debug, Clone, Deserialize)]
pub struct ImageRef {
    #[serde(default)]
    pub name: Option<String>,
}

/// One worker line, both ways round.
#[derive(Debug, Clone)]
pub struct Event {
    pub env: Envelope,
    pub payload: Payload,
    /// The original line, for a frontend that wants to keep it.
    pub raw: String,
}

impl Event {
    /// The `state` key: what a `token` event *is* -- `user`, `thinking`,
    /// `content`, `tool_calls`, `tool_output`, `done` -- and what a `status`
    /// event is a report of (`ready`, `usage`, `rate`, `retry`, `error`).
    pub fn state(&self) -> &str {
        self.payload.state.as_deref().unwrap_or("")
    }

    /// The `status` key, which is the coarse run state (`running`, `tooling`,
    /// `done`) and travels on the same envelopes as `state`.
    ///
    /// Kept apart from [`Event::state`] on purpose: a streamed row carries both
    /// (`state: tool_output`, `status: tooling`), and only one of them is what
    /// the row is. Folding them into one accessor makes every row read as
    /// `running`.
    pub fn status(&self) -> &str {
        self.payload.status.as_deref().unwrap_or("")
    }

    pub fn token(&self) -> Option<Token> {
        self.payload.token.clone().map(Token::from_value)
    }

    pub fn token_text(&self) -> String {
        self.token().map(|t| t.text()).unwrap_or_default()
    }

    /// A `Value` read as a typed struct, for the payload shapes that are an
    /// object rather than a scalar (`sessions`, `providers`, a job).
    pub fn data_as<T: for<'de> Deserialize<'de>>(&self) -> Option<T> {
        serde_json::from_value(self.env.data.clone()).ok()
    }
}

/// What the worker does with `status: tooling`: the same coarse state as
/// `running`, named for what the run is actually doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Running,
    Tools,
    Done,
    Error,
    Cancelled,
    /// A state this frontend has not heard of. Kept rather than dropped: the
    /// worker adding a state must not make an older frontend lie about it.
    Other(String),
}

impl State {
    pub fn as_str(&self) -> &str {
        match self {
            State::Idle => "idle",
            State::Running => "running",
            State::Tools => "tools",
            State::Done => "done",
            State::Error => "error",
            State::Cancelled => "cancelled",
            State::Other(s) => s,
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self, State::Running | State::Tools)
    }
}

impl Default for State {
    fn default() -> Self {
        State::Idle
    }
}

impl From<&str> for State {
    fn from(s: &str) -> Self {
        match s {
            "idle" => State::Idle,
            "running" => State::Running,
            "tools" | "tooling" => State::Tools,
            "done" => State::Done,
            "error" => State::Error,
            "cancelled" => State::Cancelled,
            "" => State::Idle,
            other => State::Other(other.into()),
        }
    }
}

/// Decode one stdout line. `Err` means the line was not JSON at all, which a
/// frontend should show rather than swallow.
pub fn parse(raw: &str) -> Result<Event, serde_json::Error> {
    let env: Envelope = serde_json::from_str(raw)?;
    // Payload keys are best-effort: an envelope whose data is not an object,
    // or has a differently-typed field, is still a valid event.
    let payload = serde_json::from_value::<Payload>(env.data.clone()).unwrap_or_default();
    Ok(Event { env, payload, raw: raw.to_string() })
}

/// The line a frontend synthesizes when the worker dies, so a UI that already
/// draws worker errors draws this one too instead of sitting there forever.
pub fn worker_exit(message: &str) -> Event {
    parse(&format!(
        r#"{{"type":"status","id":"worker-exit","data":{{"state":"error","error":{}}}}}"#,
        Value::String(message.to_string())
    ))
    .expect("a synthesized envelope is valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_streamed_text_token_is_text() {
        let ev = parse(r#"{"type":"token","id":"a","data":{"state":"content","token":"hi"}}"#).unwrap();
        assert_eq!(ev.state(), "content");
        assert_eq!(ev.token(), Some(Token::Text("hi".into())));
        assert_eq!(ev.token_text(), "hi");
    }

    #[test]
    fn a_tool_calls_token_is_kept_as_calls() {
        let ev = parse(r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell"}]}}"#)
            .unwrap();
        assert_eq!(ev.state(), "tool_calls");
        assert!(matches!(ev.token(), Some(Token::Calls(_))));
        assert!(ev.token_text().contains("shell"));
    }

    #[test]
    fn an_absent_counter_stays_absent() {
        let ev = parse(r#"{"type":"status","id":"a","data":{"state":"usage","out_tokens":12}}"#).unwrap();
        assert_eq!(ev.payload.out_tokens, Some(12));
        assert_eq!(ev.payload.think_tokens, None);
        assert_eq!(ev.payload.context_max, None);
    }

    #[test]
    fn a_payload_that_is_not_an_object_is_still_an_event() {
        let ev = parse(r#"{"type":"status","id":"a","data":"nonsense"}"#).unwrap();
        assert_eq!(ev.env.kind, "status");
        assert_eq!(ev.state(), "");
    }

    #[test]
    fn an_unknown_state_is_carried_not_dropped() {
        let ev = parse(r#"{"type":"token","id":"a","data":{"state":"teleporting"}}"#).unwrap();
        assert_eq!(State::from(ev.state()), State::Other("teleporting".into()));
        assert_eq!(State::from("tooling"), State::Tools);
    }

    #[test]
    fn a_waiting_peer_carries_the_name_the_worker_put_on_it() {
        let ev = parse(
            r#"{"type":"token","id":"a","data":{"state":"peer_waiting","peer_name":"work: clearing rust code","status":"tooling"}}"#,
        )
        .unwrap();
        assert_eq!(ev.payload.peer_name.as_deref(), Some("work: clearing rust code"));
        // `peer_waiting` is a state this frontend acts on by name rather than a state
        // of the run, so it stays what the wire called it and the coarse state keeps
        // coming off `status`.
        assert_eq!(State::from(ev.state()), State::Other("peer_waiting".into()));
        assert_eq!(State::from(ev.status()).busy(), true);
        // A knock with no name on it is still a knock, and the absence has to survive
        // the parse: the status line then says a peer is waiting without inventing one.
        let ev = parse(r#"{"type":"token","id":"a","data":{"state":"peer_waiting","status":"tooling"}}"#)
            .unwrap();
        assert_eq!(ev.payload.peer_name, None);
    }

    #[test]
    fn state_and_status_are_different_keys() {
        let ev = parse(r#"{"type":"token","id":"a","data":{"state":"tool_output","status":"tooling"}}"#)
            .unwrap();
        assert_eq!(ev.state(), "tool_output");
        assert_eq!(ev.status(), "tooling");
    }

    #[test]
    fn a_non_json_line_is_an_error() {
        assert!(parse("Traceback (most recent call last):").is_err());
    }
}
