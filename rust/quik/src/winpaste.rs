//! What Windows has already queued behind the key that was just read.
//!
//! The Windows console has no bracketed paste. A paste arrives as one key event
//! per character, and the newline in it is the same key a person presses to send
//! a message. Crossterm reads those records one at a time, so when the first
//! pasted newline is handled the rest of the paste is already waiting in the
//! console's input queue; asking the console how much is waiting is what tells
//! the two apart. A timer cannot: a slow frame looks exactly like fast typing.
//!
//! Only key-down records count. The console reports a release for every press,
//! so counting releases would make an ordinary keystroke look like a burst of
//! one. Modifier presses are left in on purpose: they arrive before the key they
//! modify, so a modifier can never sit *behind* the enter a person pressed.

use std::mem::zeroed;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{
    GetStdHandle, PeekConsoleInputW, INPUT_RECORD, KEY_EVENT, STD_INPUT_HANDLE,
};

/// How many key presses are queued behind the one just read.
///
/// A redirected stdin is not a console and the calls fail there; zero is the
/// answer that leaves typing exactly as it was.
pub fn pending_key_presses() -> usize {
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return 0;
    }
    // A paste's remaining characters are the first records in the queue, so a
    // short peek answers the only question asked here: is anything behind this
    // key? The full queue can be thousands of records long during a big paste,
    // and none of the rest changes the answer.
    let mut records: [INPUT_RECORD; 32] = unsafe { zeroed() };
    let mut read = 0u32;
    let ok = unsafe {
        PeekConsoleInputW(
            handle,
            records.as_mut_ptr(),
            records.len() as u32,
            &mut read,
        )
    };
    if ok == 0 {
        return 0;
    }
    records[..read as usize]
        .iter()
        .filter(|record| record.EventType == KEY_EVENT as u16)
        .filter(|record| unsafe { record.Event.KeyEvent.bKeyDown != 0 })
        .count()
}
