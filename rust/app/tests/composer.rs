//! The composer's keys, proved rather than read.
//!
//! `05-editor.md` names four things about the field that the code claimed and
//! nothing had exercised: it is **multi-line**, **Enter submits**, **a refused
//! submit is a newline**, and **Shift-Enter always inserts**. None of them shows
//! up in a frame - a newline is invisible in a picture, and the refusal path is
//! exactly the part a one-line field could not express - so this test types into
//! the real `App` in a test window and reads the draft back off the field.
//!
//! Two things make it hermetic. `CUACODE_HOME` is a scratch state root, so no
//! session of anyone's is read; and `CUACODE_WORKER` is a stub that swallows
//! every envelope and answers nothing, so the send path is real (an agent exists,
//! `chat` is called, the field is cleared) with no model, no key and no network.
//!
//! The composer only exists when a worker started: `main.rs` draws it under
//! `.when(self.agent.is_some())`, because a field with nothing to send to is a
//! field asking you to talk to nobody. So the refusal this test exercises is the
//! source's own - **an empty prompt** - not a missing worker.

use cuacode_app::App;
use gpui_kit::AppContext as _;
use gpui_kit::TestAppContext;

/// A stand-in worker: it swallows every envelope and never answers.
const STUB: &str = "import sys, time\nfor _line in sys.stdin:\n    time.sleep(3600)\n";

/// A state root under the temp dir, emptied first so a stale `sessions/` from an
/// earlier run cannot make this one read a conversation it did not write.
fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cuacode-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch state root");
    dir
}

#[gpui_kit::test]
fn the_composer_keys(cx: &mut TestAppContext) {
    let home = scratch("composer");
    let stub = home.join("stub.py");
    std::fs::write(&stub, STUB).expect("write the stub worker");
    unsafe {
        std::env::set_var("CUACODE_HOME", &home);
        std::env::set_var("CUACODE_WORKER", &stub);
    }
    cx.update(gpui_kit::init);

    let (app, cx) = cx.add_window_view(|window, cx| App::new(None, stub.clone(), window, cx));
    // A draw, because a character is delivered through the input handler the
    // focused field registers while it paints.
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    // **The keys are the field's from the moment the window opens.** A window with
    // one field that must be clicked before typing is a field that looks broken on
    // the first keystroke.
    assert!(
        cx.update(|window, cx| app.read(cx).composer_focused(window, cx)),
        "the composer's field holds the keys as soon as the window opens"
    );
    assert_eq!(app.read_with(cx, |app, cx| app.draft(cx)), "", "an empty field");

    // **Enter on an empty prompt inserts a newline.** The submit is declined - there
    // is nothing to send - and the source's path for that is the editor splitting the
    // block, not the key doing nothing (`ComposerPromptEditorTiptap.tsx:1281`).
    cx.simulate_keystrokes("enter");
    assert_eq!(
        app.read_with(cx, |app, cx| app.draft(cx)),
        "\n",
        "a refused submit inserts a newline"
    );

    // Typing lands after the line that Enter made: the buffer is multi-line, which is
    // the thing a one-line field could not do.
    cx.simulate_input("hello");
    assert_eq!(
        app.read_with(cx, |app, cx| app.draft(cx)),
        "\nhello",
        "typing lands at the caret"
    );

    // **Shift-Enter never submits**: `composer-logic.ts:85` forces a null intent, so it
    // takes the same split-block path the refused Enter did.
    cx.simulate_keystrokes("shift-enter");
    assert_eq!(
        app.read_with(cx, |app, cx| app.draft(cx)),
        "\nhello\n",
        "shift-enter inserts and does not submit"
    );

    // **Enter submits.** With the stub worker up and text in the field the send is
    // actioned, and the field is what that is measured on: it empties.
    cx.simulate_keystrokes("enter");
    assert_eq!(
        app.read_with(cx, |app, cx| app.draft(cx)),
        "",
        "Enter sends the draft and clears the field"
    );
}
