//! CuaCode: the app.
//!
//! The base of an agent orchestrator. A rail of projects (a directory the user
//! promoted) with the sessions that stand in each one, the session you are
//! looking at, and what that session is. No agent half yet: this draws the
//! shell, and the shell is meant to be worth looking at on its own.
//!
//! ```text
//!   cuacode                     open it
//!   cuacode --home <dir>        read a different state root
//!   cuacode --fold <name>       open with a project folded
//!   cuacode doctor              the build, the tree, and whether origin moved
//!   cuacode doctor --upgrade    pull and rebuild to answer that
//! ```
//!
//! What it reads, and the whole of what it reads: `~/.cuacode/projects.json` and
//! `~/.cuacode/sessions/*/meta.json`, exactly as `handler/session/store.py`
//! writes them. No worker, no process, no IPC: see `state.rs` for why.
//!
//! **Every surface is opaque**, and that is a decision with a reason: gpui-pre has no
//! per-element backdrop filter (its eight paint primitives include no such thing), so a
//! translucent composer could only ever show a sharp page through itself - transparency
//! with no blur. A window-level `Blurred` material does blur the *desktop*, but on this
//! platform it also makes every opaque token render see-through, so the rail showed the
//! app behind it. Solid ember, checked against a contrast target in `theme.rs`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::{Notification, NotificationDelivery, NotificationType};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit::component::WindowExt as _;

mod agent;
mod doctor;
mod md;
mod motion;
mod state;
mod theme;
mod view;

/// The name the headless render harness (`tests/shot.rs`) reaches the app through.
///
/// An integration test can only see this crate's **library**, and the `App` is declared
/// below as `pub` for exactly that reason. This module is one stable name in front of
/// it, so moving the declaration does not reach into the test.
mod app {
    pub use crate::{App, Brands, ChainAssets};
}

use view::icon;

/// The window's own actions, every one reachable from the menu bar and from its
/// key equivalent. A macOS app with no menu bar is an app you cannot drive, and
/// a `⌘` chord a focused control would otherwise eat reaches the menu first.
actions!(cuacode, [
    ToggleRail,
    ToggleSidebar,
    Reload,
    /// The editing verbs are performed by the OS off `os_action`; this exists so
    /// a menu item has an action to name, and does nothing by design.
    EditNoop,
]);

fn main() {
    real_main()
}

/// The app. A function rather than the body of `main` because this file is also the
/// crate's library (see `Cargo.toml`), and a `fn main` that the render harness does not
/// call must still be the entry point of the **bin** target.
fn real_main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `doctor` prints and leaves: no window, no worker, and nothing written to
    // the state root. It is what both frontends point at when they have news
    // about an update, so it has to work with the window being the problem.
    if args.first().map(String::as_str) == Some("doctor") {
        std::process::exit(doctor::run(&args[1..]));
    }
    // The worker's own main.py, which is the parent of the tree this binary was
    // built from. `CUACODE_WORKER` wins when it is set, which is what `run.sh`
    // pins, so a dev launch and a bundle agree.
    let worker = std::env::var_os("CUACODE_WORKER")
        .map(PathBuf::from)
        .or_else(|| {
            let here = std::env::current_exe().ok()?;
            // target/release/cuacode -> rust/ -> the tree root
            let root = here.parent()?.parent()?.parent()?.parent()?;
            let main = root.join("main.py");
            main.exists().then_some(main)
        })
        .unwrap_or_default();
    if let Some(home) = flag(&args, "--home") {
        // Set rather than passed down, so `state::state_root` stays the single
        // place that decides where the state root is.
        unsafe { std::env::set_var("CUACODE_HOME", home) };
    }
    let folded = flag(&args, "--fold");

    gpui_kit::application()
        // `LastWindowClosed`, not gpui's macOS default of `Explicit`. With
        // Explicit, closing the window leaves the process running with no window
        // at all: still in the Dock, with no way out but Force Quit. A window
        // that will not close is the worst bug an app like this can have.
        .with_quit_mode(QuitMode::LastWindowClosed)
        // Every icon in the window is a Lucide glyph, and a glyph that resolves
        // to nothing looks exactly like a layout bug.
        // The brand marks first, then the icon set: `Brands` answers for
        // `brands/*` and declines everything else, so the Lucide glyphs still
        // resolve through the set underneath.
        .with_assets(ChainAssets::new(Brands, gpui_kit::assets::AllAssets::new("")))
        .run(move |cx| {
            gpui_kit::init(cx);
            install_menu(cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("CuaCode".into()),
                    appears_transparent: true,
                    // The lights float over the top bar, at the same inset and
                    // in the same rung of the strip that superset puts them: a
                    // transparent titlebar with the controls at (14, 17).
                    traffic_light_position: Some(point(px(14.0), px(17.0))),
                }),
                // A **material**, so the desktop behind the window is blurred and every
                // translucent surface here is glass over it. `glass.rs` corrects which
                // material immediately after this opens: gpui-pre asks for `Selection`,
                // which stopped vending a backdrop layer on macOS 26, and
                // `UnderWindowBackground` is the one that still blurs.
                // **Opaque, and that is the decision.** A `Blurred` window buys a real
                // desktop blur, but on this platform it also makes every opaque token
                // render see-through - the rail and the transcript showed the app behind
                // them - and gpui-pre has no per-element backdrop filter, so the composer
                // could never have been blurred glass *over the conversation* anyway.
                // Solid colour is the honest answer: what is painted is what is seen.
                window_background: WindowBackgroundAppearance::Opaque,
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.0), px(820.0)),
                    cx,
                ))),
                window_min_size: Some(size(px(900.0), px(560.0))),
                ..Default::default()
            };
            let folded = folded.clone();
            let worker = worker.clone();
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| App::new(folded.clone(), worker.clone(), window, cx))
            })
            .expect("failed to open the window");
        });
}

/// Two asset sources as one: the first that answers, wins.
///
/// `with_assets` takes one source, and this window has two: the vendored brand
/// marks and the bundled Lucide set. A `None` means "not mine, ask the next one",
/// which is exactly the contract gpui's `AssetSource` defines, so chaining is a
/// try in order and nothing more.
pub struct ChainAssets<A: gpui_kit::AssetSource, B: gpui_kit::AssetSource>(A, B);

impl<A: gpui_kit::AssetSource, B: gpui_kit::AssetSource> ChainAssets<A, B> {
    /// Public because the harness builds the same chain the window does: a frame drawn
    /// without the brand marks is a frame missing a third of its icons.
    pub fn new(a: A, b: B) -> Self {
        Self(a, b)
    }
}

impl<A: gpui_kit::AssetSource, B: gpui_kit::AssetSource> gpui_kit::AssetSource
    for ChainAssets<A, B>
{
    fn load(&self, path: &str) -> gpui_kit::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(found) = self.0.load(path)? {
            return Ok(Some(found));
        }
        self.1.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        let mut out = self.0.list(path)?;
        out.extend(self.1.list(path)?);
        Ok(out)
    }
}

/// The brand marks, as an asset source.
///
/// gpui asks the application's `AssetSource` for a path and gets bytes back, so
/// the marks are **in the binary** rather than read from disk at paint time: a
/// provider list that has to touch the filesystem to draw its own icons is a list
/// that is wrong when the path is. `include_str!` is the whole mechanism, and the
/// path it serves is `brands/<key>.svg` whichever layout the crate is built to.
pub struct Brands;

impl gpui_kit::AssetSource for Brands {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let Some(key) = path.strip_prefix("brands/").and_then(|p| p.strip_suffix(".svg")) else {
            // Anything the component library wants falls through to the Lucide
            // set, which is registered underneath this one.
            return Ok(None);
        };
        Ok(BRAND_SVG
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, svg)| std::borrow::Cow::Borrowed(svg.as_bytes())))
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        Ok(BRAND_SVG
            .iter()
            .map(|(name, _)| format!("brands/{name}.svg").into())
            .filter(|p: &gpui_kit::SharedString| p.starts_with(path))
            .collect())
    }
}

/// Every vendored mark, as bytes. `state::BRANDS` names them; this is the same
/// list with the file's own contents in it, and the two are checked against each
/// other by a test rather than trusted to stay in step.
const BRAND_SVG: [(&str, &str); 24] = [
    ("ollama", include_str!("../assets/brands/ollama.svg")),
    ("openai", include_str!("../assets/brands/openai.svg")),
    ("anthropic", include_str!("../assets/brands/anthropic.svg")),
    ("deepseek", include_str!("../assets/brands/deepseek.svg")),
    ("nvidia", include_str!("../assets/brands/nvidia.svg")),
    ("openrouter", include_str!("../assets/brands/openrouter.svg")),
    ("together", include_str!("../assets/brands/together.svg")),
    ("huggingface", include_str!("../assets/brands/huggingface.svg")),
    ("claude", include_str!("../assets/brands/claude.svg")),
    ("gemini", include_str!("../assets/brands/gemini.svg")),
    ("google", include_str!("../assets/brands/google.svg")),
    ("meta", include_str!("../assets/brands/meta.svg")),
    ("qwen", include_str!("../assets/brands/qwen.svg")),
    ("mistral", include_str!("../assets/brands/mistral.svg")),
    ("groq", include_str!("../assets/brands/groq.svg")),
    ("grok", include_str!("../assets/brands/grok.svg")),
    ("cohere", include_str!("../assets/brands/cohere.svg")),
    ("kimi", include_str!("../assets/brands/kimi.svg")),
    ("moonshot", include_str!("../assets/brands/moonshot.svg")),
    ("minimax", include_str!("../assets/brands/minimax.svg")),
    ("zhipu", include_str!("../assets/brands/zhipu.svg")),
    ("glmv", include_str!("../assets/brands/glmv.svg")),
    ("fireworks", include_str!("../assets/brands/fireworks.svg")),
    ("perplexity", include_str!("../assets/brands/perplexity.svg")),
];

/// What a menu's foot says: the consequence of choosing, in one line.
///
/// A menu is a decision, and only two of these four decisions have a consequence
/// a person cannot see from the values. The other two say nothing rather than
/// filling the space.
fn menu_foot(menu: List) -> Option<&'static str> {
    match menu {
        List::Project => Some("A session's location is fixed once it has started."),
        List::Permissions => Some("Ask is the safe default; off never asks."),
        _ => None,
    }
}

/// The rows, as the shape the drawing wants. A `Vec` and a pass, rather than
/// making the menu builder aware of grouping.
fn into_rows(rows: Vec<state::MenuRow>) -> Vec<state::MenuRow> {
    rows
}

/// A popover's own chrome: one width, one edge, one shadow, and the arrival.
///
/// The chrome lives in one function now that the model's picker stacks a second
/// block inside the same card as the list: two cards drawn as two boxes is what a
/// popover with a slider under it must not look like. `body` is the inside.
///
/// Two shadows and no border: `float` puts it above the page, and `lift` is the 1px
/// inset white along its top - the light the material catches. That rim is what makes a
/// translucent plane read as glass; without it a tint is a flat patch, which is what the
/// first attempt here looked like.
fn card(body: Div) -> AnyElement {
    motion::arrive(
        motion::FAST,
        motion::OUT_EXPO,
        ElementId::Name("menu-card".into()),
        6.0,
        v_flex()
            .w(px(theme::MENU_W))
            .flex_shrink_0()
            .rounded(px(theme::R_LG))
            .bg(theme::popover())
            // Both shadows together: the float lifts it off the page, the rim is the
            // light along its top edge. A border would be a second edge beside the rim.
            .shadow(theme::float())
            .shadow(theme::lift())
            .p(px(4.0))
            .child(body),
    )
}

/// A mark per provider, so a list of nine is scannable. Not their key: a key is
/// an identifier and a mark is what you recognise without reading.
fn provider_icon(name: &str) -> IconName {
    match name {
        "ollama" => IconName::Server,
        "anthropic" => IconName::Sparkles,
        "openai" => IconName::Bot,
        "openrouter" => IconName::Waypoints,
        "deepseek" => IconName::Atom,
        "nvidia" => IconName::CircuitBoard,
        "together" => IconName::Boxes,
        "qubrain" => IconName::Brain,
        "Synthetic" => IconName::Hexagon,
        _ => IconName::Cloud,
    }
}

/// `--name value`, or `None`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// The menu bar. The two editing groups are the OS's (`os_action`), which is how
/// the system is told to perform a verb for whatever is focused.
fn install_menu(cx: &mut gpui_kit::App) {
    let file = Menu::new("File").items(vec![
        MenuItem::action("Reload Sessions", Reload),
    ]);
    let edit = Menu::new("Edit").items(vec![
        MenuItem::os_action("Undo", EditNoop, OsAction::Undo),
        MenuItem::os_action("Redo", EditNoop, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action("Cut", EditNoop, OsAction::Cut),
        MenuItem::os_action("Copy", EditNoop, OsAction::Copy),
        MenuItem::os_action("Paste", EditNoop, OsAction::Paste),
        MenuItem::os_action("Select All", EditNoop, OsAction::SelectAll),
    ]);
    let view = Menu::new("View").items(vec![
        MenuItem::action("Toggle Projects", ToggleRail),
        MenuItem::action("Toggle Sidebar", ToggleSidebar),
    ]);
    cx.set_menus(vec![file, edit, view]);

    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleRail, None),
        KeyBinding::new("cmd-l", ToggleSidebar, None),
        KeyBinding::new("cmd-r", Reload, None),
    ]);
}

/// A project folded shut, by whatever the row calls it.
type Folded = Vec<String>;

/// How many transcript rows the window reads and draws.
///
/// A ceiling, and a marked one: a session with ten thousand rows is one that
/// wants a scroller that reads as it goes. Reading the tail means the newest
/// work is what is on screen when the window opens, which is the right thing to
/// be wrong about in the meantime.
const ROWS: usize = 400;

/// The tick's nap while something is moving: 33ms is one frame at 30fps, which
/// is fast enough that a hover reads as a blend.
const TICK: Duration = Duration::from_millis(33);
/// The nap while nothing is. A settled window still has to notice a session
/// being written to, and four polls a second is plenty to notice that with, at
/// an eighth of the wakeups.
const IDLE: Duration = Duration::from_millis(250);
/// How often the selected directory's git state is re-read. A `git status` is
/// 45ms of subprocess, so it is on a clock and not on a frame, and four seconds
/// is faster than anyone reads a diff column.
const GIT_INTERVAL: Duration = Duration::from_secs(4);

/// What was read off disk last time, and the stamp it was read under.
///
/// This exists because the render path may not touch the disk. `render` runs 30
/// times a second while a hover is travelling, and a window whose every frame
/// parses a session record is a window that stutters under the pointer for no
/// reason a user can see. Read once, keyed on `(length, mtime)`, and a selection
/// change or a file that actually moved is the only thing that re-reads.
struct Cache {
    /// The session the two fields belong to.
    session: String,
    transcript: (Option<(u64, u64)>, Vec<state::Turn>),
    /// The same records, folded into the rows a person reads. Folded here rather
    /// than in `render`: it is a pass over every row, and a frame does not do a
    /// pass over every row.
    rows: Vec<state::Row>,
    plan: (Option<(u64, u64)>, Vec<(String, String)>),
    /// The directory's git state, the time it was last read, and what it said.
    /// A subprocess, so it is on its own slow interval and never in a frame.
    changes: (String, std::time::Instant, state::Changes),
    /// The file the sidebar is showing, what it is, and where it came from.
    diff: (String, String, state::Viewing),
    /// The directory's tracked tree, with the same stamp as its changes: both
    /// come from `git`, so they are read together and never separately.
    tree: (String, Vec<state::Node>, bool),
}

impl Cache {
    fn empty() -> Cache {
        Cache {
            session: String::new(),
            transcript: (None, Vec::new()),
            rows: Vec::new(),
            plan: (None, Vec::new()),
            changes: (String::new(), std::time::Instant::now(), state::Changes::default()),
            diff: (String::new(), String::new(), state::Viewing::Empty(String::new())),
            tree: (String::new(), Vec::new(), false),
        }
    }

    /// Bring both fields up to date for `id`, reading only what changed.
    ///
    /// Returns whether anything moved, which is what tells the clock whether to
    /// keep painting.
    fn sync(&mut self, home: &Path, id: &str) -> bool {
        if self.session != id {
            self.session = id.to_string();
            self.transcript = (None, Vec::new());
            self.rows = Vec::new();
            self.plan = (None, Vec::new());
        }
        let dir = home.join("sessions").join(id);
        let mut moved = false;
        let t_stamp = state::stamp(&dir.join("messages.jsonl"));
        if t_stamp != self.transcript.0 {
            self.transcript = (t_stamp, state::transcript(home, id, ROWS));
            self.rows = state::fold(&self.transcript.1);
            moved = true;
        }
        let p_stamp = state::stamp(&dir.join("todo.json"));
        if p_stamp != self.plan.0 {
            self.plan = (p_stamp, state::plan(home, id));
            moved = true;
        }
        moved
    }
}



/// Where each transcript row sits, in content coordinates: `(top, bottom)` per row.
///
/// Written by the transcript column's own `prepaint` listener - the only moment the
/// laid-out boxes exist - and read in the frame by the minimap and by a strip's jump.
/// A `prepaint` callback gets bounds and an `&mut App` and no context, so it cannot
/// reach `self`; this is the shared cell it writes instead.
type RowBounds = std::rc::Rc<std::cell::RefCell<Vec<(f32, f32)>>>;

/// Whether a scroller is at its end, from the two numbers that say so.
///
/// A free function so it can be tested without a GPU. The offset is negative as the
/// content moves up, so the end is `offset == -max`; a pixel of slack, because the
/// clamped offset and the rounded maximum can differ in the last decimal place after a
/// fractional layout. A scroller with no room to scroll is at its end, which is also the
/// answer before the first layout pass.
pub fn at_end(offset: f32, max: f32) -> bool {
    if max <= 0.0 {
        return true;
    }
    (-offset - max).abs() <= 1.0
}

/// One `git` read of a directory, done on its own thread.
///
/// The changes and the tree together, because they are the same question - what is in
/// this directory - asked of the same repository, and five subprocesses between them.
struct GitRead {
    dir: String,
    changes: state::Changes,
    /// `None` when the tree was already known for this directory and only the
    /// changes were due.
    tree: Option<(Vec<state::Node>, bool)>,
}

/// Whether the view follows new output, or holds still because the reader moved it.
///
/// This exists because the handle cannot answer the question on its own. `offset` and
/// `child_bounds` are the **last layout's** values, so a reader who has just scrolled may
/// still read as "at the foot" for a frame - and a write landing in that frame would
/// re-pin and snap them back down. That was the "after scrolling up it glitches itself
/// down", and it is not fixable by asking the handle more carefully: at that moment the
/// handle genuinely says the reader is at the foot.
///
/// So the reader's own gesture is the authority. A wheel **stops** following, and following
/// only resumes on a frame that had no wheel and found the view at the foot. That frame is
/// post-layout truth, so it is the reader actually being back at the bottom rather than a
/// stale read of where they used to be.
///
/// Whole rule, as a machine, so it can be tested without a GPU:
///
/// ```text
///   wheel       -> following off, and this frame's at-foot read is not trusted
///   frame(paint)-> if it was trusted and at-foot: following back on
///   record lands-> pull to the foot only while following
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Follow {
    /// Whether a record landing now should carry the view down with it.
    on: bool,
    /// When a wheel last arrived. **This is the fix for the snap-to-bottom bug.**
    ///
    /// The handle's offset is the *last layout's*, so the frame right after a wheel
    /// still reports the pre-wheel position - which reads as "at the foot" - and the
    /// old rule ("a frame that found the view at the foot re-arms following") handed
    /// it straight back. Since the clock naps at 250ms when nothing is moving, a
    /// record landing a second or two later called `scroll_to_bottom` on a view the
    /// reader had deliberately scrolled, and it snapped down. So a wheel now parks
    /// following for `SETTLE` past the last one, which is longer than any stale read
    /// can survive, and only a genuine arrival at the foot re-arms it.
    wheel_at: Option<Instant>,
    /// A landing is owed to the next frame. **One-shot**: gpui's `scroll_to_bottom` sets a
    /// flag its layout pass takes, so asking twice is asking again - and asking on every
    /// frame is what snapped the offset back to the foot and made scrolling up impossible.
    pin: bool,
}

/// How long after the last wheel the handle's read is trusted again. Longer than
/// one 250ms idle tick, so a nap landing mid-gesture cannot re-arm following.
const SETTLE: Duration = Duration::from_millis(400);

impl Follow {
    fn new() -> Follow {
        Follow { on: true, wheel_at: None, pin: false }
    }

    /// The reader moved the view: follow nothing until they have provably stopped
    /// and are provably back at the foot.
    fn scrolled(&mut self) {
        self.on = false;
        self.wheel_at = Some(Instant::now());
        self.pin = false;
    }

    /// A frame was drawn. `at_foot` is the handle's read for this frame, and it is
    /// the **honest** one now (see `App::scroll_at_bottom`): the offset has reached
    /// the maximum, so there is nothing below to scroll to.
    ///
    /// The settle window stays as a second guard rather than the only one. It costs
    /// nothing and it costs nothing to be wrong about, unlike the read it used to be
    /// compensating for.
    fn drew(&mut self, at_foot: bool) {
        let settled = self.wheel_at.is_none_or(|t| t.elapsed() > SETTLE);
        if at_foot && settled {
            self.on = true;
        }
    }

    /// Take the landing this frame owes, if it owes one.
    fn take_pin(&mut self) -> bool {
        std::mem::take(&mut self.pin)
    }

    /// Whether a record that just landed should pull the view down.
    fn follows(&self, moved: bool) -> bool {
        moved && self.on
    }
}

/// Every list the composer can open. One menu open at a time, and which one is
/// what decides where a click lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum List {
    Project,
    Provider,
    Model,
    Permissions,
}

pub struct App {
    ws: state::Workspace,
    /// What the composer may offer, out of the worker's own config.
    choices: state::Choices,
    /// Each project's icon token, from `projects.json`'s `icons`.
    icons: std::collections::HashMap<String, String>,
    /// The worker's own main.py, so its registry and catalogs can be asked.
    worker: PathBuf,
    /// What each provider actually serves, fetched off the UI thread: a network
    /// call per provider, so it is never on the render path nor on the tick.
    catalogs: std::collections::HashMap<String, Vec<String>>,
    /// The provider whose catalog is being fetched right now, so a second request
    /// for the same one does not start twice.
    fetching: Option<String>,
    /// Answers coming back from those fetches.
    fetched: (Sender<(String, Vec<String>)>, Receiver<(String, Vec<String>)>),
    /// The same shape for `git`, and for the same reason. `git status` and the
    /// tree walk are **five subprocesses**, 10-45ms of machine time, and they used
    /// to run inside the app's own tick - which is the thread that draws. The window
    /// froze for that long every four seconds, which is exactly the "weird lag
    /// spikes" it got. `git` does not know about frames and does not need to be asked
    /// on one, so it is asked on its own thread and the answer is applied by the tick,
    /// like the transcript and the catalog already are.
    git_in_flight: (Sender<GitRead>, Receiver<GitRead>),
    /// The directory a read is out for, so the four-second clock asks once. `None`
    /// when the window has nothing to ask about, which is also what stops a selection
    /// with no directory from asking forever.
    git_read: Option<String>,
    /// The composer's settings, as the window is holding them for the next run.
    /// They are the window's until a session starts, which is the one moment
    /// they become a session's.
    provider: String,
    model: String,
    permission: String,
    /// The directory the next session will stand in. Empty is the window's own
    /// default: no directory, which is a real choice and not an absence.
    project: Option<std::path::PathBuf>,
    /// Where the message is being typed.
    input: Entity<InputState>,
    _input_sub: Subscription,
    /// The menu that is open, if one is.
    menu: Option<List>,
    /// Where the pointer pressed to open it, in window coordinates. The menu is
    /// anchored here rather than to a corner of the window: a popover belongs under
    /// the control it came from, and the control is wherever the chip happens to be
    /// in a row that reflows.
    menu_at: Point<Pixels>,
    /// The rows of the open menu, built the moment it opened.
    ///
    /// A menu's rows are **not** free: the model list reads `meta.json` of every
    /// session on disk to order itself by what this account has actually run,
    /// which is 67ms over 459 sessions. Building them inside `render` meant that
    /// cost was paid on *every frame* while the menu was open - two seconds of
    /// CPU per second of having it open - which is what "the app feels like it is
    /// lagging every time I touch something" was. Built once, on the press, and
    /// the list is redrawn from the cache like everything else here.
    menu_rows: Vec<state::MenuRow>,
    /// The session the content area is showing, by id.
    sel: Option<String>,
    folded: Folded,
    rail_open: bool,
    sidebar_open: bool,
    /// What is on screen, read off disk once per change rather than per frame.
    cache: Cache,
    /// Whether the view follows new output, and whether the last frame is trusted.
    /// See [`Follow`]: the offset the handle reports is the last layout's, so a reader who
    /// has just scrolled still reads as "at the foot" and cannot be asked.
    ///
    /// A conversation opens at its **newest** row, which is the one you came
    /// back for, and this window opened at the oldest: the whole session was
    /// below the fold and the first thing on screen was an hour-old turn. The
    /// flag is set whenever the content changes underneath, and cleared once the
    /// scroll has been asked for.
    follow: Follow,
    /// Which runs are open, by index, and which session they are open in. Keyed
    /// on the session so switching away and back does not open yesterday's run
    /// in today's conversation.
    open_runs: (String, std::collections::HashSet<usize>),
    /// A minimap mark the reader pressed: the **row index** to bring to the top of
    /// the transcript. One-shot, like a pin, because `scroll_to_top_of_item` is a
    /// request the next layout takes.
    jump: Option<usize>,
    /// The transcript's own scroll offset. gpui needs to be handed this or a
    /// scroll has nowhere to live between frames.
    scroll: ScrollHandle,
    /// Where each row sits, from the last layout. See [`RowBounds`].
    row_bounds: RowBounds,
    /// The sidebar's width, and whether the pointer is dragging its edge. The
    /// width is the window's and persists across a session switch: a split you
    /// set is a split you meant. `side_right` is the window's own right edge in
    /// the same coordinates the pointer arrives in, so a drag can set the width
    /// outright instead of accumulating a delta that drifts.
    /// The rail's width and whether its edge is being dragged. Same shape as the
    /// sidebar's, because they are the same control on two edges.
    rail_w: f32,
    resizing_rail: bool,
    /// The width the pointer is *over* while a drag is live, and the width the
    /// window is actually laid out at.
    ///
    /// The split between them is the whole trick. A drag fires a mouse-move per
    /// **pixel**, and every one of those marks the window dirty, and gpui-pre has
    /// no partial invalidation: a re-render is the transcript (up to 400 rows,
    /// each its own element tree) plus the rail plus the tree. That is cheap once
    /// and expensive forty times a second.
    ///
    /// So the live drag only moves the divider - a 1px line at a new x - and the
    /// new width is *committed* on release. The window keeps up with the pointer
    /// and pays for a full layout once, at the end.
    drag_rail: Option<f32>,
    side_w: f32,
    side_right: f32,
    resizing_side: bool,
    /// The same preview width for the sidebar: what the pointer is over, not what
    /// the window is laid out at.
    drag_side: Option<f32>,
    /// Where the keyboard's cursor is in an open menu, if there is one. Held so
    /// the arrows and enter can drive the list the way a menu is expected to.
    menu_cursor: Option<usize>,
    /// Which sidebar tab: 0 changes, 1 files, 2 a file's diff.
    side_tab: usize,
    /// Folders the user has **opened**, by path. Everything starts shut.
    ///
    /// This used to be the other way round - a set of *closed* folders, open by
    /// default - and a rail of thirty headings with 461 sessions under them all at
    /// once is a wall you scroll past to find anything. A session tree is something
    /// you drill into, so the default shows the shape of your work, not every leaf.
    open_dirs: Vec<String>,
    /// The same for the Files tab's tree, and the same default: a checkout's tree
    /// starts as its top level, and you open the folder you want.
    open_tree_dirs: Vec<String>,
    /// Which of the three change categories are open.
    open_cats: [bool; 3],
    /// The agent half: one worker process, and what it is doing. `None` when no
    /// worker could be started at all, which is a window that draws its rail and
    /// says why it cannot run anything.
    agent: Option<agent::Agent>,
    /// Sessions with a turn that finished while you were looking somewhere else, by
    /// id. The rail marks these and nothing else does: this is the "waiting for you"
    /// half of t3code's row, and it is cleared by selecting the session.
    unseen: std::collections::HashSet<String>,
    /// The one clock, started at boot. Held so it is started once rather than
    /// per frame: a task spawned inside `render` is a task per frame.
    _tick: Task<()>,
    /// The session whose name is being edited in the composer's field, if any.
    ///
    /// A rename is a prompt, and this window has no modal: the composer's own field
    /// becomes the prompt, its placeholder says what is being named, and Enter commits
    /// it. The alternative - a second input box in a popover - is a form for a
    /// one-word answer.
    renaming: Option<String>,
    /// Whether origin has moved past this build, asked once and applied by the
    /// tick. A fetch is a network round trip; the window paints thirty times a
    /// second and never waits for one. Asked at boot only: an update does not
    /// arrive twice an hour, and a window left open all day is the same code
    /// either way.
    news: Option<cuacode::version::News>,
    news_in_flight: (Sender<cuacode::version::News>, Receiver<cuacode::version::News>),
}

impl App {
    /// Public because the render harness builds one against its own off-screen window.
    /// Everything else about the app is private and stays that way.
    pub fn new(folded: Option<String>, worker: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ws = state::Workspace::read();
        // The newest session opens selected: a window that arrives with nothing
        // selected says nothing at all, and the newest one is the one you were
        // last in.
        let sel = ws.first().map(|s| s.id.clone());
        let mut cache = Cache::empty();
        if let Some(id) = sel.as_deref() {
            cache.sync(&ws.home, id);
        }
        // The worker's path, so its registry can be asked what providers exist.
        // It is the only question this window asks Python, and it is asked once.
        let choices = state::choices(&ws.home, &worker);
        // What each provider has actually been run with, from the sessions on
        // disk: the honest half of the model list.
        let provider = choices.active.clone();
        let model = choices.model_of(&provider);
        let permission = choices.permission.clone();
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Ask, or tell this agent what to do\u{2026}")
        });
        let _input_sub = cx.subscribe_in(&input, window, |this: &mut App, _, ev, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                // In rename mode the field is a prompt, not a composer: the same Enter
                // commits the name rather than starting a run.
                if this.renaming.is_some() {
                    this.commit_rename(window, cx);
                } else {
                    this.send(window, cx);
                }
            }
        });
        let icons = state::icons(&ws.home);
        // The agent half: one worker, started here and held for the life of the
        // window. A worker that will not start is not a dead window: the rail is
        // read off disk and draws regardless, and the composer says why it cannot
        // run anything rather than looking like it worked.
        let boot_dir = sel
            .as_deref()
            .and_then(|id| ws.project_of(id))
            .map(|p| p.dir.clone());
        let agent = match agent::Agent::start(&ws.home, boot_dir.as_deref()) {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("no worker: {e}");
                None
            }
        };
        let mut app = App {
            ws,
            icons,
            // The provider the window opens on is fetched at boot, so its model
            // list is ready before anyone presses the chip.
            worker,
            catalogs: std::collections::HashMap::new(),
            fetching: None,
            fetched: std::sync::mpsc::channel(),
            git_in_flight: std::sync::mpsc::channel(),
            git_read: None,
            choices,
            provider,
            model,
            permission,
            project: None,
            input,
            _input_sub,
            menu: None,
            menu_at: point(px(0.0), px(0.0)),
            menu_rows: Vec::new(),
            sel,
            folded: Vec::new(),
            rail_open: true,
            sidebar_open: true,
            cache,
            follow: Follow::new(),
            open_runs: (String::new(), std::collections::HashSet::new()),
            jump: None,
            scroll: ScrollHandle::new(),
            row_bounds: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            rail_w: theme::RAIL,
            resizing_rail: false,
            drag_rail: None,
            side_w: theme::SIDE,
            side_right: 1280.0,
            resizing_side: false,
            drag_side: None,
            menu_cursor: None,
            side_tab: 0,
            open_dirs: Vec::new(),
            open_tree_dirs: Vec::new(),
            open_cats: [true, true, true],
            agent,
            unseen: std::collections::HashSet::new(),
            renaming: None,
            news: None,
            news_in_flight: std::sync::mpsc::channel(),
            _tick: Task::ready(()),
        };
        app.fetch_catalog(&app.provider.clone());
        app.ask_updates();
        // The window's four settings are the worker's, and the worker starts with
        // the config's. Told once here so a turn sent before a chip is moved still
        // runs on what the chips are showing; a chip moved later tells it again.
        app.tell_provider();
        app.tell_model();
        let mode = app.permission.clone();
        app.tell("permission.mode", serde_json::json!({ "mode": mode }));
        if let Some(name) = folded {
            app.folded.push(name);
        }
        // A blend has to be repainted while it travels, and the working dot
        // breathes on the same clock, so one task drives both. Two naps: a frame
        // while something is moving, and four polls a second while nothing is,
        // which is what notices a session being written to without a reload.
        //
        // `window` is **not** captured here: it is a borrow that lives for the
        // build, and a spawned task outlives it. The one thing the loop needs from
        // the window is whether it is in front, and that is asked on the spot below.
        app._tick = cx.spawn(async move |this, cx| {
            let mut nap = IDLE;
            // The last state of the worker's run, so a turn *ending* can be told from
            // one that never started. The falling edge is the moment that means "done,
            // and it is your turn now".
            let mut was_busy = false;
            // The id the worker was holding last nap, so following a newly-created
            // session is an edge rather than a state that re-fires forever.
            let mut last_held = String::new();
            loop {
                cx.background_executor().timer(nap).await;
                let alive = this
                    .update(cx, |app, cx| {
                        // The disk is checked here and not in `render`: a session
                        // being written while you watch it arrives on its own,
                        // and no frame pays for a read to make that happen.
                        let moved = match app.sel.clone() {
                            Some(id) => app.cache.sync(&app.ws.home, &id),
                            None => false,
                        };
                        // Follow the output, but only for a reader who is at the foot. See
                        // [`Follow`]: the decision is `moved && on`, and `on` is the reader's
                        // intent - put down by a wheel, picked up again by a frame that
                        // found them back at the bottom.
                        if app.follow.follows(moved) {
                            app.follow.pin = true;
                        }
                        // Git on its own thread and on its own slow clock: five
                        // subprocesses, so it is asked for and applied here rather than
                        // run here. The first call is what makes it due and returns at
                        // once; the answer lands on a later tick.
                        app.ask_git();
                        let git_moved = app.apply_git();
                        let catalog_moved = app.drain_catalogs();
                        let news_moved = app.apply_news();
                        // The model menu's rows are the catalog, so when it lands
                        // the open list is rebuilt once.
                        if catalog_moved {
                            if let Some(m) = app.menu {
                                app.menu_rows = app.build_menu_rows(m);
                            }
                        }
                        // The worker's own stream: what it is doing *now*, which
                        // no file knows. A run in flight is a window that keeps
                        // painting, because the whole point is watching it.
                        let worker_moved = match app.agent.as_mut() {
                            Some(a) => a.poll(),
                            None => false,
                        };
                        let busy = app.agent.as_ref().is_some_and(|a| a.live().busy);
                        // A turn just finished, or failed. Three things happen on that one
                        // falling edge, and they are the notification: the rail row is marked
                        // so its state reads "waiting on you"; an **OS notification** lands
                        // so it is seen with the window in the background; and a line is
                        // pushed into the window so it is seen with the app in front. The
                        // edge is the only place this is knowable, because `busy` is the
                        // worker's live state and not a fact on disk.
                        //
                        // The delivery is `InAppAndSystem`, which is t3code's
                        // `notifications` mode: a desktop app that only ever told you
                        // in-app is an app you have to be looking at.
                        if was_busy && !busy {
                            let held = app
                                .agent
                                .as_ref()
                                .map(|a| a.holding().to_string())
                                .unwrap_or_default();
                            let failed = app
                                .agent
                                .as_ref()
                                .map(|a| !a.live().error.is_empty())
                                .unwrap_or(false);
                            let elsewhere = !held.is_empty() && app.sel.as_deref() != Some(held.as_str());
                            if elsewhere {
                                app.unseen.insert(held.clone());
                            }
                            // A turn you are watching in the window it happened in needs
                            // no toast: the answer is already on screen in front of you.
                            // Whether the window is in front is asked of the app, because
                            // the task cannot hold a borrow of the window itself.
                            let behind = cx.active_window().is_none();
                            if !held.is_empty() && (elsewhere || behind) {
                                let title = app
                                    .session(&held)
                                    .map(|s| s.label())
                                    .unwrap_or_else(|| "A session".into());
                                let (body, kind) = if failed {
                                    let said = app
                                        .agent
                                        .as_ref()
                                        .map(|a| a.live().error.clone())
                                        .unwrap_or_default();
                                    (
                                        if said.is_empty() {
                                            "The turn failed.".to_string()
                                        } else {
                                            said
                                        },
                                        NotificationType::Error,
                                    )
                                } else {
                                    ("Finished. Your turn.".to_string(), NotificationType::Success)
                                };
                                // Pushed through the **window the app is in**, looked
                                // up rather than borrowed: the spawned task outlives the
                                // `&mut Window` this constructor was handed.
                                if let Some(handle) = cx.active_window() {
                                    let _ = handle.update(cx, |_, window, cx| {
                                        window.push_notification(
                                            Notification::new()
                                                .id1::<App>(format!("turn:{held}"))
                                                .title(title)
                                                .message(body)
                                                .with_type(kind)
                                                .placement(Anchor::TopRight)
                                                .delivery(NotificationDelivery::InAppAndSystem),
                                            cx,
                                        );
                                    });
                                }
                            }
                        }
                        was_busy = busy;
                        // The rail follows the worker: a session it just created is a
                        // folder the window has not listed yet, so the list is re-read
                        // and the new row selected. This is what makes the first message
                        // of a new conversation appear in the rail beside it.
                        let held = app
                            .agent
                            .as_ref()
                            .map(|a| a.holding().to_string())
                            .unwrap_or_default();
                        // **Once per new held id**, not once per nap. This branch used
                        // to re-run on every idle tick for as long as the worker held a
                        // session whose folder was not on disk, and each pass re-read the
                        // whole state root and set the pin. `last_held` makes it a rising
                        // edge, which is what it always meant: a session the worker has
                        // *just* started is a row the rail has not listed yet.
                        let newly_held = held != last_held;
                        last_held = held.clone();
                        let followed = if newly_held
                            && !held.is_empty()
                            && app.sel.as_deref() != Some(held.as_str())
                            && app.session(&held).is_none()
                        {
                            // The list is re-read, because a session the worker just
                            // created is a folder this window has not listed yet. The
                            // selection only moves to it if the folder is now real: a
                            // worker's boot session stands for nothing until something
                            // is said, and its folder is not born until the first
                            // record, so an empty id is a session with no row to land
                            // on and there is nothing to select.
                            app.reload_keeping(true, cx);
                            if app.session(&held).is_some() {
                                app.cache.sync(&app.ws.home.clone(), &held);
                                app.sel = Some(held.clone());
                                app.follow.pin = true;
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        // A frame's worth of the follow rule: if this frame found the view
                        // at the foot and no wheel arrived since the last one, the reader is
                        // back at the bottom and following resumes. See [`Follow`].
                        app.follow.drew(app.scroll_at_bottom());
                        let blending = motion::any_blend_alive();
                        // The rail catches up with a round that just landed.
                        //
                        // A session's `cwd` is written to `meta.json` with its first
                        // commit (`Session::set_cwd` writes nothing before a record
                        // exists), so a brand-new session sits in the rail under "No
                        // directory" until the round that makes its directory real
                        // lands. This is that moment: the transcript moved and the row
                        // we are showing still has no directory, so the list is read
                        // again and the row moves to its project. One reload per new
                        // session, and nothing on a settled window.
                        if moved {
                            let stale = app
                                .sel
                                .as_deref()
                                .and_then(|id| app.session(id))
                                .is_some_and(|s| s.cwd.trim().is_empty());
                            if stale {
                                app.reload_keeping(true, cx);
                            }
                        }
                        if moved
                            || git_moved
                            || catalog_moved
                            || news_moved
                            || worker_moved
                            || followed
                            || blending
                            || busy
                        {
                            cx.notify();
                        }
                        nap = if blending || busy { TICK } else { IDLE };
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });
        app
    }

    /// Re-read the state root. The window's only source of truth is the disk, so
    /// this is what a reload is: not a reconnect, just the same read again.
    ///
    /// `keep_empty` is for the one caller that re-reads *because a new session
    /// appeared*: there, and only there, "nothing selected" is a real state - it is
    /// the new conversation you are typing into - and re-selecting the newest row
    /// on disk would throw you out of it and into somebody else's session.
    fn reload_keeping(&mut self, keep_empty: bool, cx: &mut Context<Self>) {
        self.ws = state::Workspace::read();
        if self.sel.as_ref().is_some_and(|id| self.session(id).is_none()) {
            self.sel = None;
        }
        if self.sel.is_none() && !keep_empty {
            self.sel = self.ws.first().map(|s| s.id.clone());
        }
        // The selection may have moved, and the cache is keyed on it, so this is
        // what makes a reload actually show the new thing rather than the old.
        if let Some(id) = self.sel.clone() {
            self.cache.sync(&self.ws.home, &id);
        }
        cx.notify();
    }

    /// A manual reload. **The landing is asked for here**, by the one caller that means
    /// it, and not inside `reload_keeping`.
    ///
    /// `reload_keeping` used to set the pin itself, and that was the "it forces itself
    /// down" bug: the live-transcript tick calls it whenever the worker holds a session
    /// whose folder is not on disk yet, so every idle 250ms nap queued a
    /// `scroll_to_bottom` and dragged the reader back to the foot just after every
    /// attempted scroll. A function whose job is "re-read the state root" must not also
    /// decide where the reader is looking.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.reload_keeping(false, cx);
        self.follow.pin = true;
        self.follow.on = true;
    }

    fn session(&self, id: &str) -> Option<&state::Session> {
        self.ws
            .projects
            .iter()
            .flat_map(|p| p.sessions.iter())
            .chain(self.ws.other.iter())
            .find(|s| s.id == id)
    }

    fn selected(&self) -> Option<&state::Session> {
        self.sel.as_deref().and_then(|id| self.session(id))
    }

    fn is_folded(&self, name: &str) -> bool {
        !self.open_dirs.iter().any(|f| f == name)
    }

    fn toggle_fold(&mut self, name: &str, cx: &mut Context<Self>) {
        match self.open_dirs.iter().position(|f| f == name) {
            Some(i) => {
                self.open_dirs.remove(i);
            }
            None => self.open_dirs.push(name.to_string()),
        }
        cx.notify();
    }

    /// What a row's second line says, which is only ever the part its heading did
    /// not already say.
    fn caption(&self, project: Option<&state::Project>, session: &state::Session) -> state::Caption {
        state::caption(&session.cwd, project.map(|p| p.dir.as_path()))
    }

    /// Whether the session's location can still be changed.
    ///
    /// Only until the session has started. The directory is stamped on the
    /// session by the worker and it decides where every one of its tools runs,
    /// so moving it under a live session is not a setting, it is a lie about
    /// where the work already happened. The worker's own rule is the same one:
    /// `set_cwd` only takes a value before the first record.
    fn location_locked(&self) -> bool {
        self.selected().is_some()
    }

    /// What the model chip says: which model, and how hard it thinks.
    ///
    /// It is long, and the chip sits in a row of four, so the row can be tight.
    /// Cutting it to a leaf was tried and is worse: `flash` alone does not say
    /// which model, and a chip that hides the one thing it exists to show is not
    /// earning its width. The name is drawn in full and clipped by the chip's own
    /// box when it has to be.
    ///
    /// The effort level rides on it now that the level is set from inside this
    /// control rather than from a chip of its own: one control that says *which
    /// model, thinking this hard* is what replaced the pair.
    fn model_label(&self) -> String {
        let m = self.model.trim();
        let m = if m.is_empty() { "model" } else { m };
        let live = self.agent.as_ref().map(|a| a.live().effort).unwrap_or_default();
        if live.trim().is_empty() {
            m.to_string()
        } else {
            format!("{m} \u{b7} {}", live.trim())
        }
    }

    /// What the effort chip would say, in the worker's own word: `high effort`, or
    /// `default effort` for the empty level.
    ///
    /// Kept because it is the one place the empty level is spelled for a *chip*: the
    /// effort now rides on the model's own label, which says nothing for the default
    /// rather than `model · default`, and the track inside the picker does the naming.
    /// Dead until something wants a chip-shaped effort again.
    #[allow(dead_code)]
    fn effort_label(&self) -> String {
        let live = self.agent.as_ref().map(|a| a.live().effort).unwrap_or_default();
        match live.trim() {
            "" => "default effort".into(),
            other => format!("{other} effort"),
        }
    }

    /// What the project chip says.
    ///
    /// A session that is already standing somewhere says *there*, not where the
    /// next one would go: the chip is a caption on what you are looking at
    /// first, and an invitation second. With nothing selected it is the window's
    /// own choice for the next run, and no directory is a real choice rather
    /// than an absence, so it says so.
    fn project_label(&self) -> String {
        if let Some(id) = self.sel.as_deref() {
            if let Some(p) = self.ws.project_of(id) {
                return p.name.clone();
            }
            if let Some(s) = self.session(id) {
                if !s.cwd.trim().is_empty() {
                    return state::leaf(Path::new(&s.cwd));
                }
            }
            return "No directory".into();
        }
        match &self.project {
            Some(p) => state::leaf(p),
            None => "No directory".into(),
        }
    }

    /// Start a new conversation.
    ///
    /// Two things happen, and both are the point. The **selection clears**, which
    /// is what unlocks the composer's location chip: the worker only takes a
    /// directory before a session's first record, so a new conversation is the one
    /// moment the chip is a choice, and the rail having nothing selected is how the
    /// window says "this one is not written yet". And the **worker starts a fresh
    /// session** under this window's own settings, so the first message runs on
    /// what the composer is showing rather than on whatever was last run.
    ///
    /// Nothing is written until the first message: the worker creates the folder
    /// with the first record, so a new session opened and left alone leaves no
    /// empty session behind.
    fn new_session(&mut self, cx: &mut Context<Self>) {
        self.sel = None;
        self.side_tab = 0;
        self.cache = Cache::empty();
        self.follow = Follow::new();
        self.open_runs = (String::new(), std::collections::HashSet::new());
        let cwd = self.project.clone();
        if let Some(agent) = self.agent.as_mut() {
            if let Err(e) = agent.new_session(cwd.as_deref()) {
                eprintln!("new session: {e}");
            }
        }
        cx.notify();
    }

    /// Archive a session: gone from every listing, back by moving the folder in.
    ///
    /// The worker does the move (`session.archive`), because the store that owns
    /// the layout is the one that should change it - a frontend renaming a
    /// directory under it would be a second thing that knows the shape of
    /// `~/.cuacode`. The answer is a `status` with `archived`, and the rail is
    /// re-read on the next tick like any other change.
    ///
    /// A row the worker is holding cannot be abandoned mid-turn, so the archive is
    /// simply not offered for one: the click would be a lie.
    fn archive_session(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.agent.as_ref().is_some_and(|a| a.holding() == id && a.live().busy) {
            return;
        }
        self.tell("session.archive", serde_json::json!({ "id": id }));
        // Gone from the rail at once, rather than a tick late: the row the pointer
        // is on should not still be there when the click lands.
        self.ws = state::Workspace::read();
        if self.sel.as_deref() == Some(id) {
            self.sel = None;
            self.cache = Cache::empty();
        }
        self.unseen.remove(id);
        cx.notify();
    }

    /// Delete a session for good.
    ///
    /// The worker owns this (`session.delete` -> `store.delete`), because the store
    /// that laid the folder out is the one that may remove it. Nothing here reaches
    /// into `~/.cuacode` itself.
    ///
    /// The one session the window must not delete is the one a turn is running in:
    /// the worker is writing to that folder, and a delete under a live run is a
    /// folder removed while it is being appended to. A held session that is idle is
    /// fine - the worker closes it first.
    fn delete_session(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.agent.as_ref().is_some_and(|a| a.holding() == id && a.live().busy) {
            return;
        }
        self.tell("session.delete", serde_json::json!({ "id": id }));
        self.ws = state::Workspace::read();
        // A prompt naming a session that no longer exists would sit there with a blank
        // name and send a rename for a dead id, so it goes with the session.
        if self.renaming.as_deref() == Some(id) {
            self.renaming = None;
        }
        if self.sel.as_deref() == Some(id) {
            self.sel = None;
            self.cache = Cache::empty();
        }
        self.unseen.remove(id);
        cx.notify();
    }

    /// Rename a session: the prompt above the composer's field, because this window
    /// has no modal.
    ///
    /// A name set by hand is stamped `user` by the worker, which is what stops its
    /// own namer overwriting it later - so the worker is the one that has to be told,
    /// and this only asks.
    fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Prefilled with the name it has: a rename is an edit, and an empty box makes
        // you retype a name you were only there to fix.
        let current = self.session(id).map(|s| s.label()).unwrap_or_default();
        self.renaming = Some(id.to_string());
        self.menu = None;
        self.input.update(cx, |state, cx| {
            state.set_value(current, window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    /// Commit a rename from the field above the composer.
    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.renaming.clone() else { return };
        let title = self.input.read(cx).value().trim().to_string();
        // An empty name is not a rename, it is a cancel: the worker refuses one anyway,
        // and a row that cannot be told apart from its old self is worse than the old
        // name.
        if title.is_empty() {
            self.cancel_rename(window, cx);
            return;
        }
        self.tell("session.rename", serde_json::json!({ "id": id, "title": title }));
        // Written here as well as at the worker, because the rail's row is drawn from
        // `meta.json` and the worker's write lands on the next tick: a rename you
        // cannot see until then reads as one that did not take. The tick re-reads the
        // file and the worker's own stamp wins.
        self.ws = state::Workspace::read();
        self.cancel_rename(window, cx);
    }

    /// Leave rename mode, putting the field back to a composer.
    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = None;
        self.input.update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    /// Whether the worker is holding a conversation other than the one selected.
    ///
    /// A send from the middle of the rail is a conversation this worker is not
    /// holding, so it is loaded into it first. One live conversation at a time,
    /// and what the rail is showing says which one that is.
    fn needs_load(&self) -> Option<String> {
        let agent = self.agent.as_ref()?;
        let id = self.sel.as_deref()?;
        let held = agent.holding();
        // The worker's boot session has no id until it says one; a window that
        // has just opened is not "switching" to anything.
        if held.is_empty() || (held == id && agent.asked_for() == id) {
            return None;
        }
        (held != id).then(|| id.to_string())
    }

    /// Send what is in the field, or stop what is running.
    ///
    /// This is the one function that changes when a worker is wired in, which
    /// is what the base app always said it was. Three things decide what a press
    /// means, and the order matters: a live run makes it the Stop; a session the
    /// worker is not holding is loaded first; and everything else is a message.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Everything that reads `self` first, so the worker can be borrowed
        // mutably below without fighting it.
        let busy = self.agent.as_ref().is_some_and(|a| a.live().busy);
        if busy {
            if let Some(agent) = self.agent.as_mut() {
                let _ = agent.cancel();
            }
            cx.notify();
            return;
        }
        let text = self.input.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        // A send into a conversation the worker is not holding is a load first and
        // the message behind it.
        let load = self.needs_load();
        let cwd = self
            .sel
            .as_deref()
            .and_then(|id| self.ws.project_of(id))
            .map(|p| p.dir.clone())
            .or_else(|| self.project.clone());
        let Some(agent) = self.agent.as_mut() else {
            return;
        };
        if let Some(id) = load {
            let _ = agent.load(&id);
        }
        if let Err(e) = agent.chat(text.trim(), cwd.as_deref()) {
            eprintln!("send: {e}");
        }
        self.input.update(cx, |state, cx| state.set_value("", window, cx));
        self.menu = None;
        // The window follows a live run, which is what watching one means.
        self.follow.pin = true;
        cx.notify();
    }

    // ------------------------------------------------------------ the composer

    fn composer(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let locked = self.location_locked();
        let mut chips: Vec<AnyElement> = Vec::new();

        // The project. Locked the moment a session is selected: it is where that
        // session stands, not a setting about it.
        // One mark per chip, and every one of them says what the chip *is*:
        // where this runs, which account answers, which model, and what is
        // allowed. The provider mark is the provider's own (the same map the menu
        // rows use), so the chip and the list agree.
        let project_mark = if locked { IconName::LockKeyhole } else { IconName::Folder };
        chips.push(self.chip_el(List::Project, Some(project_mark), &self.project_label(), locked, cx));
        chips.push(self.chip_el(
            List::Provider,
            None,
            &self.provider,
            false,
            cx,
        ));
        // The model's mark is its provider's, because that is who serves it: a
        // gear beside `deepseek-v4.1-flash` says nothing about anything. An
        // unknown provider has no mark, so the chip loses it rather than wearing
        // a lookalike.
        chips.push(self.chip_el(List::Model, None, &self.model_label(), false, cx));
        chips.push(self.chip_el(
            List::Permissions,
            Some(IconName::ShieldCheck),
            &self.permission,
            false,
            cx,
        ));

        // The settings the worker has to be told before a turn: who answers, and
        // how hard it thinks. They are on the chips, so they are sent when a chip
        // changes rather than per keystroke; this is only the boot case, where the
        // window's choice has never reached the worker at all.
        let live = self.agent.as_ref().map(|a| a.live());
        let busy = live.as_ref().is_some_and(|l| l.busy);
        // Mid-run the control is the Stop, and it is the same control: one circle
        // at the end of the row, and what it does is what the run is doing.
        let send = motion::mark_on_hover(
            view::send_button(motion::hover_t("composer:send"), busy || self.can_send(cx), busy),
            "composer:send",
            cx.listener(|this, _, window, cx| this.send(window, cx)),
        )
        .into_any_element();
        let field = div()
            .flex_1()
            .min_w(px(0.))
            .child(Input::new(&self.input).appearance(false))
            .into_any_element();
        // What the worker is doing, and the one thing it can be waiting on a person
        // for. Drawn above the field inside the card, which is where a run's own
        // state belongs: it is about the message you are writing, not a banner.
        let status = self.status_line(cx);
        // In rename mode the field is a prompt and the card says so, because a field
        // that silently turns into something else is a field you type a message into by
        // mistake. The card wears an accent edge while it is a prompt, which is the one
        // other place this window uses the accent for a control.
        let status = match self.renaming.as_deref() {
            Some(id) => {
                let name = self.session(id).map(|s| s.label()).unwrap_or_default();
                Some(view::rename_note(&name))
            }
            None => status,
        };
        view::composer(
            field,
            chips,
            send,
            self.input.focus_handle(cx).is_focused(window),
            status,
            self.renaming.is_some(),
        )
        .into_any_element()
    }

/// The line above the field: what the run is doing, or what it failed at.
///
/// Nothing here is invented. An empty line is an idle worker; the pulse is a
/// run in flight, which the worker's own state said; and the prompt is the
/// question the worker is blocked on, which cannot be answered from anywhere
/// else in this window.
    fn status_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let live = self.agent.as_ref()?.live();
        if let Some(ask) = live.asking {
            return Some(view::ask_row(
                &ask,
                cx.listener(|this, _, _, cx| this.answer(true, cx)),
                cx.listener(|this, _, _, cx| this.answer(false, cx)),
            ));
        }
        if !live.error.is_empty() {
            return Some(view::run_note(&live.error, view::RunTone::Failed));
        }
        live.busy.then(|| view::run_note(&self.phase_word(), view::RunTone::Working))
    }

    /// The worker's phase, as a word a person reads.
    ///
    /// The worker says `running` and `tools`. Those are its own names for its own
    /// states, and the status line used to print them raw - `Working… running` is a
    /// machine describing itself in its own vocabulary. A status line is for the
    /// person waiting, so it is a sentence: what is happening, now.
    fn phase_word(&self) -> String {
        let phase = self.agent.as_ref().map(|a| a.live().phase).unwrap_or_default();
        match phase.as_str() {
            "tools" | "tooling" => "Running a tool\u{2026}".into(),
            "running" => "Thinking\u{2026}".into(),
            _ => "Working\u{2026}".into(),
        }
    }

    /// Whether the field has anything in it. The send control is filled on this
    /// and not on "a session is selected": an accent send beside an empty field
    /// is a button that does nothing.
    fn can_send(&self, cx: &Context<Self>) -> bool {
        !self.input.read(cx).value().trim().is_empty()
    }

    /// Answer the question the worker is blocked on. The worker is waiting, so
    /// this is the one thing on screen that unblocks it.
    fn answer(&mut self, allow: bool, cx: &mut Context<Self>) {
        if let Some(a) = self.agent.as_mut() {
            let _ = a.answer(allow);
        }
        cx.notify();
    }

    /// One setting chip, with its own hover clock and its own menu.
    fn chip_el(
        &self,
        menu: List,
        mark: Option<IconName>,
        value: &str,
        locked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // A provider chip wears the provider's own logo, not a glyph that
        // resembles one. That is the whole point of the `brands/` set.
        let brand = match menu {
            List::Provider => Some(self.provider.clone()),
            List::Model => Some(self.provider.clone()),
            _ => None,
        };
        let key = format!("chip:{menu:?}");
        let t = motion::hover_t(&key);
        let open = self.menu == Some(menu);
        let body = match brand {
            Some(p) => {
                let unkeyed = self
                    .choices
                    .provider(&p)
                    .map(|row| !row.keyed)
                    .unwrap_or(false);
                view::chip_brand(&p, value, if open { 1.0 } else { t }, locked, unkeyed)
            }
            None => view::chip(mark, value, if open { 1.0 } else { t }, locked),
        };
        motion::mark_on_hover(
            body,
            &key,
            cx.listener(move |this, ev: &ClickEvent, _, cx| {
                if locked {
                    return;
                }
                // Where the pointer pressed the chip, which is where the menu is
                // anchored. The click carries its own position, so the menu lands
                // **under the chip that opened it** rather than at some fixed corner
                // of the window - which is what it did, and why it read as a panel
                // dropped in the middle of the transcript.
                this.menu_at = ev.position();
                this.menu = if this.menu == Some(menu) { None } else { Some(menu) };
                // Built here, on the press: once per opening, not once per frame.
                this.menu_rows = match this.menu {
                    Some(m) => this.build_menu_rows(m),
                    None => Vec::new(),
                };
                // A menu that needs a list the window does not have yet asks for
                // it here, once, and goes on drawing while it arrives.
                if this.menu == Some(List::Model) {
                    let provider = this.provider.clone();
                    this.fetch_catalog(&provider);
                }
                cx.notify();
            }),
        )
        .into_any_element()
    }

    /// The list a chip opens: `(what to draw, what it does)`.
    ///
    /// One builder for every menu, because they are the same control with
    /// different rows: a project, a provider, a model, a permission mode.
    /// The list a chip opens: what each row says, and its own mark.
    ///
    /// One builder for every menu, because they are the same control with
    /// different rows. Every row carries an icon: a list of nine bare words is a
    /// list you have to read, and the mark is what makes it scannable.
    fn build_menu_rows(&self, menu: List) -> Vec<state::MenuRow> {
        match menu {
            // Where the next session stands. The window's own default first,
            // because "no directory" is a real choice and the one most runs take
            // when you are not in a checkout.
            List::Project => {
                let mut rows = vec![state::MenuRow::new("No directory", "run anywhere")
                    .current(self.project.is_none())];
                for p in self.ws.projects.iter() {
                    rows.push(
                        state::MenuRow::new(p.name.clone(), state::short_path(&p.dir))
                            .current(self.project.as_deref() == Some(p.dir.as_path())),
                    );
                }
                if self.ws.projects.is_empty() {
                    // An empty menu has to say why it is empty, or it reads as a
                    // control that failed to open.
                    rows.push(
                        state::MenuRow::new("No projects yet", "add one from the rail's footer")
                            .enabled(false),
                    );
                }
                rows
            }
            // Which account answers. One that has no key is *listed and not
            // pickable*, with a sentence saying what to do about it: hiding it
            // makes the list look short, and offering it makes the click fail.
            List::Provider => {
                let mut rows: Vec<state::MenuRow> = self
                    .choices
                    .providers
                    .iter()
                    .map(|p| {
                        let host = state::HOSTS
                            .iter()
                            .find(|(n, _)| *n == p.name)
                            .map(|(_, h)| (*h).to_string())
                            .unwrap_or_default();
                        let mut row = state::MenuRow::new(p.name.clone(), host)
                            .current(p.name == self.provider)
                            .enabled(p.keyed)
                            .brand(state::brand(&p.name));
                        if !p.keyed {
                            // In words a person can act on, and the variable's
                            // name after them rather than instead of them.
                            row.note = format!("{} \u{b7} no key yet", row.note);
                        }
                        row
                    })
                    .collect();
                // The one you are on leads, so the list opens on the answer to
                // "what am I using" rather than at the top of an alphabet.
                rows.sort_by_key(|r| !r.current);
                rows
            }
            // Which model. Grouped, because the difference between the twelve
            // models this account has run and the four hundred and fifty an
            // aggregator serves is the whole question this menu answers.
            List::Model => {
                let rows: Vec<state::MenuRow> =
                    state::model_rows(
                        &self.ws.home,
                        &self.provider,
                        self.catalogs.get(&self.provider).map(Vec::as_slice).unwrap_or(&[]),
                    )
                    .into_iter()
                    .map(|r| {
                        let note = if !r.caps.is_empty() { r.caps } else { r.age };
                        state::MenuRow::new(r.name.clone(), note)
                            .current(r.name == self.model)
                            .group(if r.used_here { "Used on this account" } else { "Available" })
                    })
                    .collect();
                if rows.is_empty() {
                    let waiting = self.fetching.as_deref() == Some(self.provider.as_str());
                    return vec![state::MenuRow::new(
                        if waiting { "Reading the catalog…".to_string() } else { self.model.clone() },
                        if waiting {
                            format!("asking {} what it serves", self.provider)
                        } else if self.catalogs.contains_key(&self.provider) {
                            format!("{} serves nothing this account can use", self.provider)
                        } else {
                            String::new()
                        },
                    )
                    .enabled(!waiting)];
                }
                rows
            }
            List::Permissions => state::PERMISSIONS
                .iter()
                .map(|(name, note)| {
                    state::MenuRow::new(*name, *note).current(*name == self.permission)
                })
                .collect(),
        }
    }

    /// The model menu, plus the one setting that belongs to the model rather than to
    /// the account: how hard it thinks.
    ///
    /// The shape this follows: the model's picker carries the effort level **as a slider**
    /// rather than as a fifth chip beside it, which is t3code's `ComposerSurface` -
    /// the model control opens, and the reasoning rung is set from inside it.
    fn model_card(&self, cx: &mut Context<Self>) -> AnyElement {
        card(v_flex()
            .w_full()
            .gap(px(1.0))
            .child(self.menu_body(List::Model, cx))
            .child(self.effort_card(cx)))
    }

    /// How hard this conversation thinks, drawn as the ladder the worker owns.
    ///
    /// The worker's vocabulary, not this window's: five rungs (`handler/agent/effort.py`),
    /// and a sixth value that is not a rung at all - the empty level, which is the
    /// model's own default. So an empty setting lights nothing and the track says so,
    /// rather than pretending its lowest rung is in force.
    ///
    /// It is a **track and not a knob** on purpose: gpui-pre has no slider control and
    /// no element rotation, so a segment lit for every rung up to the one in force is
    /// the shape a slider can honestly take here.
    fn effort_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self.agent.as_ref().map(|a| a.live().effort).unwrap_or_default();
        // The worker's own five, and the empty default is a sixth value that is not
        // on the ladder: `state::EFFORT` writes it first, so the rungs are its tail.
        let rungs: Vec<(&str, &str)> = state::EFFORT.iter().skip(1).copied().collect();
        let at = rungs.iter().position(|(n, _)| *n == live.trim());
        let note = match at {
            Some(i) => rungs[i].1,
            None => "the model's own default for this model",
        };
        let word = if live.trim().is_empty() { "default".to_string() } else { live.trim().to_string() };

        let mut track = h_flex().w_full().h(px(5.0)).gap(px(3.0));
        for (i, (name, _)) in rungs.iter().enumerate() {
            let filled = at.is_some_and(|a| i <= a);
            let key = format!("effort:{name}");
            let t = motion::hover_t(&key);
            let level = (*name).to_string();
            track = track.child(
                motion::mark_on_hover(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .rounded(px(3.0))
                        .child(view::effort_seg(filled, t)),
                    &key,
                    cx.listener(move |this, _, _, cx| {
                        this.set_effort(&level);
                        cx.notify();
                    }),
                ),
            );
        }

        let mut card = v_flex()
            .w_full()
            .gap(px(theme::SPACE_XS))
            .pt(px(theme::SPACE_SM))
            .px(px(theme::SPACE_SM))
            .child(div().w_full().h(px(1.0)).bg(theme::line()))
            .child(
                h_flex()
                    .w_full()
                    .pt(px(theme::SPACE_XS))
                    .items_center()
                    .gap(px(theme::SPACE_SM))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(theme::SMALL))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::faint())
                            .child("How hard it thinks"),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(theme::SMALL))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if at.is_some() { theme::accent() } else { theme::ghost() })
                            .child(word),
                    ),
            )
            .child(track)
            .child(
                div()
                    .w_full()
                    .text_size(px(theme::MICRO))
                    .text_color(theme::ghost())
                    .child(note.to_string()),
            );
        // The empty level is a real setting and the way back to it, so it gets a
        // control rather than being unreachable once a rung has been picked.
        if at.is_some() {
            let key = "effort:reset";
            let t = motion::hover_t(key);
            card = card.child(motion::mark_on_hover(
                div()
                    .w_full()
                    .mt(px(2.0))
                    .py(px(3.0))
                    .rounded(px(theme::R_CHIP))
                    .text_size(px(theme::MICRO))
                    .text_color(motion::mix(theme::ghost(), theme::text_soft(), t))
                    .bg(motion::mix(theme::ink(0.0), theme::wash_hover(), t))
                    .child("Use the model's own default"),
                key,
                cx.listener(|this, _, _, cx| {
                    this.set_effort("");
                    cx.notify();
                }),
            ));
        }
        card.into_any_element()
    }

    /// The open menu, drawn over the window above the composer.
    ///
    /// Their `DropdownMenuContent`, quoted: `bg-popover text-popover-foreground
    /// rounded-lg border p-1 shadow-md`, items `relative flex items-center gap-2
    /// rounded-sm px-2 py-1.5 text-sm` with `focus:bg-accent`, and the whole
    /// thing arriving on `animate-in fade-in-0 zoom-in-95 slide-in-from-top-2` at
    /// 150ms.
    ///
    /// What the *ux* added, and what a list of names was missing:
    ///
    ///   - a heading on every menu, saying what the list is choosing. `Provider`
    ///     is a word; nine hosts with no heading is a list you have to interpret.
    ///   - groups, when one is meaningful. The model list is 12 and 450 at once,
    ///     so it says which is which.
    ///   - a row you cannot pick **looks like one**: dimmed, with a sentence for
    ///     a note instead of a host, so eight unusable rows read as eight
    ///     unconfigured providers and not as eight failures of this window.
    ///   - the foot says what will happen when you choose. A menu is a decision,
    ///     and the two things that decide here are the provider and the
    ///     directory.
    fn menu_card(&self, menu: List, cx: &mut Context<Self>) -> AnyElement {
        card(self.menu_body(menu, cx))
    }

    /// The inside of a menu: a heading, the rows, and the foot. No chrome, so the
    /// model's own card can stack the effort track under the same frame.
    fn menu_body(&self, menu: List, cx: &mut Context<Self>) -> Div {
        let rows = self.menu_rows.clone();
        let title = match menu {
            List::Project => "Where this session runs",
            List::Provider => "Which account answers",
            List::Model => "Which model",
            List::Permissions => "What it may do without asking",
        };
        let mut col = v_flex().w_full().gap(px(1.0)).child(view::menu_heading(title));

        // Grouped: a heading per group, in the order the rows arrive, and a
        // group with no rows draws nothing.
        //
        // The rows live in a **scrolling** box, and that is the fix for the menu
        // that covered the window: the model list is this account's dozen plus an
        // aggregator's four hundred, and laying all of them out at full height put
        // a slab over the transcript. A popover is capped and scrolls; a cap
        // without a scroller just clips the list instead.
        let mut list = v_flex()
            .id(ElementId::Name(format!("menu-scroll:{menu:?}").into()))
            .w_full()
            .max_h(px(theme::MENU_MAX_H))
            .overflow_y_scroll()
            .gap(px(1.0));
        let mut last_group: Option<String> = None;
        for row in into_rows(rows) {
            if row.group != last_group {
                if let Some(g) = row.group.as_deref() {
                    list = list.child(view::menu_group(g));
                }
                last_group = row.group.clone();
            }
            let key = format!("menu:{:?}:{}", menu, row.value);
            let t = motion::hover_t(&key);
            let value = row.value.clone();
            let enabled = row.enabled;
            list = list.child(motion::mark_on_hover(
                view::menu_item(&row, t, self.menu_cursor.is_some_and(|_| false)),
                &key,
                cx.listener(move |this, _, _, cx| {
                    if !enabled {
                        return;
                    }
                    this.choose(menu, &value, cx);
                }),
            ));
        }
        col = col.child(list);

        // The foot: what choosing does, named. Their own menubar has no such
        // row; this window needs one because the two decisions here are not
        // obvious from the values alone.
        if let Some(foot) = menu_foot(menu) {
            col = col.child(view::menu_foot(foot));
        }

        col
    }

    /// Whether a row of a menu is the one the setting is on. The model's picker marks
    /// its own current row from `List::Model` above; this is the ladder for anything
    /// built from a `(value, note)` table, and it is unused today.
    #[allow(dead_code)]
    fn menu_value(&self, menu: List, value: &str) -> bool {
        match menu {
            List::Project => self.project_label() == value,
            List::Provider => self.provider == value,
            List::Model => self.model == value,
            List::Permissions => self.permission == value,
        }
    }

    /// Take a row of a menu: write the setting, and close the list.
    ///
    /// The window's settings are the worker's settings. A chip that changes one
    /// here and not there is a chip that lies about who is about to answer, so
    /// every branch that touches the provider, the model or the permission mode
    /// tells the worker too. There is no second command: the worker has one
    /// action per setting, and they are the four the chips set.
    fn choose(&mut self, menu: List, value: &str, cx: &mut Context<Self>) {
        match menu {
            List::Project => {
                self.project = if value == "No directory" {
                    None
                } else {
                    self.ws
                        .projects
                        .iter()
                        .find(|p| p.name == value)
                        .map(|p| p.dir.clone())
                };
                // A new project means the next thing sent stands somewhere else.
                // Where a session's directory is decided is the `terminal`
                // envelope, sent with the message, so there is nothing to tell the
                // worker here: this is a choice about the next session, and the
                // next session gets it when it starts.
            }
            List::Provider => {
                self.provider = value.to_string();
                // The model belongs to the provider, so it follows rather than
                // staying on a name the new account may not serve.
                let m = self.choices.model_of(value);
                self.model = if m.is_empty() { self.model.clone() } else { m };
                // `provider.use` switches the account and rebuilds the history in
                // the new provider's dialect; it carries the model with it.
                self.tell_provider();
            }
            List::Model => {
                self.model = value.to_string();
                // A model is set on the provider it belongs to: `provider.set`
                // writes the choice back to the config, which is what makes it
                // stick for the next conversation too.
                self.tell_model();
            }
            List::Permissions => {
                self.permission = value.to_string();
                self.tell("permission.mode", serde_json::json!({ "mode": value }));
            }
        }
        self.menu = None;
        cx.notify();
    }

    /// Set how hard this conversation thinks, in the worker's own vocabulary.
    ///
    /// The label is `Default` for the empty level, so the empty string is what the
    /// row carries; the worker reads "" as *the model's own*. Set at once, so the
    /// track answers on this frame rather than on the tick the worker's confirmation
    /// lands: the worker is still the owner, and its own reply overwrites this with
    /// the level it actually took. A level it refuses (an `off` on a model that
    /// cannot stop thinking) settles back to what is in force.
    fn set_effort(&mut self, level: &str) {
        self.tell("session.effort", serde_json::json!({ "effort": level }));
        if let Some(a) = self.agent.as_mut() {
            a.set_effort_local(level);
        }
    }

    /// Tell the worker one thing. A send that fails is said, not swallowed: a
    /// chip that did not reach the worker is a chip showing a setting that is not
    /// in force.
    fn tell(&mut self, action: &str, fields: serde_json::Value) {
        if let Some(a) = self.agent.as_mut() {
            if let Err(e) = a.command(action, fields) {
                eprintln!("worker: {action}: {e}");
            }
        }
    }

    fn tell_provider(&mut self) {
        let name = self.provider.clone();
        self.tell("provider.use", serde_json::json!({ "name": name }));
    }

    fn tell_model(&mut self) {
        let name = self.provider.clone();
        let model = self.model.clone();
        self.tell("provider.set", serde_json::json!({ "name": name, "model": model }));
    }

    // ------------------------------------------------------------ the rail

    fn project(&self, dir: &Path) -> Option<&state::Project> {
        self.ws.projects.iter().find(|p| p.dir == dir)
    }

    fn rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let total = self.ws.shown();
        let mut list = v_flex().w_full().pb(px(theme::SPACE_MD));
        let mut rows: Vec<AnyElement> = Vec::new();

        // Borrowed, not cloned. This used to be `self.ws.projects.clone()` and
        // `self.ws.other.clone()`, which copied every project and every session
        // *per frame* just to walk them - and `render` runs on every pixel of a
        // resize drag.
        let projects = &self.ws.projects;
        let others = &self.ws.other;
        for project in projects {
            let name = project.name.clone();
            let dir = project.dir.clone();
            // The fold is keyed on the **directory**, not the name: two loose
            // headings can share a leaf (`Work/x` and `demo/x`), and folding one by
            // name would fold both.
            let fold_key = project.dir.display().to_string();
            let head_key = format!("ph:{fold_key}");
            let plus_key = format!("pp:{fold_key}");
            let open = !self.is_folded(&fold_key);
            // A loose directory is a heading and nothing else, so it has no plus:
            // promoting it is not something this window offers from here, and a
            // dead control on a row is the thing to avoid.
            let mark = project.promoted.then(|| state::Icon::of(
                self.icons
                    .get(&project.dir.display().to_string())
                    .map(String::as_str)
                    .unwrap_or(""),
            ));
            rows.push(
                motion::mark_on_hover(
                    view::project_head(
                        &project,
                        mark.as_ref(),
                        open,
                        motion::hover_t(&head_key),
                        motion::hover_t(&plus_key),
                    ),
                    &head_key,
                    cx.listener(move |this, _, _, cx| this.toggle_fold(&fold_key, cx)),
                )
                .into_any_element(),
            );
            if open {
                for s in project.sessions.iter() {
                    rows.push(self.session_row(self.project(&dir), s, cx));
                }
            }
        }

        // The one true "No directory": sessions whose record carries no `cwd` at
        // all. Everything else with a directory of its own is a heading above, so
        // this is a short list rather than the 375-row pile it was.
        if !self.ws.other.is_empty() {
            rows.push(view::group_head("No directory").into_any_element());
            for s in others {
                rows.push(self.session_row(None, s, cx));
            }
        }

        list = list.children(rows);

        h_flex()
            .h_full()
            .flex_shrink_0()
            .bg(theme::rail())
            .child(
                v_flex()
            .w(px(self.rail_w))
            .h_full()
            .flex_shrink_0()
            .child({
                let key = "rail:new";
                let t = motion::hover_t(key);
                motion::mark_on_hover(
                    view::rail_head(total, t),
                    key,
                    cx.listener(|this, _, _, cx| this.new_session(cx)),
                )
                .into_any_element()
            })
            .child(
                v_flex()
                    .id("rail-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .when(self.ws.projects.is_empty() && self.ws.other.is_empty(), |d| {
                        d.child(view::rail_empty())
                    })
                    .child(list),
            )
            .child({
                let add_key = "foot:add";
                let gear_key = "foot:gear";
                let t = motion::hover_t(add_key);
                let g = motion::hover_t(gear_key);
                motion::mark_on_hover(
                    view::rail_foot(t, g),
                    add_key,
                    cx.listener(|_this, _, window, cx| {
                        // The one control in this window that changes a fact
                        // rather than a view: a directory the user picks, added
                        // to their own `projects.json`.
                        let rx = cx.prompt_for_paths(PathPromptOptions {
                            files: false,
                            directories: true,
                            multiple: false,
                            prompt: Some("Which directory should be a project?".into()),
                        });
                        let weak = cx.weak_entity();
                        cx.spawn_in(window, async move |this, cx| {
                            let paths = match rx.await {
                                Ok(Ok(Some(paths))) => paths,
                                _ => return,
                            };
                            let Some(path) = paths.first() else { return };
                            let path = path.clone();
                            let added = state::add_project(&path);
                            this.update_in(cx, |app: &mut App, _, cx| {
                                match added {
                                    Ok(()) => app.reload(cx),
                                    Err(e) => eprintln!("add project: {e}"),
                                }
                                let _ = weak;
                            })
                            .ok();
                        })
                        .detach();
                    }),
                )
                .into_any_element()
            })
            )
            // The rail's own edge: the same grab strip as the sidebar's, wired
            // the same way. Two dividers, one control. Only the press is here:
            // the move and the release are the root's, because a move that has
            // to stay inside a six-pixel strip is a move that stops on the
            // first pixel of a real drag.
            .child(
                motion::mark_hover_el(
                    view::split("right", self.resizing_rail),
                    "split:rail",
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.resizing_rail = true;
                        cx.notify();
                    }),
                ),
            )
            .into_any_element()
    }

    /// One session's state, from the three live facts and the window's own memory of
    /// what it has not shown you yet.
    ///
    /// Split out because the rail needs it in two places: the row itself, and the
    /// heading it stands under, which has to say when something below it wants a
    /// person even while the heading is folded shut.
    fn session_state(
        &self,
        id: &str,
        live: &agent::Live,
        held: &str,
        unseen: &std::collections::HashSet<String>,
    ) -> view::State {
        view::row_state(id, held, live.busy, live.asking.is_some(), !live.error.is_empty(), unseen)
    }

    fn session_row(
        &self,
        project: Option<&state::Project>,
        s: &state::Session,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!("s:{}", s.id);
        let t = motion::hover_t(&key);
        let selected = self.sel.as_deref() == Some(s.id.as_str());
        let caption = self.caption(project, s);
        // What this row says about the run, in t3code's own order: a question the
        // worker is blocked on beats a failure beats a run in flight beats a turn
        // that finished while you were elsewhere beats nothing at all. Only the row
        // the worker is holding can be in the first three, because the worker holds
        // one session; `unseen` is the fourth and is about this window, not the file.
        let live = self.agent.as_ref().map(|a| a.live()).unwrap_or_default();
        let held = self.agent.as_ref().map(|a| a.holding().to_string()).unwrap_or_default();
        let state = self.session_state(&s.id, &live, &held, &self.unseen);
        // The project's own mark, on the session's row: their rail puts the
        // **project** favicon on the thread row, and it is what says where a
        // session stands now that the row carries no path under it.
        motion::mark_on_hover(
            view::session_row(
                s,
                selected,
                t,
                &caption,
                state,
                cx.listener({
                    let id = s.id.clone();
                    move |this, _, window, cx| this.start_rename(&id, window, cx)
                }),
                cx.listener({
                    let id = s.id.clone();
                    move |this, _, _, cx| this.delete_session(&id, cx)
                }),
                cx.listener({
                    let id = s.id.clone();
                    move |this, _, _, cx| this.archive_session(&id, cx)
                }),
            ),
            &key,
            cx.listener({
                let id = s.id.clone();
                move |this, _, _, cx| {
                    this.sel = Some(id.clone());
                    // Read now rather than on the next tick: a click that shows
                    // the old session for 33ms reads as a click that missed.
                    this.cache.sync(&this.ws.home.clone(), &id);
                    // A session you just opened is one you want to read from the
                    // bottom of, and one you have now seen.
                    this.follow.pin = true;
                    this.unseen.remove(&id);
                    cx.notify();
                }
            }),
        )
        .into_any_element()
    }

    /// The selected session's plan, borrowed from the cache.
    ///
    /// Borrowed and not cloned. This was `self.cache_for(&id).1`, which **cloned the
    /// whole transcript and the plan together** - four hundred rows, up to 256KB of
    /// text - and it runs in `render`, in *both* the content and the sidebar, thirty
    /// times a second while anything animates. A quarter-megabyte copy per frame is a
    /// window that feels heavy under the hand, which is exactly the report it got.
    /// The plan is a handful of short strings; the transcript is not, and this is only
    /// ever read for the plan.
    fn plan(&self) -> &[(String, String)] {
        match self.sel.as_deref() {
            Some(id) if self.cache.session == id => &self.cache.plan.1,
            _ => &[],
        }
    }

    // ------------------------------------------------------------ the sidebar

    /// Whether the transcript scroller has actually reached its end.
    ///
    /// **This was the scroll bug**, and it was here the whole time.
    ///
    /// It used to ask `logical_scroll_bottom()`, on the belief that it returns the
    /// distance still below the viewport. It does not: it returns the **bottom offset
    /// of the last child that is in view**, which is a small number at almost any
    /// scroll position, because the child at the bottom of the viewport is by
    /// definition near the viewport's bottom. So this answered `true` while the reader
    /// was parked in the middle of a conversation, `drew(true)` re-armed following on
    /// an idle tick, and the next record landing pulled the view to the foot. That is
    /// "it forces itself down", and no amount of tuning the re-arm delay could fix it
    /// because the read it was tuning was not the read being asked for.
    ///
    /// The honest question is whether the offset has reached the **maximum** offset,
    /// and `max_offset` is exactly that: `content size - viewport size`, computed by
    /// the layout pass. Reaching the end means `offset == -max_offset` (the offset is
    /// negative as the content moves up), so there is no more to scroll.
    ///
    /// Before the first layout `max_offset` is zero and the offset is zero, which
    /// reads as "at the end" - the right answer for a transcript that is not laid out
    /// yet, and the reason a fresh window opens at its newest row.
    fn scroll_at_bottom(&self) -> bool {
        at_end(f32::from(self.scroll.offset().y), f32::from(self.scroll.max_offset().y))
    }

    /// Which turn's mark is the one the reader is looking at: the first mark whose
    /// row is inside the viewport.
    ///
    /// The scroller's own `child_bounds` are what answers it, so this is the same
    /// measurement the scroll is laid out from rather than a second opinion about
    /// it. `resolveTimelineMinimapCurrentIndex` is t3code's version of the rule,
    /// including the tie-break: the first in view wins, so the mark tracks the top
    /// of the screen rather than the bottom.
    fn mark_in_view(&self, marks: &[state::Anchor]) -> Option<usize> {
        // **From the measured row extents, not from the handle.** `bounds_for_item` asks
        // the scroller about its own children - and this scroller has exactly one child,
        // the whole transcript column, whose children are the rows. So every lookup here
        // returned `None`, this always answered `None`, and the strip never moved: the
        // active turn stayed where it was whatever you clicked or scrolled past.
        let rows = self.row_bounds.borrow();
        let (_, offset_y) = self.scroll.logical_scroll_top();
        let view = self.scroll.bounds();
        let top = f32::from(view.top() - offset_y);
        let bottom = top + f32::from(view.size.height);
        for (i, mark) in marks.iter().enumerate() {
            // A turn is the one you are looking at when its row overlaps the window.
            if let Some((r_top, r_bottom)) = rows.get(mark.row) {
                if *r_bottom > top && *r_top < bottom {
                    return Some(i);
                }
            }
        }
        // Nothing overlaps: a turn taller than the viewport is what that looks like, so
        // the mark is the last turn that began above the fold.
        let mut before: Option<usize> = None;
        for (i, mark) in marks.iter().enumerate() {
            if rows.get(mark.row).is_some_and(|(r_top, _)| *r_top <= top) {
                before = Some(i);
            }
        }
        before
    }

    /// Ask the provider what it serves, on a thread.
    ///
    /// Once per provider per run, and only when a menu or the window wants the
    /// list: the answer is the account's own catalog and does not change while
    /// the window is open. `fetching` is what stops a second press starting a
    /// second call for the same provider.
    fn fetch_catalog(&mut self, provider: &str) {
        if self.catalogs.contains_key(provider)
            || self.fetching.as_deref() == Some(provider)
        {
            return;
        }
        self.fetching = Some(provider.to_string());
        let tx = self.fetched.0.clone();
        let worker = self.worker.clone();
        let name = provider.to_string();
        std::thread::spawn(move || {
            let list = state::catalog(&worker, &name);
            let _ = tx.send((name, list));
        });
    }

    /// The catalog, if it has landed.
    fn drain_catalogs(&mut self) -> bool {
        let mut moved = false;
        while let Ok((name, list)) = self.fetched.1.try_recv() {
            // An empty answer is not an answer: it means the endpoint could not
            // be reached, and caching it would leave the menu empty forever. The
            // fetch is allowed to happen again.
            if !list.is_empty() {
                self.catalogs.insert(name.clone(), list);
            }
            if self.fetching.as_deref() == Some(name.as_str()) {
                self.fetching = None;
            }
            moved = true;
        }
        moved
    }

    /// Ask `git` about the selected directory, if it is due and not already out.
    ///
    /// **Nothing here blocks.** This used to be `sync_git`, which ran the subprocesses
    /// inline: `state::changes` is two `git` calls and `state::tree` is three more on a
    /// cold directory, and all five ran on the thread that draws. The window stalled
    /// for 10-45ms every four seconds, which is the "lag spike" - it looks random
    /// because it lands wherever the frame happens to be, and it is worst on the
    /// directory you are actually working in, which is the one with the most to say.
    fn ask_git(&mut self) {
        let Some(dir) = self.selected_dir() else {
            return;
        };
        let dir = dir.display().to_string();
        let due = self.cache.changes.0 != dir || self.cache.changes.1.elapsed() > GIT_INTERVAL;
        // `git_read` is the directory a read is already out for, so a tick that fires
        // four times while `git` is slow does not start four readers of the same repo.
        if !due || self.git_read.as_deref() == Some(dir.as_str()) {
            return;
        }
        let want_tree = self.cache.tree.0 != dir;
        self.git_read = Some(dir.clone());
        let tx = self.git_in_flight.0.clone();
        std::thread::spawn(move || {
            let path = PathBuf::from(&dir);
            let changes = state::changes(&path);
            let tree = want_tree.then(|| state::tree(&path));
            let _ = tx.send(GitRead { dir, changes, tree });
        });
    }

    /// Take a `git` answer that has landed. Returns whether what is drawn moved.
    fn apply_git(&mut self) -> bool {
        // The selection may have moved while `git` ran. A read of a directory nobody is
        // looking at any more is dropped rather than applied: writing it into the cache
        // would date the *new* directory's entry with the old one's changes, and the
        // four-second clock would then sit still while showing the wrong repo's branch.
        let Some(current) = self.selected_dir().map(|p| p.display().to_string()) else {
            return false;
        };
        let mut moved = false;
        while let Ok(read) = self.git_in_flight.1.try_recv() {
            if self.git_read.as_deref() == Some(read.dir.as_str()) {
                self.git_read = None;
            }
            if read.dir != current {
                continue;
            }
            let changed = read.changes != self.cache.changes.2;
            self.cache.changes = (read.dir.clone(), std::time::Instant::now(), read.changes);
            if let Some((files, capped)) = read.tree {
                self.cache.tree = (read.dir.clone(), files, capped);
            }
            // A file that moved is a diff that is stale.
            self.cache.diff.1.clear();
            moved |= changed;
        }
        moved
    }

    /// Ask, once, whether origin has moved past this build.
    ///
    /// On its own thread for the same reason `git` is: a fetch is a network round
    /// trip measured in hundreds of milliseconds, and this is the thread that draws.
    fn ask_updates(&mut self) {
        let tx = self.news_in_flight.0.clone();
        std::thread::spawn(move || {
            let _ = tx.send(cuacode::version::check());
        });
    }

    /// Take the answer if it has landed. Returns whether the window changed, which
    /// is what makes the tick repaint: the chip is a fact that arrives once, not a
    /// state that is polled.
    fn apply_news(&mut self) -> bool {
        let Ok(news) = self.news_in_flight.1.try_recv() else {
            return false;
        };
        let moved = self.news.as_ref() != Some(&news);
        self.news = Some(news);
        moved
    }

    /// The directory the selected session stands in, which is what git is asked
    /// about: the project it belongs to, then the session's own `cwd`.
    fn selected_dir(&self) -> Option<PathBuf> {
        let id = self.sel.as_deref()?;
        if let Some(p) = self.ws.project_of(id) {
            return Some(p.dir.clone());
        }
        let cwd = self.session(id)?.cwd.clone();
        (!cwd.trim().is_empty()).then(|| PathBuf::from(cwd))
    }

    /// The diff the sidebar is showing a file of, re-read when it changes.
    fn diff_of(&mut self, path: &str) {
        let Some(dir) = self.selected_dir() else {
            return;
        };
        let dir = dir.display().to_string();
        if self.cache.diff.0 == path && self.cache.diff.1 == dir {
            return;
        }
        let view = state::file_view(Path::new(&dir), path);
        self.cache.diff = (path.to_string(), dir, view);
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let session = self.selected();
        let project = self.sel.as_deref().and_then(|id| self.ws.project_of(id));
        let dir = self.selected_dir();
        let changes = &self.cache.changes.2;
        let plan = self.plan();

        // The body, by tab. Changes is the product of this column: what the
        // checkout is carrying, in their three categories and their own order.
        let body: AnyElement = match self.side_tab {
            0 => {
                let mut col = v_flex().w_full().child(view::changes_body(
                    changes,
                    &dir.as_ref().map(|d| state::short_path(d)).unwrap_or_default(),
                ));
                for (i, (title, kind, files)) in [
                    ("Staged", "staged", &changes.staged),
                    ("Changes", "unstaged", &changes.unstaged),
                    ("Untracked", "", &changes.untracked),
                ]
                .iter()
                .enumerate()
                {
                    if files.is_empty() {
                        continue;
                    }
                    let key = format!("cat:{i}");
                    let t = motion::hover_t(&key);
                    let open = self.open_cats[i];
                    col = col.child(motion::mark_on_hover(
                        view::category_head(title, kind, files.len(), open, t),
                        &key,
                        cx.listener(move |this, _, _, cx| {
                            this.open_cats[i] = !this.open_cats[i];
                            cx.notify();
                        }),
                    ));
                    if open {
                        let mut rows = v_flex().w_full().gap(px(1.0));
                        for (j, f) in files.iter().enumerate() {
                            let key = format!("file:{i}:{j}");
                            let t = motion::hover_t(&key);
                            let selected = self.cache.diff.0 == f.path && self.side_tab == 2;
                            let path = f.path.clone();
                            rows = rows.child(motion::mark_on_hover(
                                view::file_row(f, t, selected),
                                &key,
                                cx.listener(move |this, _, _, cx| {
                                    this.diff_of(&path);
                                    this.side_tab = 2;
                                    cx.notify();
                                }),
                            ));
                        }
                        col = col.child(rows);
                    }
                }
                col.into_any_element()
            }
            // The Files tab: the directory's whole tree, as their own `FilesView`
            // draws it, one row per file with its folder beside its name so two
            // files called the same thing are two rows.
            // The Files tab: the directory's own tree, which is what a file
            // browser is. Folders open, files open their diff, and the filter
            // keeps a folder that has anything matching under it.
            1 => {
                let (_, nodes, capped) = &self.cache.tree;
                // The tree is open by default: a checkout you are browsing is a tree you
                // are drilling through, and a folder that shuts every time you look away
                // is a folder you have to open twice. The rail is the other way round -
                // there the default is *shut*, because its headings are many and its
                // leaves are hundreds.
                let open = |path: &str| !self.open_tree_dirs.iter().any(|p| p == path);
                // Borrowed and filtered, both: nothing is copied on this path.
                // The rows were cloned per frame before, which is what made a
                // resize drag heavy.
                //
                // No filter box above this. There was one, it was a picture of a
                // field for months, and when it became a real one the honest
                // answer was still no: a checkout of a few hundred files is
                // readable by scrolling, and the box cost a strip of the column to
                // solve a problem this tree does not have.
                let shown: Vec<&state::Node> = nodes.iter().collect();
                let rows = state::tree_rows_ref(&shown, &open);
                let mut col = v_flex().w_full().gap(px(1.0));
                if nodes.is_empty() {
                    col = col.child(view::not_yet_icon(
                        IconName::FolderTree,
                        "No files: this directory is not a checkout.",
                    ));
                } else if rows.is_empty() {
                    col = col.child(view::not_yet_icon(IconName::SearchX, "Nothing matches that."));
                }
                for (node, depth) in rows {
                    let key = format!("tree:{}", node.path);
                    let t = motion::hover_t(&key);
                    let selected = self.cache.diff.0 == node.path;
                    let is_open = open(&node.path);
                    let dir = node.is_dir();
                    let path = node.path.clone();
                    col = col.child(motion::mark_on_hover(
                        view::tree_row(&node, depth, t, selected, is_open),
                        &key,
                        cx.listener(move |this, _, _, cx| {
                            if dir {
                                // A folder opens and shuts; a file opens its diff.
                                match this.open_tree_dirs.iter().position(|p| *p == path) {
                                    Some(i) => {
                                        this.open_tree_dirs.remove(i);
                                    }
                                    None => this.open_tree_dirs.push(path.clone()),
                                }
                            } else {
                                this.diff_of(&path);
                                this.side_tab = 2;
                            }
                            cx.notify();
                        }),
                    ));
                }
                if *capped {
                    col = col.child(view::not_yet_icon(
                        IconName::Ellipsis,
                        &format!("and more: holding the first {}", state::TREE_LIMIT),
                    ));
                }
                v_flex().w_full().child(col).into_any_element()
            }
            // The diff, which is what a file row opens onto.
            _ => v_flex()
                .w_full()
                .child({
                    // The header is its own hover target so the close control has a
                    // face to light up, and the press closes the diff by going back
                    // to the tab the file was opened from.
                    let key = "diff:x";
                    motion::mark_on_hover(
                        view::file_head(&self.cache.diff.0, motion::hover_t(key)),
                        key,
                        cx.listener(|this, _, _, cx| {
                            this.side_tab = 0;
                            cx.notify();
                        }),
                    )
                    .into_any_element()
                })
                .child(
                    div()
                        .id("diff-scroll")
                        .max_h(px(420.0))
                        .overflow_y_scroll()
                        .child(view::file_view_pane(&self.cache.diff.2)),
                )
                .into_any_element(),
        };

        // The plan, at the foot of the column and always there. It is the
        // session's own todo list, written by the worker as the agent works, so
        // it is the one thing in this sidebar that is about the *work* rather
        // than about the checkout.
        let plan_block = {
            let mut col = v_flex().w_full().flex_shrink_0().border_t_1().border_color(theme::line());
            col = col.child(
                h_flex()
                    .w_full()
                    .h(px(26.0))
                    .items_center()
                    .gap(px(theme::SPACE_SM))
                    .px(px(theme::SPACE_SM))
                    .child(icon(IconName::ListTodo, 13.0, theme::muted()))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::muted())
                            .child("Plan"),
                    ),
            );
            col = col.child(
                div()
                    .id("plan-scroll")
                    .max_h(px(200.0))
                    .overflow_y_scroll()
                    .px(px(theme::SPACE_SM))
                    .pb(px(theme::SPACE_SM))
                    .child(view::plan_body(&plan)),
            );
            col
        };

        h_flex()
            .h_full()
            .flex_shrink_0()
            .child(
                // The press starts the drag; the root follows it and commits it.
                // See `App::drag_to` for why the move cannot live on this strip.
                div()
                    .id("split")
                    .h_full()
                    .w(px(6.0))
                    .flex_shrink_0()
                    .cursor_col_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.resizing_side = true;
                            cx.notify();
                        }),
                    )
                    .child(div().h_full().w(px(1.0)).ml(px(2.5)).bg(if self.resizing_side {
                        theme::accent()
                    } else {
                        theme::line()
                    })),
            )
            .child(
                v_flex()
                    .w(px(self.side_w))
                    .h_full()
                    .flex_shrink_0()
                    .bg(theme::page())
                    .child({
                        let keys = ["tab:0", "tab:1"];
                        let hovers = [motion::hover_t(keys[0]), motion::hover_t(keys[1])];
                        let active = if self.side_tab == 2 { 1 } else { self.side_tab };
                        let labels = ["Changes", "Files"];
                        let mut row = h_flex()
                            .w_full()
                            .h(px(theme::HEAD))
                            .flex_shrink_0()
                            .bg(theme::page())
                            .border_b_1()
                            .border_color(theme::line());
                        for i in 0..2 {
                            row = row.child(motion::mark_on_hover(
                                view::sidebar_tab(i, labels[i], i == active, hovers[i]),
                                keys[i],
                                cx.listener(move |this, _, _, cx| {
                                    this.side_tab = i;
                                    cx.notify();
                                }),
                            ));
                        }
                        row
                    })
                    .child(
                        div()
                            .id("side-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .p(px(theme::SPACE_XS))
                            .child(body),
                    )
                    .child(plan_block),
            )
            .into_any_element()
    }

    // ------------------------------------------------------------ the content

    fn content(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        // The landing, consumed **before** anything borrows this app: `scroll_to_bottom`
        // sets a flag gpui's layout pass takes, so this is a one-shot ask and it has to be
        // taken once, at the moment the frame that owes it is built.
        //
        // It is taken here rather than down at the scroller because holding `self`
        // immutably across the build (the selected session is a borrow of it) forbids a
        // mutation down there. This is the same decision either way, and taken here it is
        // taken exactly once.
        let pin = self.follow.take_pin();
        // The minimap's jump is taken here too, for the same reason the landing is:
        // both are one-shot asks the next layout consumes, and both would otherwise
        // have to be taken while the selected session is borrowed.
        let jump = self.jump.take();
        let session = self.selected();
        // What the head's second line says about where this stands. A session in a
        // project says the project's own path; a session that was never promoted
        // says its own `cwd`, which is the same fact from the other side. It read
        // `no directory` in *both* cases before, which was true of neither: a
        // brand-new session standing in this very checkout said it had no
        // directory, because the project it will belong to is decided by the round
        // that follows, not by the row that already exists.
        let caption = match self.sel.as_deref() {
            Some(id) => self
                .ws
                .project_of(id)
                .map(|p| p.where_line())
                .or_else(|| {
                    self.session(id)
                        .map(|s| s.cwd.trim())
                        .filter(|cwd| !cwd.is_empty())
                        .map(|cwd| state::short_path(Path::new(cwd)))
                })
                .unwrap_or_else(|| "no directory".into()),
            None => String::new(),
        };
        // **The column's width and whether there is a strip: one decision.**
        //
        // This was two guesses that disagreed. The column insisted on `MEASURE` (736) and
        // centred itself in whatever was left - and at this window size that left 7px a
        // side, so the strip either floated far from the text or was hidden entirely. Both
        // of those are the ways it went wrong.
        //
        // So it is arithmetic, done once: the content area gives the text the measure it
        // wants, the strip a band of its own on each side (symmetric, so the text stays
        // centred), and takes back what it must when the window is narrow. A strip is drawn
        // whenever there is more than one turn and there is room for the *band* - which is
        // far less room than before.
        let strip_on = self.agent.is_some() && self.cache.rows.len() > 1;
        let band = theme::MAP_HIT_W + theme::SPACE_MD;
        let avail = {
            let width = f32::from(window.bounds().size.width);
            let rail_w = if self.rail_open { self.rail_w } else { 0.0 };
            let side_w = if self.sidebar_open { self.side_w } else { 0.0 };
            (width - rail_w - side_w).max(0.0)
        };
        // The narrowest a conversation may get before the strip is dropped: below this the
        // text is unreadable and the strip is the lesser thing to lose.
        let floor = 420.0;
        let strip_gutter = strip_on && avail - band * 2.0 >= floor;
        // The column is the measure when it fits and the rest of the room when it does not.
        let measure = if strip_gutter {
            theme::MEASURE.min(avail - band * 2.0)
        } else {
            theme::MEASURE.min(avail)
        };
        let body: AnyElement = if self.ws.shown() == 0 {
            view::empty_state(&state::short_path(&self.ws.home), 0.0, 0.0).into_any_element()
        } else if let Some(s) = session {
            // The conversation, from the cache: read off disk when the file
            // moved, never in `render`. A frame that opens a file is a frame
            // that stutters, and this one runs thirty times a second.
            let rows = &self.cache.rows;
            // A run is open or shut by its index, and a run starts shut: the
            // whole point of the fold is that a session with two hundred calls
            // opens as a conversation rather than as a log.
            let sid = s.id.clone();
            let is_open = |i: usize| self.open_runs.0 == sid && self.open_runs.1.contains(&i);
            // The fold's own target: a press on a run's head opens it or shuts
            // it, which is the whole interaction a folded run needs.
            let wrap = |i: usize, head: AnyElement| -> AnyElement {
                let key = format!("run:{sid}:{i}");
                let t = motion::hover_t(&key);
                motion::mark_on_hover(
                    div()
                        .w_full()
                        .rounded(px(theme::R_CHIP))
                        .bg(motion::mix(theme::ink(0.0), theme::wash_hover(), t))
                        .px(px(theme::SPACE_XS))
                        .child(head),
                    &key,
                    cx.listener({
                        let sid = sid.clone();
                        move |this, _, _, cx| {
                        if this.open_runs.0 != sid {
                            this.open_runs = (sid.clone(), std::collections::HashSet::new());
                        }
                        if !this.open_runs.1.insert(i) {
                            this.open_runs.1.remove(&i);
                        }
                        cx.notify();
                        }
                    }),
                )
                .into_any_element()
            };
            // Each row's own extent, from the column's `prepaint`: this is what makes
            // the minimap's strips sit where their turns are, and what a strip's jump
            // lands on.
            let bounds = self.row_bounds.clone();
            let col = view::transcript_with(
                rows,
                &wrap,
                &is_open,
                measure,
                Some(Box::new(move |kids: Vec<Bounds<Pixels>>, _, _| {
                    let origin = kids.first().map(|b| f32::from(b.top())).unwrap_or(0.0);
                    *bounds.borrow_mut() = kids
                        .iter()
                        .map(|b| {
                            let top = f32::from(b.top()) - origin;
                            (top, top + f32::from(b.size.height))
                        })
                        .collect();
                })),
            );
            // Land at the end **once**, when this frame is the one that should.
            //
            // `scroll_to_bottom()` sets a flag the layout pass *takes* (`mem::take` in
            // gpui's `div.rs`), then forces the offset to the bottom on that layout. So it
            // is a one-shot ask, not a state to hold: asking while the flag is already set
            // does nothing extra, and asking on a later frame asks *again*.
            //
            // Holding it as a bool and re-asking every frame is what made scrolling up
            // impossible: every frame snapped the offset back to the bottom, so a wheel
            // was undone before the next paint. The `pin_bottom` flag is now consumed - set
            // by a selection, by a send, or by the clock on a settled tick when the reader
            // is at the foot - and cleared here whether or not it lands.
            // Land at the end **once**, when this frame is the one that should.
            //
            // `scroll_to_bottom()` sets a flag the layout pass *takes* (`mem::take` in
            // gpui's `div.rs`), then forces the offset to the bottom on that layout. So
            // it is a one-shot ask, not a state to hold: asking while the flag is
            // already set does nothing extra, and asking on a later frame asks *again*.
            if pin {
                self.scroll.scroll_to_bottom();
            }
            // A jump the minimap asked for.
            //
            // **By offset, not by item.** `scroll_to_top_of_item` takes an index into the
            // scroller's own children - and this scroller has exactly one child, the whole
            // transcript column, with the rows as *its* children. So every index above 0 was
            // out of range and `scroll_to_top_of_item` did nothing at all, which is what
            // which read as clicking it and nothing happening.
            //
            // The row's real extent is already measured (see `row_bounds`), so the jump is
            // an offset: put that row's top at the top of the viewport.
            if let Some(row) = jump {
                if let Some((top, _)) = self.row_bounds.borrow().get(row).copied() {
                    self.scroll.set_offset(point(px(0.0), px(-top)));
                }
            }
            // gpui's own scrolling, which is what this app has always used.
            //
            // **The column is centred, and the strip is placed against it.** The
            // transcript is capped at `MEASURE`, and nothing centred it - so it hugged the
            // left of the content area and left a 280px dead zone on the right, which is
            // where the strip then went: stranded against the sidebar instead of beside
            // the text. One wrapper centres the column,
            // and the gutter the strip needs is now half of what the column has left over.
            div()
                .id("transcript")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                // A wheel is the reader taking the view, and it is the **only** authority
                // on where they are: the handle cannot be asked, because its offset is the
                // last layout's and a fresh scroll still reads as "at the foot". So any
                // wheel stops the following until the view has settled and is provably
                // back at the foot. See [`Follow`].
                .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, cx| {
                    this.follow.scrolled();
                    // **Repaint on the wheel**, because the minimap's active strip is drawn
                    // from the reader's position: without this the strip kept whatever
                    // colour it had and only caught up when something else asked for a
                    // frame. gpui's own scroll repaints the scroller; it does not know
                    // about a sibling that is a map of it.
                    cx.notify();
                }))
                // **The column is centred by a row, not by a margin.** gpui has no
                // `mx_auto` (it compiled to nothing, so the column stayed wedged against
                // the left and left a dead band on the right - which is where the strip
                // then floated, far from the text). A flex row
                // with the column as its one child, centred, is what gpui has.
                .child(
                    h_flex()
                        .w_full()
                        .justify_center()
                        // **The text column gives up the strip's width.** At this window
                        // size the content area is ~750 and the column is capped at 736, so
                        // the "gutter" was 7px a side - no gutter at all. The strip then had
                        // nowhere to be except over the text or out in space, which is what
                        // every time it drew. Reserving the strip's own width on both sides
                        // keeps the column centred *and* gives the strip a real home beside
                        // the text. Symmetric, so the text does not shift off centre.
                        .px(px(if strip_gutter { band } else { 0.0 }))
                        .child(col),
                )
                .into_any_element()
        } else {
            view::nothing_selected().into_any_element()
        };
        // The transcript's own furniture, drawn **over** it: the minimap down the
        // right edge, and the way back to the end when the reader has left it.
        //
        // Both are derived per frame from the rows the cache already holds, so
        // neither costs a read: `anchors` is one pass over the rows, and the index
        // of the turn in view is the scroller's own answer about its children.
        let marks = state::anchors(&self.cache.rows);
        let current = (!marks.is_empty()).then(|| self.mark_in_view(&marks)).flatten();
        let below = current.map_or(0, |i| marks.len().saturating_sub(i + 1));
        let at_foot = self.scroll_at_bottom();
        let overlay = if self.agent.is_some() && !marks.is_empty() {
            // The strip. Only drawn when there is more than one turn: a single mark
            // is a control that cannot do anything.
            let strip = if marks.len() > 1 {
                let rows: Vec<usize> = marks.iter().map(|m| m.row).collect();
                Some(view::minimap(&marks, current, &mut |i, face| {
                    // The row this pip stands for, copied out before the listener
                    // takes it: the listener outlives this call and a `usize` is the
                    // only thing it needs.
                    let row = rows[i];
                    motion::mark_on_hover(
                        face,
                        &format!("pip:{i}"),
                        cx.listener(move |this, _, _, cx| {
                            this.jump = Some(row);
                            // A jump is the reader choosing where to be, so following
                            // stays off until they settle somewhere.
                            this.follow.on = false;
                            cx.notify();
                        }),
                    )
                }))
            } else {
                None
            };
            // The way back. It is a **pin**, not a jump: the reader asked for the
            // end, and the end is wherever the newest record lands.
            let back = if !at_foot {
                Some(view::to_bottom(below, motion::hover_t("to:bottom"), cx.listener(
                    |this, _, _, cx| {
                        this.follow.pin = true;
                        this.follow.on = true;
                        cx.notify();
                    },
                )))
            } else {
                None
            };
            // **The strip goes in the gutter, not over the text.** The reading column is
            // `MEASURE` wide and centred, so on a window wider than that there is real
            // empty space to the right of it - and that is where a table of contents
            // belongs. On a narrower window there is no gutter, so the strip is not drawn
            // at all rather than laid over the conversation: t3code does the same thing
            // (`hitStripWidth > 0` or it goes inert).
            // The width of the **content column**, which is the window less both side
            // panels - and it must be both. This only subtracted the rail, so with the
            // sidebar open the strip was placed as if there were 250px more room than
            // there is, and it landed over the sidebar instead of in the gutter. That is
            // where it actually lands.
            //
            // `content()` is laid out as `rail | content | sidebar`, so the gutter is half
            // of what the content column has left over its own measure.
            let width = f32::from(window.bounds().size.width);
            let rail_w = if self.rail_open { self.rail_w } else { 0.0 };
            let side_w = if self.sidebar_open { self.side_w } else { 0.0 };
            // The room is reserved by the text column itself now (see `strip_gutter`), so
            // this only has to know whether that happened.
            let _ = (width, rail_w, side_w);
            if !strip_gutter {
                None
            } else {
                // **Two absolute children, not one column.** They used to be a single
                // `justify_between` stack, so the strip was *centred in the space left
                // above the button* - which meant it moved whenever the button appeared or
                // went away, and stayed put while the reader scrolled - the reported bug: a
                // location mark that moves with the jump button for no reason. They are unrelated
                // things, so they are unrelated elements.
                //
                // The strip is centred **in the transcript's own viewport**, which is what
                // it is a map of; the button is pinned to the foot of the gutter. Neither
                // knows the other exists.
                let strip = strip.map(|s| {
                    v_flex()
                        .absolute()
                        // **Anchored to the column, not to the window.** `right` of the
                        // content area put the strip at the far edge, a few hundred pixels
                        // from the text. The gutter is now measured at
                        // run time, so this is where the text actually ends.
                        // Inside the band the column reserved, so the strip is beside the
                        // text rather than out in the empty area. Centred in that band.
                        .right(px((band - theme::MAP_HIT_W) / 2.0))
                        .top(px(theme::SPACE_LG))
                        .bottom(px(theme::COMPOSER_FLOOR_H))
                        .items_end()
                        // Centred in the transcript's own viewport, which is what it maps.
                        // Its own element now, so the jump button cannot move it.
                        .justify_center()
                        // **No `block_mouse_except_scroll` here, and that was the bug.**
                        //
                        // That call makes every hitbox behind this one ignore *all* mouse
                        // input, which is what let the wheel through - but it also means
                        // the box itself is the only thing that can be clicked, and the
                        // strips inside it are hitboxes behind it. So every strip's own
                        // click was swallowed, which read as the click doing nothing: it was
                        // exactly that: the click landed on a transparent panel, not on the
                        // strip.
                        //
                        // It is not needed: this box is only `MAP_HIT_W` wide and sits
                        // beside the text, so the wheel over the conversation never reaches
                        // it anyway. Only the strips' own boxes take a click.
                        .child(s)
                        .into_any_element()
                });
                let back = back.map(|b| {
                    v_flex()
                        .absolute()
                        .right(px(theme::SPACE_SM))
                        .bottom(px(theme::COMPOSER_FLOOR_H + theme::SPACE_LG))
                        .items_end()
                        .child(b)
                        .into_any_element()
                });
                // The two are **separate absolutely-positioned children of the same
                // relative box**, so neither can move the other. (An outer wrapper with
                // `inset_0` also worked, but a box whose only job is to be transparent is
                // one more thing that can collapse to zero and hide both.)
                let mut items: Vec<AnyElement> = Vec::new();
                items.extend(strip);
                items.extend(back);
                (!items.is_empty()).then(|| {
                    v_flex()
                        .absolute()
                        .inset_0()
                        .children(items)
                        .into_any_element()
                })
            }
        } else {
            None
        };
        v_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(view::content_head(session, &caption))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(body)
                    .children(overlay)
                    // The menu is NOT drawn here, inside a box that is clipped and
                    // scrolled: it is drawn at the window's root from `menu_at`, so a
                    // row can be wide and the list is not cut off by an ancestor.
                    ,
            )
            // The composer appears whenever there is a worker to send to, which
            // includes a **new** conversation: the rail has nothing selected, the
            // location chip is unlocked, and the first thing you type is the
            // session. The one place it does not appear is a window with no worker
            // at all, where a field would be a field asking you to talk to nothing.
            //
            // It is a **sibling of the column, not a row in it** (`v_flex` here is the
            // outer shell, and the two children overlap): the transcript fills the shell
            // and the composer floats over its foot, so the conversation runs *under* the
            // box rather than stopping at it. That is the whole point of the shape - the
            // last thing the model said stays visible while you write the reply, and the
            // scrim below fades it out as it goes under.
            .when(self.agent.is_some(), |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .right(px(0.))
                        .bottom(px(0.))
                        // The overlay is a **sibling over the transcript**, so without this
                        // it is the thing under the pointer and the wheel never reaches
                        // the scroller beneath it - which is exactly "I cannot scroll up,
                        // it pushes me down". This lets the scroll through and keeps every
                        // other mouse event, so the card's own controls still work.
                        .block_mouse_except_scroll()
                        // The scrim first, so it sits *under* the card: the transcript fades
                        // out as it goes behind the box rather than being cut off mid-glyph
                        // by its edge.
                        .child(view::composer_scrim(theme::COMPOSER_FLOOR_H))
                        .child(self.composer(window, cx)),
                )
            })
            .into_any_element()
    }
}

impl App {
    /// Move whichever divider a drag owns to the pointer's own x, and keep the
    /// window painting.
    ///
    /// One method for both dividers, because they are the same control on two
    /// edges: the rail's width is the pointer's x, the sidebar's is measured back
    /// from the window's right edge, and every other thing about the drag is
    /// shared. The preview moves and the layout does not: the width the window is
    /// actually laid out at is committed on release, or a move per pixel would
    /// rebuild the whole transcript per pixel.
    fn drag_to(&mut self, x: f32, cx: &mut Context<Self>) {
        if self.resizing_rail {
            let next = x.clamp(theme::RAIL_MIN, theme::RAIL_MAX);
            if (next - self.drag_rail.unwrap_or(self.rail_w)).abs() > 0.5 {
                self.drag_rail = Some(next);
                cx.notify();
            }
        } else if self.resizing_side {
            let next = (self.side_right - x).clamp(theme::SIDE_MIN, theme::SIDE_MAX);
            if (next - self.drag_side.unwrap_or(self.side_w)).abs() > 0.5 {
                self.drag_side = Some(next);
                cx.notify();
            }
        }
    }

    /// Commit whichever drag was live, and stop tracking it.
    ///
    /// The one moment the whole layout moves. `end_drag` is also the release
    /// path for a drag that the pointer left the window on, so it takes no
    /// position: a release with no moves still commits where the pointer last
    /// was, which is `drag_*` or, failing that, the width already set.
    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.resizing_rail {
            if let Some(w) = self.drag_rail.take() {
                self.rail_w = w;
            }
            self.resizing_rail = false;
        }
        if self.resizing_side {
            if let Some(w) = self.drag_side.take() {
                self.side_w = w;
            }
            self.resizing_side = false;
        }
        cx.notify();
    }

    /// The ghost divider for whichever drag is live, if either is.
    fn drag_ghost(&self, window: &Window) -> Option<AnyElement> {
        let h = f32::from(window.bounds().size.height);
        // The rail's edge: the window's left is the rail's left, so the pointer's
        // x is the line's x.
        if let Some(x) = self.drag_rail {
            return Some(view::split_ghost(x, h).into_any_element());
        }
        // The sidebar's: its width is measured from the window's right edge.
        if let Some(w) = self.drag_side {
            return Some(view::split_ghost(f32::from(window.bounds().size.width) - w, h).into_any_element());
        }
        None
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The split needs the window's own right edge in the same coordinates a
        // pointer event arrives in. macOS reports a mouse position as
        // `locationInWindow`, which is window-relative, so the right edge is the
        // window's **width** - not `bounds().right()`, which is the global screen
        // coordinate and is off by wherever the window happens to sit. That was
        // the sidebar's drag computing against the wrong origin.
        self.side_right = f32::from(window.bounds().size.width);
        // A drag is followed from here and not from the divider.
        //
        // `.on_mouse_move` on an element only fires while the pointer is inside
        // that element's own hitbox (`hitbox.is_hovered`, gpui's `InteractiveElement`).
        // The divider is six pixels wide, so the pointer leaves it on the first
        // pixel of a drag and the move stream stops dead: the divider could only
        // ever track a drag that stayed within three pixels of where it started.
        // That is the "resize does not work" this window shipped with.
        //
        // The root fills the window, so its hitbox covers the whole of it and the
        // moves arrive wherever the pointer goes. The strip's own `mouse_down` is
        // still what starts a drag, because that is the only place a drag may
        // start.
        let resizing = self.resizing_rail || self.resizing_side;
        div()
            .size_full()
            .bg(theme::page())
            .text_color(theme::text())
            .on_action(cx.listener(|this, _: &ToggleRail, _, cx| {
                this.rail_open = !this.rail_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Reload, _, cx| this.reload(cx)))
            .on_action(cx.listener(|_: &mut App, _: &EditNoop, _, _| {}))
            // Escape shuts an open menu. It has to live on the root because the
            // field holds focus while a list is open, so the key never reaches the
            // menu itself: without this, a popover could only be dismissed by
            // pressing its chip a second time, which is not what escape is for.
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key != "escape" {
                    return;
                }
                // Escape backs out of whichever prompt is open. Rename first: it is the
                // one that was asked for most recently, and it holds text the user typed.
                if this.renaming.is_some() && !this.menu.is_some() {
                    this.cancel_rename(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                } else if this.menu.is_some() {
                    this.menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .when(resizing, |d| {
                d.on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                    this.drag_to(f32::from(ev.position.x), cx);
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_drag(cx)))
            })
            .child(
                v_flex()
                    .size_full()
                    // The bar, then its one button wrapped as its own target.
                    // The strip itself is not clickable: a navbar that is one
                    // big button is a navbar you close by accident every time you
                    // reach for the window.
                    .child({
                        let key = "top:toggle";
                        let t = motion::hover_t(key);
                        let toggle = motion::mark_on_hover(
                            view::topbar_toggle(t),
                            key,
                            cx.listener(|this, _, _, cx| {
                                this.rail_open = !this.rail_open;
                                cx.notify();
                            }),
                        );
                        view::topbar(&state::short_path(&self.ws.home), toggle.into_any_element())
                            .child(view::version_chip(
                                self.news.as_ref().and_then(|n| n.behind()),
                            ))
                    })
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h(px(0.))
                            .when(self.rail_open, |d| d.child(self.rail(cx)))
                            .child(self.content(window, cx))
                            .when(self.sidebar_open, |d| d.child(self.sidebar(cx))),
                    ),
            )
            // The live divider, drawn last so it sits over both panels. The
            // panels are still laid out at the old widths - they have to be, or
            // every pixel of pointer movement would rebuild the transcript - so
            // without this line the divider would sit still and then jump on
            // release. The line following the pointer is what makes the commit
            // read as a resize rather than as a bug.
            .children(self.drag_ghost(window))
            // ...and the open menu, over everything. A popover is anchored to the
            // control that opened it: `menu_at` is where the chip was pressed, so the
            // list sits above that point with its own left edge a little in from the
            // pointer. Drawn at the root because a list inside the composer's card or
            // the transcript's scroller is a list an ancestor clips.
            .children(self.menu.map(|m| {
                // Anchored to the chip that opened it: `menu_at` is where the chip
                // was pressed, so the list's bottom sits just above that point and
                // it grows upward - the chips are at the foot of the window and a
                // menu that opened downward would have nowhere to go. Its left is
                // the pointer's, kept inside the window so a chip near the right
                // edge does not push the panel off it.
                let window_w = f32::from(window.bounds().size.width);
                let left = (f32::from(self.menu_at.x) - 12.0).min(window_w - theme::MENU_W - 8.0).max(8.0);
                div()
                    .absolute()
                    .left(px(left))
                    .bottom(px(f32::from(window.bounds().size.height) - f32::from(self.menu_at.y) + 10.0))
                    .child(if m == List::Model { self.model_card(cx) } else { self.menu_card(m, cx) })
            }))
    }
}

#[cfg(test)]
mod tests {
    // Named, not globbed: the parent's `use gpui_kit::*` re-exports gpui's own
    // `test` macro, which shadows the builtin one and wants a context.

    #[test]
    fn a_flag_takes_the_value_after_it() {
        let args = vec!["--fold".to_string(), "alpha".to_string()];
        assert_eq!(super::flag(&args, "--fold").as_deref(), Some("alpha"));
        assert_eq!(super::flag(&args, "--home"), None);
    }

    /// A probe: the harness's own test lives in `shot.rs`, which needs the
    /// `test-support` feature that only the dev-dependency turns on.
    #[test]
    fn the_dev_dependency_turns_on_the_headless_context() {
        let _ = std::mem::size_of::<Option<gpui_kit::VisualTestContext>>();
    }

    /// The follow rule, and the two bugs it fixes.
    ///
    /// "Cannot scroll up, it moves me straight down" and then "after scrolling up it
    /// glitches itself down" were both this machine being wrong. It is a unit test because
    /// the rule is the whole bug: a version that needs a live transcript to catch a wrong
    /// boolean is a version nobody runs.
    #[test]
    fn a_reader_who_scrolled_is_left_alone() {
        let mut f = super::Follow::new();
        assert!(f.follows(true), "a window opens following its own output");

        // The reader scrolls up. From here a record landing must NOT pull them down.
        f.scrolled();
        assert!(!f.follows(true), "a write dragged a reader who had scrolled away");

        // The very next frame reports the *pre-wheel* offset, which still reads as
        // "at the foot". This is the bug: trusting that read re-armed following and
        // the next record snapped the view down. It must be ignored.
        f.drew(true);
        assert!(
            !f.follows(true),
            "a stale at-foot read re-pinned a reader who had just scrolled"
        );

        // A frame *inside* the settle window must not re-arm it either, which is the
        // part the old "skip exactly one frame" rule got wrong: the clock naps at
        // 250ms, so the frame that lands after the gesture is still too early.
        f.drew(true);
        assert!(!f.follows(true), "following came back before the gesture had settled");

        // Once the gesture has settled (the clock is past SETTLE), a genuine arrival
        // at the foot re-arms it. Simulated by backdating the wheel rather than by
        // sleeping: the rule is about elapsed time, not about the test's patience.
        f.wheel_at = Some(std::time::Instant::now() - super::SETTLE * 2);
        f.drew(true);
        assert!(f.follows(true), "a reader who settled back at the foot stopped following");
    }

    #[test]
    fn scrolled_away_stays_scrolled_away() {
        // The reader parks in the middle of a long conversation: every frame reads
        // "not at the foot", so following stays off for as long as they are there.
        let mut f = super::Follow::new();
        f.scrolled();
        for _ in 0..20 {
            f.drew(false);
            assert!(!f.follows(true), "a parked reader was dragged down by a write");
        }
    }

    /// The bug, in the shape it was reported: "if I scroll after like 2 seconds
    /// it pushes the whole thing and it goes to the bottom".
    ///
    /// Two seconds is five idle naps, and the old rule re-armed following on the
    /// first frame after the wheel. This drives that timeline: scroll, let several
    /// frames pass while a record lands, and the view must stay where it was put.
    #[test]
    fn a_record_two_seconds_after_a_scroll_does_not_yank_the_view() {
        let mut f = super::Follow::new();
        f.scrolled();
        // The frames between the gesture and the write, each reading the stale
        // at-foot offset, because the layout that would move it has not run.
        for _ in 0..5 {
            f.drew(true);
        }
        // The write lands. Nothing is owed, so nothing is pinned, so nothing moves.
        assert!(!f.follows(true), "a write two seconds after a scroll yanked the view");
        assert!(!f.take_pin(), "a landing was queued for a reader who had scrolled");
    }

    /// The scroll bug, in the two numbers it was decided from.
    ///
    /// The old read was `logical_scroll_bottom()`, the distance from the bottom
    /// visible child to the viewport's bottom - a small number in the middle of a long
    /// conversation, because the child at the bottom of the viewport is by definition
    /// near the viewport's bottom. So the case below is the bug: a reader 50px from the
    /// end of a 900px scroll range must NOT read as at the end, and the old formula
    /// said it did.
    #[test]
    fn being_partway_down_is_not_being_at_the_end() {
        assert!(!super::at_end(-850.0, 900.0), "a reader 50px from the end read as at the end");
        assert!(!super::at_end(-450.0, 900.0), "a reader halfway down read as at the end");
        assert!(!super::at_end(0.0, 900.0), "a reader at the top read as at the end");
        // ...and the end really is the end, including the fractional-layout slack.
        assert!(super::at_end(-900.0, 900.0));
        assert!(super::at_end(-899.5, 900.0));
        // A scroller with no room to scroll: at the end, which is also the answer
        // before the first layout pass.
        assert!(super::at_end(0.0, 0.0));
        assert!(super::at_end(0.0, -1.0));
    }

    #[test]
    fn a_pin_lands_exactly_once() {
        // gpui's `scroll_to_bottom` is a flag its layout pass takes, so asking twice is
        // asking again - and asking every frame is what made the scroll impossible.
        let mut f = super::Follow::new();
        f.pin = true;
        assert!(f.take_pin(), "the frame that owes the landing performs it");
        assert!(!f.take_pin(), "the next frame must not ask again");

        // And a reader who scrolls cancels a landing that has not happened yet: a write can
        // set the pin and the wheel can arrive before the frame that would honour it.
        f.pin = true;
        f.scrolled();
        assert!(!f.take_pin(), "a landing survived the reader scrolling away");
    }
}
