# CuaCode on Windows

CuaCode runs natively on Windows through Python, Rust/MSVC, and Win32 APIs. It
does not require WSL, a virtual machine, or an emulator.

## Requirements

- Windows 10 or 11
- Python 3.10 or newer, with `venv`
- Rust and the MSVC toolchain, only to build a frontend
- Visual Studio Build Tools with **Desktop development with C++**, when Rust
  needs to link the Windows CRT

## Install the Python side

From the repository root:

```powershell
py -3 -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

The frontends find this `.venv` automatically. `CUACODE_PYTHON` can point them
at another interpreter, and it wins over the checkout's environment.

## Build the frontends

```powershell
cd rust
cargo build --release -p cuacode-quik --bin quik
cargo build --release -p cuacode-fastrs --bin fastrs
cargo build --release -p cuacode-rs --bin cuacode-doctor
cd ..
```

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
exits. `--upgrade` performs a fast-forward pull and rebuilds the Windows
frontends with Cargo; it does not require WSL or Git Bash. The running doctor is
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
window being driven occupies the right 70%; both stop above the taskbar.

## Terminal notes

Windows' legacy console input API does not support the enhanced keyboard
protocol requested by the Unix builds. The Windows frontends skip that optional
request instead of failing at startup. `Ctrl+J` inserts a newline in the
composer; a modified Enter may not be distinguishable from plain Enter in every
Windows terminal.
