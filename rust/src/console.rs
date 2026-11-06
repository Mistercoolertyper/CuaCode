//! The console's input mode, kept the way this frontend needs it.
//!
//! The console's input mode belongs to the *console*, not to a process. Anything
//! attached to the same console can `SetConsoleMode` on it, and the mode decides
//! what the *next* reader receives -- so a program that turns on
//! `ENABLE_VIRTUAL_TERMINAL_INPUT` (a PTY-ish tool, a REPL, a child that thinks it
//! owns the terminal) changes what this frontend reads next, and changes it for
//! every process on that console, not just for itself.
//!
//! What that costs, measured on Windows 11 24H2: with VT input on, the console
//! stops handing back mouse records and encodes input as escape sequences instead.
//! A pointer crossing the window then arrives as a run of characters --
//! `ESC [ < b ; x ; y M` for every move, from Windows Terminal's conpty side --
//! which this frontend reads as keys and types into the message being written, and
//! the keyboard's own keys stop matching any binding, because the encodings
//! changed under them. The mode stayed changed after the process that set it
//! exited; nothing puts it back. That is the whole of the reported symptom.
//!
//! Two things keep it from happening, and both are needed. `tools/shell` and the
//! MCP client give every child **no console of this one's** (`CREATE_NO_WINDOW`),
//! so a child has nothing to change; that is the cause being removed. This module
//! is the console being defended anyway: whatever else on the machine does reach
//! this console, the mode is put back to what this frontend needs, and the queue
//! is emptied of the escape-sequence run that was already encoded into it.
//!
//! ponytail: on Unix the same class of failure exists (`tcsetattr` on the shared
//! terminal) and the same defence would apply, but crossterm's Unix raw mode is
//! per-file-descriptor and a stray writer is far rarer there. Nothing is asserted
//! on that side yet; the isolation fix in the workers is the cross-platform part.

/// `ENABLE_VIRTUAL_TERMINAL_INPUT`: input encoded as escape sequences. The one
/// bit that does the damage, and the one this frontend must not have set.
pub const VT_INPUT: u32 = 0x0200;
/// `ENABLE_WINDOW_INPUT`: a resize arrives as an event.
const WINDOW_INPUT: u32 = 0x0008;
/// `ENABLE_MOUSE_INPUT`: the pointer arrives as mouse records. Which is the whole
/// reason VT input is a problem: it is the same moves, encoded differently.
const MOUSE_INPUT: u32 = 0x0010;
/// `ENABLE_EXTENDED_FLAGS`: the keypad and the arrows keep their own codes.
const EXTENDED_FLAGS: u32 = 0x0080;
/// The three raw mode clears. Named here because [`wanted`] asserts them.
const NOT_RAW: u32 = 0x0001 | 0x0002 | 0x0004;   // PROCESSED | LINE | ECHO

/// The mode this frontend needs, given whatever some other process left behind.
///
/// Not a constant, and not `current & !VT_INPUT` either: a process that set VT
/// input has usually *cleared* the event bits while it was at it, and the frontend
/// left without mouse input stops seeing the pointer at all -- a different bug with
/// the same cause. So the three event bits are put back, the three raw-mode bits
/// are cleared, and VT input goes. Everything else in the mode is left alone,
/// because it is not this module's business what the window manager asked for.
pub fn wanted(current: u32) -> u32 {
    (current | WINDOW_INPUT | MOUSE_INPUT | EXTENDED_FLAGS) & !(VT_INPUT | NOT_RAW)
}

/// The longest run swallowed as one sequence before it is given up on.
///
/// A mouse report is about a dozen characters and a paste report is short; the
/// cap is what stops a stray escape from disabling the field for good, since the
/// end of a sequence that never arrives would otherwise never arrive either.
const SEQUENCE_CAP: usize = 64;

/// Where a run that began with ESC has got to.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Run {
    /// Not in a sequence: this is text.
    #[default]
    Text,
    /// One ESC seen, waiting for the byte that says what kind it is.
    Escape,
    /// `ESC [` -- parameters, then a final byte in `@`..`~`.
    Csi,
    /// `ESC O` -- exactly one more byte.
    Ss3,
    /// `ESC ]` -- an operating-system command, ending at BEL or at `ESC \`.
    Osc,
    /// A `ESC \` inside an OSC: the string terminator's second half.
    OscEsc,
}

/// A character as a text field should see it, with terminal sequences removed.
///
/// Belt to the mode's braces, and the half that works in the very frame the drift
/// happens in: a mouse report encoded as characters arrives as `ESC [ < b ; x ; y M`,
/// and to a key handler that is an escape followed by printable text. Swallowing
/// the escape and the run it starts is what keeps `[<0;35;12M` out of the message.
///
/// Deliberately not a VT parser. It recognises the one shape that hurts -- a run
/// introduced by ESC -- and drops it. A `[` or a `0` typed by hand is not preceded
/// by an escape and is untouched, which is the property that matters: a filter that
/// eats honest punctuation is worse than the bug it is fixing.
#[derive(Default)]
pub struct Typing {
    run: Run,
    /// Characters spent in the current run, so a sequence that never ends cannot
    /// swallow the rest of the session.
    spent: usize,
}

impl Typing {
    /// The character to keep, or `None` for one that belongs to a sequence.
    pub fn keep(&mut self, c: char) -> Option<char> {
        if self.run == Run::Text {
            if c == '\x1b' {
                self.begin(Run::Escape);
                return None;
            }
            // A control character that is not a newline, a tab or a backspace was
            // not typed into a text field: raw mode reports the keys a keyboard
            // sends by name, so anything left here came from a sequence.
            return (!c.is_control() || c == '\n' || c == '\t').then_some(c);
        }

        self.spent += 1;
        if self.spent > SEQUENCE_CAP {
            // No end arrived: stop waiting for one rather than swallow the session.
            self.run = Run::Text;
            self.spent = 0;
            return None;
        }

        let next = match (self.run, c) {
            (Run::Escape, '[') => Run::Csi,
            (Run::Escape, 'O') => Run::Ss3,
            (Run::Escape, ']') => Run::Osc,
            // Any other byte after ESC is a two-character sequence (ESC M, ESC 7,
            // ESC =), so the run is over and the character went with it.
            (Run::Escape, _) => Run::Text,
            // Parameters and intermediates are below `@`; the first byte at or above
            // it is the one that ends the sequence.
            (Run::Csi, c) if c.is_ascii() && (c as u8) >= 0x40 => Run::Text,
            (Run::Csi, _) => Run::Csi,
            (Run::Ss3, _) => Run::Text,
            (Run::Osc, '\x07') => Run::Text,
            (Run::Osc, '\x1b') => Run::OscEsc,
            (Run::Osc, _) => Run::Osc,
            (Run::OscEsc, '\\') => Run::Text,
            (Run::OscEsc, _) => Run::Osc,
            (Run::Text, _) => Run::Text,
        };
        if next == Run::Text {
            self.spent = 0;
        }
        self.run = next;
        None
    }

    /// Whether the last character seen was part of a sequence rather than text.
    /// Read by the tests; a frontend does not need to ask.
    pub fn mid_sequence(&self) -> bool {
        self.run != Run::Text
    }

    fn begin(&mut self, run: Run) {
        self.run = run;
        self.spent = 1;
    }
}

#[cfg(windows)]
pub use win::Claim;

#[cfg(not(windows))]
pub use quiet::Claim;

#[cfg(windows)]
mod win {
    use super::wanted;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
                                                  FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{
        FlushConsoleInputBuffer, GetConsoleMode, GetStdHandle, SetConsoleMode, STD_INPUT_HANDLE,
    };

    /// The console's input handle, for as long as this frontend is drawing.
    ///
    /// Opened through `CONIN$` rather than taken from `STD_INPUT_HANDLE`, because
    /// the two are the same console only while stdin has not been redirected -- and
    /// a frontend whose stdin is a pipe still draws in a console, and still has its
    /// input mode moved by whatever else is on it. `CONIN$` is that console either
    /// way. The standard handle is the fallback for the case `CONIN$` cannot open.
    pub struct Claim {
        handle: isize,
        /// Whether `handle` came from `CreateFileW` and so wants closing.
        owned: bool,
        claimed: u32,
        /// How many times something else has moved the mode since the last look.
        /// Read by the test that drives this, and by nothing in a frame.
        drifts: usize,
    }

    /// `CONIN$` as UTF-16 with its terminator.
    const CONIN: [u16; 7] = [b'C' as u16, b'O' as u16, b'N' as u16, b'I' as u16,
                             b'N' as u16, b'$' as u16, 0];

    /// Whether a handle from these APIs is a real one.
    fn usable(h: isize) -> bool {
        h != 0 && h != -1 && h != INVALID_HANDLE_VALUE as isize
    }

    fn open() -> Option<(isize, bool)> {
        // SAFETY: the name is a NUL-terminated static, and null/default arguments are
        // the documented ones for a console device.
        let h = unsafe {
            CreateFileW(CONIN.as_ptr(), FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                        FILE_SHARE_READ | FILE_SHARE_WRITE, core::ptr::null(),
                        OPEN_EXISTING, 0, core::ptr::null_mut())
        } as isize;
        if usable(h) {
            return Some((h, true));
        }
        let std = unsafe { GetStdHandle(STD_INPUT_HANDLE) } as isize;
        usable(std).then_some((std, false))
    }

    fn mode_of(handle: isize) -> Option<u32> {
        let mut mode = 0u32;
        let ok = unsafe { GetConsoleMode(handle as _, &mut mode) };
        (ok != 0).then_some(mode)
    }

    impl Claim {
        /// Take the mode as this frontend needs it, and remember it.
        ///
        /// Returns a claim even when there is no console, so a caller never has to
        /// branch on the platform or on how it was launched.
        pub fn take() -> Claim {
            let Some((handle, owned)) = open() else {
                return Claim { handle: 0, owned: false, claimed: 0, drifts: 0 };
            };
            let mut claim = Claim { handle, owned, claimed: 0, drifts: 0 };
            if let Some(mode) = mode_of(handle) {
                // Assert it once up front as well as later: a console handed to this
                // process already carrying VT input is a console whose first frame
                // would otherwise be read wrong.
                let want = wanted(mode);
                unsafe { SetConsoleMode(handle as _, want) };
                claim.claimed = want;
            }
            claim
        }

        /// Put the mode back if anything moved it, and drop what it already
        /// encoded. `Some(what_it_had_become)` when it had to.
        ///
        /// The flush is the half that matters to the person typing: by the time a
        /// drift is noticed, the pointer's escape sequence is already sitting in the
        /// queue as characters, and restoring the mode does not take those back.
        /// Emptying the queue spends whatever was pressed during the window -- a key
        /// pressed into a frontend that was reading garbage anyway -- and that is the
        /// right trade against a message box full of `[<0;35;12M`.
        pub fn reassert(&mut self) -> Option<u32> {
            if self.handle == 0 {
                return None;
            }
            let mode = mode_of(self.handle)?;
            if mode == self.claimed {
                return None;
            }
            unsafe { SetConsoleMode(self.handle as _, self.claimed) };
            unsafe { FlushConsoleInputBuffer(self.handle as _) };
            self.drifts += 1;
            Some(mode)
        }

        /// How many times the console was found moved. Nothing draws it; the test
        /// that proves the guard works reads it.
        pub fn drifts(&self) -> usize {
            self.drifts
        }
    }

    impl Drop for Claim {
        /// The console the frontend opened is the frontend's to close. A mode left
        /// as it was found is not restored: the process that comes next is the
        /// shell, and the mode this frontend wants is not the mode a shell wants.
        /// crossterm's own cleanup is what puts the terminal back.
        fn drop(&mut self) {
            if self.owned && self.handle != 0 {
                unsafe { CloseHandle(self.handle as _) };
            }
        }
    }
}

#[cfg(not(windows))]
mod quiet {
    /// Unix: crossterm's raw mode is this process's own file descriptor, and the
    /// isolation fix in the workers keeps a child off the terminal entirely, so
    /// there is no shared console mode to keep. The type exists so a frontend can
    /// hold one without a `cfg` at the call site.
    pub struct Claim;

    impl Claim {
        pub fn take() -> Claim {
            Claim
        }

        pub fn reassert(&mut self) -> Option<u32> {
            None
        }

        pub fn drifts(&self) -> usize {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mode a Windows Terminal console is in while quik is drawing: the one
    /// measured on this machine, and the one the drift was seen against.
    const DRAWING: u32 = WINDOW_INPUT | MOUSE_INPUT | EXTENDED_FLAGS;

    #[test]
    fn vt_input_is_never_left_on() {
        assert_eq!(wanted(DRAWING | VT_INPUT), DRAWING);
    }

    #[test]
    fn the_event_bits_come_back_if_something_cleared_them() {
        // A process that turned VT input on usually cleared these on the way, and a
        // frontend that left them cleared stops seeing the pointer. This is the mode
        // a fresh child console starts in (`0x01f7`, measured), with VT input on and
        // window input gone: the two event bits that matter come back and the rest of
        // it is left as it was found rather than defaulted to something.
        let found = 0x01f7 | VT_INPUT;
        let got = wanted(found);
        assert_eq!(got & (WINDOW_INPUT | MOUSE_INPUT | EXTENDED_FLAGS),
                   WINDOW_INPUT | MOUSE_INPUT | EXTENDED_FLAGS);
        assert_ne!(got & VT_INPUT, VT_INPUT);
        // Whatever else it had, it still has: INSERT_MODE, QUICK_EDIT and 0x0100 are
        // not this module's to decide.
        assert_eq!(got & !(WINDOW_INPUT | MOUSE_INPUT | EXTENDED_FLAGS | VT_INPUT | NOT_RAW),
                   found & !(WINDOW_INPUT | MOUSE_INPUT | EXTENDED_FLAGS | VT_INPUT | NOT_RAW));
    }

    #[test]
    fn raw_mode_bits_are_cleared() {
        // LINE, ECHO and PROCESSED: what crossterm's raw mode takes away, and what
        // a child handing back a cooked console would give back.
        assert_eq!(wanted(0x0007 | WINDOW_INPUT | MOUSE_INPUT | EXTENDED_FLAGS), DRAWING);
    }

    #[test]
    fn nothing_else_is_invented() {
        // INSERT_MODE and QUICK_EDIT are the window's business, not this module's.
        let with_insert = DRAWING | 0x0020;
        assert_eq!(wanted(with_insert), with_insert);
    }

    #[test]
    fn asserting_an_already_right_mode_changes_nothing() {
        // Called every pass of the loop, so it has to converge rather than drift.
        assert_eq!(wanted(DRAWING), DRAWING);
        // Twice applied is once applied, whatever it was given.
        let once = wanted(0x01f7 | VT_INPUT);
        assert_eq!(wanted(once), once);
    }

    /// Every character of `text` through a fresh filter, as a field would keep it.
    fn typed(text: &str) -> String {
        let mut filter = Typing::default();
        text.chars().filter_map(|c| filter.keep(c)).collect()
    }

    #[test]
    fn a_mouse_report_leaves_nothing_behind() {
        // The shape that started this: one SGR mouse report per pointer move, which
        // is what arrives as characters when VT input is on.
        assert_eq!(typed("\x1b[<0;35;12M"), "");
        assert_eq!(typed("\x1b[<35;35;12M\x1b[<35;36;12M"), "");
    }

    #[test]
    fn honest_text_still_gets_through() {
        // A filter that eats punctuation is worse than the bug: these are the
        // characters a report is built from, typed by hand.
        assert_eq!(typed("[<0;35;12M"), "[<0;35;12M");
        assert_eq!(typed("a [ b ] c 0;1 M"), "a [ b ] c 0;1 M");
        assert_eq!(typed("cargo test --all-targets"), "cargo test --all-targets");
    }

    #[test]
    fn text_either_side_of_a_report_survives() {
        // The realistic frame: the report lands in the middle of what is being
        // typed, and both halves of the sentence have to still be there.
        assert_eq!(typed("ab\x1b[<35;7;9Mcd"), "abcd");
    }

    #[test]
    fn an_unterminated_sequence_gives_up_rather_than_eat_the_field() {
        let mut filter = Typing::default();
        assert_eq!(filter.keep('\x1b'), None);
        assert!(filter.mid_sequence());
        for _ in 0..SEQUENCE_CAP {
            filter.keep('0');
        }
        // Past the cap the run is abandoned, so typing works again.
        assert!(!filter.mid_sequence());
        assert_eq!(filter.keep('x'), Some('x'));
    }

    #[test]
    fn an_operating_system_command_ends_at_its_terminator() {
        // OSC 9 (a notification) is written by the frontend itself; one arriving in
        // the input has to be swallowed whole, both spellings of its end.
        assert_eq!(typed("\x1b]9;hi\x07after"), "after");
        assert_eq!(typed("\x1b]9;hi\x1b\\after"), "after");
    }

    #[test]
    fn a_newline_and_a_tab_are_text() {
        assert_eq!(typed("a\nb\tc"), "a\nb\tc");
    }
}
