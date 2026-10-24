//! The whole conversation, folded from the recorded wire into rows.
//!
//! This is the frontend's own model rather than its drawing: it proves the
//! transcript a UI would show, with no terminal, no worker and no model call.
//! The fixture covers a real session -- thinking, tool calls, tool results,
//! runtime notices, a replay of a user turn.

use cuacode::transcript::{Row, Transcript};
use cuacode::wire;

const FIXTURE: &str = include_str!("../fixtures/replay.jsonl");

fn folded() -> Transcript {
    let mut log = Transcript::default();
    for line in FIXTURE.lines().filter(|l| !l.trim().is_empty()) {
        let ev = wire::parse(line).expect("every fixture line is an envelope");
        log.apply(&ev);
    }
    log
}

#[test]
fn every_event_a_transcript_draws_becomes_a_row() {
    let log = folded();
    let count = |f: fn(&Row) -> bool| log.rows.iter().filter(|r| f(r)).count();
    assert!(count(|r| matches!(r, Row::User { .. })) > 0);
    assert!(count(|r| matches!(r, Row::Thinking { .. })) > 0);
    assert!(count(|r| matches!(r, Row::Content { .. })) > 0);
    assert!(count(|r| matches!(r, Row::Notice { .. })) > 0);
    // One row per call, and every one of them answered.
    let tools = log.rows.iter().filter(|r| matches!(r, Row::Tool { .. })).count();
    assert!(tools > 100, "185 calls in the fixture, got {tools}");
    let answered = log
        .rows
        .iter()
        .filter(|r| matches!(r, Row::Tool { result: Some(_), .. }))
        .count();
    assert!(answered > 100, "{answered} of {tools} calls carry a result");
    // Events a transcript has no row for -- sessions, the boot line, the title,
    // usage -- are not rows.
    assert!(log.rows.len() < FIXTURE.lines().count());
}

#[test]
fn no_row_holds_a_tool_body() {
    // The point of the row model: a shell result is kilobytes and a row is a
    // line. User turns and runtime notices are text and may be long; a call and
    // its result may not.
    for row in folded().rows {
        if let Row::Tool { name, args, result, .. } = &row {
            let len = name.len() + args.len() + result.as_deref().unwrap_or("").len();
            assert!(len < 400, "a tool row that long is a body, not a row: {row:?}");
        }
    }
}

#[test]
fn every_replayed_call_keeps_its_name() {
    // The tool_calls token is the provider's shape and there are three of them.
    // A name arriving as "?" means one was not understood.
    let log = folded();
    let names: Vec<String> = log
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::Tool { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(names.len() > 100, "{} calls", names.len());
    assert!(!names.iter().any(|n| n == "?"), "a call's name was not decoded: {names:?}");
    assert!(names.iter().any(|n| n == "shell"), "the fixture runs shell commands");
}

#[test]
fn the_replayed_rounds_read_in_order() {
    // A user turn, then that turn's calls, each answered before the next call
    // appears -- which is what makes a replayed round legible.
    let log = folded();
    let first_user = log.rows.iter().position(|r| matches!(r, Row::User { .. })).unwrap();
    let first_tool = log.rows.iter().position(|r| matches!(r, Row::Tool { .. })).unwrap();
    assert!(first_user < first_tool);
    let unanswered_ever_answered = log
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r, Row::Tool { .. }))
        .scan(false, |seen_result, (_, r)| {
            let has = matches!(r, Row::Tool { result: Some(_), .. });
            *seen_result |= has;
            Some(*seen_result)
        })
        .count();
    assert_eq!(unanswered_ever_answered, log.rows.iter().filter(|r| matches!(r, Row::Tool { .. })).count());
}

#[test]
fn wrapping_keeps_the_offset_honest() {
    use cuacode::transcript::wrap_text;
    assert_eq!(wrap_text("", 10), vec![""]);
    assert_eq!(wrap_text("hello", 10), vec!["hello"]);
    assert_eq!(wrap_text("one two three", 7), vec!["one two", "three"]);
    // A word longer than the row is split rather than running off the edge.
    let long = wrap_text(&"x".repeat(25), 10);
    assert_eq!(long.len(), 3);
    assert!(long.iter().all(|l| l.chars().count() <= 10));
}

#[test]
fn the_runtime_blocks_a_user_turn_carries_are_collapsed() {
    // The fixture is full of recall pointers and project docs. A chat log that
    // prints them whole is unreadable, which is what the first version did.
    let log = folded();
    let notices: Vec<&Row> =
        log.rows.iter().filter(|r| matches!(r, Row::Notice { .. })).collect();
    assert!(!notices.is_empty(), "a real session has runtime notices");
    let mut collapsed = 0;
    for row in notices {
        let Row::Notice { head, hidden } = row else { continue };
        assert!(!head.contains('\n'), "a notice head is one line: {head:?}");
        assert!(head.len() < 200, "a notice head is a label, not a block: {head:?}");
        if *hidden > 0 {
            collapsed += 1;
        }
    }
    assert!(collapsed > 0, "the fixture's recall blocks should have been collapsed");
}
