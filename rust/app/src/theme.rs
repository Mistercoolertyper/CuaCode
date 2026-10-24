//! The window's tokens: the palette, the numbers, and nothing that draws.
//!
//! The palette is superset's `ember`, quoted from their own source rather than
//! eyeballed off their screenshots:
//!
//! ```text
//!     packages/shared/src/themes/built-in/ember.ts
//!     apps/desktop/src/renderer/globals.css
//! ```
//!
//! Their dark theme is *warm*, which is the whole character of it: a brown-black
//! background (#151110), greys that lean warm, a warm off-white for text, and
//! one hot orange (#e07850) for everything you can act on. A neutral grey-blue
//! window is what everyone else ships; this is not that.
//!
//! Two of their rules are load-bearing and are kept here exactly:
//!
//! - A row is marked by a low-opacity wash of the *foreground* (`--fill-hover`
//!   7%, `--fill-selected` 10%) rather than by a grey fill, so a lit row tracks
//!   whatever the text does instead of fighting it.
//! - The accent means "you can act on this" and nothing else. The other hues are
//!   states, not decoration.
//!
//! Every surface is opaque, and that is the one place this departs from
//! superset: their window is a material with translucent planes over it, this one
//! is a page. A window that is one color can be checked, and #151110 over a
//! bright desktop is not #151110.
//!
//! Numbers drive layout, colors paint, and neither reads the other.

use gpui_kit::*;

/// A token, written as the hex they wrote it in and converted once.
///
/// Their palette is a literal hex list in `ember.ts`, so these are the same
/// literals and can be checked against their file by eye. The shape is web hex
/// with the alpha last, `#rrggbbaa`, because that is how the CSS writes it.
/// Reading it as `aarrggbb` turns every alpha into the blue channel.
pub fn hex(rgb: u32, alpha: f32) -> Hsla {
    let r = ((rgb >> 16) & 0xff) as f32 / 255.0;
    let g = ((rgb >> 8) & 0xff) as f32 / 255.0;
    let b = (rgb & 0xff) as f32 / 255.0;
    Rgba { r, g, b, a: alpha }.into()
}

// ---- numbers ----
//
// Their own layout, read off the components rather than invented: the top bar is
// `h-12`, every pane head is `h-10`, the rail is 280 wide and the right sidebar
// 250, both clamped by their own stores.

/// The window's one row of controls, and the room the traffic lights need.
pub const TOPBAR: f32 = 48.0;
/// A pane's head, and the rail's tab strip: the same rung, so the chrome reads
/// as one shell across the top of both columns.
pub const HEAD: f32 = 40.0;
/// The rail. `DEFAULT_WORKSPACE_SIDEBAR_WIDTH` in their `workspace-sidebar-state`,
/// and their own clamping bounds: 225 to 400.
pub const RAIL: f32 = 280.0;
pub const RAIL_MIN: f32 = 225.0;
pub const RAIL_MAX: f32 = 400.0;
/// The right sidebar. `DEFAULT_SIDEBAR_WIDTH` in their `sidebar-state`, and
/// their own clamping bounds: `MIN_SIDEBAR_WIDTH` 200, `MAX_SIDEBAR_WIDTH` 500.
pub const SIDE: f32 = 250.0;
pub const SIDE_MIN: f32 = 200.0;
pub const SIDE_MAX: f32 = 500.0;
/// The traffic lights sit at (14, 13) and are 54 wide in practice; their top bar
/// reserves 80 for them.
pub const LIGHTS: f32 = 80.0;

/// The row height of a workspace row: `py-2` around a 13px line.
///
/// A row's **minimum**. The height of a row is its content plus `ROW_PAD`, so a
/// two-line row is taller by exactly the second line and nothing else.
pub const ROW: f32 = 36.0;
/// A row's own vertical padding. `ROW` minus a name line, halved, so a one-line
/// row lands on `ROW` exactly: 36 = 16 + 2 * 10.
pub const ROW_PAD: f32 = (ROW - NAME_LINE) / 2.0;
/// A rail row's name line, and the smaller line a caption gets under it. The two
/// together plus the padding are the height of a two-line row, which is why they
/// are numbers rather than literals in a builder.
pub const NAME_LINE: f32 = 16.0;
pub const CAPTION_LINE: f32 = 14.0;
/// A session row, which is **one line**: t3code's row is `h-9` (36px) whatever it
/// holds, and that is what makes a rail of forty sessions scannable instead of a
/// wall. `ROW` is the same number and is kept for the rows that carry a caption.
pub const ROW_SM: f32 = 36.0;
/// A project's head: `py-1.5` around a 14px line.
pub const ROW_HEAD: f32 = 30.0;
/// A rail section's header, which is their `CollapsibleSectionHeader`'s `h-8`: the
/// rung that says "this names a group" rather than "this is another row".
pub const HEAD_SM: f32 = 28.0;
/// Body text, small labels, and the rail's own size. Their row is `text-[13px]`.
pub const BODY: f32 = 13.0;
pub const SMALL: f32 = 11.0;
pub const MICRO: f32 = 10.0;
/// A line's own height, for the places a row is laid out by hand.
pub const LINE: f32 = 20.0;

/// A control's height, and the ones their buttons declare (`h-8`, `h-7`).
pub const CONTROL: f32 = 32.0;
pub const CONTROL_SM: f32 = 28.0;

/// The composer's field line.
///
/// t3code has **no min-height on the editor at all**: the field is one line of text
/// (`text-sm` at its own leading, about 20px) plus `pt-3.5` above it and `pb-2` below,
/// which lands the whole body at about 42px. This was 40 as a *floor*, which is a
/// different thing: a floor makes an empty composer as tall as a full one. There is no
/// floor now.
pub const COMPOSER_LINE: f32 = 20.0;
/// The field's own padding: their body is `px-3 pt-3.5 pb-2 sm:px-4 sm:pt-4`.
pub const COMPOSER_PAD_X: f32 = 16.0;
pub const COMPOSER_PAD_TOP: f32 = 14.0;
pub const COMPOSER_PAD_BOTTOM: f32 = 8.0;
/// The controls row under the field: their footer is `px-3 pb-3 sm:px-4 sm:pb-4` with
/// 24px-high controls in it, so the row is the controls plus that padding and nothing
/// else. This was a 44px `min_h`, which is an empty row reserving height it did not use.
pub const COMPOSER_FOOTER: f32 = 28.0;

/// The minimap's own numbers, quoted from t3code's `MessagesTimeline`.
///
/// Their strip is `h-0.5 w-6` - **2px tall and 24 wide** - spaced `8px` apart on a
/// `bg-border/15` rail. Every strip is this size; only the colour says which one is the
/// turn in view.
pub const MAP_STRIP_H: f32 = 2.0;
pub const MAP_STRIP_W: f32 = 24.0;
pub const MAP_SPACING: f32 = 8.0;
/// How wide a strip's **target** is, which is not how wide the strip is.
///
/// The face is 24px; the box you click is the whole gutter, so the pointer does not have
/// to find a 2px line, which is what made it feel hard to click. The boxes tile
/// vertically at `MAP_SPACING`, so there is no dead space anywhere in the column.
pub const MAP_HIT_W: f32 = 40.0;
/// A strip at rest, and one the pointer is on.
///
/// The **active** turn is not here: it is `text_soft` (near-white warm off-white from
/// ember's own `foreground`), because it is the one mark that means "you are here" and
/// grey does not say that. These were `map_rest`/`map_near` plus an accent-orange active
/// strip, which read as ugly - orange is this palette's "you can act on this" and a
/// scroll position is not a control.
pub fn map_rest() -> Hsla {
    hex(0xa8a5a3, 0.3)
}
pub fn map_near() -> Hsla {
    hex(0xa8a5a3, 0.55)
}

/// The line height of an answer's body text. Their editor is 14px at a 1.625
/// leading, and prose wants more air than code does.
pub const ANSWER_LINE: f32 = 22.0;

/// A run's own line. Their `TurnFoldTimelineRow` is `text-sm` in a row about
/// 26px tall; this sits on the same rung as the rail's rows so a run reads as
/// furniture of the conversation rather than as a sentence in it.
pub const RUN_LINE: f32 = 26.0;

/// How wide the conversation's text may get, which is t3code's own
/// `--chat-content-max-width: 46rem` (736px).
///
/// This is the one number that decides whether a long answer is readable. At a
/// full-window measure a sentence runs past the point where the eye finds the next
/// line, and the composer and the transcript stop agreeing about where the column
/// is. 736 is where they put it, and it is about two alphabets and a half per line.
pub const MEASURE: f32 = 736.0;

/// The room the transcript keeps at its foot for the **floating** composer.
///
/// The composer is not a row in the column: it is positioned over the column's foot, so
/// the conversation runs underneath it. This is how much of the column it covers, which
/// is what the transcript has to be able to scroll past - otherwise the last row could
/// never be lifted clear of the box, and that is the row you are answering.
///
/// One card: the field's line, the settings row, the card's own padding, and the gap above
/// its top edge.
pub const COMPOSER_FLOOR_H: f32 = 132.0;

// ---- the popover ----
//
// A menu is anchored to the chip that opened it, and the chips live in the
// composer at the foot of the window, so a menu has to open **upward** and it
// has to fit. Its width is one control's worth - wide enough for a model name
// and its note, narrow enough that it reads as a popover and not as a panel -
// and its height is capped and scrolled rather than left to the length of the
// list. Before that cap the model menu was 12 rows plus 450 more, laid out at
// full height: a slab from the top of the window to the bottom of it.

/// The popover's width. `DropdownMenuContent` is `w-64` (256) at its default.
pub const MENU_W: f32 = 300.0;
/// How tall a menu may get before it scrolls: `max-h-80` there.
pub const MENU_MAX_H: f32 = 320.0;

// ---- radii ----
//
// One number, `--radius: 0.625rem` (10px), with sm/md/lg walking -4/-2/+0/+4.

pub const R: f32 = 10.0;
pub const R_SM: f32 = R - 4.0;
pub const R_MD: f32 = R - 2.0;
pub const R_LG: f32 = R;
/// A chip, a keycap, a status badge: their own `rounded` (4px) and `rounded-md`.
pub const R_CHIP: f32 = 4.0;
/// The composer. Their `ComposerBody` is `rounded-2xl` (16px), one rung above
/// the base ladder and the only thing in the window that round.
pub const R_COMPOSER: f32 = 16.0;

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;
pub const SPACE_XL: f32 = 24.0;

// ---- surfaces ----
//
// ember's tokens, **opaque**. The window is a page: the page, the rail and the planes on
// them are solid, and what is painted is what is seen at any desktop. That is what keeps
// the palette's warmth - `#151110` over a bright wallpaper is not `#151110`.
//
// The glass is **the composer's own window**, not these tokens - see `composer.rs`.
// gpui has no per-element backdrop blur at all (a `blur_radius` here is a shadow's
// softness and nothing more), so only a window can blur what is behind it. The input box
// is its own small window floating over this one, and these surfaces stay solid under it.

/// ember `background` (#151110): the conversation's plane, and everything the rail and
/// the sidebar are not. Opaque, so what is painted is what is seen.
pub fn page() -> Hsla {
    hex(0x151110, 1.0)
}

/// ember `sidebar` / `tertiary` (#1a1716): the rail's own rung, one step up from the page.
pub fn rail() -> Hsla {
    hex(0x1a1716, 1.0)
}

/// ember `tertiary-active` / `sidebar-accent` (#252220): a rail row the pointer is on, and
/// a tab that is selected.
pub fn rail_hover() -> Hsla {
    hex(0x252220, 1.0)
}

/// ember `card` / `popover` (#201e1c): a plane raised off the page.
pub fn plate() -> Hsla {
    hex(0x201e1c, 1.0)
}

/// ember `popover` (#201e1c): what a menu hangs on.
///
/// The same literal as `card`, and it used to be `#232025` - a value invented here
/// rather than quoted, which leaned **purple** against this palette's warm greys and
/// read as a different product's panel dropped into this one. superset's own
/// `--popover` is their `--card`: a warmer, darker plane than any of the greys above
/// it, and that warmth is what keeps a menu looking like it belongs to the window.
pub fn popover() -> Hsla {
    hex(0x201e1c, 1.0)
}

/// ember `secondary` / `muted` / `accent` (#2a2827): the raised rung, for a chip
/// or a control face.
pub fn chip() -> Hsla {
    hex(0x2a2827, 1.0)
}

/// The composer's surface. **Opaque.**
///
/// This was a 7% white "glass", and it was a lie twice over: gpui-pre cannot blur what
/// is behind a box inside the window, so all it did was show a sharp transcript - or the
/// desktop behind the app - through a tint: transparency with no blur at all. Solid is
/// what it can honestly be: `plate`, one rung
/// above the page, with the rim and the float doing the work of saying it is raised.
pub fn composer() -> Hsla {
    hex(0x201e1c, 1.0)
}

/// ember `border` (#2a2827). A hairline, because a warm theme's greys sit close
/// together and the border is what separates them. Their every panel edge.
pub fn line() -> Hsla {
    hex(0x2a2827, 1.0)
}

/// ember `ring` (#3a3837): the same edge, one step up. A field's outline.
pub fn line_strong() -> Hsla {
    hex(0x3a3837, 1.0)
}

/// ember `fill-hover`: a wash of the *foreground*, so a row lit by the pointer
/// tracks whatever the text does rather than going grey. Their rule, kept
/// exactly: 7% of the foreground.
pub fn wash_hover() -> Hsla {
    text().opacity(0.07)
}

/// ember `fill-selected`: the same idea, one step further. 10%.
pub fn wash_selected() -> Hsla {
    text().opacity(0.10)
}

/// A border at a fraction of the foreground, which is what their
/// `border-foreground/10` keycap and their `border-border/70` tab both are.
pub fn hairline(a: f32) -> Hsla {
    text().opacity(a)
}

// ---- text ----

/// ember `foreground` (#eae8e6): warm off-white, never `#fff`. Pure white on a
/// warm black is a different product.
pub fn text() -> Hsla {
    hex(0xeae8e6, 1.0)
}

/// A row's name when it is not the active one: their `text-foreground/80`.
pub fn text_soft() -> Hsla {
    hex(0xeae8e6, 0.8)
}

/// ember `muted-foreground` (#a8a5a3).
pub fn muted() -> Hsla {
    hex(0xa8a5a3, 1.0)
}

/// Their `text-muted-foreground/70`: a tab that is not selected, a hint that is
/// meant to be found rather than read.
pub fn faint() -> Hsla {
    hex(0xa8a5a3, 0.7)
}

/// Quieter still: a path's parent, a count in a heading.
pub fn ghost() -> Hsla {
    hex(0xa8a5a3, 0.5)
}

/// A soft-white wash at an arbitrary coverage, for the one place a fixed coverage
/// is what is wanted rather than the foreground's tone.
///
/// At 0 it is fully transparent, which is what a surface at rest wears: a blend
/// between two colors needs a starting point, and "no fill at all" is one, so no
/// call site has to invent a transparent token of its own.
pub fn ink(a: f32) -> Hsla {
    Rgba { r: 1.0, g: 1.0, b: 1.0, a }.into()
}

// ---- the accent, and the states that are not it ----

/// ember `highlight` / `sidebar-primary` (#e07850): the one hue that means you
/// can act on this. Hot orange on warm black is the signature.
pub fn accent() -> Hsla {
    hex(0xe07850, 1.0)
}

/// ember `highlight-foreground` (#151110): text on the accent, which is the
/// page's own colour so the two cannot drift apart.
pub fn on_accent() -> Hsla {
    hex(0x151110, 1.0)
}

/// ember `highlight-match` (rgba(224,120,80,0.2)): the accent at a fifth, behind
/// a match or a badge.
pub fn accent_wash() -> Hsla {
    hex(0xe07850, 0.2)
}

/// ember `success` (#00c950), for a finished run.
pub fn success() -> Hsla {
    hex(0x00c950, 1.0)
}

/// ember `warning` (#e5c07b): the blocked state, a quieter relative of the
/// accent rather than a second personality. Their `StatusIndicator` uses the
/// amber rung for it.
pub fn warning() -> Hsla {
    hex(0xe5c07b, 1.0)
}

/// ember `destructive` (#cc4444) is a *fill*, not a text tone: it reads at
/// 2.9:1 on this page, which is not a sentence anybody can read. Errors wear
/// their `destructive-foreground` instead, which clears 4.5:1.
pub fn danger() -> Hsla {
    hex(0xffcccc, 1.0)
}

/// The fill version, for the rare case where red is a plate rather than words
/// on one.
pub fn danger_fill() -> Hsla {
    hex(0xcc4444, 1.0)
}

/// ember `destructive-foreground` (#ffcccc) as a wash, which is what their own
/// error surfaces use. A failing step wears this behind the thing it was about.
pub fn danger_wash() -> Hsla {
    hex(0xffcccc, 0.12)
}

/// The status marks, quoted from superset's own `StatusIndicator` and
/// `WorkspaceStatusBadge`. These are Tailwind palette values, not ember tokens:
/// the dots deliberately sit outside the theme's warmth so a state is never
/// mistaken for decoration.
///
/// `permission` (their yellow-500/400), `failed` (red-500/400),
/// `working` (amber-500), `review` (green-500).
pub fn state_permission() -> Hsla {
    hex(0xf0b100, 1.0)
}
pub fn state_failed() -> Hsla {
    hex(0xfb2c36, 1.0)
}
pub fn state_working() -> Hsla {
    hex(0xfe9a00, 1.0)
}
pub fn state_review() -> Hsla {
    hex(0x00c950, 1.0)
}
/// Their `WorkspaceDiffStats`: emerald for what was added, a soft red for what
/// went.
pub fn diff_added() -> Hsla {
    hex(0x00bc7d, 1.0)
}
pub fn diff_removed() -> Hsla {
    hex(0xff6467, 1.0)
}
/// Their unread dot: `bg-blue-500`.
pub fn unread() -> Hsla {
    hex(0x2b7fff, 1.0)
}

/// A project's gradient circle, by their `grad:<n>` token. Eight rungs, quoted
/// from the set their own `ColorSelector` offers; an out-of-range byte wraps
/// rather than panicking, because the token is user data.
pub fn gradient(n: u8) -> (Hsla, Hsla) {
    let (a, b) = crate::state::GRADIENTS[n as usize % crate::state::GRADIENTS.len()];
    (hex(a, 1.0), hex(b, 1.0))
}

// ---- shadows ----
//
// superset's own shadow scale for the controls, and the glass pair below for the
// planes. `shadow-xs` is a field's own edge; the rim and the float are what make a
// translucent surface read as a surface.

/// `shadow-xs`: 0 1px 2px rgb(0 0 0 / 0.05). A field's own edge.
pub fn shadow_xs() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.0), px(1.0), hex(0x000000, 0.10)).blur_radius(px(2.0))]
}

// ---- glass ----
//
// Two shadows and one idea: a plane catches light along its top edge, and it sits
// somewhere in the stack. The **rim is the thing that makes a translucent sheet read as
// glass** - a tint with no edge is a flat patch, and a flat patch is not a surface.

/// The inner top highlight every raised plane wears, and the whole reason a translucent
/// sheet reads as glass instead of as a flat patch.
pub fn lift() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hex(0xffffff, 0.10),
        offset: point(px(0.0), px(1.0)),
        blur_radius: px(1.0),
        spread_radius: px(0.0),
        inset: true,
    }]
}

/// The shadow a floating plane casts on the page under it.
///
/// Deep and soft on purpose: the composer sits *over* the transcript, so its shadow is
/// what says so. A tight shadow under a big card looks stuck on; this one reads as lift.
pub fn float() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.0), px(12.0), hex(0x000000, 0.45)).blur_radius(px(28.0)),
        BoxShadow::new(px(0.0), px(2.0), hex(0x000000, 0.30)).blur_radius(px(6.0)),
    ]
}

/// The same lift for something that sits above the page without floating.
pub fn shadow_sm() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.0), px(3.0), hex(0x000000, 0.30)).blur_radius(px(10.0))]
}

// ---- fonts ----
//
// superset bundles no interface face at all: it draws in the system font and
// ships only SF Mono, which is macOS's own. Nothing to bundle, and the window
// looks native on the machine it is on.
//
// The UI face is named nowhere: gpui's own default is `.SystemUIFont`, which is
// the right answer, and naming a family in gpui is a lookup that can miss and
// fall back in silence. This constant is the record of that decision.
pub const SANS: &str = ".SystemUIFont";
/// Code, paths, and a branch name. macOS's system monospace.
pub const MONO: &str = "SF Mono";

/// One row of a mono column lines up with the next, which is what a mono face
/// is for. Both are 0.8em above the body size, their own `font-mono text-[11px]`.
pub fn mono(size: f32) -> Font {
    font(MONO)
}

#[cfg(test)]
mod tests {
    /// The arithmetic a row's height is built from, checked rather than assumed:
    /// a two-line row is exactly one caption line taller than a one-line row.
    #[test]
    fn a_row_is_its_content_plus_its_padding() {
        let one = NAME_LINE + 2.0 * ROW_PAD;
        assert_eq!(one, ROW, "a one-line row must land on ROW exactly");
        let two = NAME_LINE + CAPTION_LINE + 1.0 + 2.0 * ROW_PAD;
        assert!(
            (two - ROW - CAPTION_LINE - 1.0).abs() < f32::EPSILON,
            "a two-line row is {two}, which is not one caption line over ROW ({})",
            ROW + CAPTION_LINE
        );
    }


    // Named, not globbed: the parent's `use gpui_kit::*` re-exports gpui's own
    // `test` macro, which shadows the builtin one and wants a context it has no
    // way to get here.
    use super::{
        CAPTION_LINE, NAME_LINE, ROW, ROW_PAD,
        accent, danger, danger_fill, faint, ghost, muted, page, plate, rail, rail_hover,
        state_failed, state_permission, state_review, state_working, success, text, text_soft,
        warning, Hsla, Rgba,
    };

    /// sRGB to linear, and a relative luminance, which is what a contrast ratio
    /// is computed from.
    fn lin(c: f32) -> f32 {
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }

    fn lum(c: Hsla) -> f32 {
        let r: Rgba = c.into();
        0.2126 * lin(r.r) + 0.7152 * lin(r.g) + 0.0722 * lin(r.b)
    }

    fn ratio(a: Hsla, b: Hsla) -> f32 {
        let (la, lb) = (lum(a), lum(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Every surface a tone can land on. These are **opaque**, so the contrast checks
    /// below compare a tone to its surface directly - which is the reason the window is a
    /// page and not a material. The glass is the composer's own window; see `theme.rs`'s
    /// note over the surface tokens.
    fn surfaces() -> Vec<(&'static str, Hsla)> {
        vec![
            ("page", page()),
            ("rail", rail()),
            ("rail-hover", rail_hover()),
            ("plate", plate()),
        ]
    }

    /// Alpha-composite `top` over `under`, for the one tone that is not opaque: the washes.
    fn over(top: Hsla, under: Hsla) -> Hsla {
        let t: Rgba = top.into();
        let u: Rgba = under.into();
        let a = t.a + u.a * (1.0 - t.a);
        let mix = |x: f32, y: f32| (x * t.a + y * u.a * (1.0 - t.a)) / a;
        Rgba { r: mix(t.r, u.r), g: mix(t.g, u.g), b: mix(t.b, u.b), a }.into()
    }

    #[test]
    fn every_text_tone_is_readable_on_its_own_surface() {
        // WCAG's numbers: 7 for body text, 4.5 for muted, 3.0 for the furniture.
        for (name, surface) in surfaces() {
            for (tone, label, want) in [
                (text(), "text", 7.0),
                (text_soft(), "text-soft", 6.0),
                (muted(), "muted", 4.5),
                (faint(), "faint", 3.0),
                (ghost(), "ghost", 2.0),
            ] {
                let got = ratio(surface, tone);
                assert!(got >= want, "{label} on {name} is {got:.2}, want {want}+");
            }
        }
    }

    #[test]
    fn the_accent_is_readable_as_a_fill_and_as_words() {
        // As a fill: ember's own #151110 on it. As words: on the page.
        let on = ratio(accent(), super::on_accent());
        assert!(on >= 4.5, "text on the accent is {on:.2}, want 4.5+");
        let as_words = ratio(page(), accent());
        assert!(as_words >= 3.0, "the accent as a mark is {as_words:.2}, want 3+");
    }

    #[test]
    fn a_state_mark_is_visible_and_a_failure_is_readable() {
        // A dot is a graphic, so 3.0 is the bar for the marks...
        let s = page();
        for (name, c) in [
            ("permission", state_permission()),
            ("failed", state_failed()),
            ("working", state_working()),
            ("review", state_review()),
            ("success", success()),
            ("warning", warning()),
        ] {
            let r = ratio(s, c);
            assert!(r >= 3.0, "{name} is {r:.2} on the page, want 3+");
        }
        // ...and a failure that is words has to clear 4.5.
        let t = ratio(s, danger());
        assert!(t >= 4.5, "an error's message is {t:.2}, want 4.5+");
        // The raw destructive token is a fill precisely because it is not words.
        let raw = ratio(s, danger_fill());
        assert!(raw < 4.5, "destructive has drifted into a text tone: {raw:.2}");
    }

    #[test]
    fn every_surface_is_opaque() {
        // The window is a page, not a material: what is painted is what is seen at any
        // desktop. That is what keeps ember's warmth (#151110 over a bright wallpaper is
        // not #151110), and it is also what lets the contrast checks above compare a tone
        // to its surface directly. The glass is the composer's own window, which is a
        // different thing entirely.
        for (name, c) in surfaces() {
            let a: Rgba = c.into();
            assert_eq!(a.a, 1.0, "{name} is not opaque");
        }
    }

    #[test]
    fn the_ladder_goes_up_from_the_page() {
        // A rail that reads darker than the page is a rail nobody can see the edge of; the
        // raised rung has to be lighter than both.
        let l = |c: Hsla| lum(c);
        assert!(l(rail()) > l(page()), "the rail is not above the page");
        assert!(l(rail_hover()) > l(rail()), "a hovered row is not above the rail");
        assert!(l(plate()) > l(page()), "a plate is not above the page");
    }
}
