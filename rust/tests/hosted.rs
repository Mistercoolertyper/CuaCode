//! A worker **hosted** on a box, proved against the box.
//!
//! Opt-in, because it needs a machine: `CUACODE_TEST_BOX=fuzzy cargo test -p cuacode-rs --test hosted`.
//! Everything else in this crate's tests runs offline.
//!
//! What is asserted is the whole point of the shape: the worker is not a child of this
//! process, so letting go of it and coming back finds the same worker, still holding the
//! same conversation. That is what makes "close the laptop, the box keeps working" a
//! fact rather than a hope.

use cuacode::hosts;
use cuacode::wire::Envelope;
use cuacode::worker::Worker;

fn boxed() -> Option<hosts::Host> {
    let name = std::env::var("CUACODE_TEST_BOX").ok()?;
    let found = hosts::load(&cuacode::state_root())
        .into_iter()
        .find(|h| h.name == name);
    assert!(found.is_some(), "CUACODE_TEST_BOX={name} is not in hosts.json");
    found
}

/// Ask something the worker answers on its reader thread, and wait for the answer.
fn list(w: &mut Worker, rx: &std::sync::mpsc::Receiver<cuacode::wire::Event>, id: &str) -> bool {
    w.send(&Envelope::cmd(id, "session.list", serde_json::json!({})))
        .expect("send into the pane");
    for _ in 0..60 {
        for ev in rx.try_iter() {
            if ev.env.kind == "sessions" && ev.env.id == id {
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

#[test]
fn a_hosted_worker_outlives_the_client() {
    let Some(h) = boxed() else {
        eprintln!("hosted: set CUACODE_TEST_BOX=<name in hosts.json> to run this");
        return;
    };

    // First attach: starts the pane if the box has none, then reads its log.
    let (mut first, rx) = Worker::attach(&h).expect("attach");
    assert!(list(&mut first, &rx, "probe-1"), "the hosted worker answered session.list");

    // Let go. For a hosted worker this closes the tail and sends nothing: `stop` is not
    // on the wire, and the pane on that box carries on.
    drop(first);
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Back again: the same worker, still answering, with no second one started.
    let (mut second, rx2) = Worker::attach(&h).expect("reattach");
    assert!(list(&mut second, &rx2, "probe-2"), "the same worker answered after the client left");
}
