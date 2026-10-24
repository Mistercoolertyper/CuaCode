//! How anything that moves moves, in one place.
//!
//! Two mechanisms, and no call site gets its own numbers:
//!
//! - **Blends.** A hover is a paint-time blend between two colors, driven by a
//!   clock the pointer starts. gpui's own `.hover()` snaps by construction, and
//!   a snapping row is the difference between a window that feels alive and one
//!   that feels like a form.
//! - **The pulse.** A dot that means "working" breathes on the same clock.
//!
//! Everything reads the clock at paint time, so an interrupted blend continues
//! from where it was rather than jumping. The window keeps painting while any
//! blend is alive: the app's own 33ms tick asks `any_blend_alive` and only then
//! asks for a frame.
//!
//! The durations are superset's: their hover transitions are 150ms and their
//! `thinking-pulse` is 1.8s. Nothing here is shorter than 120ms, because below
//! that a blend reads as a snap with a frame dropped.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::component::animation::EffectTransition;
use gpui_kit::*;

/// How long a hover blend takes. Their own `transition-colors` default.
pub const HOVER: Duration = Duration::from_millis(150);

struct Fade {
    on: bool,
    since: Instant,
    /// The value the surface was showing when the pointer flipped it, so a flip
    /// mid-travel re-anchors instead of jumping.
    t0: f32,
}

thread_local! {
    static BLENDS: RefCell<HashMap<String, Fade>> = RefCell::new(HashMap::new());
}

impl Fade {
    fn value(&self) -> f32 {
        let t = (self.since.elapsed().as_secs_f32() / HOVER.as_secs_f32()).min(1.0);
        if self.on {
            self.t0 + (1.0 - self.t0) * t
        } else {
            self.t0 * (1.0 - t)
        }
    }
}

/// Record the pointer's arrival or leave on a surface. The blend itself is read
/// at paint time; this only keeps the clock.
pub fn mark_hover(key: &str, entered: bool) {
    BLENDS.with_borrow_mut(|map| {
        let f = map
            .entry(key.to_string())
            .or_insert(Fade { on: false, since: Instant::now(), t0: 0.0 });
        if f.on != entered {
            f.t0 = f.value();
            f.since = Instant::now();
            f.on = entered;
        }
    });
}

/// How far a surface's hover has travelled: 0 at rest, 1 under the pointer.
///
/// A lookup and a clock read, and nothing else.
///
/// This used to sweep the map of settled entries on every call, which meant
/// walking the whole blend map and allocating a `Vec<String>` of dead keys -
/// and `hover_t` is called **once per row per frame**. On a rail of four
/// hundred rows that is four hundred walks over the map and four hundred
/// allocations for every frame a hover is travelling, which is the frame that
/// has to keep up with the pointer. The sweep is `sweep`, and the app calls it
/// once a frame.
pub fn hover_t(key: &str) -> f32 {
    BLENDS.with_borrow(|map| map.get(key).map(Fade::value).unwrap_or(0.0))
}

/// Whether any blend is still travelling, which is what keeps frames coming
/// while one runs.
///
/// This is also where settled entries age out: an element unmounts without a
/// leave event, so an entry can outlive the row that made it. The sweep is here
/// and not in `hover_t` because `hover_t` runs **once per row per frame** and a
/// walk of the whole map there is a walk per row; this runs once a tick.
///
/// An entry that is `on` is kept whatever its age: it is the surface under the
/// pointer, and dropping it would put the row back to its resting colour while
/// the pointer is still on it. Only settled entries age out.
pub fn any_blend_alive() -> bool {
    BLENDS.with_borrow_mut(|map| {
        map.retain(|_, f| f.on || f.since.elapsed() <= HOVER * 4);
        map.values().any(|f| f.since.elapsed() < HOVER)
    })
}

/// A color blended between two others, alpha included.
pub fn mix(one: Hsla, other: Hsla, t: f32) -> Hsla {
    let l = |a: f32, b: f32| a + (b - a) * t;
    Hsla { h: l(one.h, other.h), s: l(one.s, other.s), l: l(one.l, other.l), a: l(one.a, other.a) }
}

// ---- the hover clock ----

/// A row that keeps the hover clock for every surface inside it.
///
/// The row is what the pointer arrives at, and the parts that light up are its
/// children, so the clock is kept on the wrapper and the children read it. This
/// is the one place that knows hover is a stateful listener at all: a builder in
/// `view` returns a plain `Div` and a call site wraps it here.
///
/// A row is also the thing that is *clicked*, so the two listeners live together
/// and a call site cannot wire one without the other.
pub fn mark_on_hover(
    el: Div,
    key: &str,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let owned = key.to_string();
    el.id(ElementId::Name(owned.clone().into()))
        .cursor_pointer()
        .on_hover(move |entered, _, _| mark_hover(&owned, *entered))
        .on_click(listener)
}

/// The hover clock for a surface that is **already stateful** and must stay so.
///
/// `mark_on_hover` takes a plain `Div` because it has to put an id on it, and an
/// element that already needs an id of its own - one carrying a tooltip - cannot
/// go through it without losing that id. So this variant keeps whatever the
/// caller built and attaches only the clock and the click.
pub fn mark_on_hover_el(
    el: Stateful<Div>,
    key: &str,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let owned = key.to_string();
    el.cursor_pointer()
        .on_hover(move |entered, _, _| mark_hover(&owned, *entered))
        .on_click(listener)
}

/// The same, for a surface that is not clickable.
#[allow(dead_code)] // the window's own planes are static for now; a tooltip will want it
pub fn mark_hover_el(el: Div, key: &str) -> Stateful<Div> {
    let owned = key.to_string();
    el.id(ElementId::Name(owned.clone().into()))
        .on_hover(move |entered, _, _| mark_hover(&owned, *entered))
}

// ---- entrances ----

/// A cubic-bezier easing, evaluated by bisection with a Newton step: values clamp
/// hard to [0, 1] because gpui asserts on a delta outside it.
#[derive(Clone, Copy)]
pub struct Curve {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl Curve {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// The curve as an easing fn, in the shape gpui's `with_easing` wants.
    pub fn easing(self) -> impl Fn(f32) -> f32 {
        move |t| self.eval(t.clamp(0.0, 1.0))
    }

    fn eval(&self, t: f32) -> f32 {
        // x(u) is monotone on [0,1], so one bisection pass finds u for t.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        let mut u = t;
        for _ in 0..12 {
            let x = bezier(self.x1, self.x2, u);
            if x < t {
                lo = u;
            } else {
                hi = u;
            }
            u = (lo + hi) * 0.5;
        }
        bezier(self.y1, self.y2, u).clamp(0.0, 1.0)
    }
}

fn bezier(p1: f32, p2: f32, u: f32) -> f32 {
    let v = 1.0 - u;
    3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u
}

/// Ease-out-expo: the arrival curve. Nothing lands, it arrives.
pub const OUT_EXPO: Curve = Curve::new(0.16, 1.0, 0.3, 1.0);

/// Their own hover and popover travel: 150ms, `animate-in` with a
/// `slide-in-from-top-2` and a `zoom-in-95`.
pub const FAST: Duration = Duration::from_millis(150);
/// Their `block-fade`: 320ms ease-out, for a block of content arriving.
pub const BLOCK: Duration = Duration::from_millis(320);

/// An appearance: a fade and a small travel, from one curve and one duration, so
/// a plane that arrives does not grow its own numbers.
pub fn arrive<E: IntoElement + Styled + 'static>(
    duration: Duration,
    curve: Curve,
    id: ElementId,
    from_y: f32,
    el: E,
) -> AnyElement {
    EffectTransition::new(duration)
        .ease(curve.easing())
        .slide_y(px(from_y), px(0.))
        .fade(0.0, 1.0)
        .apply(el, id)
        .into_any_element()
}


// ---- the pulse ----

/// Their `thinking-pulse`: 1.8s ease-in-out, opacity 0.35 to 0.9. A dot that
/// means "working" breathes on the app's own clock, which is also what keeps the
/// window painting while a run is in flight.
pub fn pulse_phase() -> f32 {
    let since = EPOCH.with(|e| e.elapsed()).as_secs_f32();
    let t = since.rem_euclid(PULSE.as_secs_f32()) / PULSE.as_secs_f32();
    // ease-in-out, as their keyframes declare it, over one period.
    let s = 0.5 - 0.5 * (t * std::f32::consts::TAU).cos();
    s * s * (3.0 - 2.0 * s)
}

/// Their `--animate-thinking-pulse: thinking-pulse 1.8s ease-in-out infinite`.
pub const PULSE: Duration = Duration::from_millis(1800);

thread_local! {
    /// The app's own origin, so the pulse is a phase and not an absolute time:
    /// two windows opening a second apart breathe together.
    static EPOCH: Instant = Instant::now();
}

#[cfg(test)]
mod tests {
    use super::format_elapsed_opt;


    #[test]
    fn an_elapsed_time_reads_as_words() {
        assert_eq!(format_elapsed_opt(42).as_deref(), Some("42s"));
        assert_eq!(format_elapsed_opt(725).as_deref(), Some("12m 5s"));
        assert_eq!(format_elapsed_opt(0), None);
    }
}

/// An elapsed time in words, or nothing when there is nothing to say. The
/// `Option` is the point: a run with no clock is not a run of zero seconds.
#[allow(dead_code)] // the readout for a run in flight, which this window has none of yet
pub fn format_elapsed_opt(secs: i64) -> Option<String> {
    match secs {
        s if s <= 0 => None,
        s if s < 60 => Some(format!("{s}s")),
        s if s < 3600 => Some(format!("{}m {}s", s / 60, s % 60)),
        s => Some(format!("{}h {}m", s / 3600, (s % 3600) / 60)),
    }
}
