# CuaCode on Windows

CuaCode runs natively on Windows through Python, Rust/MSVC, and Win32 APIs. It
does not require WSL, a virtual machine, or an emulator.

## Requirements

- Windows 10 or 11
- Python 3.10 or newer, with `venv`
- Rust and the MSVC toolchain, only to build a frontend
- Visual Studio Build Tools with **Desktop development with C++**, when Rust
  needs to link the Windows CRT

## Install

From the repository root, one command does everything below:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

It finds Python 3.10 or newer (`CUACODE_PYTHON`, then `py -3`, then `python`),
creates `.venv`, installs `requirements.txt`, and checks that the worker's
imports resolve. It then builds the frontends through MSVC, one `--bin` per
cargo call, and keeps going when one fails, like `build.sh --keep-going`. Last,
it puts this checkout's `bin` directory on the user `PATH`, so `cuacode` works
from any directory. Nothing is reinstalled that is already there: an existing
`.venv` and existing binaries are reused.

```powershell
.\install.ps1 --no-build     python side only
.\install.ps1 --no-venv      install into the interpreter as found
.\install.ps1 -Help          the flags
```

## Install the Python side

By hand, from the repository root:

```powershell
py -3 -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

The frontends find this `.venv` automatically. `CUACODE_PYTHON` can point them
at another interpreter, and it wins over the checkout's environment.

## Build the frontends

`install.ps1` does this. By hand, from the repository root:

```powershell
cd rust
cargo build --release -p cuacode-quik --bin quik
cargo build --release -p cuacode-fastrs --bin fastrs
cargo build --release -p cuacode-rs --bin cuacode-doctor
cd ..
```

One `--bin` per cargo call: several in one call is the bug `doctor --upgrade`
had to be fixed for. The GPUI app crate is not on the Windows path yet.

The binaries are written to `rust\target\release\quik.exe` and
`rust\target\release\fastrs.exe`. The doctor binary is written to
`rust\target\release\cuacode-doctor.exe`.

`quik` is the compact terminal frontend. `fastrs` is the larger one. Both use
the same worker and the same session files. The remaining commands assume you
are back at the repository root.

## Launch from source

Set the worker and interpreter explicitly when launching an executable directly:

```powershell
$env:CUACODE_PYTHON = "$PWD\.venv\Scripts\python.exe"
$env:CUACODE_WORKER = "$PWD\main.py"
.\rust\target\release\quik.exe
```

For the `cuacode` command, put this checkout's `bin` directory on `PATH`:

```powershell
$env:PATH = "$PWD\bin;$env:PATH"
cuacode
```

With no arguments the launcher starts `quik`. Diagnostics do not open a
terminal UI:

```powershell
cuacode doctor
cuacode doctor --upgrade
```

`doctor` prints the build, checkout, update, worker, and state-root status and
exits. `--upgrade` fetches the remote's default branch and fast-forwards the
checkout onto it, then rebuilds the Windows frontends with Cargo; it does not
require WSL or Git Bash. It names the branch from the remote rather than from
the local branch's upstream, so a checkout cloned from a fork still updates
from the project. It moves whichever branch you are standing on, and refuses
rather than merging when that would not be a fast-forward. The running doctor is
staged as `cuacode-doctor.next.exe` and replaces itself before the next `doctor`
launch, because Windows cannot overwrite a running executable.

`CUACODE_CWD` selects the directory stamped on a new session. If it is unset,
the directory in which the frontend was started is used.

## Configuration

The first launch creates:

```text
%USERPROFILE%\.cuacode\config.json
```

Choose a provider and model in the `/provider` palette, or edit the file before
starting. API keys can also come from the environment variables documented in
the main README.

## Computer use

The Windows tools call Win32 directly for window discovery, screenshots, mouse,
keyboard, and scrolling. They do not need a desktop-accessibility permission
grant.

The agent and the target window must be on the same interactive desktop. Input
cannot cross into an elevated process unless CuaCode is running at the same
integrity level, and UAC's secure desktop is not part of a normal screenshot.
When snapping, the terminal occupies the left 30% of the usable desktop and the
window being driven occupies the right 70%; both stop above the taskbar. The
terminal is identified by the console this session is attached to, not by its
executable name, so a second terminal running the same app is left where it is.
A window the agent parks is also brought to the front, not merely resized: the
computer-use tools act on the window a person can see, and the keyboard follows
focus.

A process these tools start is given no console of its own, so it cannot write
into the terminal the frontend is drawing in. That matters most for the shell
tool: PowerShell prompts for a missing parameter on the console rather than on
the redirected stream, so a command such as an argument-less `Write-Output`
used to paint "Supply values for the following parameters" across the composer
and leave it there. It runs non-interactively now, which turns the prompt into
an error on stderr, and a launched app is detached from the console for the
same reason.

## Terminal notes

Windows' legacy console input API does not support the enhanced keyboard
protocol requested by the Unix builds. The Windows frontends skip that optional
request instead of failing at startup. `Ctrl+J` inserts a newline in the
composer; a modified Enter may not be distinguishable from plain Enter in every
Windows terminal.

The console input API also has no bracketed paste, so a paste arrives as one key
event per character, newlines included. `quik` asks the console whether more key
presses are queued behind the key it just read: an Enter with the rest of a
block waiting behind it is a line in the message, while an Enter with an empty
queue sends it. That keeps a multi-line paste in one message without guessing
from timing.

### AltGr characters

A layout that puts a character behind AltGr — `@` on `Q`, `{` on `7`, `€` on
`E` in German — could not be typed at all. Windows reports AltGr as `Ctrl+Alt`
plus the character the layout produced, and the frontends read any Ctrl as "this
is a binding", so the character was dropped before the composer saw it. Worse,
`@` or `/` at the start of a word opened a menu instead of being typed.

The frontends now treat `Ctrl+Alt` with a symbol or accented letter as AltGr:
that is the character the layout made, so it is typed, and it is never a
trigger. `Ctrl+Alt` with a plain letter, and the non-character keys (Backspace,
the arrows), still bind as before, because a binding is spelled that way and
AltGr's level carries none of those. The rule reads no operating system, but the
`Ctrl+Alt` encoding that makes it necessary is Windows' own.

### Typing `/` in a sentence

`/` at the start of a word opens the command menu, and the space after it used
to go into that menu as a filter, where it matched nothing — leaving the menu up
and owning the keyboard. Enter then reached a menu with no row under the cursor
and did nothing, so `something / something else` could not be sent.

A token ends at a space, so the menu about that token ends there too: a space at
the command menu closes it and is typed as text. Enter at the command menu with
nothing selected sends the message, since an unknown `/word` is prose. A menu
opened by a command has no line of its own and keeps its usual Enter.
