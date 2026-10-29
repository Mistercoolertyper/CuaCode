//! Every global key, as a named action: what it does, what it is spelled, and the
//! one file that changes it.
//!
//! The table below is the whole of what a keystroke can mean at this screen's own
//! level. `main.rs` asks this map what a key is and runs the action; nothing
//! downstream matches a key code again except the two things that are not
//! bindings at all (see `main.rs`'s `fn key`: AltGr's character, and a newline
//! inside a Windows paste).
//!
//! What is deliberately **not** here is every modal key: the ask panel's
//! y/n/arrows, a picker's rows, an opened call's scrolling, and the `/` `$` `@`
//! menu's own navigation. A config file must not be able to take away the key
//! that answers a permission prompt, so those keys are not bindings and this file
//! cannot reach them.
//!
//! The file is `keys.json` in the state root, next to `config.json`:
//!
//! ```json
//! { "fold": "ctrl+y", "calls": ["ctrl+o", "f2"] }
//! ```
//!
//! A row replaces that action's chords and leaves every other action alone. A row
//! that cannot be read is a complaint on the transcript and nothing else: the
//! defaults stay, and a broken file never stops the screen from opening.

use std::fmt;
use std::path::Path;

use crossterm::event::{KeyCode, KeyModifiers};

/// The name the file is looked for under, in the state root.
pub const FILE: &str = "keys.json";

/// One thing a key can be told to do. One variant per global binding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Send,
    Newline,
    Paste,
    Left,
    WordLeft,
    LineStart,
    Home,
    Right,
    WordRight,
    LineEnd,
    End,
    Backspace,
    WordBack,
    Delete,
    WordDelete,
    KillStart,
    KillEnd,
    Undo,
    Up,
    Down,
    PageUp,
    PageDown,
    Stop,
    Trigger,
    Palette,
    Fold,
    Calls,
    Background,
    Host,
    Detach,
    Quit,
}

/// The table: the action, the name the file calls it, the words `/help` prints
/// beside it, every spelling that means it, and the group that row belongs to.
/// This is the one place a binding is defined; `fn key` runs the action and never
/// names a key again.
///
/// The spellings are in the order a person is meant to read them: the first is the
/// key, and the rest are the other ways the same thing arrives. More than one is
/// the norm rather than the exception, because a terminal reports one spelling or
/// another and none of them is wrong: shift-enter where it reports a shifted key,
/// alt-enter where it maps the key to `1b 0d`, ctrl-j where it reports neither. A
/// mac terminal sends cmd+left as ctrl-a before this program ever sees it, which is
/// why one action can have four.
///
/// The rows are in group order, and that is the order `/help` prints them in: a
/// list of thirty keys with no order is a wall, and the groups are the only thing
/// that makes one findable.
const TABLE: &[(Action, &str, &str, &[&str], &str)] = &[
    (
        Action::Send,
        "send",
        "send",
        &["enter"],
        "the message",
    ),
    (
        Action::Newline,
        "newline",
        "a new line in the message",
        &["shift+enter", "alt+enter", "ctrl+j"],
        "the message",
    ),
    (
        Action::Paste,
        "paste",
        "attach the picture on the clipboard",
        &["ctrl+v"],
        "the message",
    ),
    (
        Action::Trigger,
        "trigger",
        "the menus: commands, skills, files",
        &["/", "$", "@"],
        "the message",
    ),
    (
        Action::Backspace,
        "backspace",
        "a character, or a whole picture",
        &["backspace"],
        "editing",
    ),
    (
        Action::WordBack,
        "word-back",
        "a word back",
        &["ctrl+backspace", "alt+backspace", "ctrl+alt+backspace"],
        "editing",
    ),
    (
        Action::Delete,
        "delete",
        "a character forward",
        &["delete"],
        "editing",
    ),
    (
        Action::WordDelete,
        "word-delete",
        "a word forward",
        &["ctrl+delete", "alt+delete", "ctrl+alt+delete"],
        "editing",
    ),
    (
        Action::KillStart,
        "kill-start",
        "everything before the caret on the line",
        &["cmd+backspace", "ctrl+u"],
        "editing",
    ),
    (
        Action::KillEnd,
        "kill-end",
        "to the end of the line",
        &["cmd+delete", "cmd+k", "ctrl+k"],
        "editing",
    ),
    (
        Action::Undo,
        "undo",
        "put back what the last delete took",
        &["ctrl+y"],
        "editing",
    ),
    (Action::Left, "left", "move the caret", &["left"], "the caret"),
    (
        Action::WordLeft,
        "word-left",
        "a word left",
        &["alt+b", "alt+left", "ctrl+left", "ctrl+alt+left"],
        "the caret",
    ),
    (
        Action::LineStart,
        "line-start",
        "the start of the line",
        &["ctrl+a", "cmd+left"],
        "the caret",
    ),
    (
        Action::Right,
        "right",
        "move the caret",
        &["right"],
        "the caret",
    ),
    (
        Action::WordRight,
        "word-right",
        "a word right",
        &["alt+f", "alt+right", "ctrl+right", "ctrl+alt+right"],
        "the caret",
    ),
    (
        Action::LineEnd,
        "line-end",
        "the end of the line",
        &["ctrl+e", "cmd+right"],
        "the caret",
    ),
    (
        Action::Home,
        "home",
        "the start of the line, or of the conversation",
        &["home"],
        "the caret",
    ),
    (
        Action::End,
        "end",
        "the end of the line, or of the conversation",
        &["end"],
        "the caret",
    ),
    (
        Action::Up,
        "up",
        "a line of the message, or of the conversation",
        &["up"],
        "the conversation",
    ),
    (
        Action::Down,
        "down",
        "a line of the message, or of the conversation",
        &["down"],
        "the conversation",
    ),
    (
        Action::PageUp,
        "page-up",
        "ten lines of the conversation",
        &["pageup"],
        "the conversation",
    ),
    (
        Action::PageDown,
        "page-down",
        "ten lines of the conversation",
        &["pagedown"],
        "the conversation",
    ),
    (
        Action::Stop,
        "stop",
        "stop the round, or empty what you typed",
        &["esc"],
        "the conversation",
    ),
    (
        Action::Fold,
        "fold",
        "every step's calls; thinking one line each",
        &["ctrl+t"],
        "the app",
    ),
    (
        Action::Calls,
        "calls",
        "the calls, to open one in full",
        &["ctrl+o"],
        "the app",
    ),
    (
        Action::Palette,
        "palette",
        "the actions, to run one from",
        &["ctrl+p"],
        "the app",
    ),
    (
        Action::Background,
        "background",
        "push the running call into the background",
        &["ctrl+b"],
        "the app",
    ),
    (
        Action::Host,
        "host",
        "the machine this conversation runs on",
        &["ctrl+g"],
        "the app",
    ),
    (
        Action::Detach,
        "detach",
        "let go of the box; it keeps working",
        &["ctrl+d"],
        "the app",
    ),
    (Action::Quit, "quit", "quit", &["ctrl+c"], "the app"),
];

impl Action {
    /// The action a name in the file means, if any.
    pub fn named(name: &str) -> Option<Action> {
        TABLE.iter().find(|row| row.1 == name).map(|row| row.0)
    }
}

/// One spelling of a key: the key itself and the modifiers held with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    /// Whether a key event is this chord.
    ///
    /// A character already carries its own case, and a terminal may or may not
    /// report the shift that made it: kitty reports shift on `/` and xterm sends
    /// the bare character. So a chord on a character asks about ctrl, alt and cmd
    /// and ignores shift, which is the only reading where `/` opens the menu on
    /// both. A named key is compared exactly, because there the modifiers are the
    /// whole difference between enter and shift-enter.
    fn fits(&self, code: KeyCode, mods: KeyModifiers) -> bool {
        if self.code != code {
            return false;
        }
        if matches!(code, KeyCode::Char(_)) {
            let held = KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER;
            (self.mods & held) == (mods & held)
        } else {
            self.mods == mods
        }
    }
}

/// Read a chord the way a person writes it: `ctrl+t`, `alt+enter`, `f1`, `/`,
/// `shift+ctrl+space`. Modifiers in any order, joined by `+`, no spaces.
///
/// The error carries the text it could not read, because that text is what the
/// complaint has to quote: a row that says `ctrl+nope` is not helped by being told
/// that something was refused.
pub fn parse(text: &str) -> Result<Chord, String> {
    let mut mods = KeyModifiers::empty();
    let mut rest = text;
    while let Some((head, tail)) = rest.split_once('+') {
        mods |= match head.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => KeyModifiers::CONTROL,
            "alt" | "option" => KeyModifiers::ALT,
            "shift" => KeyModifiers::SHIFT,
            "cmd" | "super" | "meta" => KeyModifiers::SUPER,
            _ => return Err(text.to_string()),
        };
        rest = tail;
    }
    let code = code_of(rest).ok_or_else(|| text.to_string())?;
    Ok(Chord { code, mods })
}

/// The key at the end of a chord's spelling.
fn code_of(name: &str) -> Option<KeyCode> {
    let named = match name.to_ascii_lowercase().as_str() {
        "space" => Some(KeyCode::Char(' ')),
        "esc" | "escape" => Some(KeyCode::Esc),
        "enter" | "return" => Some(KeyCode::Enter),
        "tab" => Some(KeyCode::Tab),
        "backspace" => Some(KeyCode::Backspace),
        "delete" | "del" => Some(KeyCode::Delete),
        "insert" | "ins" => Some(KeyCode::Insert),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        "pageup" | "pgup" => Some(KeyCode::PageUp),
        "pagedown" | "pgdn" => Some(KeyCode::PageDown),
        "up" => Some(KeyCode::Up),
        "down" => Some(KeyCode::Down),
        "left" => Some(KeyCode::Left),
        "right" => Some(KeyCode::Right),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    // f1 to f12, and the bare `f` is still a character.
    if let Some(n) = name.to_ascii_lowercase().strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=12).contains(&n) {
            return Some(KeyCode::F(n));
        }
    }
    // One character and no more: `/`, `@`, `a`.
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(KeyCode::Char(c)),
        _ => None,
    }
}

/// A chord written back in the spelling `parse` reads, so the file and `/help`
/// and this type all say the same thing.
impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.contains(KeyModifiers::CONTROL) {
            f.write_str("ctrl+")?;
        }
        if self.mods.contains(KeyModifiers::ALT) {
            f.write_str("alt+")?;
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            f.write_str("shift+")?;
        }
        if self.mods.contains(KeyModifiers::SUPER) {
            f.write_str("cmd+")?;
        }
        f.write_str(&key_name(self.code))
    }
}

fn key_name(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        // No binding is spelled on any other key. A chord on one would still print
        // something rather than nothing, so a mistake in a file is visible.
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

/// What every action is bound to.
#[derive(Clone, PartialEq, Debug)]
pub struct Keymap {
    bound: Vec<Bound>,
}

#[derive(Clone, PartialEq, Debug)]
struct Bound {
    action: Action,
    chords: Vec<Chord>,
}

/// What the table says with no file: the bindings this screen shipped with.
pub fn defaults() -> Keymap {
    Keymap {
        bound: TABLE
            .iter()
            .map(|row| Bound {
                action: row.0,
                chords: row
                    .3
                    .iter()
                    .map(|text| parse(text).expect("a default chord that does not parse is a bug"))
                    .collect(),
            })
            .collect(),
    }
}

/// One row of the key list: the group it belongs under, the one key that is it,
/// every other spelling of that key, and what it does.
///
/// One key and a tail rather than every chord in the key column: an action with
/// four spellings printed as four is the keymap dumped on the page, and what a
/// person reads is the key and the fact that there are other ways.
#[derive(Debug)]
pub struct KeyRow {
    pub group: &'static str,
    pub key: String,
    pub also: String,
    pub what: &'static str,
}

impl Keymap {
    /// The action a key event is, if it is any of them.
    pub fn action(&self, code: KeyCode, mods: KeyModifiers) -> Option<Action> {
        self.bound
            .iter()
            .find(|bound| bound.chords.iter().any(|chord| chord.fits(code, mods)))
            .map(|bound| bound.action)
    }

    /// The rows `/help` prints, in the table's order and therefore in groups. An
    /// action a file left with no key of its own is left out: a row with nothing
    /// beside it is not a key list.
    pub fn rows(&self) -> Vec<KeyRow> {
        TABLE
            .iter()
            .filter_map(|row| {
                let bound = self.bound.iter().find(|b| b.action == row.0)?;
                let (key, rest) = bound.chords.split_first()?;
                Some(KeyRow {
                    group: row.4,
                    key: key.to_string(),
                    also: rest.iter().map(Chord::to_string).collect::<Vec<_>>().join(", "),
                    what: row.2,
                })
            })
            .collect()
    }

    /// Bind one action. Whatever else held one of these chords lets go of it, which
    /// is what rebinding means: a file can move ctrl-y without having to say who
    /// used to have it.
    fn set(&mut self, action: Action, chords: Vec<Chord>) {
        for bound in &mut self.bound {
            if bound.action != action {
                bound.chords.retain(|chord| !chords.contains(chord));
            }
        }
        match self.bound.iter_mut().find(|bound| bound.action == action) {
            Some(bound) => bound.chords = chords,
            None => self.bound.push(Bound { action, chords }),
        }
    }
}

/// The keymap at `path`, and everything wrong with the file it came from.
///
/// No file is the defaults in silence: nobody changed anything, and a screen that
/// greeted every launch with a line about a file that is not there would be
/// noise. A file that is there and cannot be read, or cannot be understood, is
/// not silence: it says what it could not do and leaves the defaults standing, so
/// the worst a broken file does is cost a line of the transcript.
///
/// ponytail: the file is read once, at launch, and never watched. Editing it
/// takes the next start, which is the same deal `config.json` offers and needs no
/// thread, no stamp and no partial read of a file being saved.
pub fn load(path: &Path) -> (Keymap, Vec<String>) {
    let mut map = defaults();
    let mut said = Vec::new();
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (map, said),
        Err(e) => {
            said.push(format!("{}: {e}", path.display()));
            return (map, said);
        }
    };
    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(e) => {
            said.push(format!("{}: {e}", path.display()));
            return (map, said);
        }
    };
    let Some(rows) = parsed.as_object() else {
        said.push(format!("{}: not an object of actions", path.display()));
        return (map, said);
    };
    // What this file has already claimed. The defaults are not in here: taking a
    // chord off a default is what an override is for. Two rows of the file on one
    // chord is ambiguous, and there the later row is the one refused, so the first
    // reading of the file is the one that holds.
    let mut claimed: Vec<(Chord, &str)> = Vec::new();
    for (name, value) in rows {
        let Some(action) = Action::named(name) else {
            said.push(format!("{}: no action called `{name}`", path.display()));
            continue;
        };
        let spellings: Option<Vec<&str>> = match value {
            serde_json::Value::String(one) => Some(vec![one.as_str()]),
            serde_json::Value::Array(list) => list.iter().map(serde_json::Value::as_str).collect(),
            _ => None,
        };
        let Some(spellings) = spellings else {
            said.push(format!("{}: `{name}` is not a chord or a list of chords", path.display()));
            continue;
        };
        let mut chords = Vec::new();
        let mut refused = false;
        for text in spellings {
            match parse(text) {
                Ok(chord) => chords.push(chord),
                Err(text) => {
                    said.push(format!("{}: cannot read `{text}`", path.display()));
                    refused = true;
                }
            }
        }
        if chords.is_empty() {
            if !refused {
                said.push(format!("{}: `{name}` is bound to nothing", path.display()));
            }
            continue;
        }
        for chord in &chords {
            if let Some((_, other)) = claimed.iter().find(|(taken, _)| taken == chord) {
                said.push(format!(
                    "{}: `{chord}` is bound to both `{other}` and `{name}`",
                    path.display()
                ));
                refused = true;
            }
        }
        if refused {
            continue;
        }
        claimed.extend(chords.iter().map(|chord| (*chord, name.as_str())));
        map.set(action, chords);
    }
    (map, said)
}

#[cfg(test)]
mod tests {
    use super::*;

    use KeyCode::{Char, F};

    const CONTROL: KeyModifiers = KeyModifiers::CONTROL;

    /// A file in the temp dir, named after this process so two test runs do not
    /// read each other's.
    fn file(name: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("quik-keys-{}-{name}.json", std::process::id()));
        std::fs::write(&path, body).expect("write the test file");
        path
    }

    /// A chord is read the way a person writes it, and written back the same way.
    #[test]
    fn a_good_chord_parses_and_reads_back_as_itself() {
        assert_eq!(parse("ctrl+t"), Ok(Chord { code: KeyCode::Char('t'), mods: CONTROL }));
        assert_eq!(parse("/"), Ok(Chord { code: KeyCode::Char('/'), mods: KeyModifiers::NONE }));
        assert_eq!(parse("space"), Ok(Chord { code: KeyCode::Char(' '), mods: KeyModifiers::NONE }));
        assert_eq!(parse("f1"), Ok(Chord { code: KeyCode::F(1), mods: KeyModifiers::NONE }));
        // Modifiers in any order, and the same chord either way.
        assert_eq!(parse("alt+shift+enter"), parse("shift+alt+enter"));
        for text in ["ctrl+t", "alt+enter", "shift+enter", "ctrl+shift+enter", "f12", "/", "@", "space", "esc"] {
            let chord = parse(text).expect(text);
            assert_eq!(chord.to_string(), text, "a chord is spelled the way it is read");
            assert_eq!(parse(&chord.to_string()), Ok(chord));
        }
    }

    /// A spelling it cannot read comes back as the text it could not read.
    #[test]
    fn a_bad_chord_is_refused_with_its_own_text() {
        for text in ["", "ctrl+", "hyper+t", "ctrl+nope", "f13", "ab", "ctrl t"] {
            assert_eq!(parse(text), Err(text.to_string()), "{text}");
        }
    }

    /// A file that rebinds one action moves that action and leaves the rest alone,
    /// including the chord it used to have and the action that used to have the new
    /// one.
    #[test]
    fn a_file_rebinds_one_action_and_leaves_the_rest() {
        let path = file("one", r#"{ "fold": "ctrl+y" }"#);
        let (map, said) = load(&path);
        assert!(said.is_empty(), "{said:?}");
        assert_eq!(map.action(Char('y'), CONTROL), Some(Action::Fold));
        assert_eq!(map.action(Char('t'), CONTROL), None, "fold's old key stayed");
        assert_eq!(map.action(Char('o'), CONTROL), Some(Action::Calls), "another action moved");
        assert_eq!(map.action(Char('u'), CONTROL), Some(Action::KillStart));
        // The list a second spelling is for: one action, two keys.
        let path = file("two", r#"{ "calls": ["ctrl+o", "f2"] }"#);
        let (map, said) = load(&path);
        assert!(said.is_empty(), "{said:?}");
        assert_eq!(map.action(F(2), KeyModifiers::NONE), Some(Action::Calls));
        assert_eq!(map.action(Char('o'), CONTROL), Some(Action::Calls));
        // The first spelling is the key and f2 is the other way: a row is one key
        // with a tail, not every chord it has.
        let calls = map.rows().into_iter().find(|row| row.key == "ctrl+o").expect("no calls row");
        assert_eq!(calls.also, "f2");
    }

    /// A name nobody has is a complaint, not a panic, and not a change.
    #[test]
    fn a_file_naming_an_action_nobody_has_complains_and_changes_nothing() {
        let path = file("nobody", r#"{ "frobnicate": "ctrl+x" }"#);
        let (map, said) = load(&path);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("frobnicate"), "{said:?}");
        assert_eq!(map, defaults());
    }

    /// A chord two rows both want is refused for the second of them: the first
    /// reading of the file is the one that holds, and the other action keeps what
    /// it had.
    #[test]
    fn two_rows_on_one_chord_are_complained_about_and_the_second_is_refused() {
        let path = file("twice", r#"{ "calls": "ctrl+y", "fold": "ctrl+y" }"#);
        let (map, said) = load(&path);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("ctrl+y"), "{said:?}");
        // The file is read in name order, so `calls` gets there first.
        assert_eq!(map.action(Char('y'), CONTROL), Some(Action::Calls));
        assert_eq!(map.action(Char('t'), CONTROL), Some(Action::Fold));
    }

    /// A file that is not the shape wanted, or not a file, is the defaults and one
    /// line each: never a screen that does not open.
    #[test]
    fn a_broken_file_is_the_defaults_and_a_complaint() {
        for body in ["{ not json", "[]", r#"{ "fold": 7 }"#, r#"{ "fold": [] }"#, r#"{ "fold": ["ctrl+t", "nope"] }"#] {
            let path = file("broken", body);
            let (map, said) = load(&path);
            assert_eq!(map, defaults(), "{body}");
            assert!(!said.is_empty(), "{body}");
        }
        // No file at all is not a complaint.
        let (map, said) = load(std::path::Path::new("/nonexistent/keys.json"));
        assert_eq!(map, defaults());
        assert!(said.is_empty(), "{said:?}");
    }

    /// The table itself: every action has rows, no chord is in two, and no two
    /// actions answer to one name.
    #[test]
    fn the_table_is_one_binding_per_key_and_one_name_per_action() {
        let map = defaults();
        let mut seen: Vec<(Chord, Action)> = Vec::new();
        for bound in &map.bound {
            assert!(!bound.chords.is_empty(), "{:?} has no key", bound.action);
            for chord in &bound.chords {
                if let Some((_, other)) = seen.iter().find(|(c, _)| c == chord) {
                    panic!("{chord} is on both {other:?} and {:?}", bound.action);
                }
                seen.push((*chord, bound.action));
            }
        }
        let mut names: Vec<&str> = Vec::new();
        for row in TABLE {
            assert!(!names.contains(&row.1), "{} is two actions", row.1);
            names.push(row.1);
            assert_eq!(Action::named(row.1), Some(row.0));
        }
        // Every key the table names is a row in the list `/help` prints.
        assert_eq!(map.rows().len(), TABLE.len());
    }
}
