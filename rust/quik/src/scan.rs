//! The status row's scanner, in cells.
//!
//! opencode's, quoted from `packages/tui/src/ui/spinner.ts` in the checkout under
//! `~/.cuacode/sessions/<id>/notebook/opencode`, and not a lookalike. The tones are
//! `view::ink`'s: nothing here names a colour of its own.

use cuacode::wire::State;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::view::{PAGE, ink, mix};

/// The status row's scanner: opencode's, on a wire of eight cells.
///
/// `packages/tui/src/ui/spinner.ts` `createFrames` builds a bidirectional scanner
/// over `width` cells (`:273`) with a six-step trail, a hold of nine frames at its
/// right end and thirty at its left (`:275-276`), which is `8 + 9 + 7 + 30 = 54`
/// frames to a cycle (`:306`). `getScannerState` (`:25-104`) walks a frame index
/// through those four legs, and `calculateColorIndex` (`:106-135`) reads as a step
/// of the trail.
pub const DOT_CELLS: usize = 8;
/// The frame opencode ticks it at: `interval={40}` where the prompt draws it
/// (`component/prompt/index.tsx:1917`), and `animationFps = 1000 / interval`
/// (`component/spinner-renderable.ts:92`), so twenty-five steps a second. The
/// loop's own clock while a round runs is `MOVING_MS`, and it is the same number:
/// one frame of this screen, one step of the scanner.
pub const DOT_STEP_MS: u128 = 40;

/// What is left of the tone a cell behind the head: opencode's alphas
/// (`ui/spinner.ts:209-221`), head first, flattened on `PAGE` because a cell has no
/// alpha of its own. Their second step is also brightened 1.15x as a glare, which
/// a terminal gets for free at 0.9 and does not need again.
const TRAIL: [f64; 6] = [1.0, 0.9, 0.65, 0.42, 0.27, 0.18];
/// Frames the head rests at the right end of the wire, and at the left.
const HOLD_END: usize = 9;
const HOLD_START: usize = 30;
/// One cycle: the sweep right, the hold at its end, the sweep back, the hold at the
/// left. `createFrames`'s own sum, `ui/spinner.ts:306`.
const SCAN_FRAMES: usize = DOT_CELLS + HOLD_END + (DOT_CELLS - 1) + HOLD_START;

/// Where the scanner is on one frame: the head's cell, which way it is going, the
/// frames into a hold and its length, and the cells into a sweep and its length.
/// `getScannerState`'s four legs, in its order -- forward, hold at the end,
/// backward, hold at the start -- and the two progress pairs are zero on each
/// other's legs.
fn scan(frame: usize) -> (isize, bool, usize, usize, usize, usize) {
    let at = frame % SCAN_FRAMES;
    let back = DOT_CELLS - 1;
    if at < DOT_CELLS {
        (at as isize, true, 0, 0, at, DOT_CELLS)
    } else if at < DOT_CELLS + HOLD_END {
        let held = at - DOT_CELLS;
        (back as isize, true, held, HOLD_END, 0, 0)
    } else if at < DOT_CELLS + HOLD_END + back {
        let moved = at - DOT_CELLS - HOLD_END;
        let head = (back - 1 - moved) as isize;
        (head, false, 0, 0, moved, back)
    } else {
        let held = at - DOT_CELLS - HOLD_END - back;
        (0, false, held, HOLD_START, 0, 0)
    }
}

/// The scanner at the status row's left end, and the tone it is drawn in.
///
/// While a round is in flight the head sweeps the wire with six cells of trail
/// behind it, drains at the end, comes back, and rests at the left before it goes
/// again -- opencode's own motion, not a dot lapping a track. The colour says what
/// the round is doing in the same tones the conversation says it in: grey while the
/// model thinks, blue while a tool has it, red on a round that failed, and the text
/// tone with nothing running, which is what a finished round settles to.
///
/// The cells it is not on breathe: opencode draws them at 0.6 of the tone, faded to
/// 0.3 of that as a hold runs and back up as it moves (`ui/spinner.ts:64-79`), so
/// the wire recedes while the head rests and lifts as it comes over. Their unlit
/// cell is `⬝`, a family this screen draws nowhere else, so the wire is the
/// `·` it has always been and `■` is the only new mark.
pub fn dots(state: &State, ms: u128) -> Vec<Span<'static>> {
    let tone = match state {
        State::Tools => ink::running(),
        State::Running => ink::dim(),
        State::Error => ink::failed(),
        _ => ink::text(),
    };
    if !state.busy() {
        return vec![Span::styled(
            "\u{b7}".repeat(DOT_CELLS),
            Style::new().fg(tone),
        )];
    }
    let page = Color::Rgb(PAGE.0, PAGE.1, PAGE.2);
    let (head, forward, held, hold, moved, sweep) = scan((ms / DOT_STEP_MS) as usize);
    // The resting cells' fade: down across a hold, up across a sweep.
    let fade = if hold > 0 {
        1.0 - 0.7 * (held as f64 / hold as f64).min(1.0)
    } else if sweep > 0 {
        0.3 + 0.7 * (moved as f64 / (sweep.max(2) - 1) as f64).min(1.0)
    } else {
        1.0
    };
    let resting = mix(page, tone, 0.6 * fade);
    (0..DOT_CELLS as isize)
        .map(|cell| {
            // Positive is behind the head, whichever way it is going. A hold steps
            // the trail on, which is the head draining where it stopped.
            let step = (if forward { head - cell } else { cell - head }) + held as isize;
            match step {
                0..=5 => Span::styled(
                    "\u{25a0}",
                    Style::new().fg(mix(page, tone, TRAIL[step as usize])),
                ),
                _ => Span::styled("\u{b7}", Style::new().fg(resting)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scanner's geometry: a sweep to the right end, the trail behind the head,
    /// the return, and a cycle that comes back to where it started. Positions, not
    /// pixels -- the tones under them are opencode's alphas and `dots` owns them.
    #[test]
    fn the_scanner_walks_the_wire_and_comes_back() {
        let lit = |ms: u128| -> Vec<usize> {
            dots(&State::Running, ms)
                .iter()
                .enumerate()
                .filter(|(_, span)| span.content.starts_with('\u{25a0}'))
                .map(|(cell, _)| cell)
                .collect()
        };
        let at = |frame: u128| frame * DOT_STEP_MS;
        assert_eq!(lit(0), vec![0], "the head enters at the left");
        assert_eq!(
            lit(at(7)),
            (2..8).collect::<Vec<_>>(),
            "six cells of trail at the right end"
        );
        assert_eq!(
            lit(at(13)),
            vec![7],
            "the trail drains where the head stopped"
        );
        assert_eq!(lit(at(17)), vec![6, 7], "and it comes back from the right");
        assert_eq!(lit(at(23)), (0..6).collect::<Vec<_>>(), "back at the left");
        assert_eq!(lit(at(54)), lit(0), "one cycle is 54 frames");
    }
}
