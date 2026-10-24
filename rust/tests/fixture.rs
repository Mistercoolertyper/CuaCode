//! The fixture is real stdout: one conversation's whole replay, 624 lines,
//! recorded from the worker by `fixtures/record.py`. If these types stop
//! agreeing with the Python worker, this fails. No model call, no network, no
//! worker process.

use cuacode::wire::{self, Token};

const FIXTURE: &str = include_str!("../fixtures/replay.jsonl");

fn events() -> Vec<wire::Event> {
    FIXTURE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| wire::parse(l).expect("every fixture line is an envelope"))
        .collect()
}

fn count(evs: &[wire::Event], kind: &str, state: &str) -> usize {
    evs.iter().filter(|e| e.env.kind == kind && e.state() == state).count()
}

#[test]
fn every_line_decodes() {
    // Not a literal count: it changes every time the fixture is re-recorded, and the
    // point is that no line was dropped or refused, not how many there were. The
    // floor is the tripwire -- an empty or truncated recording is not a recording.
    assert_eq!(events().len(), FIXTURE.lines().filter(|l| !l.trim().is_empty()).count());
    assert!(events().len() > 100, "the fixture is a conversation, not a smoke test");
}

#[test]
fn only_envelope_types_a_frontend_knows() {
    let known = ["status", "token", "sessions", "context", "usage", "providers", "models",
                 "skills", "commands", "files", "projects", "jobs", "detail", "permission"];
    for e in events() {
        assert!(known.contains(&e.env.kind.as_str()), "unexpected envelope type {:?}", e.env.kind);
    }
}

#[test]
fn the_boot_line_says_which_session_and_mode() {
    let evs = events();
    let ready = evs.first().expect("a first event");
    assert_eq!((ready.env.kind.as_str(), ready.state()), ("status", "ready"));
    assert!(ready.payload.session_id.as_deref().is_some_and(|s| !s.is_empty()));
    assert!(ready.payload.mode.is_some());
}

#[test]
fn a_replay_covers_every_row_a_transcript_draws() {
    let evs = events();
    for state in ["user", "thinking", "content", "tool_calls", "tool_output", "notice"] {
        assert!(count(&evs, "token", state) > 0, "no {state} rows in the fixture");
    }
    assert_eq!(count(&evs, "token", "done"), 1);
    assert!(count(&evs, "token", "tool_output") > 100, "a computer-use transcript is mostly tool results");
}

#[test]
fn text_tokens_are_text_and_call_tokens_are_arrays() {
    let evs = events();
    let user = evs.iter().find(|e| e.state() == "user").unwrap();
    assert!(matches!(user.token(), Some(Token::Text(_))));
    assert!(!user.token_text().is_empty());

    // The one branch that is kept verbatim rather than dropping the event.
    let calls = evs.iter().find(|e| e.state() == "tool_calls").unwrap();
    assert!(matches!(calls.token(), Some(Token::Calls(_))));
    assert!(calls.token_text().contains('{'));
}

#[test]
fn a_tool_row_carries_its_index_and_result() {
    let evs = events();
    let out = evs.iter().filter(|e| e.state() == "tool_output").collect::<Vec<_>>();
    assert!(out[0].payload.index.is_some());
    assert!(out[0].payload.result.is_some());
    // Indices are the frontend's handle back into tool.detail: dense, from 0.
    let indices = out.iter().filter_map(|e| e.payload.index).collect::<Vec<_>>();
    assert_eq!(indices[0], 0);
    assert_eq!(indices, (0..out.len() as i64).collect::<Vec<_>>());
}

#[test]
fn a_replay_redraws_in_the_order_it_happened() {
    let evs = events();
    let at = |state: &str| evs.iter().position(|e| e.state() == state).unwrap();
    assert!(at("user") < at("tool_calls"));
    assert!(at("tool_calls") < at("tool_output"));
    assert_eq!(evs.last().unwrap().state(), "done");
    assert!(evs.last().unwrap().payload.msg_count.unwrap_or(0) > 0);
}

#[test]
fn the_session_list_reply_carries_meta() {
    let evs = events();
    let list = evs.iter().find(|e| e.env.kind == "sessions").expect("a sessions reply");
    let first = &list.env.data["sessions"][0];
    assert!(first["id"].is_string());
    assert!(first["provider"].is_string());
    assert!(first["turns"].is_number());
}

#[test]
fn both_titles_arrive_and_the_first_clears_a_stale_one() {
    let evs = events();
    let titles = evs.iter().filter(|e| e.state() == "session_title").collect::<Vec<_>>();
    assert_eq!(titles.len(), 2, "one for the booted session, one for the loaded one");
    // The boot session has no name yet, and saying so is the point: a frontend
    // is showing the name of the conversation that just went.
    assert_eq!(titles[0].payload.title.as_deref(), Some(""));
    assert!(titles[1].payload.title.as_deref().is_some_and(|t| !t.is_empty()));
    assert!(titles[1].payload.session_id.is_some());
}
