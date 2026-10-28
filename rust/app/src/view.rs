//! What the shell is made of: the chrome, the rail, and the two planes.
//!
//! Draws, and owns no state. Everything here is a builder that takes the facts
//! and returns an element, so `main.rs` holds the state and this file holds the
//! look. Sizes are read from `theme`, and the hover blend arrives as a `t`
//! because the clock lives in `motion`.
//!
//! The layout is superset's, region for region (`_dashboard/layout.tsx`):
//!
//! ```text
//!   +------------+-------------------------------------+-------------+
//!   | rail       |  top bar (one row of controls)      |             |
//!   |  header    +-------------------------------------+  sidebar    |
//!   |  projects  |  content head (the group strip)     |             |
//!   |  sessions  +-------------------------------------+             |
//!   |  footer    |  the conversation                   |             |
//!   +------------+-------------------------------------+-------------+
//! ```
//!
//! Nothing here floats: superset separates planes with hairlines, and a card
//! with a shadow in a warm theme reads as a different product. The two shadows
//! that are used are theirs, on a control and on the empty state's mark.

use gpui_kit::assets::IconName;
// The scrollbar element, which draws a bar inside the window with our own tokens:
// gpui's `div` scroll reserves the room for one and paints no chrome.
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::md;
use crate::state::{Changes, Changed, Icon, MenuRow, Node, Project, Row, Session, Step, Viewing};
use crate::state;
use crate::{motion, theme};

// ---------------------------------------------------------------- marks

/// The head of an open file: what it is, and the way back.
///
/// This was one line of 11px mono grey with a bare `X` at the end, which is the
/// shape a debug readout takes rather than a header. A path is a *location* and a
/// header is about the thing itself, so the file's own name is the title in the
/// interface face (readable) and its directory is the context underneath in mono
/// (scannable). The close control is the same icon button the rest of the window
/// uses, so it has a face to light up.
pub fn file_head(path: &str, t: f32) -> Div {
    // The name, and what it sits in. A path is split at its last separator; a
    // path with none is all name.
    let (parent, name) = match path.rsplit_once('/') {
        Some((dir, file)) => (dir, file),
        None => ("", path),
    };
    h_flex()
        .w_full()
        .h(px(theme::HEAD))
        .flex_shrink_0()
        .items_center()
        .gap(px(theme::SPACE_SM))
        .px(px(theme::SPACE_MD))
        .border_b_1()
        .border_color(theme::line())
        .child(icon(IconName::FileDiff, 14.0, theme::muted()))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(px(theme::BODY))
                        .text_color(theme::text_soft())
                        .child(name.to_string()),
                )
                .children((!parent.is_empty()).then(|| {
                    div()
                        .w_full()
                        .truncate()
                        .font(font(theme::MONO))
                        .text_size(px(theme::MICRO))
                        .text_color(theme::ghost())
                        .child(parent.to_string())
                })),
        )
        .child(
            div().flex_shrink_0().child(icon_button(IconName::X, t, theme::faint())),
        )
}

/// A line icon at a size and a tone.
///
/// gpui's own `IconName` draws at the text size in the text colour, so a mark
/// that is meant to be 15px in the muted tone says so here rather than at every
/// call site. The `flex_shrink_0` matters: an icon in a row that can be squeezed
/// is the first thing to go, and a row that loses its mark reads as broken.
pub fn icon(name: IconName, size: f32, tone: Hsla) -> Div {
    div().flex_shrink_0().text_size(px(size)).text_color(tone).child(name)
}

/// A provider's own brand mark, drawn from the vendored set.
///
/// `svg()` with the path the `Brands` source serves. gpui tints an svg with the
/// text colour, so a mark that should keep its own colours must be given none: a
/// logo recoloured into this palette is no longer the recognisable thing it was
/// there to be.
pub fn brand(file: &str, size: f32) -> Div {
    // The size and the colour go on the `svg` itself, not on a box around it.
    // gpui's `Svg` lays out to its own style, so an unsized one is a zero-sized
    // mark inside a correctly sized parent, which draws nothing at all; and its
    // paint reads `style.text.color`, which it takes from itself before it
    // inherits.
    div().flex_shrink_0().child(
        svg()
            .path(format!("brands/{file}"))
            .size(px(size))
            .text_color(theme::text()),
    )
}

/// A provider's mark when it has one, and the letter when it does not.
///
/// A wrong mark is worse than no mark, so this never falls back to a Lucide
/// lookalike: a provider the window does not know wears its own first letter.
pub fn provider_mark(provider: &str, size: f32) -> Div {
    match crate::state::brand(provider) {
        Some(file) => brand(file, size),
        None => div()
            .flex_shrink_0()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::R_CHIP))
            .bg(theme::chip())
            .text_size(px(size * 0.6))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::muted())
            .child(
                provider
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
            ),
    }
}

/// A plain dot, the mark a row wears for a state. Their `size-1.5` is 6px; this
/// is 7, which is the smallest dot that survives a non-retina capture.
pub fn dot(tone: Hsla) -> Div {
    div().flex_shrink_0().size(px(7.0)).rounded_full().bg(tone)
}

/// The mark for a row that is working, turned by the app's own clock.
///
/// `circle-dashed`, breathing: a dashed ring pulsing between two opacities reads
/// as *in progress* the way a spinner does, and it is drawn with the same paint-time
/// clock `working_dot` already uses. A real rotating arc is not available here -
/// gpui-pre has no element rotation, and a canvas for a 13px glyph in a rail row is
/// not worth what it would cost - so this is the honest version of the same idea,
/// and it is the same phase for every working row.
pub fn spinner(size: f32, tone: Hsla) -> Div {
    let phase = motion::pulse_phase();
    div()
        .flex_shrink_0()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(IconName::CircleDashed, size, tone.opacity(0.45 + 0.55 * phase)))
}

// ---------------------------------------------------------------- the rail
//
// The rail answers "where is my work", and a project is the unit: their own
// sidebar is a list of projects, each holding the branches and worktrees that
// stand in it (`WorkspaceSidebar/ProjectSection`). So it is projects, and under
// each one the sessions standing there.

/// The rail's own head: what the column is, a count, and the one control that
/// starts a conversation.
///
/// Theirs is three nav rows (Workspaces, Tasks, Pull requests) over a
/// `New Workspace` button, inside a `border-b` strip. The other two are the
/// product's own surfaces and drawing them here would be three labels that do
/// nothing; the strip, the name and the new-conversation control are the part
/// that is real.
///
/// The control is **a word and a mark**, not a bare `+`: a lone glyph in the
/// corner of a column is a thing you have to press to find out what it does, and
/// this one starts a session, which is the whole product. It is a ghost control,
/// so at rest it is a quiet row and the pointer is what makes it a target.
pub fn rail_head(sessions: usize, new_t: f32) -> Div {
    let tone = motion::mix(theme::muted(), theme::text(), new_t);
    h_flex()
        .w_full()
        .h(px(theme::HEAD))
        .flex_shrink_0()
        .items_center()
        .gap(px(theme::SPACE_SM))
        // The group's own inset plus a row's, so this head's mark lines up with the
        // project marks under it instead of standing 6px to their left. The rail has one
        // left edge and everything in it lands on it (`docs/design/02-sidebar.md`).
        .px(px(theme::SPACE_SM + theme::ROW_PAD))
        .child(icon(IconName::Layers2, 15.0, theme::text()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::text())
                .child("Sessions"),
        )
        // A count, quiet, and only when there is one to give. It goes quiet under
        // the pointer so the new-session control has the room it needs, rather
        // than both of them being squeezed.
        .children((sessions > 0 && new_t < 0.5).then(|| {
            div()
                .flex_shrink_0()
                .text_size(px(theme::SMALL))
                .text_color(theme::ghost())
                .child(sessions.to_string())
        }))
        .child(
            h_flex()
                .flex_shrink_0()
                .h(px(theme::CONTROL_SM))
                .items_center()
                .gap(px(6.0))
                .rounded(px(theme::R_MD))
                .bg(motion::mix(theme::ink(0.0), theme::wash_selected(), new_t))
                .px(px(theme::SPACE_SM))
                .child(icon(IconName::SquarePen, 13.0, tone))
                .child(
                    div()
                        .text_size(px(theme::SMALL))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(tone)
                        .child("New"),
                ),
        )
}

/// One heading's row: its mark, its name, how many sessions are under it, and the
/// chevron that folds it. Their `ProjectHeader`, which is a name, a parenthesised
/// count, a plus and a chevron.
///
/// `mark` is `None` for a **loose** directory: one the user never promoted, kept
/// here only because sessions ran in it. It wears a folder, not a project's own
/// thumbnail, and it carries no plus - promoting a directory is the user's act and
/// this row is not the place it is offered. A control that does nothing is worse
/// than a control that is not there.
pub fn project_head(
    project: &Project,
    mark: Option<&Icon>,
    open: bool,
    hover_t: f32,
    plus_t: f32,
) -> Div {
    h_flex()
        .w_full()
        // `h-8 ... gap-2.5 px-(--sidebar-row-content-inset) rounded-(--control-radius)` -
        // `SidebarProjectFolderHeader.tsx:77`. 32 tall, 10 between the mark and the name,
        // 10 at each side, 8 round: `CONTROL`, `ROW_PAD`, `ROW_PAD`, `R_MD`.
        .h(px(theme::CONTROL))
        .flex_shrink_0()
        .items_center()
        .gap(px(theme::ROW_PAD))
        .px(px(theme::ROW_PAD))
        .rounded(px(theme::R_MD))
        .bg(motion::mix(theme::ink(0.0), theme::wash_hover(), hover_t))
        .child(match mark {
            Some(m) => thumbnail(&project.name, m),
            None => div()
                .flex_shrink_0()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(IconName::Folder, 14.0, theme::ghost())),
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::BODY))
                .font_weight(if project.promoted { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                .text_color(if project.promoted { theme::text() } else { theme::text_soft() })
                .child(project.name.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(theme::SMALL))
                .text_color(theme::ghost())
                .child(project.sessions.len().to_string()),
        )
        // The plus reveals under the pointer, the way every trailing control in
        // their rail does, and keeps its width when it is not there so the count
        // never steps sideways. Loose headings have none, and keep the same width
        // anyway so every row's count lands in the same column.
        .child(
            div()
                .flex_shrink_0()
                .size(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::R_CHIP))
                .bg(motion::mix(theme::ink(0.0), theme::wash_selected(), plus_t))
                .children(project.promoted.then(|| {
                    icon(IconName::Plus, 13.0, motion::mix(theme::ink(0.0), theme::muted(), plus_t))
                })),
        )
        // The chevron is not a control of its own: the whole head is the fold,
        // so a second hover target inside it would be the same click twice.
        .child(
            div()
                .flex_shrink_0()
                .size(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(
                    if open { IconName::ChevronDown } else { IconName::ChevronRight },
                    13.0,
                    theme::faint(),
                )),
        )
}

/// The project's mark: their `ProjectThumbnail`.
///
/// Theirs is `size-6 rounded` (24px) with a 1.5px border, and it holds the
/// project's own icon when there is one: an emoji, a gradient circle, or an
/// uploaded image. With none set it is the first letter on `bg-muted`.
///
/// The image form (an `img:data:` URL) is not drawn: this window has no image
/// pipeline, and their own `ProjectThumbnail` falls back to the letter when the
/// image fails to load. That fallback is what happens here, except that the
/// border keeps the project's own gradient so the fact that an image is set is
/// not lost.
pub fn thumbnail(name: &str, icon: &Icon) -> Div {
    let frame = div()
        .flex_shrink_0()
        // `size-4` on the rail's folder header (`SidebarProjectFolderHeader.tsx:79`).
        .size(px(16.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::R_CHIP));
    match icon {
        Icon::Glyph(g) => frame
            .bg(theme::chip())
            .text_size(px(theme::SMALL))
            .child(g.clone()),
        Icon::Gradient(n) => {
            let (a, b) = theme::gradient(*n);
            frame.bg(a).text_size(px(theme::MICRO)).font_weight(FontWeight::MEDIUM).text_color(b).child(
                name.chars().next().unwrap_or('?').to_uppercase().to_string(),
            )
        }
        // Nothing set, and an image that cannot be decoded: the letter, which is
        // their own fallback, on the muted face.
        Icon::Image | Icon::Letter => frame
            .border_1()
            .border_color(theme::line())
            .bg(theme::chip())
            .text_size(px(theme::MICRO))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::muted())
            .child(name.chars().next().unwrap_or('?').to_uppercase().to_string()),
    }
}

/// A session row's state: t3code's vocabulary, and its precedence.
///
/// Their `resolveSidebarThreadStatus` walks the same ladder and gives up the same
/// thing: **one state per row**, the most urgent one, because a row showing two
/// states shows neither. The order is theirs and it is argued from what the person
/// can do about it - a question blocks the run, so it wins over everything; then a
/// failure, which needs an answer before anything else moves; then a run in flight;
/// then a turn that finished while you were elsewhere. `Settled` is the resting
/// state, and it is a real one: a conversation nobody owes anything on.
///
/// Two of theirs are left out on purpose. `approval` and `input` are both the
/// worker blocked on a person, and this window already draws that once, in the
/// composer, where the Yes and No are: the same fact on the rail would be a state
/// with no button in it.
///
/// It is a fact about **this window**, not about the record. The worker holds one
/// session, so exactly one row can be working and only it can be asked a question;
/// nothing here is guessed from a file.
///
/// The variants are declared **least urgent first**, so `Ord` is the ladder and a
/// heading can ask for the worst state underneath it with a plain `max`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum State {
    /// Nothing is owed on it.
    Settled,
    /// A turn finished while you were looking at something else.
    Waiting,
    /// A run is in flight.
    Working,
    /// This session's last turn failed.
    Failed,
    /// The worker is blocked on a question in this session.
    Ask,
}

impl State {
    /// The word a row shows for it.
    ///
    /// t3code's own vocabulary, in its own title case: `Working`, `Approval`, `Done`,
    /// `Failed` (`SidebarThreadRow.tsx:723`-`:778`). This is the file's largest finding -
    /// the source draws a **word plus icon** and never a dot, because a dot cannot say
    /// which of three needs-you states it means and forty rows of coloured dots is a
    /// Christmas tree. `Settled` says nothing at all: it is the absence of a state, so
    /// the row keeps showing its age instead.
    pub fn label(self) -> &'static str {
        match self {
            State::Ask => "Approval",
            State::Failed => "Failed",
            State::Working => "Working",
            State::Waiting => "Done",
            State::Settled => "",
        }
    }

    /// The mark that stands beside the word, theirs: `CircleDashedIcon` while a run is in
    /// flight, `ShieldQuestionIcon` for an approval, `CircleCheckIcon` for a finished run
    /// nobody has read, `CircleAlertIcon` for a failure.
    pub fn mark(self) -> Option<IconName> {
        match self {
            State::Ask => Some(IconName::ShieldQuestionMark),
            State::Failed => Some(IconName::CircleAlert),
            State::Working => Some(IconName::CircleDashed),
            State::Waiting => Some(IconName::CircleCheckBig),
            State::Settled => None,
        }
    }

    /// The hue the word and its mark wear. `Settled` is the quiet one, which is
    /// what makes the other four read as states rather than as decoration.
    ///
    /// `Done` is green (`success`), not `unread`: the source's unread is "the run
    /// finished and you have not looked", and it renders as that word in the success
    /// colour (`SidebarThreadRow.tsx:775`-`:778`). The blue `unread` token stays for the
    /// job it was made for and gets no second meaning here.
    pub fn tone(self) -> Hsla {
        match self {
            State::Ask => theme::state_permission(),
            State::Failed => theme::state_failed(),
            State::Working => theme::state_working(),
            State::Waiting => theme::success(),
            State::Settled => theme::ghost(),
        }
    }

    /// Whether the row's trailing slot is this state's to take. A settled row is
    /// not a state, it is the absence of one, so it keeps showing its age.
    pub fn speaks(self) -> bool {
        self != State::Settled
    }
}

/// The state for one session row, from the three facts the window actually has.
///
/// `held` is the one session the worker is holding and `busy`/`asking`/`failed` are
/// its live state, so the first three rungs are only ever true of that row. `unseen`
/// is this window's own memory of a turn that finished while you were elsewhere.
/// The ladder is t3code's; see [`State`] for why it is ordered the way it is.
pub fn row_state(
    id: &str,
    held: &str,
    busy: bool,
    asking: bool,
    failed: bool,
    unseen: &std::collections::HashSet<String>,
) -> State {
    if held == id {
        if asking {
            return State::Ask;
        }
        if failed {
            return State::Failed;
        }
        if busy {
            return State::Working;
        }
    }
    if unseen.contains(id) {
        return State::Waiting;
    }
    State::Settled
}

/// A session's row.
///
/// **One line, 36px, whatever it holds.** `flex h-9 items-center gap-2.5 px-2.5`
/// (`SidebarThreadRow.tsx:1223`): `ROW`, `ROW_PAD`, `ROW_PAD`. t3code's rail uses the
/// one-line `slim` and `folder` layouts - the two-line layouts in that file are for a
/// row that carries a branch, a machine and a diff stat, and a rail of forty of those
/// is a wall. A second line is what made this rail read as a list of paragraphs.
///
/// Inside a project the row is indented `ps-9` (`:1228`) - `ROW`, one row's height as
/// the indent, which is the chevron and the folder mark wide - so a session's title
/// lines up with the project's own name above it.
///
/// The trailing slot is where a row says what it is doing, and it says it in a **word
/// and an icon** (`:1268`, `:1313`): never a dot. A settled row has nothing to say and
/// shows its age instead, in the same fixed 32px slot so the column does not jitter
/// as ages tick over.
///
/// The selected row wears a wash of the *foreground* (`--fill-selected`, 10%),
/// their own rule, and not a left bar: an accent edge down every selection is a
/// second accent in the window, and the accent means "you can act on this".
pub fn session_row(
    session: &Session,
    selected: bool,
    hover_t: f32,
    in_folder: bool,
    state: State,
    on_rename: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    on_delete: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    on_archive: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> Div {
    let wash = if selected {
        theme::wash_selected()
    } else {
        motion::mix(theme::ink(0.0), theme::wash_hover(), hover_t)
    };
    let name_tone = if selected { theme::text() } else { theme::text_soft() };
    let mut row = h_flex()
        .w_full()
        .h(px(theme::ROW))
        .flex_shrink_0()
        .items_center()
        .gap(px(theme::ROW_PAD))
        .rounded(px(theme::R_SM))
        .bg(wash)
        .pl(px(if in_folder { theme::ROW } else { theme::ROW_PAD }))
        .pr(px(theme::ROW_PAD))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::BODY))
                .line_height(px(theme::NAME_LINE))
                .font_weight(if selected { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                .text_color(name_tone)
                .child(session.label()),
        );
    // The trailing slot, and it is one slot. The age is what a settled row says;
    // while a run is in flight or a turn is waiting on you, the **state** takes the
    // slot, because that is the thing that wants attention. Under the pointer it is
    // the three row actions instead - one slot, and what is in it is whatever you are
    // most likely to want from this row right now.
    if hover_t > 0.02 {
        let t = hover_t;
        // Rename, delete, archive: the three things you want from a row of your own
        // history, in the order you want them least. Every one stops the event, because
        // the row is itself a click target and pressing one of these must not also
        // select the row - which is where a rename prompt would land on the wrong
        // session.
        row = row.child(rail_action(
            format!("rename:{}", session.id),
            IconName::Pencil,
            t,
            on_rename,
        ));
        row = row.child(rail_action(
            format!("delete:{}", session.id),
            IconName::Trash,
            t,
            on_delete,
        ));
        row = row.child(rail_action(
            format!("archive:{}", session.id),
            IconName::Archive,
            t,
            on_archive,
        ));
        return row;
    }
    row.child(
        h_flex()
            // `relative ml-auto flex h-6 min-w-8 shrink-0 items-center justify-end`:
            // 32 is the floor, so an age going from `9m` to `12m` does not move the
            // column (`SidebarThreadRow.tsx:1268`).
            .flex_shrink_0()
            .h(px(theme::SPACE_XL))
            .min_w(px(theme::CONTROL))
            .items_center()
            .justify_end()
            .gap(px(theme::SPACE_XS))
            .child(if state.speaks() {
                h_flex()
                    .items_center()
                    .gap(px(theme::SPACE_XS))
                    .text_size(px(theme::SMALL))
                    .text_color(state.tone())
                    .children(state.mark().map(|m| icon(m, 12.0, state.tone())))
                    .child(state.label())
                    .into_any_element()
            } else {
                div()
                    .text_size(px(theme::SMALL))
                    .text_color(theme::muted())
                    .child(session.age())
                    .into_any_element()
            }),
    )
}

/// One of a rail row's trailing controls: a 20px target that lifts under the pointer.
///
/// Every one stops its own mouse-down, because the row underneath is a click target
/// too: without that, renaming a session would also select it, and deleting one would
/// select the row it just removed.
fn rail_action(
    key: String,
    mark: IconName,
    t: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(key.into()))
        .flex_shrink_0()
        .size(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::R_CHIP))
        .cursor_pointer()
        .bg(motion::mix(theme::ink(0.0), theme::wash_selected(), t))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |ev, w, cx| on_click(ev, w, cx))
        .child(icon(mark, 13.0, motion::mix(theme::ghost(), theme::muted(), t)))
}

/// The scrim that fades the transcript out as it goes under the composer.
///
/// Without it a row would be cut off mid-glyph by the card's edge, which reads as a bug
/// rather than as depth. It is the page's own colour, so the fade is into the page and not
/// into a grey band: clear at the top, solid by the foot.
pub fn composer_scrim(height: f32) -> Div {
    div()
        .absolute()
        .left(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .h(px(height))
        .bg(linear_gradient(
            180.0,
            linear_color_stop(theme::page().opacity(0.0), 0.0),
            linear_color_stop(theme::page(), 1.0),
        ))
}

/// A group heading: sentence case and quiet. An uppercase grey micro-label is
/// the strongest "a tool made this" tell there is.
///
/// Their `CollapsibleSectionHeader` is `h-8 ... gap-2 ... text-xs font-medium` at
/// `px-2` inside a `mx-0.5 h-8` wrapper (`collapsible-section-header.tsx:30`,
/// `SidebarListMarkers.tsx:125`) - so 32 tall, 11px type, and a rule beside the
/// name that fills the row. The wrapper's 2px plus the group's 8px inset is the same
/// 10px a row's content sits at, which is what makes the whole rail line up.
///
/// **No hover style at all**: the source's class list carries none, so the head is
/// where the pointer does nothing.
pub fn group_head(label: &str) -> Div {
    h_flex()
        .w_full()
        .h(px(theme::CONTROL))
        .flex_shrink_0()
        .items_center()
        .gap(px(theme::SPACE_SM))
        .px(px(2.0))
        .child(div().text_size(px(theme::SMALL)).text_color(theme::faint()).child(label.to_string()))
        // `h-px min-w-2 flex-1` in `bg-sidebar-border/50` - a hairline, not a border:
        // 50% of the rail's 8% white.
        .child(
            div()
                .flex_1()
                .min_w(px(theme::SPACE_SM))
                .h(px(1.0))
                .bg(theme::hairline(0.04)),
        )
}

/// The rail's foot: add a repository, and settings. Their own footer is exactly
/// these two controls, a ghost button that takes the width and an icon beside it.
///
/// `flex flex-col gap-2 px-[--sidebar-content-inset] py-1` with `size-8` utility
/// buttons (`sidebar.tsx:633`, `:722`): 8 at the sides, 4 above and below, 8 between,
/// and 32-high controls. No rule above it - the source draws none, and the rail is
/// already two surfaces deep.
pub fn rail_foot(add_t: f32, gear_t: f32) -> Div {
    h_flex()
        .w_full()
        .flex_shrink_0()
        .items_center()
        .gap(px(theme::SPACE_SM))
        .px(px(theme::SPACE_SM))
        .py(px(theme::SPACE_XS))
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.))
                .h(px(theme::CONTROL))
                .items_center()
                .gap(px(theme::SPACE_SM))
                .rounded(px(theme::R_MD))
                .px(px(theme::SPACE_SM))
                .bg(motion::mix(theme::ink(0.0), theme::wash_selected(), add_t))
                .child(icon(
                    IconName::FolderPlus,
                    14.0,
                    motion::mix(theme::muted(), theme::text(), add_t),
                ))
                .child(
                    div()
                        .truncate()
                        .text_size(px(theme::BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(motion::mix(theme::muted(), theme::text(), add_t))
                        .child("Add repository"),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .size(px(theme::CONTROL))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::R_MD))
                .bg(motion::mix(theme::ink(0.0), theme::wash_hover(), gear_t))
                .child(icon(IconName::Settings2, 15.0, motion::mix(theme::muted(), theme::text(), gear_t))),
        )
}

/// The rail with nothing in it at all. Their own sidebar's empty state is
/// `flex flex-col items-center gap-2 px-2 py-6 text-center text-xs
/// text-muted-foreground/60` - `Sidebar.tsx:3481`: 8, 24, and one size of type.
pub fn rail_empty() -> Div {
    v_flex()
        .w_full()
        .items_center()
        .gap(px(theme::SPACE_SM))
        .px(px(theme::SPACE_SM))
        .py(px(theme::SPACE_XL))
        .child(
            div()
                .text_size(px(theme::SMALL))
                .text_color(theme::faint())
                .child("No projects yet"),
        )
        .child(
            div()
                .text_size(px(theme::SMALL))
                // The source's "Add project" chip (`Sidebar.tsx:3484`-`:3491`, `R_SM` 6,
                // `size-3` icon) is the rail's foot's own add control here, so this line
                // points at it instead of drawing a second one.
                .text_color(theme::ghost())
                .child("Add a directory below."),
        )
}

// ---------------------------------------------------------------- the top bar

/// The window's one row of controls.
///
/// Theirs holds the sidebar toggle, history, the ports pill and an account menu,
/// and the traffic lights sit inside it at the left. The pieces that are this
/// window's are the toggle and the title of what you are on; the rest are the
/// product's own surfaces and are not drawn as decoration.
///
/// The strip is **not** a button. Only the one control on it is: wrapping the
/// whole bar in a click listener is what made the navbar a full-width hit target
/// for a sidebar toggle.
/// `toggle` is the sidebar button, passed in rather than drawn here.
///
/// That is the whole fix for a bug with two symptoms: this function used to draw
/// its own inert panel icon *and* the caller appended a clickable one after the
/// flex spacer, so the button on screen was the dead copy and the live one sat
/// off at the right edge where nothing showed it. A strip that draws the control
/// and a caller that wraps a different one is two controls on one row, and one of
/// them is always wrong.
///
/// **One plane, not two stacked strips.** The strip and the 40px head under it are
/// the same surface, and there is no border between them: a `border-b` here is
/// exactly what made the shell read as two bars with buttons piled on one
/// (`docs/design/01-window-shell.md`). The plane is `page` across the
/// conversation and `rail` over the rail's own column, so the top of the window
/// continues into the column beneath it instead of cutting across both.
///
/// `band` is the rail's width while it is open. Collapsed, there is no rail to
/// continue, and the toggle stands on the page where the traffic lights' room
/// ends.
pub fn topbar(band: Option<f32>, subtitle: &str, toggle: AnyElement) -> Div {
    let (w, bg) = match band {
        Some(w) => (w, theme::rail()),
        None => (theme::LIGHTS + theme::CONTROL_SM + theme::SPACE_MD, theme::page()),
    };
    h_flex()
        .w_full()
        .h(px(theme::TOPBAR))
        .flex_shrink_0()
        .bg(theme::page())
        .pr(px(theme::SPACE_MD))
        .child(
            h_flex()
                .h_full()
                .w(px(w))
                .flex_shrink_0()
                .items_center()
                .bg(bg)
                // The room the traffic lights need, because they float over this row.
                .pl(px(theme::LIGHTS))
                .pr(px(theme::SPACE_MD))
                .child(toggle),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.0))
                .h_full()
                .items_center()
                .gap(px(theme::SPACE_SM))
                .pl(px(theme::SPACE_MD))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(theme::BODY))
                        .text_color(theme::faint())
                        .child(subtitle.to_string()),
                ),
        )
}

/// The build stamp at the right end of the top bar: which code this is, and when
/// origin has moved past it, how far behind.
///
/// The only thing this window says about itself rather than about the work, so it
/// sits at the far end in the faintest tone the palette has - except when there is
/// an update, which is a thing to act on, and is drawn in the amber rung with the
/// command that acts on it. Silent about a check that could not be made: a warning
/// nobody can do anything about is the banner this window does not have.
pub fn version_chip(behind: Option<usize>) -> Div {
    let (words, tone) = match behind {
        Some(n) => (
            format!(
                "behind {n} {} \u{b7} cuacode doctor --upgrade",
                if n == 1 { "update" } else { "updates" }
            ),
            theme::warning(),
        ),
        None => (format!("v{}", cuacode::version::ID), theme::faint()),
    };
    div()
        .flex_shrink_0()
        .text_size(px(theme::SMALL))
        .text_color(tone)
        .child(words)
}

/// The bar's left button, which is the one control on it.
///
/// A builder of its own because the strip around it is **not** a button: wrapping
/// the whole bar in one listener is what made the navbar a full-width hit target
/// for a sidebar toggle.
pub fn topbar_toggle(t: f32) -> Div {
    icon_button(IconName::PanelLeft, t, theme::faint())
}

// ---------------------------------------------------------------- controls

/// Their `Button`, `outline` variant: an edge, a plate, and a `shadow-xs`.
pub fn outline_button(label: &str, t: f32) -> Div {
    h_flex()
        .flex_shrink_0()
        .h(px(theme::CONTROL))
        .items_center()
        .gap(px(6.0))
        .rounded(px(theme::R_MD))
        .border_1()
        .border_color(theme::line_strong())
        .bg(motion::mix(theme::plate(), theme::chip(), t))
        .shadow(theme::shadow_xs())
        .px(px(theme::SPACE_MD))
        .text_size(px(theme::BODY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::text())
        .child(label.to_string())
}

/// Their `Button`, `default` variant: the one control in this window you can
/// act on, in the accent.
pub fn accent_button(label: &str, t: f32) -> Div {
    h_flex()
        .flex_shrink_0()
        .h(px(theme::CONTROL))
        .items_center()
        .gap(px(6.0))
        .rounded(px(theme::R_MD))
        .bg(motion::mix(theme::accent(), theme::text(), 0.12 * t))
        .px(px(theme::SPACE_MD))
        .text_size(px(theme::BODY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::on_accent())
        .child(label.to_string())
}

/// An icon button, their `size-icon-sm`: a square that lifts under the pointer.
pub fn icon_button(name: IconName, t: f32, tone: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .size(px(theme::CONTROL_SM))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::R_MD))
        .bg(motion::mix(theme::ink(0.0), theme::wash_selected(), t))
        .child(icon(name, 15.0, motion::mix(tone, theme::text(), t)))
}

// ---------------------------------------------------------------- the content

/// The content's own head: the strip the conversation sits under.
///
/// Their `ChatHeader` (`apps/web/src/components/chat/ChatHeader.tsx:279`) is a
/// favicon, the project's name, a literal `/`, then the thread's own title taking
/// the rest of the row at `text-sm font-medium`. It carries **no border under it**:
/// the header and the conversation are one page (`ChatView.tsx:3134`, `bg-background`,
/// `items-center`, and nothing else).
///
/// `path` is the fallback for the first crumb: a session standing somewhere the
/// user never promoted has no project to name, and its own directory is the honest
/// answer rather than the word "project".
pub fn content_head(session: Option<&Session>, project: Option<&str>, path: &str) -> Div {
    let row = h_flex()
        .w_full()
        .h(px(theme::HEAD))
        .flex_shrink_0()
        .items_center()
        .gap(px(theme::SPACE_SM))
        .bg(theme::page())
        .px(px(theme::SPACE_MD));
    let Some(s) = session else {
        return row
            .child(div().text_size(px(theme::BODY)).text_color(theme::ghost()).child("No session"));
    };
    row.child(icon(IconName::SquareTerminal, 14.0, theme::muted()))
        .child(
            div()
                .flex_shrink_0()
                .max_w(px(160.0))
                .truncate()
                .text_size(px(theme::BODY))
                .text_color(theme::muted())
                .child(project.unwrap_or(path).to_string()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(theme::BODY))
                .text_color(theme::ghost())
                .child("/"),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(theme::BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::text())
                .child(s.label()),
        )
}

/// The empty window, built to their `Empty` component's measurements: a centered
/// column at a 24px gap, a raised mark, an 18px title, a muted line, then the
/// actions. This is the one place a sentence earns a line, because a window with
/// no rows has to say what it is.
pub fn empty_state(home: &str, add_t: f32, none_t: f32) -> Div {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(theme::SPACE_XL))
        .child(
            // Their `EmptyMedia variant="icon"`: a 40px rounded box in `bg-muted`
            // with the icon at 24px in the *foreground* tone, not muted.
            div()
                .flex_shrink_0()
                .size(px(40.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::R_LG))
                .bg(theme::chip())
                .child(icon(IconName::FolderPlus, 20.0, theme::text())),
        )
        .child(
            v_flex()
                .items_center()
                .gap(px(theme::SPACE_SM))
                .child(
                    div()
                        // `H1` 20: their empty title is `text-xl font-semibold`
                        // (`ui/empty.tsx:92`), the one new type size 09 asks for.
                        .text_size(px(theme::H1))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child("No projects yet"),
                )
                .child(
                    div()
                        .text_size(px(theme::BODY))
                        .line_height(px(21.0))
                        .text_color(theme::muted())
                        .child("Promote a directory and the sessions standing in it appear here."),
                ),
        )
        .child(
            h_flex()
                .gap(px(theme::SPACE_SM))
                .child(accent_button("Add repository", add_t))
                .child(
                    outline_button("No directory", none_t)
                        .child(icon(IconName::Plus, 14.0, theme::muted())),
                ),
        )
        .child(
            div()
                .max_w(px(520.0))
                .truncate()
                .text_size(px(theme::SMALL))
                .font(font(theme::MONO))
                .text_color(theme::ghost())
                .child(home.to_string()),
        )
}

// ---------------------------------------------------------------- the sidebar

/// One tab of the sidebar's strip: their `getSidebarHeaderTabButtonClassName`,
/// which is `h-full px-3 text-xs`, the active one on `bg-border/30` in the
/// foreground tone and the other on `text-muted-foreground/70` lifting to the
/// muted tone under the pointer.
pub fn sidebar_tab(index: usize, label: &str, on: bool, t: f32) -> Div {
    let mark = if index == 0 { IconName::GitCompareArrows } else { IconName::Files };
    div()
        .h_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(theme::SPACE_MD))
        .bg(if on {
            theme::chip()
        } else {
            motion::mix(theme::ink(0.0), theme::wash_hover(), t)
        })
        .child(icon(mark, 14.0, if on { theme::text() } else { theme::faint() }))
        .child(
            div()
                .text_size(px(theme::SMALL))
                .text_color(if on {
                    theme::text()
                } else {
                    motion::mix(theme::faint(), theme::muted(), t)
                })
                .child(label.to_string()),
        )
}

/// Their `CategorySection`: a collapsible heading with a count badge, then its
/// rows. A category with no files draws nothing at all, which is why an empty one
/// is not a heading over nothing.
///
/// The two live kinds wear their own badge: unstaged on amber, staged on green,
/// exactly as their `liveWorkBadgeClass` does.
pub fn category_head(title: &str, kind: &str, count: usize, open: bool, t: f32) -> Div {
    let tone = match kind {
        "unstaged" => theme::warning(),
        "staged" => theme::diff_added(),
        _ => theme::ghost(),
    };
    h_flex()
        .w_full()
        .h(px(26.0))
        .flex_shrink_0()
        .items_center()
        .gap(px(theme::SPACE_SM))
        .rounded(px(theme::R_CHIP))
        .bg(motion::mix(theme::ink(0.0), theme::wash_hover(), t))
        .px(px(theme::SPACE_SM))
        .child(icon(
            if open { IconName::ChevronDown } else { IconName::ChevronRight },
            12.0,
            theme::faint(),
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::SMALL))
                .text_color(theme::muted())
                .child(title.to_string()),
        )
        .child(badge_el(count, tone, kind))
}

/// Their count badge: a small tinted pill. The live kinds carry their own colour
/// and the rest are quiet.
fn badge_el(count: usize, tone: Hsla, kind: &str) -> Div {
    let live = !kind.is_empty();
    div()
        .flex_shrink_0()
        .h(px(16.0))
        .flex()
        .items_center()
        .rounded(px(4.0))
        .bg(if live { tone.opacity(0.15) } else { theme::chip() })
        .px(px(5.0))
        .text_size(px(theme::MICRO))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if live { tone } else { theme::ghost() })
        .child(count.to_string())
}

/// One changed file, as their `FileItem`: a status mark, the file's name, and
/// `+n -n` in the mono face.
///
/// The status colours are quoted from `ChangesView/utils/status.tsx`: added and
/// untracked on green, modified on yellow, deleted on red, renamed on blue,
/// copied on purple. The counts are `font-mono text-[10px]` at reduced opacity,
/// which is what keeps a column of them from shouting.
pub fn file_row(file: &Changed, t: f32, selected: bool) -> Div {
    let tone = status_tone(&file.status);
    h_flex()
        .w_full()
        .h(px(24.0))
        .flex_shrink_0()
        .items_center()
        .gap(px(6.0))
        .rounded(px(theme::R_CHIP))
        .bg(if selected {
            theme::wash_selected()
        } else {
            motion::mix(theme::ink(0.0), theme::wash_hover(), t)
        })
        .px(px(theme::SPACE_MD))
        .child(status_mark(&file.status, tone, file.is_dir()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::SMALL))
                .text_color(if selected { theme::text() } else { theme::text_soft() })
                .child(file.name()),
        )
        // Where it is, in the ghost tone, so two files with one name are two
        // rows. Their row keeps this for a tooltip; this window has none yet.
        .children((!file.parent().is_empty() && !file.is_dir()).then(|| {
            div()
                .flex_shrink_0()
                .truncate()
                .font(font(theme::MONO))
                .text_size(px(theme::MICRO))
                .text_color(theme::ghost())
                .child(file.parent())
        }))
        .children((file.added > 0 || file.removed > 0).then(|| {
            h_flex()
                .flex_shrink_0()
                .gap(px(4.0))
                .font(font(theme::MONO))
                .text_size(px(theme::MICRO))
                .child(div().text_color(theme::diff_added()).child(format!("+{}", file.added)))
                .child(div().text_color(theme::diff_removed()).child(format!("-{}", file.removed)))
        }))
}

// ---------------------------------------------------------------- the file tree
//
// Their `FilesView`: `ROW_HEIGHT` 28, `TREE_INDENT` 10 per level, a chevron on a
// folder, `LuFile`/`LuFolder` for the mark, and a search box on a `border-b`
// strip above it. A tree and not a list: nesting is the information a file
// browser exists to show.

/// How tall a tree row is, and how far each level indents. Their own constants.
pub const TREE_ROW: f32 = 28.0;
pub const TREE_INDENT: f32 = 10.0;

/// One row of the file tree, at a depth.
///
/// A folder wears a chevron and can be opened; a file wears the mark and opens
/// its diff. Both are the same height and the same indent step, because a tree
/// whose rows are different sizes is a tree you cannot scan down.
pub fn tree_row(node: &Node, depth: usize, t: f32, selected: bool, open: bool) -> Div {
    let mark = if node.is_dir() { IconName::Folder } else { IconName::FileText };
    h_flex()
        .w_full()
        .h(px(TREE_ROW))
        .flex_shrink_0()
        .items_center()
        .gap(px(4.0))
        .rounded(px(theme::R_CHIP))
        .bg(if selected {
            theme::wash_selected()
        } else {
            motion::mix(theme::ink(0.0), theme::wash_hover(), t)
        })
        .pl(px(theme::SPACE_SM + TREE_INDENT * depth as f32))
        .pr(px(theme::SPACE_SM))
        .child(
            div()
                .flex_shrink_0()
                .w(px(14.0))
                .flex()
                .items_center()
                .justify_center()
                .children(node.is_dir().then(|| {
                    icon(
                        if open { IconName::ChevronDown } else { IconName::ChevronRight },
                        12.0,
                        theme::faint(),
                    )
                })),
        )
        .child(icon(
            mark,
            14.0,
            if node.is_dir() { theme::muted() } else { theme::ghost() },
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::SMALL))
                .text_color(if selected || node.is_dir() {
                    theme::text_soft()
                } else {
                    theme::muted()
                })
                .child(node.name.clone()),
        )
}

/// Their `getStatusColor`, as this palette's tones.
///
/// **Two of their nine have no tone here, and that is the answer rather than a
/// gap**: git's `renamed` is blue and its `copied` is purple, and this palette spends
/// its blue on `unread` and has no purple at all. They draw `muted` and the *glyph*
/// carries which they are ([`status_mark`]), which is the one signal that does not
/// need a new colour.
fn status_tone(status: &str) -> Hsla {
    match status {
        "added" | "untracked" => theme::diff_added(),
        "modified" => theme::warning(),
        "deleted" => theme::diff_removed(),
        "renamed" | "copied" => theme::muted(),
        _ => theme::ghost(),
    }
}

/// Their `getStatusIndicator`, which is a diff glyph per status. The nearest four
/// in the bundled set, since it does not carry VSCode's own icons.
///
/// A folder is a folder before it is a status: their own `FolderRow` does the
/// same, because a row that stands for a whole directory is a different thing
/// from a row that stands for a file, and a status glyph on it hides that.
fn status_mark(status: &str, tone: Hsla, dir: bool) -> Div {
    let name = if dir {
        IconName::FolderClosed
    } else {
        match status {
            "added" | "untracked" => IconName::FilePlus,
            "deleted" => IconName::FileMinus,
            "renamed" | "copied" => IconName::FileSymlink,
            _ => IconName::FilePen,
        }
    };
    div().flex_shrink_0().flex().items_center().child(icon(name, 12.0, tone))
}

/// The Changes tab's body: the header row, then the three categories.
///
/// Their `ChangesHeader` is a row of small controls on a `px-2 py-1.5` strip and
/// their body is the categories. This is the same, holding the two facts this
/// window actually has: the branch, and how much moved.
pub fn changes_body(changes: &Changes, dir: &str) -> Div {
    let mut col = v_flex().w_full();

    // A directory that is not a checkout has no branch, so it gets no branch
    // row. This row used to be built unconditionally, and the icon is a git mark
    // - a small branching glyph with three nodes - so a plain directory drew a
    // lone *merge icon* in the corner of an otherwise empty column with no name
    // beside it. That is the thing that reads as a stray bug rather than as a
    // state. The check comes first, now.
    if !changes.repo {
        return col.child(v_flex().w_full().child(not_yet_icon(
            IconName::FolderTree,
            "This directory is not a git checkout.",
        )));
    }

    col = col.child(
        h_flex()
            .w_full()
            .h(px(28.0))
            .flex_shrink_0()
            .items_center()
            .gap(px(theme::SPACE_SM))
            .px(px(theme::SPACE_SM))
            .child(icon(IconName::GitBranch, 13.0, theme::muted()))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(theme::SMALL))
                    .text_color(theme::muted())
                    .child(changes.branch.clone()),
            )
            .children((changes.total() > 0).then(|| {
                h_flex()
                    .flex_shrink_0()
                    .gap(px(4.0))
                    .font(font(theme::MONO))
                    .text_size(px(theme::MICRO))
                    .child(div().text_color(theme::diff_added()).child(format!("+{}", changes.added())))
                    .child(div().text_color(theme::diff_removed()).child(format!("-{}", changes.removed())))
            })),
    );

    if changes.total() == 0 {
        // Clean is a *state*, and the one a checkout is in most of the time.
        // It read as `Clean. ~/path` in the corner, which is a sentence about a
        // directory wearing the same grey as everything that failed. A settled
        // tree gets the check, the word, and the path underneath it where a path
        // belongs: a locator, not a sentence's object.
        return col.child(
            v_flex()
                .w_full()
                .items_center()
                .gap(px(theme::SPACE_SM))
                .px(px(theme::SPACE_LG))
                .py(px(28.0))
                .child(icon(IconName::CircleCheckBig, 18.0, theme::diff_added()))
                .child(
                    div()
                        .text_size(px(theme::BODY))
                        .text_color(theme::muted())
                        .child("Working tree clean"),
                )
                .children((!dir.trim().is_empty()).then(|| {
                    div()
                        .max_w(px(180.0))
                        .truncate()
                        .font(font(theme::MONO))
                        .text_size(px(theme::MICRO))
                        .text_color(theme::ghost())
                        .child(dir.to_string())
                })),
        );
    }
    col
}

/// What a file looks like when it is opened: a patch coloured by its lines, or a
/// whole file drawn as code.
///
/// Two things, and the difference matters. A patch's `+` and `-` lines *mean*
/// something, so they are coloured; a whole file's lines mean nothing on their
/// own, and painting them is what made an untracked file a green smear with no
/// readable text in it.
///
/// The size is theirs: their editor is 14px with a 1.625 leading, and this column
/// is narrow, so 13px on a 20px line. Monospace, left-aligned, and **not**
/// wrapped on the patch side, because a wrapped diff code line is a code line you
/// cannot count.
pub fn file_view_pane(view: &Viewing) -> Div {
    match view {
        Viewing::Empty(why) => v_flex().w_full().child(not_yet_icon(IconName::FileQuestionMark, why)),
        Viewing::Whole(text) => code_lines(text, None),
        Viewing::Patch(patch) => code_lines(patch, Some(())),
    }
}

/// The lines of a file, with a line number and a colour per line when it is a
/// patch.
///
/// The gutter is the part that makes code readable at a column's width: without
/// it, a wrapped line and a new line look the same, which is exactly what an
/// unreadable code view is. The gutter is separated from the code by a hairline
/// rather than by more whitespace, because at this width whitespace is the
/// thing there is least of.
fn code_lines(text: &str, patch: Option<()>) -> Div {
    let mut col = v_flex().w_full().py(px(theme::SPACE_XS));
    let mut shown = 0usize;
    for (i, line) in text.lines().take(DIFF_LINES).enumerate() {
        shown = i + 1;
        let (tone, bg) = match patch {
            Some(_) if line.starts_with('+') && !line.starts_with("+++") => {
                (theme::diff_added(), theme::diff_added().opacity(0.09))
            }
            Some(_) if line.starts_with('-') && !line.starts_with("---") => {
                (theme::diff_removed(), theme::diff_removed().opacity(0.09))
            }
            Some(_) if line.starts_with("@@") => (theme::ghost(), theme::chip()),
            // A whole file, and the unchanged lines of a patch: this is the
            // reading tone, not the muted furniture tone. Code that is hard to
            // read has failed at the one thing it was there for.
            _ => (theme::text(), theme::ink(0.0)),
        };
        col = col.child(
            h_flex()
                .w_full()
                .bg(bg)
                .items_start()
                // The number gutter: quiet, mono, right-aligned, so a column of
                // them lines up and the code beside them has a hard edge to start
                // from.
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(38.0))
                        .pr(px(theme::SPACE_SM))
                        .text_size(px(theme::MICRO))
                        .line_height(px(18.0))
                        .font(font(theme::MONO))
                        .text_color(theme::ghost())
                        .text_align(TextAlign::Right)
                        .child((i + 1).to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .font(font(theme::MONO))
                        .text_size(px(theme::SMALL))
                        .line_height(px(18.0))
                        .text_color(tone)
                        .child(line.to_string()),
                ),
        );
    }
    // The ceiling says so, once, at the foot of the code. A file that stops at
    // four hundred lines with no word about it is a file that looks truncated by
    // a bug, and the old pane had no note at all.
    if shown == DIFF_LINES && text.lines().count() > DIFF_LINES {
        col = col.child(
            div()
                .w_full()
                .px(px(theme::SPACE_MD))
                .pt(px(theme::SPACE_SM))
                .font(font(theme::MONO))
                .text_size(px(theme::MICRO))
                .text_color(theme::ghost())
                .child(format!("first {DIFF_LINES} lines")),
        );
    }
    col
}

/// How many lines of a diff the column draws. Their viewer virtualises; this one
/// takes a ceiling and says so, which is the marked kind of compromise.
const DIFF_LINES: usize = 400;

/// The Files tab: the session's own plan, which is the one other true thing this
/// window holds about the session it is showing.
pub fn plan_body(plan: &[(String, String)]) -> Div {
    let mut col = v_flex().w_full().gap(px(1.0));
    if plan.is_empty() {
        return col.child(not_yet_icon(IconName::ListTodo, "No plan for this session."));
    }
    for (text, state) in plan {
        col = col.child(plan_row(text, state));
    }
    col
}

/// One line of a session's own plan (`todo.json`): a state mark and the step.
pub fn plan_row(text: &str, state: &str) -> Div {
    let done = state == "done";
    let dropped = state == "dropped";
    let mark_color = if done {
        theme::success()
    } else if dropped {
        theme::ghost()
    } else {
        theme::warning()
    };
    h_flex()
        .w_full()
        .py(px(3.0))
        .items_start()
        .gap(px(theme::SPACE_SM))
        .child(
            div().flex_shrink_0().pt(px(2.0)).child(if done {
                icon(IconName::Check, 12.0, mark_color)
            } else if dropped {
                icon(IconName::Minus, 12.0, mark_color)
            } else {
                dot(mark_color)
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(theme::SMALL))
                .line_height(px(18.0))
                .text_color(if done || dropped { theme::ghost() } else { theme::muted() })
                .child(text.to_string()),
        )
}

/// A section with nothing in it, said plainly rather than padded with a
/// placeholder row.
///
/// This was a bare line of 13px grey text pinned to the top-left corner of
/// whatever it was dropped into, which is the shape a placeholder takes and
/// reads as one: text that belongs to no box, hugging an edge, saying something
/// a person then has to work out the subject of. An absence is a state, and a
/// state gets a mark, a line, and the air to sit in - the same shape as the
/// window's own empty state, one size down.
///
/// `mark` is a glyph rather than a string so the caller says *what kind* of
/// absence this is: a folder with nothing under it is not the same nothing as a
/// filter that matched nothing.
pub fn not_yet_icon(mark: IconName, sentence: &str) -> Div {
    v_flex()
        .w_full()
        .items_center()
        .justify_center()
        .gap(px(theme::SPACE_SM))
        .px(px(theme::SPACE_LG))
        .py(px(28.0))
        .child(icon(mark, 18.0, theme::ghost()))
        .child(
            div()
                .text_size(px(theme::BODY))
                .line_height(px(18.0))
                .text_color(theme::muted())
                .text_center()
                .child(sentence.to_string()),
        )
}

/// The same, for the callers with no mark of their own to give.
#[allow(dead_code)] // the two call sites that only have a sentence use the icon form for that reason
pub fn not_yet(sentence: &str) -> Div {
    not_yet_icon(IconName::CircleDashed, sentence)
}

// ---------------------------------------------------------------- the transcript
//
// The conversation, which is the product the other two columns are chrome
// around. The vocabulary is the sibling terminal frontend's: a call reads as
// `read(src/main.rs)`, a thought folds to one line.
//
// The row itself is superset's, from `packages/ui/src/components/ai-elements/
// tool-call-row.tsx`, and three details of theirs are what make it read as a
// tool row rather than as a list item:
//
//   - the whole row is **28px tall** (`h-7`) and holds a 12px line. Rows are
//     dense: a session has hundreds of them.
//   - the mark on the left is the tool's icon, **replaced by a chevron while the
//     pointer is on the row**. That is their `isHovered && hasDetails` branch,
//     and it is why the affordance costs no width.
//   - the expanded body is **indented behind a `border-l`** (`ml-2.5 border-l
//     border-border`), not an elbow character. A hairline is a tree; an elbow is
//     a terminal.
//
// Reasoning is theirs too (`ai-elements/reasoning.tsx`): one collapsible row that
// says what it is doing while it does it and what it cost when it is done -
// "Thinking..." while streaming, "Thought for 4 seconds" after. That is the
// answer to a thought that sometimes shows and sometimes does not: it is always
// one row, and what changes is its *label*, not whether it exists.

/// What a markdown block needs from the window's state, bundled because every
/// builder from [`answer`] down needs both and two more parameters each was a wall of
/// arguments with no name.
///
/// - `open` answers whether a `<details>` at `(document, block)` is open.
/// - `press` wraps its summary row so a press can open or shut it. It takes the row
///   and returns it, which is how a `&dyn Fn` can carry a click without an `ElementId`
///   of its own: the caller's closure is the one that owns the id and the listener.
pub struct Notes<'a> {
    pub open: &'a dyn Fn(u64, usize) -> bool,
    pub press: &'a dyn Fn(u64, usize, AnyElement) -> AnyElement,
    /// Whether a fence at `(document, block)` is wrapped, and what its wrap mark does.
    /// The same pair as the disclosure's, for the same reason.
    pub wrapped: &'a dyn Fn(u64, usize) -> bool,
    pub wrap_press: &'a dyn Fn(u64, usize, AnyElement) -> AnyElement,
}

/// Everything a row of the transcript needs from the window that owns it: what a fold
/// does when pressed, whether it is open, what a turn's verbs are, and the markdown
/// state above. One struct rather than six parameters.
pub struct Rows<'a> {
    pub run: &'a dyn Fn(usize, AnyElement) -> AnyElement,
    pub is_open: &'a dyn Fn(usize) -> bool,
    /// A turn's own verbs, wrapped so a press reaches the window. `said` draws them
    /// under the bubble.
    pub retry: &'a dyn Fn(usize, &str, AnyElement) -> AnyElement,
    pub notes: Notes<'a>,
    /// The column's own width. `MEASURE` is the ideal; the caller narrows it when the
    /// window is tight, so the strip beside the text always has somewhere to be.
    pub measure: f32,
}

/// The transcript: the rows a session's own record holds, oldest first.
///
/// Every row is **id'd by its own index** (`row:n`) and that id is the only thing
/// the minimap needs: `scroll_to_top_of_item` takes an index into the scroller's
/// children, and the children are exactly these rows in this order. A jump is
/// therefore "put row N at the top", with no arithmetic about anybody's height.
///
/// `on_rows` is a listener that reports each row's laid-out extent: a `prepaint`
/// listener sees its own children's bounds, and this column's children are exactly the
/// rows - so this is where the row positions come from. They are what makes the
/// minimap say **where** a turn is instead of only how many there are, and what a
/// strip's jump lands on.
pub fn transcript_with(
    rows: &[Row],
    st: &Rows,
    on_rows: Option<Box<dyn Fn(Vec<Bounds<Pixels>>, &mut Window, &mut gpui_kit::App) + 'static>>,
) -> Div {
    let measure = st.measure;
    let mut col = v_flex()
        .w_full()
        .max_w(px(measure))
        // **No gap on the list.** The vertical rhythm is each row's own bottom padding,
        // chosen by what the row is (`MessagesTimeline.tsx:1435`-`:1445`): 8 under a fold or
        // an answer, 16 under anything that ends a turn. A uniform gap is what makes a
        // conversation read as a stack of equal blocks.
        //
        // No horizontal padding either: their row frame is `padding-inline: 0`, and the
        // lane itself is the measure (`index.css:959`). The 24px this used to keep put the
        // text 48px inside the composer's own edges, so the column and the box you answer
        // in disagreed about where the column was.
        .pt(px(theme::SPACE_LG))
        // The floor is the composer's height, and it is **room to scroll past**, not a
        // margin: the composer floats over this column, so without it the last row of a
        // conversation could never be brought out from under the box - the thing you want
        // to read while you answer it. `pin_bottom` lands on this padding, which is why it
        // is padding and not a spacer child.
        .pb(px(theme::COMPOSER_FLOOR_H));
    if rows.is_empty() {
        return col.child(v_flex().w_full().py(px(theme::SPACE_XL)).child(not_yet_icon(
            IconName::MessageSquareDashed,
            "This session has nothing recorded yet.",
        )));
    }
    if let Some(listener) = on_rows {
        col = col.on_children_prepainted(listener);
    }
    for (i, row) in rows.iter().enumerate() {
        // A row's own air, by kind. A turn ends with 16 under it, which is what groups a
        // conversation into turns; a fold and an answer end with 8, which is what keeps
        // the several rows of one turn reading as one block.
        let (el, after) = match row {
            Row::Said(text) => (
                said(
                    text,
                    measure,
                    &format!("said:{i}"),
                    (st.retry)(
                        i,
                        text,
                        icon(IconName::RotateCcw, theme::SMALL + 1.0, theme::faint())
                            .into_any_element(),
                    ),
                )
                .into_any_element(),
                theme::SPACE_LG,
            ),
            Row::Answer(text) => (answer(text, &st.notes).into_any_element(), theme::SPACE_SM),
            Row::Run { note, secs, steps } => (
                run_block(note.as_deref(), *secs, steps, (st.is_open)(i), i, st).into_any_element(),
                theme::SPACE_SM,
            ),
        };
        col = col.child(
            div()
                .id(ElementId::Name(format!("row:{i}").into()))
                .w_full()
                .pb(px(after))
                .child(el),
        );
    }
    col
}


/// A person's turn: the one row that is spoken rather than reported, so it is
/// the one row with a face of its own.
///
/// `group flex flex-col items-end gap-1` (`TimelineUserMessageRow.tsx:446`): the only
/// row in the transcript that is **right-aligned**, and the only one with a bubble
/// (`:470`) - `max-w-[80%] rounded-2xl` on `--message-surface` at `p-3`. Everything the
/// model or a tool says is text on the page.
///
/// `measure` is the lane's width, because the bubble's ceiling is a fraction of it and
/// the lane is what this row is laid out in.
///
/// Under the bubble, the **hover strip** (`TimelineUserMessageRow.tsx:648`):
/// `flex w-full max-w-[80%] items-center justify-end pe-1 text-xs tabular-nums
/// opacity-0 ... group-hover:opacity-100`, at `SMALL` and riding the window's own hover
/// clock (so its fade is `motion::FAST` and nothing here declares a duration).
///
/// It holds **one verb**: copy. The source's other one is retry, and a retry here would
/// be a send from a row into a worker the composer owns - the row has no session binding
/// and `send` reads the field - so the strip carries what this window can actually do.
/// The turn's time is not in the strip either: `messages.jsonl` carries the text and the
/// stamp is the file's own, not the record's, so there is no per-turn time to print.
///
/// ponytail: the copy mark does not swap to a tick for 1200ms (`:183`-`:200`) - that is
/// a timer per row, and the strip appearing under the pointer is the acknowledgement.
///
/// **Not verified on screen.** gpui's `on_hover` never fires under the headless test
/// platform (`tests/shot.rs` parks a pointer there and the blend stays at 0), so this
/// strip's own state is read from the code, not from a frame: what the frame shows is
/// the space it reserves under every turn.
pub fn said(text: &str, measure: f32, key: &str, retry: AnyElement) -> Stateful<Div> {
    let t = motion::hover_t(key);
    motion::mark_hover_el(
        v_flex()
            .w_full()
            .items_end()
            .gap(px(theme::SPACE_XS))
            .child(
                v_flex()
                    .max_w(px(measure * 0.8))
                    .rounded(px(theme::R_COMPOSER))
                    .bg(theme::composer())
                    .p(px(theme::SPACE_MD))
                    .child(
                        div()
                            .text_size(px(theme::BODY))
                            .line_height(px(theme::ANSWER_LINE))
                            .text_color(theme::text())
                            .child(text.to_string()),
                    ),
            )
            // The strip's own height is reserved at rest, so a row does not grow under
            // the pointer; what appears is the control, not the space for it.
            .child(
                div()
                    .w_full()
                    .max_w(px(measure * 0.8))
                    .h(px(theme::NAME_LINE))
                    .flex()
                    .items_center()
                    .justify_end()
                    .when(t > 0.0, |d| {
                        d.child(
                            h_flex()
                                .items_center()
                                .gap(px(theme::SPACE_XS))
                                .child(turn_verb(text, t, key))
                                .child(retry),
                        )
                    }),
            ),
        key,
    )
}

/// The copy verb under a turn.
fn turn_verb(text: &str, t: f32, key: &str) -> Stateful<Div> {
    let text = text.to_string();
    div()
        // An id, because a click handler is a `StatefulInteractiveElement` method: the
        // same trap `rust/AGENTS.md` names for a scroll container.
        .id(ElementId::Name(format!("copy:{key}").into()))
        .flex()
        .items_center()
        .rounded(px(theme::R_CHIP))
        .px(px(theme::SPACE_XS))
        // `SMALL` is the strip's 12px step, and the clock's own value is the fade.
        .opacity(t)
        .cursor_pointer()
        .hover(|s| s.bg(theme::wash_hover()))
        .child(icon(IconName::Copy, theme::SMALL + 1.0, theme::faint()))
        .on_click(move |_, _, cx: &mut gpui_kit::App| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone()));
        })
}

/// The answer: the model's own words, as the markdown it actually is.
///
/// This was one `Div` of raw text, so an answer arrived wearing its own syntax -
/// `**bold**` as four asterisks and a word, `` `code` `` with its backticks.
/// **That is the "the text formatting is shit" this fixes**, and it was never a
/// missing renderer: it was the source being drawn instead of the document.
///
/// Before that it was a word at a time in wrapping flex rows, which is the other
/// failure and worth naming: a flex row gaps its items by a layout constant rather
/// than by a font's own word spacing, so a real paragraph became a stack of boxes.
///
/// So it is blocks, and a block is a paragraph as markdown defines one: the text
/// keeps its words and its own spaces, and only a fence, a list, a heading or a
/// quote gets a box of its own. `md` does the parsing; this does the drawing.
///
/// `relative min-w-0 px-1 py-0.5` (`TimelineAssistantRows.tsx:32`): the lane's full
/// width, 4 at the sides and 2 above and below, and **no fill, no radius, no bubble**.
/// Only the person gets a bubble; the model is text on the page.
pub fn answer(text: &str, notes: &Notes) -> Div {
    let blocks = md::parse(text);
    let doc = doc_id(text);
    let mut col = v_flex().w_full().px(px(theme::SPACE_XS)).py(px(2.0));
    for (i, block) in blocks.iter().enumerate() {
        // The rhythm is the **gap between two blocks**, and it is one `pt` never a
        // `pb`: two 10s around a block would be 20 where CSS's collapsed margins
        // give 10. See [`block_gap`].
        col = col.child(
            div()
                .w_full()
                .pt(px(block_gap(&blocks, i)))
                .child(block_el(block, doc, i, notes)),
        );
    }
    col
}

/// A stable name for a document, so the scroll offset of one code block is not
/// shared with another answer's.
fn doc_id(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// The air above one block: the document's own rhythm.
///
/// Their rule is `margin: 0.65rem 0` on `p, ul, ol, blockquote, pre` with the first
/// child's top and the last child's bottom suppressed (`index.css:1865`-`:1882`), and
/// it is `BLOCK_GAP` 10 rather than the 8 the column used to gap by. The **items of
/// one list are one block**, and `li + li { margin-top: 0.25rem }` (`:2010`) is why
/// two items are `SPACE_XS` apart where two paragraphs are `BLOCK_GAP`.
fn block_gap(blocks: &[md::Block], i: usize) -> f32 {
    if i == 0 {
        return 0.0;
    }
    let item = |b: &md::Block| matches!(b, md::Block::Item { .. });
    if item(&blocks[i]) && item(&blocks[i - 1]) {
        theme::SPACE_XS
    } else {
        theme::BLOCK_GAP
    }
}

/// One block of a document.
///
/// `doc` and `i` are only for a fence's scroll state, which is keyed by where the
/// block is: the window remembers how far into a long code line the reader got, and
/// two blocks must not answer to the same key.
fn block_el(block: &md::Block, doc: u64, i: usize, notes: &Notes) -> AnyElement {
    let sans = font(theme::SANS);
    match block {
        md::Block::Para(spans) => v_flex()
            .w_full()
            .child(runs(spans, theme::BODY, theme::ANSWER_LINE, theme::text(), &sans, md::Align::Start))
            .into_any_element(),
        md::Block::Head { level, spans } => {
            // `h1` is `H1`, a heading above `BODY`; `h2`..`h6` draw at `BODY` weight 600,
            // because a three-step heading ladder in a 736px column is three sizes doing
            // one job (`09-type-and-spacing.md`). The leading is the spec's own row:
            // `ANSWER_LINE` 22 under the 20px title and `LINE` 20 below it.
            // ponytail: `h2`..`h6` share the one rung - a per-level ladder needs its own
            // constants, which step 09 owns.
            let (size, line) = if *level == 1 {
                (theme::H1, theme::ANSWER_LINE)
            } else {
                (theme::BODY, theme::LINE)
            };
            v_flex()
                .w_full()
                .child(runs(spans, size, line, theme::text(), &sans.clone().bold(), md::Align::Start))
                .into_any_element()
        }
        md::Block::Item { mark, spans, .. } => {
            // A **hanging gutter**: the mark wears the first `SPACE_LG` of the row
            // and the item's text starts on that edge, so a wrapped line hangs under
            // the text and not under the bullet. **Nesting changes the mark and not
            // the indent** (`index.css:1922`-`:1944`), which is why `depth` is not
            // read here: `md` has already chosen the mark its level wears.
            h_flex()
                .w_full()
                .items_start()
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(theme::SPACE_LG))
                        .pr(px(theme::SPACE_XS))
                        .text_align(TextAlign::Right)
                        .text_size(px(theme::BODY))
                        .line_height(px(theme::ANSWER_LINE))
                        .text_color(theme::faint())
                        .child(mark.clone()),
                )
                .child(div().flex_1().min_w(px(0.)).child(runs(
                    spans,
                    theme::BODY,
                    theme::ANSWER_LINE,
                    theme::text(),
                    &sans,
                    md::Align::Start,
                )))
                .into_any_element()
        }
        md::Block::Code { lang, text } => {
            code_box(lang.as_deref(), text, format!("code:{doc:x}:{i}"), doc, i, notes)
        }
        md::Block::Quote(spans) => h_flex()
            .w_full()
            .items_start()
            .gap(px(theme::SPACE_MD))
            .child(div().flex_shrink_0().w(px(2.0)).self_stretch().rounded_full().bg(theme::line_strong()))
            .child(div().flex_1().min_w(px(0.)).child(runs(
                spans,
                theme::BODY,
                theme::ANSWER_LINE,
                theme::muted(),
                &sans,
                md::Align::Start,
            )))
            .into_any_element(),
        md::Block::Rule => div()
            .w_full()
            .child(div().w_full().h(px(1.0)).bg(theme::line()))
            .into_any_element(),
        md::Block::Table { head, rows, aligns } => table_box(head, rows, aligns),
        md::Block::Footnotes(notes) => footnotes(notes),
        md::Block::Details { summary, body } => details(summary, body, doc, i, notes),
    }
}

/// A mono line at `SMALL`: their `leading-snug` (1.375) at 11px, which is 15.
fn code_line() -> f32 {
    theme::SMALL + theme::SPACE_XS
}

/// A fenced block: the fence's language over its code, in one box on the plate.
///
/// **Nested boxes**, not a band of text: the shell is `chat-markdown-codeblock
/// overflow-hidden rounded-lg border border-border/70 bg-secondary`
/// (`MarkdownCodeBlock.tsx:66`-`:71`) and the block inside it is a scroll region
/// whose own frame is neutralised while its padding is not (`index.css:2163`-
/// `:2168`). The header is the fence's language, a **copy mark**, and the **wrap
/// toggle** (`MarkdownCodeBlock.tsx:154`-`:241`) - the two verbs the source puts there,
/// and both of them are the window's, not a control this transcript invents.
fn code_box(lang: Option<&str>, text: &str, name: String, doc: u64, i: usize, notes: &Notes) -> AnyElement {
    let wrapped = (notes.wrapped)(doc, i);
    v_flex()
        .w_full()
        // A block keeps its height: this column is inside a scroller, and gpui
        // shrinks a child that does not say otherwise, which is how a tall code
        // block ends up with its text falling out of the bottom of its own box.
        .flex_shrink_0()
        .rounded(px(theme::R_MD))
        .border_1()
        .border_color(theme::line())
        .bg(theme::plate())
        .overflow_hidden()
        // The header exists **with or without a language** (`:74`-`:116` draws a file
        // mark, the label and the verbs even for a bare fence), so the two marks have a
        // row to sit in. Both are per-block state (`Notes`), for the reason `<details>`
        // has it: a block is rebuilt from the answer's text every frame.
        .child(
            h_flex()
                .w_full()
                .flex_shrink_0()
                .items_center()
                .gap(px(theme::SPACE_SM))
                // Their header is `pt-1.5 pr-1.5 pb-0 pl-3` (`:74`-`:77`): room above
                // the label and none below it, because the code's own padding is the
                // space under it.
                .px(px(theme::SPACE_SM))
                .pt(px(theme::SPACE_XS))
                .font(font(theme::MONO))
                .text_size(px(theme::SMALL))
                .line_height(px(code_line()))
                .text_color(theme::text_soft())
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .child(lang.unwrap_or("text").trim().to_string()),
                )
                .child(code_copy(text, &name))
                .child((notes.wrap_press)(
                    doc,
                    i,
                    icon(IconName::TextWrap, theme::SMALL + 1.0, if wrapped { theme::text() } else { theme::faint() })
                        .into_any_element(),
                )),
        )
        .child(if wrapped {
            // Wrapped: no scroll region at all. Their rule is `[data-wrap="true"] pre
            // { white-space: pre-wrap; overflow-wrap: anywhere }` (`index.css:2170`-
            // `:2173`), so the one thing that changes is the wrap.
            div()
                .w_full()
                .flex_shrink_0()
                .px(px(theme::SPACE_MD))
                .py(px(theme::SPACE_SM))
                .font(font(theme::MONO))
                .text_size(px(theme::SMALL))
                .line_height(px(code_line()))
                .text_color(theme::text())
                .child(text.to_string())
                .into_any_element()
        } else {
            // The fence's own scroll region. `overflow_x_scroll` asks gpui for the
            // region and keeps its offset in the element's keyed state, which is why it
            // needs an `id` (`rust/AGENTS.md`, "Traps"). `SCROLLBAR` 6 is the one width
            // the spec asks for, shared with the page's own bar.
            //
            // The tone is the component theme's (`main.rs::dress`), set once for the
            // code region and the page alike: `line` at 78%.
            div()
                .id(ElementId::Name(name.clone().into()))
                .w_full()
                .flex_shrink_0()
                .overflow_x_scroll()
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .flex_shrink_0()
                        // A code line is one line: it keeps its indentation (a code
                        // line's own spaces survive `md`'s parse) and it is the region
                        // that moves, not the wrap.
                        .whitespace_nowrap()
                        .px(px(theme::SPACE_MD))
                        .py(px(theme::SPACE_SM))
                        .font(font(theme::MONO))
                        .text_size(px(theme::SMALL))
                        .line_height(px(code_line()))
                        .text_color(theme::text())
                        .child(text.to_string()),
                )
                .into_any_element()
        })
        .into_any_element()
}

/// A table: the head row over its body, with **horizontal rules only**.
///
/// Their table is `width: 100%; border-collapse: collapse`, cells at `padding:
/// 0.45rem 0.75rem`, and a `border-bottom: 1px solid color-mix(border 60%)` on the
/// head and every body row, with **no head fill** (`index.css:2201`-`:2236`). The
/// target's numbers are in `12`'s table: `BODY` 13, `SPACE_XS` down and `SPACE_SM`
/// across, `line` at 60%; the cell cap of 24rem is dropped rather than becoming a
/// fourth number the palette is not allowed.
///
/// ponytail: a column's width is its longest cell in **characters**, clamped, because
/// gpui has no intrinsic-width flex; see the note in the body. Rows stay aligned, which
/// is the property a table cannot lose.
fn table_box(head: &[Vec<md::Span>], rows: &[Vec<Vec<md::Span>>], aligns: &[md::Align]) -> AnyElement {
    let rule = theme::line().opacity(0.6);
    // **How wide each column is.** A browser lays a table out to its content; gpui has
    // no intrinsic-width flex and no table layout, and measuring text needs the window's
    // text system, which this builder has no handle to. So a column's share is the
    // longest cell in it, in characters - clamped, because one 200-character cell in a
    // three-column table must not squeeze its neighbours to nothing.
    // ponytail: a character count is a proxy for a shaped width. Replace it with a
    // `shape_line` measure if a table ever reads wrong.
    let cols = head.len();
    let mut weights = vec![4usize; cols];
    let mut weigh = |row: &[Vec<md::Span>]| {
        for (c, cell) in row.iter().enumerate().take(cols) {
            let chars: usize = cell
                .iter()
                .map(|s| match s {
                    md::Span::Text { text, .. } => text.chars().count(),
                    md::Span::Break => 0,
                })
                .sum();
            weights[c] = weights[c].max(chars);
        }
    };
    weigh(head);
    for row in rows {
        weigh(row);
    }
    let weights: Vec<f32> = weights.iter().map(|w| (*w).clamp(4, 16) as f32).collect();
    let cell = |spans: &Vec<md::Span>, col: usize, bold: bool, top: f32| {
        let mut f = font(theme::SANS);
        if bold {
            f = f.bold();
        }
        let align = aligns.get(col).copied().unwrap_or_default();
        div()
            .flex_grow(weights.get(col).copied().unwrap_or(4.0))
            .flex_shrink(1.0)
            .flex_basis(px(0.0))
            .min_w(px(0.0))
            .px(px(theme::SPACE_SM))
            .py(px(top))
            .child(runs(spans, theme::BODY, theme::ANSWER_LINE, theme::text(), &f, align))
    };
    let mut col = v_flex().w_full().flex_shrink_0();
    col = col.child(
        h_flex()
            .w_full()
            .items_start()
            .border_b_1()
            .border_color(rule)
            .children(head.iter().enumerate().map(|(c, spans)| cell(spans, c, true, theme::SPACE_SM))),
    );
    for row in rows {
        col = col.child(
            h_flex()
                .w_full()
                .items_start()
                .border_b_1()
                .border_color(rule)
                .children(row.iter().enumerate().map(|(c, spans)| cell(spans, c, false, theme::SPACE_XS))),
        );
    }
    col.into_any_element()
}

/// The foot of the document: a rule, then the notes, small and quiet.
///
/// Theirs is `margin-top: 1.25rem; border-top: 1px; padding-top: 0.75rem; color:
/// muted; font-size: 0.75rem`, `li + li` at `0.35rem` (`index.css:2090`-`:2104`).
/// No rung is 1.25rem, so the top margin is `SPACE_LG`; 0.75rem is `SPACE_MD`, and
/// the notes are `SMALL` in `muted`.
fn footnotes(notes: &[md::Footnote]) -> AnyElement {
    let sans = font(theme::SANS);
    v_flex()
        .w_full()
        .flex_shrink_0()
        .pt(px(theme::SPACE_LG))
        .child(
            v_flex()
                .w_full()
                .border_t_1()
                .border_color(theme::line())
                .pt(px(theme::SPACE_MD))
                .gap(px(theme::SPACE_XS))
                .children(notes.iter().map(|note| {
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap(px(theme::SPACE_XS))
                        .child(
                            div()
                                .flex_shrink_0()
                                .font(sans.clone().bold())
                                .text_size(px(theme::SMALL))
                                .line_height(px(theme::ANSWER_LINE))
                                .text_color(theme::muted())
                                .child(note.id.clone()),
                        )
                        .child(div().flex_1().min_w(px(0.)).child(runs(
                            &note.spans,
                            theme::SMALL,
                            theme::ANSWER_LINE,
                            theme::muted(),
                            &sans,
                            md::Align::Start,
                        )))
                })),
        )
        .into_any_element()
}

/// A disclosure: a summary row over a body, inside two hairlines.
///
/// Theirs is `my-2 border-y border-border/60`, a summary row at `py-2 text-sm
/// font-medium` with a `size-4` chevron rotating 90 degrees, and a body at `pb-3 ps-6`
/// (`MarkdownDetails.tsx:37`-`:57`), which in the target's numbers is `SPACE_SM` above
/// and below the row and `SPACE_XL` down the body's left.
///
/// **The row is a control, and the body is behind it.** Open/shut is the window's own
/// state keyed by `(document, block)`, the same shape the fold's `open_runs` uses: a
/// block cannot own a boolean, because it is rebuilt from the answer's text every
/// frame. gpui-pre has no element rotation, so the chevron is swapped (`ChevronRight`
/// for shut, `ChevronDown` for open) rather than turned; that is the one place this
/// differs from the source's 90-degree turn.
fn details(summary: &str, body: &[md::Block], doc: u64, i: usize, notes: &Notes) -> AnyElement {
    let sans = font(theme::SANS);
    let open = (notes.open)(doc, i);
    let head = h_flex()
        .w_full()
        .items_center()
        .gap(px(theme::SPACE_SM))
        .py(px(theme::SPACE_SM))
        .child(icon(
            if open { IconName::ChevronDown } else { IconName::ChevronRight },
            14.0,
            theme::faint(),
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .font(sans.bold())
                .text_size(px(theme::BODY))
                .line_height(px(theme::ANSWER_LINE))
                .text_color(theme::text())
                .child(summary.to_string()),
        )
        .into_any_element();
    // Their summary row is the button; here the caller's closure is, because it owns
    // the id and the listener.
    let head = (notes.press)(doc, i, head);
    v_flex()
        .w_full()
        .flex_shrink_0()
        .border_t_1()
        .border_b_1()
        .border_color(theme::line().opacity(0.6))
        .child(head)
        .children(open.then(|| {
            v_flex()
                .w_full()
                .pb(px(theme::SPACE_MD))
                .pl(px(theme::SPACE_XL))
                .children(body.iter().enumerate().map(|(n, b)| {
                    div()
                        .w_full()
                        // The same rhythm the column uses, not a flat gap: the items of
                        // one list are `SPACE_XS` apart and two blocks are `BLOCK_GAP`.
                        .pt(px(block_gap(body, n)))
                        .child(block_el(b, doc, i.wrapping_mul(31).wrapping_add(n), notes))
                }))
        }))
        .into_any_element()
}

/// The reference mark a `[^1]` leaves in the prose.
///
/// Theirs is `min-width: 1rem; border-radius: 0.25rem; font-size: 0.6875rem;
/// font-weight: 600` (`index.css:2106`-`:2116`) - `SMALL` 11 at 600 in `muted`, with
/// `SPACE_XS`/2 of air so it does not run into the word before it.
fn note_mark(id: &str) -> AnyElement {
    div()
        .flex_shrink_0()
        .px(px(2.0))
        .font(font(theme::SANS).bold())
        .text_size(px(theme::SMALL))
        .line_height(px(theme::ANSWER_LINE))
        .text_color(theme::muted())
        .child(id.to_string())
        .into_any_element()
}

/// The copy mark in a fence's header (`MarkdownCodeBlock.tsx:154`-`:165`).
///
/// ponytail: no Copy-to-Check swap for 1200ms - that is a timer per block, and the
/// mark dimming under the pointer is the acknowledgement until the block has state.
fn code_copy(text: &str, name: &str) -> Stateful<Div> {
    let text = text.to_string();
    div()
        .id(ElementId::Name(format!("{name}:copy").into()))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::R_CHIP))
        .px(px(theme::SPACE_XS))
        .cursor_pointer()
        .hover(|s| s.bg(theme::wash_hover()))
        .child(icon(IconName::Copy, theme::SMALL + 1.0, theme::faint()))
        .on_click(move |_, _, cx: &mut gpui_kit::App| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone()));
        })
}

/// A run of inline spans as one wrapping line.
///
/// The words carry their own trailing spaces and the row has **no gap**, so a
/// sentence is spaced by its font and not by a layout number. That distinction is
/// the whole difference between prose and a word cloud, and it is why this is not
/// a `StyledText` with runs: a run's background has no padding and no corner, so a
/// code span drawn as one comes out as a highlighter band hugging the glyphs rather
/// than as the box it should be (`rust/AGENTS.md`, "Traps").
///
/// So a code span is drawn as an **inset box per fragment** ([`code_inset`]): a span
/// that wraps across a line break gives each of its words its own box, and the
/// spaces *between* those boxes stay in the row as word gaps, which is the space
/// the old single chip ate.
fn runs(
    spans: &[md::Span],
    size: f32,
    line: f32,
    tone: Hsla,
    base: &Font,
    // Where the words sit in the row: a table column's own alignment, `Start`
    // everywhere else.
    align: md::Align,
) -> AnyElement {
    let mut items: Vec<AnyElement> = Vec::new();
    for span in spans {
        match span {
            // A break forces the wrap: a full-width item cannot share a line with
            // anything, so putting one in the row ends the line at that point.
            md::Span::Break => items.push(div().flex_shrink_0().w_full().h(px(0.)).into_any_element()),
            md::Span::Text { text, style } if style.note => {
                items.push(note_mark(text));
            }
            md::Span::Text { text, style } if style.code => {
                items.extend(code_insets(text, size));
            }
            md::Span::Text { text, style } => {
                for token in tokens(text) {
                    if token.trim().is_empty() {
                        // A run of spaces between two items is a word gap: kept as
                        // its own item because it is where the row may wrap.
                        items.push(word_gap(token.chars().count(), size));
                        continue;
                    }
                    items.push(span_el(&token, *style, size, line, tone, base));
                }
            }
        }
    }
    let row = h_flex().w_full().min_w(px(0.)).flex_wrap().items_baseline();
    let row = match align {
        md::Align::Start => row.justify_start(),
        md::Align::Center => row.justify_center(),
        md::Align::End => row.justify_end(),
    };
    row.child(div().h(px(line))).children(items).into_any_element()
}

/// A word gap between two items, as a box the row may wrap at.
///
/// A plain word carries its own space inside itself, so the font is what spaces a
/// sentence; this is only for the spaces that fall *between* two boxes, where there
/// is no glyph to carry one.
fn word_gap(spaces: usize, size: f32) -> AnyElement {
    div().flex_shrink_0().w(px(spaces as f32 * size * 0.26)).into_any_element()
}

/// A code span as the boxes it wraps into: one inset per word, with the spaces
/// between them kept as word gaps.
fn code_insets(text: &str, size: f32) -> Vec<AnyElement> {
    let mut out: Vec<AnyElement> = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c == ' ' {
            if !word.is_empty() {
                out.push(code_inset(&word));
                word.clear();
            }
            out.push(word_gap(1, size));
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        out.push(code_inset(&word));
    }
    out
}

/// One inset of a code span: the box the glyphs sit in.
///
/// Their inline code is `border-radius: 0.25rem; background: ... 10%;
/// padding: 0.1em 0.35em; font-size: 0.85em` (`index.css:2118`-`:2127`): `R_CHIP`,
/// `wash_selected`, `SPACE_XS` across and 1px down (their `0.1em` at 11px - `SPACE_XS`
/// is a whole line's worth at this size), at `SMALL` in `MONO`. The box's own line is
/// its glyphs, so it hugs the text instead of being a 22px band around it.
fn code_inset(text: &str) -> AnyElement {
    div()
        .flex_shrink_0()
        .rounded(px(theme::R_CHIP))
        .bg(theme::wash_selected())
        .px(px(theme::SPACE_XS))
        .py(px(1.0))
        .font(font(theme::MONO))
        .text_size(px(theme::SMALL))
        .line_height(px(code_line()))
        .text_color(theme::text())
        .child(text.to_string())
        .into_any_element()
}

/// One word, in its own face.
///
/// `base` is the block's own face - sans for a paragraph, bold sans for a heading -
/// and a span's own flags are laid over it, so `**bold**` inside an `h2` and inside a
/// paragraph are both bold without either being a second definition of the font.
fn span_el(
    token: &str,
    style: md::Style,
    size: f32,
    line: f32,
    tone: Hsla,
    base: &Font,
) -> AnyElement {
    let mut f = base.clone();
    if style.bold {
        f = f.bold();
    }
    if style.italic {
        f.style = FontStyle::Italic;
    }
    let ink = if style.link {
        theme::accent()
    } else if style.strike {
        theme::ghost()
    } else {
        tone
    };
    let el = div()
        .flex_shrink_0()
        .font(f)
        .text_size(px(size))
        .line_height(px(line))
        .text_color(ink)
        .whitespace_nowrap()
        // The word keeps the space that follows it, which is how a sentence is
        // spaced by its font and not by a layout gap.
        .child(token.to_string());
    match (style.link, style.strike) {
        (true, _) => el.underline(),
        (_, true) => el.line_through(),
        _ => el,
    }
    .into_any_element()
}

/// A run of text as the words to draw: **each word keeps the space that follows
/// it**, so the gap between two words is the font's own and not a layout number.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if c == ' ' {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A run of work: the note it was working from, its head line, then its steps.
///
/// The head comes **above** what it opens, which is the conventional disclosure and
/// what the chevron on it promises: a press that revealed the steps underneath the
/// line you pressed put the control at the bottom of the thing it controls, so a
/// second press to shut it was a press on something that had moved.
///
/// The steps are a **box** - `run_body` - rather than a bare indent. A run of tool
/// calls is the one part of the conversation that is not prose, and the box is what
/// says so; before it, steps floated in the page as loose mono lines with a hairline
/// down their left and nothing to bind them.
pub fn run_block(
    note: Option<&str>,
    secs: Option<f64>,
    steps: &[Step],
    open: bool,
    index: usize,
    st: &Rows,
) -> Div {
    let failed = steps.iter().any(|s| s.failed);
    let mut col = v_flex().w_full().gap(px(theme::SPACE_XS));

    // The head: the run's own line. Its label is the thought's name and its tail is what
    // the run did, which is what makes the row scannable without being a log.
    if !steps.is_empty() || note.is_some() {
        col = col.child((st.run)(index, run_summary(note, secs, steps, open, failed)));
    }

    // The steps, behind the head. Opened by a press, and the whole run opens as
    // one thing: a call and its result are one row, and a box around each of them
    // would be a stack of cards where a list is wanted.
    //
    // **The reveal is the animation**: the box slides down out of its own head and
    // fades in over `motion::BLOCK`, which is their `block-fade`. A disclosure that
    // appears in one frame is the difference between an app and a web form, and the
    // fold is the one place in this conversation where a box arrives from nothing.
    // Keyed by the run's index so a second run opening does not disturb the first.
    // A thought with no calls opens too: its own text is what is behind the name.
    if open && (!steps.is_empty() || note.is_some()) {
        let mut inner = v_flex().w_full().gap(px(theme::SPACE_SM));
        // **The thought, whole, first.** The head names it in one clause; this is the rest
        // of what it said, as prose rather than as mono text, which is what the source does
        // for an opened reasoning row (`TimelineWorkEntryRow.tsx:120`-`:145` renders
        // markdown there). Without it the name is all the thought there is, and the fold
        // opened onto nothing but its tool calls.
        if let Some(note) = note {
            inner = inner.child(answer(note, &st.notes));
        }
        let mut calls = v_flex().w_full().gap(px(theme::SPACE_XS));
        for step in steps {
            calls = calls.child(step_row(step));
        }
        inner = inner.child(calls);
        let box_ = run_body(inner);
        col = col.child(motion::arrive(
            motion::BLOCK,
            motion::OUT_EXPO,
            ElementId::Name(format!("run-open:{index}").into()),
            -6.0,
            box_,
        ));
    }
    col
}

/// The box a run's steps live in.
///
/// One surface for the whole run, not one per step: a call and its result are one
/// thing, and a box around each of them would be a stack of cards where a list is
/// wanted. It sits on `plate`, one rung above the page, so it reads as raised off the
/// conversation without competing with the composer's card.
pub fn run_body(body: Div) -> Div {
    v_flex()
        .w_full()
        .rounded(px(theme::R_MD))
        .border_1()
        .border_color(theme::line())
        .bg(theme::plate())
        .p(px(theme::SPACE_SM))
        .child(body)
}

/// The run's head line: **what it did**, then how long it worked.
///
/// Theirs is `min-h-6` with a `size-4` icon, `row.summary` as a `block truncate` label
/// and a timestamp strip beside it at `h-6 text-sm ... tabular-nums
/// text-muted-foreground` (`TimelineWorkGroupRows.tsx:374`, `:591`, `:621`, `:295`).
/// **The summary string's wording is `not specified`** in the files that were read -
/// it is built by the timeline projection - so the label stays this tree's own
/// `5 calls \u{b7} read, edit`, which names the count and the kinds (`11`, disagreement 1).
/// The duration takes the timestamp's place: it is the second fact about the same
/// thing, and a person scanning a long session wants both.
///
/// A run the worker could not stamp says `Worked` - not `Worked for 0s`, because
/// an absence is not a zero.
pub fn run_summary(
    note: Option<&str>,
    secs: Option<f64>,
    steps: &[Step],
    open: bool,
    failed: bool,
) -> AnyElement {
    let (label, touched) = summary(secs, steps, note);
    let mut row = h_flex()
        .w_full()
        .h(px(theme::RUN_LINE))
        .items_center()
        .gap(px(theme::SPACE_SM))
        .px(px(theme::SPACE_XS))
        // The mark: their `isPending` branch is a spinner in the mark's place
        // while it works and the tool's own icon when it is done, and a warning
        // on a failure. A brain while it thinks and a terminal once it has, so
        // the same row reads correctly at both ends of a run.
        .child(icon(
            if failed {
                IconName::TriangleAlert
            } else if steps.is_empty() {
                IconName::Brain
            } else {
                IconName::SquareTerminal
            },
            // `size-4`: 16, which is `NAME_LINE`.
            theme::NAME_LINE,
            if failed { theme::danger() } else { theme::faint() },
        ))
        .child(
            // The name is the label, and it owns the width: a name is up to 54 bytes and
            // the tail beside it is short, so the label truncates and the tail keeps its
            // place. A label that could not shrink was clipped by the row's edge instead
            // of ellipsised, which reads as a rendering bug.
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(theme::BODY))
                .line_height(px(theme::ANSWER_LINE))
                .text_color(if failed { theme::danger() } else { theme::text_soft() })
                .child(label),
        );
    // What the row did, quiet, before the chevron: the count and the kinds of its calls
    // and the time they took, which is the second fact about the same thing.
    if !touched.is_empty() {
        row = row.child(
            div()
                .flex_shrink_0()
                .text_size(px(theme::SMALL))
                .text_color(theme::ghost())
                .child(touched),
        );
    }
    row.child(icon(
        if open { IconName::ChevronDown } else { IconName::ChevronRight },
        13.0,
        theme::ghost(),
    ))
    .into_any_element()
}

/// What a run's line says: **what it was doing**, then what it did.
///
/// The head is the thought's own name (`thought_name`) - the rule the terminal frontend
/// names one with, and the reason a fold reads as a row and not as a paragraph. t3code
/// has no wording to copy here: its `row.summary` is built by its timeline projection and
/// is `not specified` in the files that were read.
///
/// The tail is what the row did: the count and the kinds of its calls and the time they
/// took, in the place their timestamp strip sits. A run with no calls is a thought, and
/// its name is the whole of what it has to say, so the tail is only the time. A run the
/// worker could not stamp has no time - an absence is not a zero.
fn summary(secs: Option<f64>, steps: &[Step], note: Option<&str>) -> (String, String) {
    let time = match secs {
        Some(s) => state::duration(s),
        None => String::new(),
    };
    let calls = if steps.is_empty() {
        String::new()
    } else {
        let n = steps.len();
        format!("{n} {} \u{b7} {}", if n == 1 { "call" } else { "calls" }, tools(steps))
    };
    let join = |a: String, b: String| match (a.is_empty(), b.is_empty()) {
        (true, _) => b,
        (_, true) => a,
        _ => format!("{a}  \u{b7}  {b}"),
    };
    match note.map(cuacode::transcript::thought_name) {
        // A thought names the row; what it did and how long it took are the tail.
        Some(name) if !name.trim().is_empty() => (name, join(calls, time)),
        // No thought: the calls are the row's whole subject.
        _ if !calls.is_empty() => {
            let tail = time;
            (calls, tail)
        }
        _ if time.is_empty() => ("Worked".to_string(), String::new()),
        _ => (format!("Worked for {time}"), String::new()),
    }
}

/// The tools a run used, once each, in the order it first used them: that is the
/// shape of what it did, and a name repeated six times is not.
fn tools(steps: &[Step]) -> String {
    let mut names: Vec<String> = Vec::new();
    for s in steps {
        let name = call_name(&s.call);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.join(", ")
}

/// One step: the call, then its result.
///
/// The call's name is plain and the thing it was about is a chip, which is their
/// split: `ClickableFilePath` marks the subject, not the verb.
pub fn step_row(step: &Step) -> Div {
    let tone = if step.failed { theme::danger() } else { theme::muted() };
    let mut col = v_flex().w_full().gap(px(1.0)).child(
        h_flex()
            .w_full()
            .items_start()
            .gap(px(6.0))
            .child(
                div()
                    .flex_shrink_0()
                    .font(font(theme::MONO))
                    .text_size(px(theme::SMALL))
                    .line_height(px(18.0))
                    .text_color(tone)
                    .child(call_name(&step.call)),
            )
            .children(call_target(&step.call).map(|t| {
                div()
                    .flex_shrink_0()
                    .max_w(px(460.0))
                    .min_w(px(0.))
                    .truncate()
                    .rounded(px(theme::R_CHIP))
                    .bg(if step.failed { theme::danger_wash() } else { theme::accent_wash() })
                    .px(px(5.0))
                    .font(font(theme::MONO))
                    .text_size(px(theme::SMALL))
                    .line_height(px(18.0))
                    .text_color(theme::text())
                    .child(t.to_string())
            })),
    );
    if let Some(result) = &step.result {
        col = col.child(
            div()
                .w_full()
                .font(font(theme::MONO))
                .text_size(px(theme::MICRO))
                .line_height(px(15.0))
                .text_color(if step.failed { theme::danger() } else { theme::ghost() })
                .child(result.clone()),
        );
    }
    col
}

/// The name of a call, which is everything before its own `(`.
pub fn call_name(text: &str) -> String {
    text.split_once('(').map(|(n, _)| n.to_string()).unwrap_or_else(|| text.to_string())
}

/// What a call was about: the argument inside its own parentheses.
pub fn call_target(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once('(')?;
    let target = rest.strip_suffix(')')?;
    (!target.trim().is_empty()).then_some(target)
}

/// The window with sessions on disk and none of them picked.
///
/// Their `NoActiveThreadState` body is `max-w-lg px-8 py-12` with a `text-xl
/// font-semibold` title and a `text-sm text-muted-foreground` description
/// (`NoActiveThreadState.tsx:22`-`:27`, `ui/empty.tsx:92`, `:100`), which here is
/// `H1` 20 and `BODY` 13. The one place this differs from the source is the **way out**:
/// their file imports no button, so their empty screen names the problem and offers
/// nothing to do about it (`13-furniture.md`, "The empty state has no way out"). This one
/// gets the action the window already has.
pub fn nothing_selected(
    on_new: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> Div {
    v_flex().size_full().items_center().justify_center().child(
        v_flex()
            .items_center()
            .gap(px(theme::SPACE_MD))
            .child(icon(IconName::MousePointerClick, 20.0, theme::ghost()))
            .child(
                div()
                    .text_size(px(theme::H1))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child("Pick a session to continue"),
            )
            .child(
                div()
                    .text_size(px(theme::BODY))
                    .text_color(theme::muted())
                    .child("Select an existing session or create a new one to get started."),
            )
            .child(motion::mark_on_hover(
                accent_button("New conversation", motion::hover_t("empty:new")),
                "empty:new",
                on_new,
            )),
    )
}


// ---------------------------------------------------------------- the split
//
// One divider, used twice. Both sidebars are resizable and both take the same
// grab strip, so the behaviour is one thing: a 6px strip that draws a 1px line,
// lights it on the accent while the drag is live, and is wider than the line it
// draws because a 1px target is not a target.
//
// Their `ResizablePanel` is the same idea with more: double-click resets to the
// default width, and the width is clamped by the store rather than by the panel.
// Both of those are here too.

/// A sidebar's grab strip. `side` is which edge it sits on, so the line lands on
/// the inside of the strip rather than the outside of the window.
pub fn split(side: &str, dragging: bool) -> Div {
    let line = if dragging { theme::accent() } else { theme::line() };
    let rule = div().h_full().w(px(1.0)).bg(line);
    let rule = if side == "left" { rule.ml(px(2.5)) } else { rule.mr(px(2.5)) };
    h_flex().h_full().w(px(SPLIT_W)).flex_shrink_0().cursor_col_resize().child(rule)
}

/// How wide a sidebar's grab strip is. Not a design token: it is the width of a
/// control that has to be wider than the 1px line it draws, and the strip belongs to
/// the column it sits beside - which is why the topbar's rail band is `RAIL` plus
/// this, or the band stops 6px short of the rail under it.
pub const SPLIT_W: f32 = 6.0;

/// The divider while a drag is live: the strip, painted at the pointer's own x on
/// top of the layout, in the accent.
///
/// This is what makes the commit-on-release resize feel right rather than broken.
/// The window is still laid out at the old width - it has to be, or every pixel
/// of pointer movement would rebuild the transcript - so without a ghost the
/// divider would sit still and then jump on release. A 1px accent line that
/// follows the pointer is the whole difference, and it costs one absolutely
/// positioned element.
pub fn split_ghost(left: f32, height: f32) -> Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(0.0))
        .h(px(height))
        .w(px(1.0))
        .bg(theme::accent())
}

// ---------------------------------------------------------------- menus
//
// The parts of a dropdown, so the panel in `main.rs` is rows and nothing else.
//
// The shape is superset's (`packages/ui/src/components/ui/dropdown-menu.tsx`):
// a heading at 12px muted, an item at 14px with a leading slot, a separator as a
// hairline, and the panel the popover rung with a shadow. What this window adds
// is a group heading and a foot, because its menus are decisions rather than
// command lists.

/// A menu's heading: what the list is choosing.
pub fn menu_heading(title: &str) -> Div {
    h_flex()
        .w_full()
        .h(px(24.0))
        .items_center()
        .px(px(theme::SPACE_SM))
        .child(
            div()
                .text_size(px(theme::SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::faint())
                .child(title.to_string()),
        )
}

/// A group's heading inside a menu: quieter than the menu's own, and with a
/// hairline above it when it is not the first.
pub fn menu_group(label: &str) -> Div {
    v_flex()
        .w_full()
        .gap(px(2.0))
        .pt(px(6.0))
        .child(div().w_full().h(px(1.0)).bg(theme::line()))
        .child(
            h_flex().w_full().h(px(20.0)).items_center().px(px(theme::SPACE_SM)).child(
                div()
                    .text_size(px(theme::MICRO))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::ghost())
                    .child(label.to_string()),
            ),
        )
}

/// One menu item.
///
/// Three states, and each of them looks like itself: **current** wears the accent
/// and a check, **disabled** is dimmed and its note replaces the label's promise
/// with a sentence, and **plain** lifts under the pointer. A row that cannot be
/// picked must not look like one that can: that is the difference between a menu
/// that says "you have not set this up" and one that says "this window is
/// broken".
pub fn menu_item(row: &MenuRow, t: f32, keyboard: bool) -> Div {
    let lit = if keyboard { 1.0 } else { t };
    let dim = if row.enabled { 1.0 } else { 0.45 };
    let value_tone = if row.current {
        theme::text()
    } else if row.enabled {
        motion::mix(theme::text_soft(), theme::text(), lit)
    } else {
        theme::ghost()
    };
    h_flex()
        .w_full()
        .min_h(px(32.0))
        .items_center()
        .gap(px(theme::SPACE_SM))
        .rounded(px(theme::R_CHIP))
        .bg(motion::mix(theme::ink(0.0), theme::wash_selected(), lit))
        .px(px(theme::SPACE_SM))
        .py(px(6.0))
        .child(
            div()
                .flex_shrink_0()
                .w(px(28.0))
                .flex()
                .items_center()
                .gap(px(3.0))
                .child(match &row.brand {
                    Some(file) => brand(file, 14.0).opacity(dim),
                    None => div()
                        .size(px(14.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(IconName::Dot, 14.0, theme::ghost())),
                })
                .children(row.current.then(|| icon(IconName::Check, 12.0, theme::accent()))),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::BODY))
                .text_color(value_tone)
                .child(row.value.clone()),
        )
        .children((!row.note.is_empty()).then(|| {
            div()
                .flex_shrink_0()
                // The note yields to the name. At 180px it was eating the row: a
                // model chip read `deepse...` because its capability list wanted the
                // space, and the name is the one thing a row in this list is for.
                .max_w(px(100.0))
                .truncate()
                .text_size(px(theme::MICRO))
                .text_color(if row.enabled { theme::ghost() } else { theme::warning().opacity(0.8) })
                .child(row.note.clone())
        }))
}

/// A menu's foot: the consequence of choosing, in one quiet line.
pub fn menu_foot(sentence: &str) -> Div {
    v_flex()
        .w_full()
        .gap(px(2.0))
        .pt(px(6.0))
        .child(div().w_full().h(px(1.0)).bg(theme::line()))
        .child(
            div()
                .px(px(theme::SPACE_SM))
                .py(px(4.0))
                .text_size(px(theme::MICRO))
                .line_height(px(15.0))
                .text_color(theme::ghost())
                .child(sentence.to_string()),
        )
}

// ---------------------------------------------------------------- the composer
//
// The one place you tell an agent something: what to do, and the four settings
// that decide how it does it. Their composer (`TiptapPromptEditor`) sits at the
// foot of the content column with a row of chips under it, and this is the same
// shape with the settings this window actually has.
//
// A chip is a control that shows its current value and opens a list when it is
// pressed. Every one is a `Menu` in `main.rs`:
//
//   project     where the session stands - locked once the session has started
//   provider    which account answers
//   model       which model, on that account
//   permissions ask / accept edits / plan / bypass
//
// Nothing is drawn that cannot be changed: a locked chip says why it is locked
// rather than going quietly dead.

/// The composer.
///
/// One card, and everything is inside it: the field on top, then the row of
/// settings and the send control under it. That is superset's own
/// `ComposerBody` - `rounded-2xl bg-card ring-1 ring-border`, an editor in
/// `px-4 pt-3.5 pb-1`, and a footer in `flex min-h-12 items-center gap-2 px-3
/// pb-2.5` - and it is one surface rather than a boxed field with a row of boxed
/// chips beneath it, which is what reads as a form and not as a composer.
///
/// One edge, and the rim: nothing on the card changes when the field has focus,
/// because the source paints no focus state on it (`04-composer-card.md`, states).
/// The caret is where focus shows.
pub fn composer(
    field: AnyElement,
    chips: Vec<AnyElement>,
    send: AnyElement,
    _focused: bool,
    status: Option<AnyElement>,
    prompt: bool,
) -> Div {
    v_flex()
        .w_full()
        .flex_shrink_0()
        .items_center()
        // No gutter: the card's edges are the conversation lane's edges, so the
        // card is `MEASURE` wide like the rows above it.
        .pb(px(theme::SPACE_LG))
        .pt(px(theme::SPACE_SM))
        .child(
            v_flex()
                .w_full()
                .max_w(px(theme::MEASURE))
                .rounded(px(theme::R_COMPOSER))
                // One edge, always there: `line_strong` at rest and while typing, the
                // accent only while this is a rename prompt. `line` is too faint on
                // `composer()`.
                .border_1()
                .border_color(if prompt {
                    theme::accent()
                } else {
                    theme::line_strong()
                })
                // **Opaque.** See `theme::composer`: there is no per-element backdrop
                // blur in gpui-pre, so a translucent sheet here could only ever show a
                // sharp page through itself. Solid, one rung above the page, and the rim
                // plus the hairline are the whole lift: the source's dark card is
                // `shadow-none`, so there is no `float()` here. It stays for menus.
                .bg(theme::composer())
                .shadow(theme::lift())
                .overflow_hidden()
                // The run's own line, when there is one: above the field, because it is
                // about what happens when you press the control below it.
                .children(status.map(|line| {
                    div()
                        .w_full()
                        .px(px(theme::COMPOSER_PAD_X))
                        .pt(px(theme::COMPOSER_PAD_TOP))
                        .child(line)
                }))
                // The field. **On the card, with no ground of its own.**
                //
                // This was a second plane (`composer_field`, #1a1716) sitting inside the
                // #201e1c card - and that is the bug: "why is the
                // background colour of the input box half one gray and half another darker
                // gray". Two surfaces for one control is a box inside a box, and it is the
                // same mistake as the old well with its own corner and rim. t3code's editor
                // is laid **directly on the card** at the card's own padding, so the whole
                // thing is one surface and one colour.
                .child(
                    div()
                        .w_full()
                        .px(px(theme::COMPOSER_PAD_X))
                        .pt(px(theme::COMPOSER_PAD_TOP))
                        .pb(px(theme::COMPOSER_PAD_BOTTOM))
                        .child(
                            // The **two-line floor**, derived: two field lines plus the 8px
                            // the source adds to them (`min-h-[calc(2lh+--spacing(2))]`).
                            // An empty composer is two lines tall, so it does not grow under
                            // the pointer on the first keystroke.
                            h_flex()
                                .w_full()
                                .min_h(px(2.0 * theme::COMPOSER_LINE + theme::SPACE_SM))
                                .child(field),
                        ),
                )
                // The controls row. Their footer is `px-3 pb-3` with 24px controls in it,
                // so this is the controls plus that padding and no reserved height of its
                // own: it used to be a 44px `min_h` under a 40px field, which is where the
                // height was going.
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.))
                        .items_center()
                        // **2px apart, right-aligned** (`06-triggers-and-popovers.md`):
                        // a chip is a label and a chevron with nothing between them but
                        // this gap, so the row reads as one line of words and not as a
                        // strip of buttons. The spacer is *before* the chips, which is
                        // what puts them against the send control.
                        .gap(px(2.0))
                        // 8, not 16: the body's own 16px already holds the chips off
                        // the edge, and the source's row is `px-2 pb-2`.
                        .px(px(theme::SPACE_SM))
                        .pb(px(theme::SPACE_SM))
                        .flex_wrap()
                        .child(div().flex_1().min_w(px(0.)))
                        .children(chips)
                        .child(send),
                ),
        )
}

/// The transcript's minimap: one mark per turn, down the right edge of the
/// conversation.
///
/// The feature, as it was described ("a list of lines... you can
/// jump to them quickly from clicking on other ones"), and t3code has the same
/// strip. Its rules are theirs and they are the ones that make it usable:
///
/// - **One mark per turn**, spaced evenly, in a rail shorter than the column. The
///   strip is a table of contents, not a scale drawing of the scroll height, so a
///   long session does not turn every mark into a hairline.
/// - **The turn the reader is in runs hot**; the rest are quiet, and the strips nearer to
///   it are **shorter** rather than brighter. That is
///   `resolveTimelineMinimapCurrentIndex`: the first mark whose row is in view.
/// - **A press jumps to that turn's row**, not to a computed offset: the row is
///   what "that message" means, and one row's height is not knowable from here.
///
/// `current` is an index into `marks` and `hover` is one too, so the two states
/// are the same shape and neither invents a second kind of highlight.
pub fn minimap(
    marks: &[state::Anchor],
    active: Option<usize>,
    // **The hit box, measured at run time** - see [`map_hit_w`].
    hit_w: f32,
    // `FnMut`, not `Fn`: the call site's closure needs `&mut` to reach the window
    // context, and the loop calls it once per mark, so nothing here needs it twice.
    //
    // It is handed the **target box**, not the strip's face: the face is 2px and the box
    // is `hit_w` by `MAP_SPACING`, and the box is what a pointer has to land on.
    pip: &mut dyn FnMut(usize, Div) -> Stateful<Div>,
) -> Div {
    // One strip per turn, **evenly spaced**, packed close, in order. This is the placement
    // It went back to after a spell of drawing them at each turn's true fraction, which
    // read as a sparse column of floating lines. Even spacing
    // makes it a menu, and turns near each other still show strips near each other.
    //
    // Three rules, each of them from using it:
    //
    // - **No tooltip.** One was here, and hovering produced "a shitty
    //   notification". A 2px strip is not a thing you hover for information.
    // - **Every strip's target is the whole gutter**, not the strip: `hit_w` by
    //   `MAP_SPACING`, tiling with no gaps, so clicking is not a test of aim.
    // - **Grey, and the foreground's tone when in view.** No orange: orange is this
    //   palette's "you can act on this" and a scroll position is not a control.
    let mut col = v_flex().items_end();
    for (i, _mark) in marks.iter().enumerate() {
        let distance = active.map(|a| a.abs_diff(i));
        // **Nearer strips differ by length, not by colour** (`TimelineMinimap.tsx:225`-`:228`
        // scale them 0.667 at distance 1, 0.417 at 2 and 0.333 beyond). Brightness in this
        // palette is already spent on "the turn you are in", so dimming the neighbours
        // too would leave no mark hot; a shorter neighbour reads as "close" and keeps the
        // one you are in the only bright thing on the rail.
        let w = map_strip_w(distance);
        let hue = if distance == Some(0) {
            theme::text_soft()
        } else if motion::hover_t(&format!("pip:{i}")) > 0.35 {
            theme::map_near()
        } else {
            theme::map_rest()
        };
        let face = div()
            .h(px(theme::MAP_STRIP_H))
            .w(px(w))
            .rounded_full()
            .bg(hue);
        col = col.child(pip(
            i,
            v_flex()
                .h(px(theme::MAP_SPACING))
                .w(px(hit_w))
                .items_end()
                .justify_center()
                .child(face),
        ));
    }
    // The rail behind them: their `left-3 h-full w-px bg-border/15`
    // (`TimelineMinimap.tsx:207`) - `line` at 15%, **12px in from the box's edge**
    // (`SPACE_MD`), so it shows in the gap between two marks and nowhere else. Clamped
    // to the measured hit box, because a narrow gutter is narrower than 12.
    let rail = div()
        .absolute()
        .top(px(0.0))
        .bottom(px(0.0))
        .left(px(theme::SPACE_MD.min((hit_w - 1.0).max(0.0))))
        .w(px(1.0))
        .bg(theme::line().opacity(0.15));
    div().relative().w(px(hit_w)).child(rail).child(col)
}

/// How wide a strip's **target** is, from the measured gutter.
///
/// `min(MAP_HIT_W, gutter - SPACE_MD)`, floored at zero
/// (`MessagesTimeline.logic.ts:449`-`:455`): `MAP_HIT_W` 40 is the cap, not the width, so a
/// gutter just wide enough gets a box just wide enough rather than one that runs into the
/// text. 352 while a preview card is open is the source's other bound and has no preview
/// here to apply it to.
pub fn map_hit_w(gutter: f32) -> f32 {
    (gutter - theme::SPACE_MD).clamp(0.0, theme::MAP_HIT_W)
}

/// A strip's own width, by how far its turn is from the one in view.
///
/// `scale-x-[0.667]` at distance 1, `[0.417]` at 2 and `[0.333]` beyond
/// (`TimelineMinimap.tsx:225`-`:228`). `None` is "the reader is nowhere in this session",
/// and every mark is full length there because none of them is the one you are in.
fn map_strip_w(distance: Option<usize>) -> f32 {
    match distance {
        None | Some(0) => theme::MAP_STRIP_W,
        Some(1) => theme::MAP_STRIP_W * 0.667,
        Some(2) => theme::MAP_STRIP_W * 0.417,
        Some(_) => theme::MAP_STRIP_W * 0.333,
    }
}

/// The one control a scrolled-away reader needs: get me back to the end.
///
/// **A 28px circle, not a pill.** The first version was a wide pill carrying the words
/// "3 turns below", and it sat on the reader's own text: a label
/// that big over the conversation is furniture in the reading column, and the count is
/// something you can see from the transcript anyway. So it is the arrow alone, at the
/// size of the send control.
///
/// It has a **face**, which is the one place this differs from the minimap strips: a
/// circle over moving text has to say it is a control, and a hairline rim plus the
/// surface under it is what does that. The count is its tooltip.
pub fn to_bottom(
    below: usize,
    t: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let label = match below {
        0 => "Jump to the end".to_string(),
        1 => "1 turn below".to_string(),
        n => format!("{n} turns below"),
    };
    motion::mark_on_hover_el(
        div()
            // An id: `tooltip` is a `StatefulInteractiveElement` method.
            .id(ElementId::Name("to-bottom-face".into()))
            .size(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(motion::mix(theme::line_strong(), theme::accent(), 0.3 * t))
            .bg(motion::mix(theme::plate(), theme::chip(), t))
            .shadow(theme::shadow_sm())
            .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            .child(icon(
                IconName::ArrowDown,
                14.0,
                motion::mix(theme::muted(), theme::accent(), t),
            )),
        "to:bottom",
        on_click,
    )
    .into_any_element()
}

/// A setting **segment**: one cell of the composer's picker bar.
///
/// It has no border and no fill of its own, because the strip around it is the
/// surface: a cell inside a bordered plate that also drew its own border and its own
/// fill is two boxes for one thing. The strip is the edge; a cell is only lit under
/// the pointer. That is t3code's own control - `rounded-md border
/// border-transparent px-1.75 hover:bg-accent` - a label that lifts, not a pill.
pub fn chip(mark: Option<IconName>, value: &str, t: f32, locked: bool) -> Div {
    let tone = if locked {
        theme::ghost()
    } else {
        motion::mix(theme::muted(), theme::text(), t)
    };
    h_flex()
        // Natural size, and the strip it sits in **wraps** rather than squeezing it: a
        // chip clipped to `...` names nothing, and one crushed to its mark alone reads as
        // an icon with no label. The row gives way, not the chip.
        .flex_shrink_0()
        // `CONTROL_SM` 28: a chip is a control (`06-triggers-and-popovers.md`).
        .h(px(theme::CONTROL_SM))
        .items_center()
        // 5px between a mark and its word; the strip's own gap is what separates
        // cells.
        .gap(px(5.0))
        // `R_MD` 8, not `R_CHIP` 4: `06` is explicit that the ladder's 4 is too
        // tight for a control this tall.
        .rounded(px(theme::R_MD))
        .bg(motion::mix(theme::ink(0.0), theme::wash_hover(), if locked { 0.0 } else { t }))
        .px(px(theme::SPACE_SM))
        // `text-sm` in the source; `BODY` 13 is this window's one body size.
        .text_size(px(theme::BODY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(tone)
        // Every mark is 13px and in the chip's own tone: a set of icons at three
        // weights and three opacities is what reads as a pile of clip art rather
        // than as one control.
        .children(mark.map(|m| icon(m, 13.0, tone)))
        // The value takes the width it needs and is clipped by the row when the
        // row is tight, rather than being shortened into something that no longer
        // names the thing it is for.
        .child(div().min_w(px(0.)).truncate().child(value.to_string()))
        // The chevron is what says this opens something, and it is the control's
        // own size and tone rather than a quieter one: a chevron nobody notices
        // is a chip nobody presses.
        .children((!locked).then(|| icon(IconName::ChevronsUpDown, 12.0, theme::ghost())))
}

/// One segment of the effort track.
///
/// The track is the worker's own ladder laid out left to right, and a segment is
/// lit for every rung up to the one in force; the pointer lifts the one under it.
/// That is the "slider" this window can honestly draw - gpui-pre has no slider
/// control and no element rotation, so a track of rungs is the shape, not a knob.
pub fn effort_seg(filled: bool, t: f32) -> Div {
    let face = if filled {
        motion::mix(theme::accent().opacity(0.65), theme::accent(), t)
    } else {
        motion::mix(theme::chip(), theme::line_strong(), t)
    };
    div().flex_shrink_0().h_full().w_full().bg(face)
}

/// The same cell, with a provider's own logo in place of a glyph.
pub fn chip_brand(provider: &str, value: &str, t: f32, locked: bool, unkeyed: bool) -> Div {
    let tone = if locked {
        theme::ghost()
    } else {
        motion::mix(theme::muted(), theme::text(), t)
    };
    h_flex()
        .flex_shrink_0()
        // `CONTROL_SM` 28: a chip is a control (`06-triggers-and-popovers.md`).
        .h(px(theme::CONTROL_SM))
        .items_center()
        .gap(px(6.0))
        .rounded(px(theme::R_SM))
        .bg(motion::mix(theme::ink(0.0), theme::wash_hover(), if locked { 0.0 } else { t }))
        .px(px(theme::SPACE_SM))
        // `text-sm` in the source; `BODY` 13 is this window's one body size.
        .text_size(px(theme::BODY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(tone)
        .child(provider_mark(provider, 13.0))
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .child(if unkeyed {
                    format!("{value} (no key)")
                } else {
                    value.to_string()
                }),
        )
        .child(icon(
            IconName::ChevronsUpDown,
            12.0,
            if unkeyed { theme::warning().opacity(0.9) } else { theme::ghost() },
        ))
}

/// The send control: superset's own footer button, `size-[26px] rounded-md`.
///
/// Theirs is `bg-secondary` whether it is live or not, and the rule this theme
/// cares about more is that the accent means "you can act on this". So it is the
/// accent the moment there is something to send, and the quiet face when there
/// is not.
/// While a run is in flight it is the **Stop**: the same circle, holding a
/// square rather than an arrow, and filled whether or not the field has text -
/// because stopping a run is something you can do, and it is the only thing this
/// control can do while one is going.
pub fn send_button(t: f32, lit: bool, stop: bool) -> Div {
    let (face, ink) = if lit {
        (motion::mix(theme::accent(), theme::text(), 0.12 * t), theme::on_accent())
    } else {
        // Their disabled pair is `disabled:bg-foreground/12 disabled:text-muted-foreground`
        // (`ComposerPrimaryActions.tsx:310`), which is `chip` and `muted` here.
        (theme::chip(), theme::muted())
    };
    let mark = if stop { IconName::Square } else { IconName::ArrowUp };
    div()
        .flex_shrink_0()
        // `CONTROL` 32, and **a circle**: `rounded-full`, not the ladder's 8. It is the
        // one round control in the window, and one circle is the whole point - mid-run
        // it is the same control wearing the Stop (`07-send-and-composer-states.md`).
        .size(px(theme::CONTROL))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(face)
        // No focus ring of its own: the source's send circle omits it, and the card's
        // edge is what says where you are typing.
        .child(icon(mark, if stop { 12.0 } else { 16.0 }, ink))
}

/// How a run's own line reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunTone {
    /// A run in flight: a pulse and a sentence.
    Working,
    /// A turn that failed: the worker's own error text, in the danger tone.
    Failed,
}

/// What the run is doing, above the composer's field.
///
/// A pulse and a sentence. It used to be a bare grey string - `Working… running` -
/// which is the worker's own state name printed raw, and it read as a log line dropped
/// into a chat box. This is the same fact said to the person waiting: a mark that is
/// moving, and a word for what is happening.
///
/// On a failure it is the error's own text in the danger tone, because the first
/// thing a person wants from a failed turn is what it said.
pub fn run_note(text: &str, tone: RunTone) -> AnyElement {
    let failed = tone == RunTone::Failed;
    let ink = if failed { theme::danger() } else { theme::muted() };
    h_flex()
        .w_full()
        .items_center()
        // The status line's own numbers (`ThreadStatusLine.tsx:17`): `text-xs` -> `SMALL`,
        // `muted`, and `gap-1.5 px-1 pb-2`, which is 6/4/8 - the 6 has no rung here and
        // `SPACE_SM` 8 is the nearest, the other two are `SPACE_XS`.
        .gap(px(theme::SPACE_SM))
        .px(px(theme::SPACE_XS))
        .py(px(theme::SPACE_XS))
        .rounded(px(theme::R_MD))
        // A soft wash behind a state, so it reads as a strip of the card rather than
        // as a sentence that happens to be there. Their own error surfaces are tinted
        // the same way; a run in flight gets the neutral rung.
        .bg(if failed { theme::danger_wash() } else { theme::chip() })
        .children((!failed).then(spinner_mark))
        .children(failed.then(|| icon(IconName::CircleAlert, 13.0, ink)))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(theme::SMALL))
                .line_height(px(16.0))
                .text_color(ink)
                .child(text.to_string()),
        )
        .into_any_element()
}

/// The working mark at its own size, for a caller that has no glyph of its own.
fn spinner_mark() -> Div {
    spinner(13.0, theme::state_working())
}

/// The line above the field while the field is a rename prompt.
///
/// It says what is being renamed, because the field is prefilled with the name and a
/// prefilled box with no label is a box you cannot tell from a composer.
pub fn rename_note(name: &str) -> AnyElement {
    h_flex()
        .w_full()
        .items_center()
        .gap(px(theme::SPACE_SM))
        .px(px(theme::SPACE_SM))
        .py(px(6.0))
        .rounded(px(theme::R_MD))
        .bg(theme::accent_wash())
        .child(icon(IconName::Pencil, 13.0, theme::accent()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::SMALL))
                .line_height(px(16.0))
                .text_color(theme::text_soft())
                .child(format!("Renaming \u{201c}{name}\u{201d} \u{2014} enter to keep, escape to leave it")),
        )
        .into_any_element()
}

/// A permission question, which the worker is blocked on.
///
/// Two controls and no third: the worker is waiting, and the only two things a
/// person can say are yes and no. The summary is the worker's own one-liner about
/// what the call would do; an ask with no summary says nothing rather than
/// padding itself.
pub fn ask_row(
    ask: &crate::agent::Ask,
    on_yes: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    on_deny: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let summary = ask.summary.trim().to_string();
    v_flex()
        .w_full()
        .gap(px(theme::SPACE_SM))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(theme::SPACE_SM))
                .child(icon(IconName::ShieldQuestionMark, 13.0, theme::state_permission()))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(theme::SMALL))
                        .text_color(theme::text_soft())
                        .child(format!("Allow {}?", ask.name)),
                ),
        )
        .children((!summary.is_empty()).then(|| {
            div()
                .w_full()
                .font(font(theme::MONO))
                .text_size(px(theme::MICRO))
                .line_height(px(15.0))
                .text_color(theme::ghost())
                .child(summary.clone())
        }))
        .child(
            h_flex()
                .w_full()
                .gap(px(theme::SPACE_SM))
                .child(
                    motion::mark_on_hover(
                        outline_button("Deny", motion::hover_t("ask:no")),
                        "ask:no",
                        on_deny,
                    )
                    .flex_1()
                    .justify_center(),
                )
                .child(
                    motion::mark_on_hover(
                        accent_button("Allow", motion::hover_t("ask:yes")),
                        "ask:yes",
                        on_yes,
                    )
                    .flex_1()
                    .justify_center(),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{map_hit_w, map_strip_w, row_state, State};
    use crate::theme;
    use std::collections::HashSet;

    /// The minimap's two arithmetic rules, asserted rather than eyeballed: the box is a
    /// clamp against the gutter, and a strip's length falls away from the turn in view.
    #[test]
    fn the_minimap_narrows_with_the_gutter_and_with_distance() {
        assert_eq!(map_hit_w(400.0), theme::MAP_HIT_W, "a wide gutter hits the cap");
        assert_eq!(map_hit_w(48.0), 36.0, "the narrowest gutter the strip is drawn in");
        assert_eq!(map_hit_w(0.0), 0.0, "no gutter, no box, and not a negative one");
        assert_eq!(map_strip_w(None), theme::MAP_STRIP_W);
        assert_eq!(map_strip_w(Some(0)), theme::MAP_STRIP_W);
        assert!(map_strip_w(Some(1)) < theme::MAP_STRIP_W);
        assert!(map_strip_w(Some(2)) < map_strip_w(Some(1)));
        assert!(map_strip_w(Some(9)) < map_strip_w(Some(2)));
    }

    /// The ladder, in the order that matters: the most urgent true fact wins, and
    /// a row the worker is not holding can only ever be waiting or settled.
    #[test]
    fn a_row_shows_the_most_urgent_thing_that_is_true_of_it() {
        let none: HashSet<String> = HashSet::new();
        // A question beats a failure beats a run beats a finished turn.
        assert_eq!(row_state("a", "a", true, true, true, &none), State::Ask);
        assert_eq!(row_state("a", "a", true, false, true, &none), State::Failed);
        assert_eq!(row_state("a", "a", true, false, false, &none), State::Working);
        // ...and a row the worker is not holding cannot borrow any of them.
        assert_eq!(row_state("b", "a", true, true, true, &none), State::Settled);
        // A finished turn is the window's own memory.
        let mut unseen: HashSet<String> = HashSet::new();
        unseen.insert("b".to_string());
        assert_eq!(row_state("b", "a", true, true, true, &unseen), State::Waiting);
        // Nothing owed, nothing shown.
        assert_eq!(row_state("c", "a", false, false, false, &unseen), State::Settled);
        assert!(!State::Settled.speaks(), "a settled row keeps showing its age");
        assert!(State::Waiting.speaks());
    }
}
