//! The console claim, against a real console.
//!
//! Two things make this not a normal test. It has to *have* a console, and it must
//! not be the console a frontend is drawing in -- those are different objects, and
//! which one a child sees depends on how it was started. Measured on this machine:
//! a process started with no console of its parent's (`CREATE_NO_WINDOW`, which is
//! how `tools/shell` starts everything and how `notebook/detached.py` starts this)
//! still opens `CONIN$`, and gets a console of its own (`0x01f7`, no window) rather
//! than the frontend's (`0x0098`, `WINDOW|MOUSE|EXTENDED`). Nothing here can write
//! to the frontend's mode, because it never holds that handle.
//!
//! Refuses to run without `CONSOLE_CLAIM_OK=1`, which only `notebook/claim.bat`
//! sets, and that is documented as run through `detached.py`. `cargo test` in a
//! terminal would otherwise be a test that drifts the mode of the terminal drawing
//! it -- the bug this work fixed, reproduced by its own proof.
//!
//! Ignored by default for the same reason, and its verdict goes to the file
//! `CONSOLE_CLAIM_LOG` names, since `cargo test` owns stdout.
//!
//! What it measures is the calls: the mode is read back out of the console after
//! each step rather than compared with what the code intended.

#![cfg(windows)]

use std::io::Write;

use cuacode::console::{self, Claim};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *const core::ffi::c_void,
                   disposition: u32, flags: u32, template: isize) -> isize;
    fn GetConsoleMode(handle: isize, mode: *mut u32) -> i32;
    fn SetConsoleMode(handle: isize, mode: u32) -> i32;
    fn CloseHandle(handle: isize) -> i32;
}

const GENERIC_READ_WRITE: u32 = 0x8000_0000 | 0x4000_0000;
const OPEN_EXISTING: u32 = 3;
const INVALID: isize = -1;

/// `CONIN$` as UTF-16 with its terminator, built at compile time so no allocation
/// sits between the caller and the call.
const CONIN: [u16; 7] = [b'C' as u16, b'O' as u16, b'N' as u16, b'I' as u16,
                         b'N' as u16, b'$' as u16, 0];

/// This process's console input handle, or `None` when it has none.
fn conin() -> Option<isize> {
    // SAFETY: a null security attribute and no template are the documented "defaults"
    // arguments to CreateFileW, and the name is a NUL-terminated static.
    let h = unsafe {
        CreateFileW(CONIN.as_ptr(), GENERIC_READ_WRITE, 0, core::ptr::null(),
                    OPEN_EXISTING, 0, 0)
    };
    (h != INVALID && h != 0).then_some(h)
}

fn mode_of(handle: isize) -> Option<u32> {
    let mut m = 0u32;
    // SAFETY: the handle came from `conin`, and `m` is a live local.
    (unsafe { GetConsoleMode(handle, &mut m) } != 0).then_some(m)
}

fn set(handle: isize, m: u32) -> bool {
    // SAFETY: as above, and the mode is one just read from that console.
    unsafe { SetConsoleMode(handle, m) != 0 }
}

fn log(line: &str) {
    eprintln!("{line}");
    if let Ok(path) = std::env::var("CONSOLE_CLAIM_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// The console to measure, or a skip when this is not the moment to measure one.
///
/// `CONSOLE_CLAIM_OK` is the whole guard: without it this test could be pointed at
/// a live frontend's console by an ordinary `cargo test`.
fn console_or_skip(what: &str) -> Option<isize> {
    if std::env::var("CONSOLE_CLAIM_OK").as_deref() != Ok("1") {
        log(&format!("SKIP {what}: CONSOLE_CLAIM_OK is not set (run notebook/claim.bat)"));
        return None;
    }
    let Some(handle) = conin() else {
        log(&format!("SKIP {what}: no console to measure"));
        return None;
    };
    if mode_of(handle).is_none() {
        log(&format!("SKIP {what}: CONIN$ is not a console here"));
        return None;
    }
    Some(handle)
}

#[test]
#[ignore = "drifts a console's input mode; run it with notebook/claim.bat"]
fn the_claim_puts_a_drifted_mode_back() {
    let Some(handle) = console_or_skip("the_drifted_mode") else { return };
    let found = mode_of(handle).expect("a console that was just read");
    log(&format!("console mode at start 0x{found:04x}"));

    // Claim::take uses the process's stdin handle, which in a frontend is this same
    // console. What is measured here is the mode of the console object, so the
    // claim and the reads agree about what they are talking to.
    let mut claim = Claim::take();
    let claimed = mode_of(handle).expect("a console that was just read");
    assert_eq!(claimed, console::wanted(found), "take() did not assert the mode");
    assert_eq!(claimed & console::VT_INPUT, 0, "VT input was left on by take()");
    log(&format!("after take() 0x{claimed:04x}"));

    // The drift, exactly as another process on this console would do it: the
    // measured 0x0098 -> 0x0298 of a child turning VT input on.
    assert!(set(handle, claimed | console::VT_INPUT), "could not simulate the drift");
    let dirty = mode_of(handle).expect("a console that was just read");
    assert_ne!(dirty, claimed, "the drift did not take");
    log(&format!("drifted to 0x{dirty:04x}"));

    let seen = claim.reassert();
    assert_eq!(seen, Some(dirty), "reassert did not report the mode it found");
    assert_eq!(mode_of(handle), Some(claimed), "reassert did not put the mode back");
    assert_eq!(claim.drifts(), 1);
    log(&format!("reassert put back 0x{:04x}", mode_of(handle).unwrap()));

    // And an undisturbed console is left alone, which is what makes this safe to
    // call on every pass of a frame loop.
    assert_eq!(claim.reassert(), None);
    assert_eq!(claim.drifts(), 1);
    log("second reassert: no drift, as it should be");

    set(handle, found);
    unsafe { CloseHandle(handle) };
    log("PASS the claim put a drifted mode back");
}

#[test]
#[ignore = "drifts a console's input mode; run it with notebook/claim.bat"]
fn take_clears_what_a_previous_process_left_on() {
    let Some(handle) = console_or_skip("the_dirty_console") else { return };
    let found = mode_of(handle).expect("a console that was just read");

    // A console handed over already dirty: VT input on, and a cooked mode that was
    // never taken out of. This is what a frontend started by a wrapper sees.
    let dirty = found | console::VT_INPUT | 0x0001 | 0x0002 | 0x0004;
    assert!(set(handle, dirty), "could not dirty the console");
    log(&format!("dirtied 0x{found:04x} -> 0x{:04x}", mode_of(handle).unwrap()));

    let claim = Claim::take();
    let now = mode_of(handle).expect("a console that was just read");
    assert_eq!(now & console::VT_INPUT, 0, "VT input survived take()");
    assert_eq!(now & 0x0007, 0, "cooked-mode bits survived take()");
    assert_eq!(claim.drifts(), 0, "a claim taken on a dirty console is not a drift");
    log(&format!("take() cleaned it to 0x{now:04x}"));

    set(handle, found);
    unsafe { CloseHandle(handle) };
    log("PASS take() cleaned a console a previous process left dirty");
}
