//! What quik draws, in cells.
//!
//! One page of a Figma file, built for a terminal: a dark page, rows led by a
//! dot with their detail hanging under a corner, the line you typed on a bar of
//! its own, and the input fenced by two rules. The rows come from
//! `cuacode::transcript`: nothing here re-derives a conversation, and nothing
//! here owns one.
//!
//! The model's own text is markdown and is drawn as some of it: bold, italic,
//! code, headings, bullets and fences, with the markers gone. Nothing else on a row
//! is read that way -- a call, a result and a notice are drawn as written.

use cuacode::session;
use cuacode::transcript::{Row, describe_args, is_bulky};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::{Opened, Turn};

/// The page colour, in one place: the terminal's own background is set to this
/// at startup, so a cell nothing draws is still the page.
///
/// It is ember's `background` to the value, and it has to be: fleet draws the
/// pane `theme::term_bg()` opaque and a terminal program's page lands inside it,
/// so one digit of difference is a seam down the edge of the pane.
pub const PAGE: (u8, u8, u8) = (0x15, 0x11, 0x10);

/// The palette, in ember's own tones: the warm brown-black page, greys that lean
/// warm rather than blue, a warm off-white for text and never `#fff`, and one hot
/// orange for everything you can act on.
///
/// The tone each one quotes is named beside it, so it can be checked against
/// `superset-src/packages/shared/src/themes/built-in/ember.ts` by eye. Nothing
/// here is invented, and the two that are *derived* say so and say how.
///
/// A cell has no alpha, so a tone that is a wash over the page is flatten here and
/// the flattened value is what is written: every one of these is legible against
/// `PAGE`, and the ones that replace an old tone hold at least the contrast that
/// tone had on the old cold page (`#282c34`).
pub mod ink {
    use ratatui::style::Color;

    /// ember `foreground` (#eae8e6). Warm off-white, and never `#ffffff`: pure
    /// white on a warm black is a different product.
    pub fn text() -> Color {
        Color::Rgb(0xea, 0xe8, 0xe6)
    }
    /// ember `muted-foreground` (#a8a5a3). The header's second and third lines,
    /// a thought's own text, and anything the design drew grey.
    pub fn dim() -> Color {
        Color::Rgb(0xa8, 0xa5, 0xa3)
    }
    /// ember `muted-foreground` one rung down (#9a9794), which is fleet's own
    /// `faint` and the rung a label that is meant to be found rather than read
    /// belongs on. A result: present, and not the point.
    pub fn faint() -> Color {
        Color::Rgb(0x9a, 0x97, 0x94)
    }

    /// The corner a row's detail hangs from, which is the mark that says "this
    /// is what came back". ember `bright-black`'s warm grey, lifted to
    /// `#a8a5a3`-and-`bright-black` by half: it carries the corner and a detail
    /// panel's key names, so it is read as a tone and not only felt as a rule.
    pub fn branch() -> Color {
        Color::Rgb(0x8a, 0x86, 0x84)
    }
    /// The bar behind the line you typed. ember `ring` (#3a3837), which is the
    /// rung the window itself uses for "a surface one step above the page" (see
    /// fleet's `line_strong`). It measures 1.61:1 against the page, the same
    /// separation the old cold bar had, and the user's own text reads 9.5:1 on
    /// it. Not a tint of the foreground at full strength: this is a plate under
    /// a line, not a row lit by a pointer.
    pub fn bar() -> Color {
        Color::Rgb(0x3a, 0x38, 0x37)
    }
    /// The two rules the input sits between, and a panel's edge. ember
    /// `bright-black` (#5c5856): the same value fleet gives its pane rim, so the
    /// input's rules and the rim around the terminal are one hairline.
    pub fn rule() -> Color {
        Color::Rgb(0x5c, 0x58, 0x56)
    }
    /// A row that finished. ember's `terminal.green` (#7ec699) rather than its
    /// `success`: the terminal block is the one written *for* a pane, and a tick
    /// beside a verb reads as one word with it.
    pub fn done() -> Color {
        Color::Rgb(0x7e, 0xc6, 0x99)
    }
    /// A row that failed. ember's `terminal.red` (#dc6b6b), the terminal block's
    /// own rather than `destructive` (#cc4444), which fleet's theme records as a
    /// fill that reads 2.9:1 as words on a page.
    pub fn failed() -> Color {
        Color::Rgb(0xdc, 0x6b, 0x6b)
    }
    /// A row with nothing to report: neither finished nor failed. ember
    /// `muted-foreground` one rung down (#9a9794), which is fleet's own `faint`:
    /// one tone for "there is something here, and it is not the point", used by
    /// both the window and the pane.
    pub fn quiet() -> Color {
        Color::Rgb(0x9a, 0x97, 0x94)
    }
    /// Code, inline and fenced. ember `chart3` (#d4a84b), the warm amber in the
    /// chart row that is meant to be a colour rather than a state: `chart2` is
    /// the same green as `done` and `chart5` the same red as `failed`, so `chart3`
    /// is the only one of them that cannot be mistaken for a tick or a cross.
    pub fn code() -> Color {
        Color::Rgb(0xd4, 0xa8, 0x4b)
    }
    /// A link. ember's `terminal.blue` (#61afef). Its address is drawn beside it
    /// in `quiet`, so the colour is what says the words are clickable.
    pub fn link() -> Color {
        Color::Rgb(0x61, 0xaf, 0xef)
    }
    /// Something to act on rather than something broken: a build origin has moved
    /// past, which is neither a failure nor a state to ignore. ember `warning`
    /// (#e5c07b), the same rung the window uses for its blocked state.
    pub fn warn() -> Color {
        Color::Rgb(0xe5, 0xc0, 0x7b)
    }
    /// A call to another agent, or a message from one. Green is a tool that
    /// finished and red is one that failed; neither is true here, and a red mark
    /// beside a working peer would read as something broken. Nothing in the palette
    /// sits between them, so this is the one rung added for it.
    pub fn peer() -> Color {
        Color::Rgb(0x8f, 0xb8, 0xd8)
    }
    /// The verb of a call: what a row *did*. ember's accent (#e07850), the one
    /// hue that means "you can act on this", and a call is the thing you act on.
    /// It is 24 dE from every other tone that can share a call's row; on a call
    /// that failed it sits 12.6 dE from the red cross beside it, which is close
    /// but is two shapes as well as two hues.
    pub fn call() -> Color {
        Color::Rgb(0xe0, 0x78, 0x50)
    }
    /// A call in flight, and the bright end of the sweep over the step's name while one
    /// is running. Blue because the two halves of a round have to be told apart at a
    /// glance and the thought's sweep is warm white: the same shape of highlight in two
    /// families, which is what makes the difference readable as a colour and not as a
    /// change of subject. This is the band only -- the name it passes over rests at
    /// `running_dim`, exactly as it does under the warm one, so the row keeps the tone
    /// it always had and what arrives is a blue pass through it.
    pub fn running() -> Color {
        Color::Rgb(0x74, 0xa9, 0xd8)
    }
    /// Where that name rests while the tools have the worker: `dim` cooled by a step,
    /// not recoloured. A blue band through a warm grey reads as two things on one row;
    /// this is the same grey with the blue's own shadow in it, and it measures 7.7:1
    /// on the page against `dim`'s 7.6:1, so nothing about the row gets louder.
    pub fn running_dim() -> Color {
        Color::Rgb(0xa2, 0xa6, 0xad)
    }
}

/// The dot, the tick, the cross, the prompt, and what hangs in front of a head line
/// or its detail. Both leads are two cells wide and start at the content's own left
/// column, so a wrapped line has the whole width to wrap in and a continuation lands
/// under the text it belongs to rather than under the glyph.
///
/// U+25CF is a filled circle with text presentation, unlike the mockup's U+23FA,
/// which terminals paint as an emoji. There is no dot icon to be had, so the dot is
/// a character that cannot be mistaken for one.
const DOT: &str = "\u{25cf}";
const TICK: &str = "\u{2713}";
const CROSS: &str = "\u{2717}";
/// The peer mark: a four-pointed star. A shape and not a picture, so it takes the
/// row's colour and stays in the glyph column, and unlike a tick or a cross it says
/// nothing about whether anything worked. U+2726 over the smiling face it started as:
/// a face is an emoji in a column of dots, and this reads as a mark.
const BUDDY: &str = "\u{2726}";
pub const PROMPT: &str = "\u{276f}";
const HEAD_LEAD: &str = "\u{25cf} ";
/// It opens up and to the right, so the stroke points back at the head it
/// dropped from and the arm runs into the text beside it. U+250C is the same
/// corner the other way up, and reads as the detail having fallen off the
/// bottom of the head.
const BODY_LEAD: &str = "  \u{2514} ";
/// An opened step's detail. Four columns, which is where a call's own text starts
/// (`  \u{25cf} `), so a thought and the calls under it share one left edge and the
/// single corner above them stays the summary's mark alone.
/// The lead on a thought inside an opened step. A rail rather than a mark: a
/// thought is several lines of somebody's working, and a mark at the top of it
/// said nothing about the lines under it. The calls beside it wear a dot, so a
/// column of rails and a column of dots is two kinds of thing at a glance.
const THOUGHT_LEAD: &str = "  \u{2502} ";

/// How much room a code block needs before its box is worth drawing at all. Below
/// this the frame is more of it than the code is.
const MIN_BOX: usize = 16;

/// How long a path may draw before its middle is dropped. A path that fits is
/// left whole, which is what keeps a file in this project reading as `src/view.rs`:
/// where it is, in full, because it is short enough to say.
const PATH_MAX: usize = 40;

/// Frames for the turn indicator. A still character beside a live stream reads
/// as a hang, and the wait is the part nobody can account for.
const SPINNER: [&str; 10] = [
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}",
];

/// The shape that leads a row, which is what says at a glance what kind of row it
/// is and what became of it: a reasoning row and the answer keep the dot, a call
/// that ran gets a tick or a cross, and a call that never reported keeps a dim dot.
///
/// The mockup led every row with the same dot in one of four colours, and four
/// colours down a column is four things to tell apart from memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mark {
    Dot,
    Tick,
    Cross,
    /// Two agents talking. Its own shape because it is neither of the other two: a
    /// peer message is not work that finished, and a tick beside it would say it was.
    Buddy,
}

impl Mark {
    fn glyph(self) -> &'static str {
        match self {
            Mark::Dot => DOT,
            Mark::Tick => TICK,
            Mark::Cross => CROSS,
            Mark::Buddy => BUDDY,
        }
    }
}

/// What the worker last said about a round's thinking.
///
/// It is what the design's readout on the reasoning row is made of. All of it is
/// zero when the worker has said nothing, and the readout is then absent rather
/// than zero.
#[derive(Clone, Copy, Default, Debug)]
pub struct Round {
    pub tokens: i64,
    pub tps: f64,
    pub estimated: bool,
}

/// One run of text: what it says, and how it is drawn.
pub type Run = (String, Style);

/// One logical line of the transcript, before it is wrapped to a width: what
/// hangs in front of it, and the runs it is drawn in.
///
/// Runs rather than a string because the model writes markdown and a terminal row
/// can carry some of it: what is left here has the markers gone and the styles
/// they asked for, so wrapping and drawing never see an asterisk.
#[derive(Debug)]
pub struct Piece {
    pub lead: String,
    pub lead_style: Style,
    /// Whether a blank line sits above this piece, below it, or both. Two flags
    /// rather than one, because a row showing its detail wants air around the row
    /// and not inside it: the gap belongs above the head and after the last line
    /// of the detail, never between the head and the detail the corner joins it
    /// to.
    ///
    /// A row that is only a step sits close to the one above it, which is what
    /// makes a run of calls read as one list and an answer read as its own thing.
    air_above: bool,
    air_below: bool,
    /// One entry per line the text itself had, so a thought's paragraphs stay
    /// paragraphs.
    pub lines: Vec<Vec<Run>>,
    /// Whether the lead opens every line or only the first. A rail is part of the
    /// quote -- a paragraph that wraps must still look quoted on the line it
    /// wrapped to -- where a mark belongs to the piece once, at the front.
    pub rail: bool,
}

impl Piece {
    /// Say where this piece is given room: a blank line above it, below it, or
    /// both. The answer is what the run was for rather than a step through it, so
    /// it gets both. A row showing its detail gets air above its head and below
    /// its detail, which keeps a run of heads tight while the detail hangs
    /// straight off the head it belongs to.
    fn air(mut self, above: bool, below: bool) -> Piece {
        self.air_above = above;
        self.air_below = below;
        self
    }

    /// The piece as plain text with the styles dropped: what `--print` writes and
    /// what a test reads.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| {
                line.iter()
                    .map(|(text, _)| text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A piece drawn as written: a head line, a result, a notice.
fn plain(lead: impl Into<String>, lead_style: Style, text: &str, style: Style) -> Piece {
    Piece {
        lead: lead.into(),
        lead_style,
        air_above: false,
        air_below: false,
        rail: false,
        lines: text
            .split('\n')
            .map(|line| vec![(line.to_string(), style)])
            .collect(),
    }
}

/// A piece whose text is the model's own, drawn as the markdown it is. `room` is
/// how wide its row is, which a rule and a table are sized to.
fn prose(
    lead: impl Into<String>,
    lead_style: Style,
    text: &str,
    style: Style,
    room: usize,
) -> Piece {
    Piece {
        lead: lead.into(),
        lead_style,
        air_above: false,
        air_below: false,
        rail: false,
        lines: formatted(text, style, room),
    }
}

/// The transcript, wrapped to a width, and which screen lines each row took.
///
/// Wrapping happens here rather than in the widget so the scroll offset is
/// exact: one screen line is one line, and the offset counts them.
pub struct Drawn {
    pub lines: Vec<Line<'static>>,
    pub spans: Vec<(usize, usize)>,
    /// For every piece, the row it was drawn from and the row its step is folded
    /// against. Those two numbers are what a click on its screen lines acts on.
    pub anchors: Vec<(usize, usize)>,
}

/// Fold `pieces` onto a screen of this width, with a blank line wherever a piece
/// earns one.
///
/// The gap is a screen line, so it is counted like any other: that is what keeps
/// the scroll offset exact and a click on the gap landing on the row above it.
pub fn draw(pieces: &[Held], width: usize) -> Drawn {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans = vec![(0usize, 0usize); pieces.len()];
    for (i, held) in pieces.iter().enumerate() {
        let piece = &held.piece;
        let lead = columns(&piece.lead);
        let start = lines.len();
        let room = width.saturating_sub(lead).max(1);
        for (n, logical) in piece.lines.iter().enumerate() {
            for (k, runs) in wrap_runs(logical, room).into_iter().enumerate() {
                // A continuation is indented by as many columns as the lead draws
                // in, not by as many characters as it is written with: a lead is
                // two columns wide and two characters long only while it is ASCII.
                let indent = if piece.rail || (n == 0 && k == 0) {
                    piece.lead.to_string()
                } else {
                    " ".repeat(lead)
                };
                // A line carrying a background is a bar, and a bar spans its row
                // rather than stopping at the last word.
                let mut spans_out = vec![Span::styled(indent, piece.lead_style)];
                spans_out.extend(
                    runs.into_iter()
                        .map(|(text, style)| Span::styled(text, style)),
                );
                if let Some(bg) = piece
                    .lines
                    .first()
                    .and_then(|l| l.first())
                    .and_then(|(_, s)| s.bg)
                {
                    let used: usize = spans_out.iter().map(|s| columns(&s.content)).sum();
                    spans_out.push(Span::styled(
                        " ".repeat(width.saturating_sub(used)),
                        Style::new().bg(bg),
                    ));
                }
                lines.push(Line::from(spans_out));
            }
        }
        // Air between rows, but only where there is something to separate: a run of
        // steps sits close, because a column of calls is one list to read, and the
        // answer is given room on both sides -- even when more calls follow it. A row
        // showing its detail claims the air above its head and below its detail, so
        // the gap lands outside the row and not between a head and its own detail.
        if i + 1 < pieces.len() && (piece.air_below || pieces[i + 1].air_above) {
            lines.push(Line::from(""));
        }
        spans[i] = (start, lines.len());
    }
    Drawn { lines, spans, anchors: pieces.iter().map(|p| (p.row, p.head)).collect() }
}

/// Wrap runs onto lines of at most `width` columns, on words.
///
/// A word belongs to whichever run it was written in, so its style comes with it,
/// and so does the space in front of it: a space is not a run of its own, and one
/// dropped between two runs is the difference between `is two` and `istwo`.
///
/// Columns, not characters: a wide glyph is two of the width it takes up, and a
/// line counted in characters draws past the edge it was wrapped to. Everything
/// here that measures a word measures it in columns for that reason.
fn wrap_runs(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    // A line with nothing on it is still a line: it is the gap between two
    // paragraphs, and dropping it would run them together.
    if runs.is_empty() {
        return vec![Vec::new()];
    }
    let mut out: Vec<Vec<Run>> = Vec::new();
    let mut line: Vec<Run> = Vec::new();
    let mut used = 0usize;
    // How many spaces have been seen and not yet put anywhere. They are written
    // in front of the next word, and taken by a wrap, which is the one place they
    // were a separator rather than part of the line.
    //
    // A count, and not a flag: a code line's indent and a box's padding are runs
    // of spaces, and a row that collapsed them would be lying about the two
    // things this drawing is for. Only the space *between* two words is one space.
    let mut pending = 0usize;
    let chunks = runs
        .iter()
        .flat_map(|(text, style)| text.split_inclusive(' ').map(|c| (c, *style)));
    for (chunk, style) in chunks {
        let word = chunk.trim_end_matches(' ');
        let trailed = word.len() < chunk.len();
        if !word.is_empty() {
            if used + pending + columns(word) > width && !line.is_empty() {
                out.push(std::mem::take(&mut line));
                used = 0;
                pending = 0;
            }
            if pending > 0 {
                push(&mut line, &" ".repeat(pending), style);
                used += pending;
                pending = 0;
            }
            let mut word = word.to_string();
            // A word longer than the room left on its line is split rather than
            // allowed to run off the edge, and the room is what is left of the
            // line: a row that had already spent its indent splitting at the full
            // width would draw the indent's worth of columns past the edge.
            let mut room = width.saturating_sub(used).max(1);
            while columns(&word) > room {
                let cut = split_at(&word, room);
                let rest = word[cut..].to_string();
                word.truncate(cut);
                push(&mut line, &word, style);
                out.push(std::mem::take(&mut line));
                word = rest;
                room = width;
                used = 0;
            }
            used += columns(&word);
            if !word.is_empty() {
                push(&mut line, &word, style);
            }
        }
        pending += usize::from(trailed);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// A line of code with its tabs turned into the spaces they stand for.
///
/// A tab is a character a file counts and the terminal does not paint: ratatui
/// drops a control character rather than expanding it to a tab stop, so a `\t`
/// indent would draw as no indent at all. Four columns is the width a tab is
/// counted as here, and it has to happen before anything measures a line, because
/// a tab that reached the screen would take its indent with it.
///
/// ponytail: one fixed width, not the stop a terminal would advance to. Nothing
/// drawn here is column-aligned through a tab, so a real tab stop would be code
/// spent on a case this file does not have.
fn untabbed(line: &str) -> String {
    line.replace('\t', "    ")
}

/// How many terminal columns a string draws in.
///
/// The one measurement this file wraps by. It is the same table ratatui lays a
/// line out by, so a line wrapped to a width is a line drawn in that width: a
/// character counted as one and painted as two is exactly the overflow this
/// replaced.
///
/// A control character counts one here and is dropped when painted, so a line with
/// a tab in it draws a column narrower than this says. Narrower is the safe way to
/// be wrong; over is not.
pub(crate) fn columns(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

/// How many bytes of `text` to take to fill `room` columns without going past it.
///
/// Never zero for a non-empty string: a glyph two columns wide in one column of
/// room still has to be taken whole, because half a glyph is not a cell and not
/// taking it would be a loop that never advances.
fn split_at(text: &str, room: usize) -> usize {
    let mut at = 0;
    let mut wide = 0;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if wide + w > room {
            break;
        }
        wide += w;
        at += ch.len_utf8();
    }
    if at == 0 {
        text.chars().next().map_or(0, char::len_utf8)
    } else {
        at
    }
}

/// Add text to the run being built, continuing the style it follows if it shares it.
fn push(line: &mut Vec<Run>, text: &str, style: Style) {
    match line.last_mut() {
        Some((last, styled)) if *styled == style => last.push_str(text),
        _ => line.push((text.to_string(), style)),
    }
}

/// One piece, and where it came from.
///
/// `row` is the transcript row the piece is drawn from, which is what a click on
/// it acts on; `head` is the row at the head of the step it belongs to, which is
/// what folding it acts on. They are the same for a row that stands alone.
#[derive(Debug)]
pub struct Held {
    pub row: usize,
    pub head: usize,
    pub piece: Piece,
}

impl std::ops::Deref for Held {
    type Target = Piece;
    fn deref(&self) -> &Piece {
        &self.piece
    }
}

/// A stretch of the conversation as one thing on screen: either a row that stands
/// alone, or a step -- a thought with the calls that came out of it, or a run of
/// calls with no thought above them.
///
/// This is the compression the compact screen is: a thought and the six calls it
/// made are three lines, not fourteen, and what those calls were is on the one
/// line that hangs under the head.
enum Cluster {
    Row(usize),
    Step {
        /// The latest thought, which is what the step is called.
        think: Option<usize>,
        /// The thoughts this step swallowed on the way: a run of think, act,
        /// think again with nothing said in between is one stretch of work, and
        /// it reads as one row that keeps renaming itself.
        earlier: Vec<usize>,
        calls: Vec<usize>,
    },
}

/// Group the rows into what the screen draws as one thing.
///
/// A thought opens a step and the calls that follow it join it. A call with no
/// thought above it opens a step of its own, and the calls after it join that.
/// Anything else -- what a person said, what the model answered, a notice, a peer
/// -- ends the open step and stands on its own.
fn clusters(rows: &[Row]) -> Vec<Cluster> {
    type Open = (Option<usize>, Vec<usize>, Vec<usize>);
    let mut out = Vec::with_capacity(rows.len());
    let mut open: Option<Open> = None;
    let flush = |out: &mut Vec<Cluster>, open: &mut Option<Open>| {
        if let Some((think, earlier, calls)) = open.take() {
            out.push(Cluster::Step { think, earlier, calls });
        }
    };
    for (i, row) in rows.iter().enumerate() {
        match row {
            Row::Thinking { .. } => {
                // A thought after a round of calls, with nothing said in between,
                // is the same stretch of work carrying on. It joins the step it
                // was already in and becomes that step's new name; anything else
                // starts a step of its own.
                match open.take() {
                    Some((prev, mut earlier, calls)) if !calls.is_empty() => {
                        earlier.extend(prev);
                        open = Some((Some(i), earlier, calls));
                    }
                    Some((prev, earlier, calls)) => {
                        out.push(Cluster::Step { think: prev, earlier, calls });
                        open = Some((Some(i), Vec::new(), Vec::new()));
                    }
                    None => open = Some((Some(i), Vec::new(), Vec::new())),
                }
            }
            Row::Tool { .. } => match &mut open {
                Some((_, _, calls)) => calls.push(i),
                None => open = Some((None, Vec::new(), vec![i])),
            },
            _ => {
                flush(&mut out, &mut open);
                out.push(Cluster::Row(i));
            }
        }
    }
    flush(&mut out, &mut open);
    out
}

/// Every logical line of the transcript, each tagged with the row it came from.
///
/// The one definition of what a row looks like: `draw` wraps these for the
/// screen and `--print` writes them out unwrapped, so the two cannot drift.
/// `expanded` answers whether a step shows its detail, which is the reader's
/// folding rather than anything about the conversation, and `unfolded` answers the
/// same for one thought inside an opened step: a step shows its calls, and each
/// thought in it is a line until the reader asks for the text. `figures` is what
/// the worker reported for a row's own round, if it reported anything. `writing`
/// is the row being written right now, `flash` is which half of its blink is on,
/// and `width` is how wide a row draws, which a rule and a table are sized to.
pub fn pieces(
    rows: &[Row],
    expanded: &dyn Fn(usize, &Row) -> bool,
    unfolded: &dyn Fn(usize) -> bool,
    figures: &dyn Fn(usize) -> Option<Round>,
    writing: Option<usize>,
    flash: bool,
    width: usize,
    // How far the highlight on a newly arrived peer message has travelled, or None
    // when there is nothing to sweep. Last, so every call site keeps the order it
    // was written in down to `width`.
    shimmer: Option<usize>,
) -> Vec<Held> {
    let mut out = Vec::new();
    for cluster in clusters(rows) {
        let (i, think, earlier, calls) = match cluster {
            Cluster::Step { think, earlier, calls } => {
                let head = think.unwrap_or_else(|| calls[0]);
                (head, think, earlier, calls)
            }
            Cluster::Row(i) => (i, None, Vec::new(), Vec::new()),
        };
        if think.is_some() || !calls.is_empty() {
            // The step's own first row, which does not move when a later round
            // joins it and renames it. Folding has to be remembered against that:
            // keyed on the name, an opened step closed itself every time the work
            // moved on to its next thought.
            let start = earlier.first().copied().or(think).unwrap_or(i);
            let start = step_start(rows, start);
            let open = expanded(start, &rows[start]);
            out.extend(step(think, &earlier, &calls, shimmer, rows, open, unfolded, figures, writing, flash, width, start));
            continue;
        }
        let row = &rows[i];
        match row {
            // The line you typed, on its own bar, and the runtime text that keeps
            // only the line saying what it is. Neither has a dot.
            Row::User { text, images } => {
                // One blank line above the bar and none under it: the turn is
                // where this screen is read from, and a line of air around every
                // answer would double the height of the conversation.
                out.push(Held { row: i, head: i, piece: plain(
                    "",
                    Style::new().bg(ink::bar()),
                    &format!("{PROMPT} {text}"),
                    Style::new().fg(ink::text()).bg(ink::bar()),
                ).air(true, false) });
                if !images.is_empty() {
                    out.push(Held { row: i, head: i, piece: plain(
                        "",
                        Style::new(),
                        &format!("{} attached: {}", images.len(), images.join(", ")),
                        Style::new().fg(ink::faint()),
                    ) });
                }
            }
            Row::Peer { head, text } => {
                // The buddy leads it, in the peer colour: "agent x to y" is not a note
                // about the machine, and a plain notice row reads as one. The body hangs
                // under it like a call's result, so a long message collapses to its head
                // and opens on a press.
                let shown = expanded(i, row);
                out.push(Held { row: i, head: i, piece: led(Mark::Buddy, ink::peer(), ink::peer(), head).air(shown, false) });
                if shown && !text.trim().is_empty() {
                    let room = width.saturating_sub(columns(BODY_LEAD)).max(1);
                    // A message that just landed is swept once, and a round that is still
                    // running keeps its sweep: the same highlight either way, and the same
                    // phase, so the row does not change speed when the run ends. `shimmer`
                    // counts this screen's own clock and is None when nothing is moving.
                    let lines = match shimmer {
                        Some(phase) => shimmer_runs(
                            formatted(text, Style::new().fg(ink::dim()), room),
                            ink::dim(),
                            ink::text(),
                            Motion::Pass,
                            phase,
                        ),
                        None => formatted(text, Style::new().fg(ink::dim()), room),
                    };
                    out.push(Held { row: i, head: i, piece: Piece {
                        lead: BODY_LEAD.to_string(),
                        lead_style: Style::new().fg(ink::branch()),
                        air_above: false,
                        air_below: true,
                        rail: false,
                        lines,
                    } });
                }
            }
            Row::Notice { head, hidden } => {
                let tail = if *hidden > 0 {
                    format!("   +{hidden} lines")
                } else {
                    String::new()
                };
                out.push(Held { row: i, head: i, piece: plain(
                    "",
                    Style::new(),
                    &format!("\u{b7} {head}{tail}"),
                    Style::new().fg(ink::quiet()),
                ) });
            }
            // The answer is the model's own text, so it is the one head that is
            // read as markdown. Its dot is white whatever else is true: this is
            // what the run was for, and the one row that is not a step towards it.
            Row::Content { text, .. } => out.push(Held { row: i, head: i, piece: led_prose(
                Mark::Dot,
                ink::text(),
                ink::text(),
                text,
                width.saturating_sub(columns(HEAD_LEAD)).max(1),
            ).air(true, true) }),
            Row::Error(text) => out.push(Held { row: i, head: i, piece: led(Mark::Cross, ink::failed(), ink::failed(), text) }),
            // Thought and call rows never reach here: `clusters` has already put
            // them in a step, and `step` draws the lot.
            Row::Thinking { .. } | Row::Tool { .. } => {}
        }
    }
    out
}

/// One step on screen: a head that says what it was doing, one line under it
/// saying what that cost and what it ran, and the detail when the reader opens
/// it.
///
/// `head` is the row the fold is remembered against, which is the thought when
/// there is one and the first call when there is not. `unfolded` is the reader's
/// own fold, per thought: what it says yes to shows its text, and everything else
/// in the step is one line.
#[allow(clippy::too_many_arguments)]
fn step(
    think: Option<usize>,
    earlier: &[usize],
    calls: &[usize],
    shimmer: Option<usize>,
    rows: &[Row],
    open: bool,
    unfolded: &dyn Fn(usize) -> bool,
    figures: &dyn Fn(usize) -> Option<Round>,
    writing: Option<usize>,
    flash: bool,
    width: usize,
    head: usize,
) -> Vec<Held> {
    // The step's newest row, and whether the tools are what the worker is on: a call
    // that has not reported yet is one in flight, and one that has reported with no
    // thought after it is the wait between the tool and the model starting again.
    // Both are blue; a thought after them is the model thinking, and warm. Read off
    // the record rather than off `writing`, which still points at the thought it
    // streamed into while the tool it asked for is running.
    let newest = earlier
        .iter()
        .chain(think.iter())
        .chain(calls.iter())
        .copied()
        .max()
        .unwrap_or(head)
        .max(head);
    let in_tools = matches!(&rows[newest], Row::Tool { .. });
    // `writing` is None when nothing is being written, and `think` is None for a
    // step with no thought: comparing the two directly would call every calls-only
    // step live, which is a colour and a blink that say the wrong thing. Nothing
    // more is needed for the tools: while a call runs, and in the wait after it
    // before the model starts again, `writing` is still the thought the worker
    // streamed into before it asked for the call, so the step is already live.
    let live = think.is_some_and(|t| writing == Some(t))
        || calls.iter().any(|&c| {
            writing == Some(c) && matches!(&rows[c], Row::Tool { result: None, .. })
        });
    let failed_any = calls.iter().any(|&c| match &rows[c] {
        Row::Tool { result: Some(r), .. } => failed(r),
        _ => false,
    });
    // One mark, four states, and no second shape: green when the step is done,
    // blinking white while it is being written, red when anything in it failed.
    let lead = if live {
        blink(flash)
    } else if failed_any {
        ink::failed()
    } else {
        ink::done()
    };
    let mut out = Vec::new();

    // The head: the step's name. A thought is the model's own words, cut to the
    // sentence that says what it is doing; a run of calls is named by the tool it
    // ran, with the count when there is more than one.
    // Which thought names the step: the last one that has a call after it. A
    // merged run usually ends on a thought with nothing after it -- the wrap-up
    // before the answer -- and that one is a sentence about being finished, not a
    // name for the work.
    // Which thought names the step, in order of preference: one the small model
    // has already named, then one with a call after it (a merged run usually ends
    // on a wrap-up sentence, which is about being finished and not a name), then
    // the step's opening words. Never the newest text for its own sake: a head that
    // re-derived itself from whatever reasoning had arrived last was a head that
    // changed while you were reading it.
    let named = earlier
        .iter()
        .copied()
        .chain(think)
        .rev()
        .find(|&t| matches!(&rows[t], Row::Thinking { name: Some(_), .. }))
        .or_else(|| {
            earlier
                .iter()
                .copied()
                .chain(think)
                .rev()
                .find(|&t| calls.last().is_some_and(|&c| c > t))
        })
        .or_else(|| earlier.first().copied())
        .or(think);
    let said = match named {
        Some(t) => {
            let (text, given) = match &rows[t] {
                Row::Thinking { text, name, .. } => (text.as_str(), name.as_deref()),
                _ => ("", None),
            };
            // The worker's small model names the step when it can; these are the
            // thought's own first words until it does, and the only name there is
            // when nothing is naming anything.
            let name = given.map(str::to_string).unwrap_or_else(|| thought_name(text));
            vec![(name, Style::new().fg(ink::text()))]
        }
        None => group_head(rows, calls),
    };
    // The words on the head, which is what a detail line is compared against: a
    // thought whose whole text is already the head is not drawn again under it.
    let name = said.first().map(|(t, _)| t.clone()).unwrap_or_default();
    // The row being worked carries the band, over its own words: the name is what
    // the eye is on while a round runs, and a band that skipped it and ran under it
    // on a second line was the wrong line shimmering. Which of the two ramps it
    // runs in is `in_tools`, above: one shape of highlight in two families and two
    // motions -- warm and turning back on itself through a thought, blue and passing
    // left to right while the tools have the worker. The name itself rests at `dim`
    // under the warm one and a cooler grey under the blue, which is the same tone
    // moved a step towards the band rather than a second colour.
    let said = match (live, shimmer) {
        (true, Some(phase)) => {
            let (calm, hot, motion) = if in_tools {
                (ink::running_dim(), ink::running(), Motion::Pass)
            } else {
                (ink::dim(), ink::text(), Motion::Back)
            };
            shimmer_runs(vec![said], calm, hot, motion, phase)
                .pop()
                .unwrap_or_default()
        }
        _ => said,
    };
    out.push(Held {
        row: head,
        head,
        piece: led_runs(format!("{DOT} "), Style::new().fg(lead), said).air(true, false),
    });

    // The line under it: what the thinking cost, what the calls were, and what
    // went wrong. One line, because this row is the compression: everything a
    // reader needs to decide whether to open it, and nothing more.
    let mut meta: Vec<Run> = Vec::new();
    let said = |text: String, colour: Color, meta: &mut Vec<Run>| {
        if text.is_empty() {
            return;
        }
        if !meta.is_empty() {
            meta.push(("  \u{b7}  ".to_string(), Style::new().fg(ink::branch())));
        }
        meta.push((text, Style::new().fg(colour)));
    };
    // What the stretch cost, summed over every round in it. A merged step's rate
    // is not a number: three rounds have three of them, so it says what they cost
    // and lets the individual rounds keep their own rates in the panel.
    if let Some(round) = rounds(think, earlier, figures).filter(|r| r.tokens > 0 || r.tps > 0.0) {
        said(reasoning(round), ink::faint(), &mut meta);
    }
    // One call says what came back, which is the mock's own call box and the row
    // this screen had before it had steps: a head, and the line under it. It is
    // also the only way a failure says *what* failed without opening anything.
    // A run of calls cannot say all their results on one line, so it says what it
    // ran instead.
    if calls.len() == 1 && !(open && think.is_some()) {
        if let Row::Tool { args, result: Some(r), .. } = &rows[calls[0]] {
            if !r.is_empty() {
                let ink = if failed(r) { ink::failed() } else { ink::faint() };
                said(call_result(args, r), ink, &mut meta);
            }
        }
    }
    // A failure is the one thing on this line that is worth reading collapsed: a
    // red dot says something went wrong and this says what. One call says its own
    // result; a run of them says how much of it failed.
    let broken: Vec<usize> = calls
        .iter()
        .copied()
        .filter(|&c| matches!(&rows[c], Row::Tool { result: Some(r), .. } if failed(r)))
        .collect();
    if calls.len() > 1 && !calls.is_empty() && think.is_some() {
        said(ran(calls, rows), ink::faint(), &mut meta);
    }
    // One call said its own failure above, or says it on its own line when the
    // step is opened. A run of them cannot, so it says how much of it broke.
    if !broken.is_empty() && calls.len() > 1 {
        said(format!("{} of {} failed", broken.len(), calls.len()), ink::failed(), &mut meta);
    }
    if !meta.is_empty() {
        out.push(Held {
            row: head,
            head,
            piece: Piece {
                lead: BODY_LEAD.to_string(),
                lead_style: Style::new().fg(ink::branch()),
                air_above: false,
                air_below: false,
                rail: false,
                lines: vec![meta],
            },
        });
    }

    // Opened: the step's own rows, in the order they happened -- a thought, the
    // calls that answered it, the thought that came after. That order is the
    // record; drawing every thought and then every call was the transcript
    // rearranged to suit the shape of the data, and it put reasoning above calls
    // that had happened before it.
    if open {
        let last = earlier
            .iter()
            .chain(think.iter())
            .chain(calls.iter())
            .copied()
            .max()
            .unwrap_or(head)
            .max(head);
        let mut after_thought = false;
        for r in head..=last {
            match &rows[r] {
                Row::Thinking { text, .. } => {
                    if text.trim().is_empty() || is_name(text, &name) {
                        continue;
                    }
                    // Folded or not, the thought is a line of its own; what the
                    // reader's own fold decides is whether that line is its text or
                    // the triangle that opens it. A step full of reasoning used to be
                    // a wall of it with the calls buried in between, and the calls are
                    // what the step was opened for.
                    let mut said = if unfolded(r) {
                        thought(r, head, text, &name, width)
                    } else {
                        vec![folded_thought(r, head, text, &name, figures, width)]
                    };
                    if after_thought {
                        // The seam between what it thought and what it ran: one
                        // line, once per group, rather than between every pair.
                        for piece in said.iter_mut().take(1) {
                            piece.piece.air_above = true;
                        }
                    }
                    out.extend(said);
                    after_thought = true;
                }
                // The call the head is already named for is not drawn again: an
                // opened one-call step would otherwise say the same thing twice.
                Row::Tool { .. } if !(think.is_none() && calls.len() == 1) => {
                    let mut line = call_line(r, head, rows, writing, flash, width);
                    if after_thought {
                        line.piece.air_above = true;
                    }
                    out.push(line);
                    after_thought = false;
                }
                _ => {}
            }
        }
        // An opened step is a block and a closed one is a line, so the gap that
        // separates two of them belongs only after a block: the lines of the list
        // stay tight, and the block is not run into by the next head.
        if let Some(last) = out.last_mut() {
            last.piece.air_below = true;
        }
    }
    out
}

/// Whether this thought is the head's line already, word for word.
///
/// A thought short enough that its name is the whole of it is not drawn twice:
/// the head said it, and the detail under it would be the same sentence again.
fn is_name(text: &str, name: &str) -> bool {
    let whole = text.trim().trim_end_matches('.').trim();
    !whole.is_empty() && whole == name.trim().trim_end_matches('.').trim()
}

/// The row a step started on, given any row inside it.
///
/// A step's rows are contiguous -- thoughts and calls, nothing else -- so walking
/// back over them finds its start without building the clusters again. That start
/// is what a fold is remembered against, because the head it is named for moves
/// when a later round joins it.
pub fn step_start(rows: &[Row], at: usize) -> usize {
    let mut i = at;
    while i > 0 {
        match &rows[i - 1] {
            Row::Thinking { .. } | Row::Tool { .. } => i -= 1,
            _ => break,
        }
    }
    i
}

/// What a step's rounds cost, as one figure: the tokens summed, and a rate only
/// when there is one round to have one.
fn rounds(
    think: Option<usize>,
    earlier: &[usize],
    figures: &dyn Fn(usize) -> Option<Round>,
) -> Option<Round> {
    let mut all: Vec<Round> = earlier.iter().filter_map(|&t| figures(t)).collect();
    let last = think.and_then(|t| figures(t));
    let last_tps = last.map(|r| r.tps).unwrap_or(0.0);
    let last_est = last.map(|r| r.estimated).unwrap_or(false);
    all.extend(last);
    if all.is_empty() {
        return None;
    }
    // The rate of a step with several rounds is the rate of all of it: the tokens
    // of the rounds that timed themselves, over the time they took. Summing their
    // rates would be a number about nothing; leaving the rate out for every merged
    // step meant it never showed at all, because most steps merge.
    let timed: Vec<Round> = all.iter().copied().filter(|r| r.tps > 0.0 && r.tokens > 0).collect();
    let seconds: f64 = timed.iter().map(|r| r.tokens as f64 / r.tps).sum();
    let tokens: i64 = timed.iter().map(|r| r.tokens).sum();
    Some(Round {
        tokens: all.iter().map(|r| r.tokens).sum(),
        tps: if seconds > 0.0 { tokens as f64 / seconds } else { last_tps },
        estimated: all.iter().any(|r| r.estimated) || last_est,
    })
}

/// A thought with its text folded away, as an opened step draws it: the rail and a
/// triangle, and enough words to tell whether it is worth opening.
///
/// The words are the thought's own name, which is the same name the head carries
/// when this is the thought the step is named for. That one line says what the
/// round cost instead: a row whose whole text is the line above it has said
/// nothing, and the figure is a fact about this thought rather than the step.
fn folded_thought(
    index: usize,
    head: usize,
    text: &str,
    name: &str,
    figures: &dyn Fn(usize) -> Option<Round>,
    width: usize,
) -> Held {
    let own = thought_name(text);
    let label = if own == name {
        match figures(index) {
            Some(round) if round.tokens > 0 => {
                format!("{} tokens", tokens(round.tokens, round.estimated))
            }
            _ => "thinking".to_string(),
        }
    } else {
        own
    };
    let room = width.saturating_sub(columns(THOUGHT_LEAD) + 2).max(1);
    Held {
        row: index,
        head,
        piece: Piece {
            rail: true,
            lead: THOUGHT_LEAD.to_string(),
            lead_style: Style::new().fg(ink::branch()),
            air_above: false,
            air_below: false,
            lines: vec![vec![
                ("\u{25b8} ".to_string(), Style::new().fg(ink::branch())),
                (clip(&label, room), Style::new().fg(ink::faint())),
            ]],
        },
    }
}

/// A thought as the opened step draws it: a paragraph per rail, so a long piece of
/// working reads as quoted reasoning rather than as prose that happens to be grey,
/// and the paragraphs inside it keep the blank lines they were written with.
fn thought(index: usize, head: usize, text: &str, name: &str, width: usize) -> Vec<Held> {
    let room = width.saturating_sub(columns(THOUGHT_LEAD)).max(1);
    let mut out: Vec<Held> = Vec::new();
    for (n, paragraph) in text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .enumerate()
    {
        // The head already says this paragraph's first words: a block that opens by
        // repeating its own name has said nothing twice.
        if n == 0 && is_name(paragraph, name) {
            continue;
        }
        let lines = formatted(paragraph, Style::new().fg(ink::dim()), room);
        if lines.is_empty() {
            continue;
        }
        // The seam between two paragraphs is a rail with nothing after it, not a
        // blank line: the rail is what says this is all one piece of reasoning, and
        // a gap with no rail in it breaks the column in two.
        if !out.is_empty() {
            out.push(Held {
                row: index,
                head,
                piece: Piece {
                    rail: true,
                    lead: THOUGHT_LEAD.to_string(),
                    lead_style: Style::new().fg(ink::branch()),
                    air_above: false,
                    air_below: false,
                    lines: vec![Vec::new()],
                },
            });
        }
        out.push(Held {
            row: index,
            head,
            piece: Piece {
                rail: true,
                lead: THOUGHT_LEAD.to_string(),
                lead_style: Style::new().fg(ink::branch()),
                air_above: false,
                air_below: false,
                lines,
            },
        });
    }
    out
}

/// The head of a step with no thought in it: the tool it ran, and how many calls
/// that was.
///
/// One call is drawn whole, exactly as a call's own row says it: `shell(git log)`.
/// A mixed run is named by its verbs rather than by its first call, because the
/// first call's argument is not what the run was: `read, shell ×3`.
fn group_head(rows: &[Row], calls: &[usize]) -> Vec<Run> {
    let Some(Row::Tool { name, args, call, .. }) = calls.first().map(|&c| &rows[c]) else {
        return Vec::new();
    };
    let (verb, hint) = call_name(name, args, call);
    if calls.len() == 1 {
        let mut said = vec![(verb, Style::new().fg(ink::call()))];
        if let Some(hint) = hint {
            said.push(("(".to_string(), Style::new().fg(ink::text())));
            said.extend(arg_runs(&hint, ink::text()));
            said.push((")".to_string(), Style::new().fg(ink::text())));
        }
        return said;
    }
    vec![(tally(&verbs(calls, rows)), Style::new().fg(ink::call()))]
}

/// What a step ran, in the words the line uses: `Ran shell`, `Ran read ×4`,
/// `Ran 5 tools · read ×4, shell ×1`. Verbs, not tool names: a call's head is its
/// verb, and the line under a thought has to speak the same vocabulary.
fn ran(calls: &[usize], rows: &[Row]) -> String {
    let verbs = verbs(calls, rows);
    match verbs.len() {
        0 => String::new(),
        1 => format!("Ran {}", verbs[0]),
        // One verb, so the count is the whole of it: `Ran read ×4` says it once,
        // where "Ran 4 tools" in front of it would say four twice.
        _ if verbs.iter().all(|v| *v == verbs[0]) => format!("Ran {}", tally(&verbs)),
        n => format!("Ran {n} tools · {}", tally(&verbs)),
    }
}

/// The verbs of a run, in the order they were run.
fn verbs(calls: &[usize], rows: &[Row]) -> Vec<String> {
    calls
        .iter()
        .filter_map(|&c| match &rows[c] {
            Row::Tool { name, args, call, .. } => Some(call_name(name, args, call).0),
            _ => None,
        })
        .collect()
}

/// How many of each, most frequent first: `read ×4, shell ×1`.
///
/// Five verbs with two repeats in them is a list you count yourself, and the count
/// is the thing worth knowing at a glance. Ties go by name so the line does not
/// shuffle between two draws of the same run.
fn tally(verbs: &[String]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for verb in verbs {
        match counts.iter_mut().find(|(name, _)| name == verb) {
            Some((_, n)) => *n += 1,
            None => counts.push((verb.clone(), 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts
        .iter()
        .map(|(name, n)| format!("{name} ×{n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One call, on the single line an opened step gives it: what it ran on, and what
/// came back. The panel behind ctrl-o is where the rest of it is.
fn call_line(
    index: usize,
    head: usize,
    rows: &[Row],
    writing: Option<usize>,
    flash: bool,
    width: usize,
) -> Held {
    let Row::Tool { name, args, call, result, .. } = &rows[index] else {
        return Held { row: index, head, piece: plain("", Style::new(), "", Style::new()) };
    };
    let dot = if writing == Some(index) && result.is_none() {
        blink(flash)
    } else {
        call_ink(result.as_deref())
    };
    let (verb, hint) = call_name(name, args, call);
    let colour = arg_ink(result.as_deref());
    // A call is one row by design: the panel has the whole of it, and a row that
    // wraps takes a second line that reads like a second result. So the arguments
    // and the result are both cut to what the line has left, not wrapped.
    let lead_width = columns(&format!("  {DOT} "));
    let tail = result
        .as_deref()
        .filter(|r| !r.is_empty())
        .map(|r| call_result(args, r))
        .unwrap_or_default();
    let hint_room = width
        .saturating_sub(lead_width + columns(&verb) + 2 + columns("  \u{b7}  ") + columns(&tail))
        .max(8);
    let hint = hint.map(|h| clip(&h, hint_room));
    let mut said = vec![(verb.clone(), Style::new().fg(ink::call()))];
    let mut head_width = columns(&verb);
    if let Some(hint) = hint {
        said.push(("(".to_string(), Style::new().fg(colour)));
        said.extend(arg_runs(&hint, colour));
        said.push((")".to_string(), Style::new().fg(colour)));
        head_width += columns(&hint) + 2;
    }
    if let Some(r) = result.as_deref().filter(|r| !r.is_empty()) {
        let ink = if failed(r) { ink::failed() } else { ink::faint() };
        let used = lead_width + head_width + columns("  \u{b7}  ");
        let room = width.saturating_sub(used).max(8);
        said.push((format!("  \u{b7}  {}", clip(&tail, room)), Style::new().fg(ink)));
    }
    Held {
        row: index,
        head,
        piece: led_runs(format!("  {DOT} "), Style::new().fg(dot), said),
    }
}

/// A name for a thought, taken from the thought itself.
///
/// This is the name a row carries before any model has named it: the first
/// sentence of the thought, cut to a head line's length on a word boundary. The
/// worker can name a step with a small model and that name wins when it arrives;
/// without a worker this is the only name there is, which is why it reads as the
/// thought's own words rather than as a label invented for it.
pub fn thought_name(text: &str) -> String {
    /// Where a name stops being a name and becomes a sentence again.
    const MAX: usize = 54;
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, '#' | '*' | '-' | '>' | '`' | '+')
        })
        .replace("**", "");
    let line = line.trim();
    if line.is_empty() {
        return "Thinking".to_string();
    }
    // A whole sentence if it fits a head line. If it does not, the clause before
    // the first comma or semicolon: a name that ends mid-clause reads as a bug,
    // and one that ends at a comma still says what the step is.
    let end = line
        .find(['.', '!', '?'])
        .filter(|&e| e <= MAX)
        .or_else(|| {
            line.find(", ").filter(|&e| e >= 12 && e <= MAX)
        })
        .or_else(|| line.find("; ").filter(|&e| e >= 12 && e <= MAX))
        .unwrap_or_else(|| cut_word(line, MAX));
    let name = line[..end].trim().trim_end_matches('.').trim();
    if name.is_empty() {
        "Thinking".to_string()
    } else {
        name.to_string()
    }
}

/// How much of `text` fits before `max` bytes, cut at the last space, and never
/// inside a character.
fn cut_word(text: &str, max: usize) -> usize {
    if text.len() <= max {
        return text.len();
    }
    match text[..max].rfind(' ') {
        Some(at) => at,
        None => text[..max].char_indices().last().map_or(0, |(i, c)| i + c.len_utf8()),
    }
}

/// Whether a step has anything to open. A thought does (its own text), a run of
/// calls does (one line each), and a peer message does (the message, which is
/// often a paragraph nobody needs on screen twice).
pub fn foldable(row: &Row) -> bool {
    matches!(row, Row::Thinking { .. } | Row::Tool { .. })
        || matches!(row, Row::Peer { text, .. } if !text.trim().is_empty())
}

/// The mark that leads a row, in the shape its state has.
///
/// A colour alone was the design's idea and it does not survive a glance: four
/// colours down a column is four things to tell apart from memory.
fn led(mark: Mark, colour: Color, lead: Color, text: &str) -> Piece {
    plain(
        format!("{} ", mark.glyph()),
        Style::new().fg(lead),
        text,
        Style::new().fg(colour),
    )
}

/// The same lead, over a head that is more than one colour: a call's verb and the
/// argument it names are two things, and the head says so.
fn led_runs(lead: String, lead_style: Style, runs: Vec<Run>) -> Piece {
    Piece {
        lead,
        lead_style,
        air_above: false,
        air_below: false,
        rail: false,
        lines: vec![runs],
    }
}

/// A head line whose text is the model's own, drawn as the markdown it is.
fn led_prose(mark: Mark, colour: Color, lead: Color, text: &str, room: usize) -> Piece {
    prose(
        format!("{} ", mark.glyph()),
        Style::new().fg(lead),
        text,
        Style::new().fg(colour),
        room,
    )
}

/// How far the highlight's falloff reaches from its own centre, in characters.
///
/// The band is a falloff rather than a window: brightest where it is centred and back to
/// the row's own colour at this distance, so what travels along a line is a soft
/// highlight and not a block of white stepping through the text. The whole highlight is
/// twice this wide, and this number is what the stride below is spaced against.
const SHIMMER: usize = 7;

/// How far the highlight moves between two frames, in characters.
///
/// Read by the app as well as here: one number decides how the sweep looks, and a
/// second copy of it in the app is how a shimmer ends up moving at two speeds.
pub const SHIMMER_STEP_MS: u128 = 90;

/// How many characters of travel one step buys.
///
/// A band of five creeping a character at a time would need a hundred and twenty
/// steps to cross a message, which is four seconds of the row moving while somebody
/// is trying to read it.
const SHIMMER_STRIDE: usize = 1;

/// When a just-arrived peer message is drawn at all: a landed message is worth a
/// second's glance. It fades by not being drawn, which is what keeps a settled screen
/// exactly as it was.
///
/// Not the only reason a sweep is on: the app keeps the same highlight travelling while
/// the worker is running something, which is the rest of the time. This is how long the
/// *arrival* is marked after the run has already stopped.
pub const SHIMMER_MS: u128 = 1600;

/// `base` moved towards `top` by `t`, which is in 0..=1.
///
/// The page's tones are all `Color::Rgb`, so a falloff that lands between two of them is
/// mixed per channel and invents nothing: every step of it is a tone the palette already
/// spans. A colour that is not RGB -- a terminal on the 16-colour fallback -- is left
/// exactly as it was rather than handed an index nobody chose.
fn mix(base: Color, top: Color, t: f64) -> Color {
    let (Color::Rgb(br, bg, bb), Color::Rgb(tr, tg, tb)) = (base, top) else {
        return base;
    };
    let t = t.clamp(0.0, 1.0);
    let blend = |from: u8, to: u8| (from as f64 + (to as f64 - from as f64) * t).round() as u8;
    Color::Rgb(blend(br, tr), blend(bg, tg), blend(bb, tb))
}

/// How the highlight travels along a row.
///
/// One shape of highlight in two motions, because the two halves of a round are not
/// doing the same thing with it: a model thinking is going nowhere and a call is one
/// step of the work going past.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// In from the left, out past the right, and round again, with the row dark at
    /// both ends of the cycle. A pass: the work is advancing, and a band that has left
    /// the row is what says one has finished and the next is coming.
    Pass,
    /// Right, then back left, always on the words: it turns around inside the line
    /// instead of leaving it. Thinking does not go anywhere, and a band that left the
    /// row would say it had.
    Back,
}

/// A soft highlight travelling along a line of text.
///
/// The colour, not the characters: the words are the peer's and are drawn as written, and a
/// row that reshuffled them once a frame would be unreadable. `phase` moves the highlight
/// along, every character inside the falloff is mixed from `base` towards `top` by how far
/// it is from the highlight's centre, and everything outside keeps the colour it would have
/// had. `top` is the caller's, because the two things this sweeps are not the same
/// loudness: a peer's own words go to `ink::text`, the status line only to `ink::dim`.
fn shimmer_runs(
    lines: Vec<Vec<Run>>,
    base: Color,
    top: Color,
    motion: Motion,
    phase: usize,
) -> Vec<Vec<Run>> {
    // How far the band has to travel to have crossed the whole row. Read off the
    // longest line rather than wrapped on a fixed period: a period shorter than the line
    // puts two or three bands on it at once, which is a row of stripes and not a
    // highlight passing over it.
    let longest = lines
        .iter()
        .map(|l| l.iter().map(|(t, _)| t.chars().count()).sum::<usize>())
        .max()
        .unwrap_or(0);
    // Where the highlight is centred, which is what each character's distance is
    // measured from. The ramp reaches `SHIMMER` characters either side of it.
    let centre = match motion {
        // A whole ramp of travel either side of the words, so the sweep starts with
        // nothing lit, rises off the first character and sets past the last, and the
        // wrap lands on darkness rather than snapping the band onto the left edge.
        Motion::Pass => {
            let span = longest + 2 * SHIMMER;
            ((phase * SHIMMER_STRIDE) % span.max(1)) as isize - SHIMMER as isize
        }
        // A triangle over the words themselves: right to the last character, then left
        // again, so the band is never off the row and never jumps when it turns.
        Motion::Back => {
            let travel = longest.saturating_sub(1).max(1);
            let p = (phase * SHIMMER_STRIDE) % (2 * travel).max(1);
            (if p <= travel { p } else { 2 * travel - p }) as isize
        }
    };
    lines
        .into_iter()
        .map(|runs| {
            // Each line starts its own count at zero, so a short line is not lit from
            // wherever the line above it happened to finish.
            let mut at = 0usize;
            let mut out: Vec<Run> = Vec::new();
            for (run, style) in runs {
                for ch in run.chars() {
                    let away = (at as isize - centre).unsigned_abs();
                    // Brightest at the centre and fading to `base` at the edge of the ramp,
                    // so the highlight has a middle to it rather than an edge. Off the ramp,
                    // the colour is the row's own, byte for byte.
                    // A bell rather than a ramp: a linear one has a crease down the
                    // middle where it turns around, and a band that lands on its peak
                    // reads as a corner in the text.
                    let colour = if away < SHIMMER {
                        let u = (SHIMMER - away) as f64 / SHIMMER as f64;
                        mix(base, top, 0.5 - 0.5 * (std::f64::consts::PI * u).cos())
                    } else {
                        base
                    };
                    push(&mut out, &ch.to_string(), style.fg(colour));
                    at += 1;
                }
            }
            out
        })
        .collect()
}

/// The colour of a row that is being written: white, blinking off and on.
fn blink(flash: bool) -> Color {
    if flash { ink::text() } else { ink::quiet() }
}


/// The model's own text as the lines a row draws.
///
/// A row renderer, not a markdown implementation: what a model writes and what a
/// terminal row can carry is a set of shapes -- bold, italic, both, strikethrough,
/// code, links, headings, bullets, task lists, quotes, fences, rules and tables.
/// Anything else is drawn as written, because a row that silently dropped
/// something would be lying about the answer. `room` is how wide the row is, which
/// only a rule and a table need.
fn formatted(text: &str, style: Style, room: usize) -> Vec<Vec<Run>> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut fenced = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        i += 1;
        if trimmed.starts_with("```") {
            // The fence is a marker, not a line: what it wraps is what matters, and
            // it is drawn as the code it is.
            fenced = !fenced;
            continue;
        }
        if fenced {
            out.push(code_line(&untabbed(line), style));
            continue;
        }
        // A table is a block rather than a line: it is only a table because of the
        // line under it, and its cells are aligned to each other.
        if trimmed.starts_with('|') {
            let mut block = Vec::new();
            while i <= lines.len() && lines[i - 1].trim_start().starts_with('|') {
                block.push(lines[i - 1].trim_start().to_string());
                i += 1;
            }
            out.extend(table(&block, style, room));
            continue;
        }
        if dashed(trimmed) {
            let lead = " ".repeat(indent);
            out.push(vec![(
                format!("{lead}{}", "\u{2500}".repeat(room.saturating_sub(indent))),
                style.fg(ink::rule()),
            )]);
            continue;
        }
        // Quotes nest: one bar per `>` in front of the line.
        if let Some((level, rest)) = quote(trimmed) {
            let mut runs = vec![(
                format!("{}{}", " ".repeat(indent), "\u{2502} ".repeat(level)),
                style.fg(ink::branch()),
            )];
            runs.extend(inline(rest, style.fg(ink::faint())));
            out.push(runs);
            continue;
        }
        if let Some(title) = heading(trimmed) {
            out.push(vec![
                (" ".repeat(indent), style),
                (
                    title.trim_end().to_string(),
                    style.add_modifier(Modifier::BOLD),
                ),
            ]);
            continue;
        }
        if let Some(rest) = ["- ", "* ", "+ "]
            .iter()
            .find_map(|mark| trimmed.strip_prefix(mark))
        {
            let (mark, mark_style, rest) = match task(rest) {
                Some((true, what)) => ("\u{2611} ".to_string(), style.fg(ink::done()), what),
                Some((false, what)) => ("\u{2610} ".to_string(), style, what),
                None => ("\u{2022} ".to_string(), style, rest),
            };
            let mut runs = vec![(" ".repeat(indent), style), (mark, mark_style)];
            runs.extend(inline(rest, style));
            out.push(runs);
            continue;
        }
        out.push(inline(line, style));
    }
    out
}

/// The words a reader knows without knowing the language, which is as much of a
/// vocabulary as this has: a keyword is what makes a line read as code rather than
/// as a sentence, and the rest of a line's meaning is the reader's own.
const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "mut",
    "def",
    "class",
    "struct",
    "enum",
    "trait",
    "impl",
    "type",
    "where",
    "mod",
    "crate",
    "pub",
    "use",
    "import",
    "from",
    "as",
    "package",
    "func",
    "lambda",
    "async",
    "await",
    "if",
    "else",
    "elif",
    "match",
    "for",
    "while",
    "loop",
    "in",
    "is",
    "not",
    "and",
    "or",
    "break",
    "continue",
    "return",
    "yield",
    "try",
    "catch",
    "except",
    "finally",
    "raise",
    "with",
    "do",
    "switch",
    "const",
    "static",
    "var",
    "new",
    "this",
    "self",
    "super",
    "nil",
    "undefined",
    "true",
    "false",
    "None",
    "null",
    "void",
];

/// One line inside a fence, in the few tones a terminal reads code in: a comment,
/// a string, a keyword, a number, and the code colour for everything else.
///
/// Language-agnostic on purpose, and shallow on purpose: it is a reader's
/// convenience and not a parser, and a line coloured confidently wrong reads worse
/// than a line in one colour. Every tone is one the palette already has -- what is
/// said *about* the code is `faint`, a string is the green of a thing that is done,
/// a keyword is the link's blue, and a number is the row's own white -- so the
/// vocabulary costs `ink` nothing. The blue is a keyword's here and nothing is
/// clickable inside a fence: a fence never goes through `inline`.
///
/// ponytail: one line at a time, with no state carried between them, so a string
/// left open on one line is not known to be open on the next, and the fence's own
/// language is never read. The upgrade path is a lexer over the whole block, behind
/// this same signature.
fn code_line(text: &str, base: Style) -> Vec<Run> {
    let base = base.fg(ink::code());
    let comment = base.fg(ink::faint());
    let string = base.fg(ink::done());
    let keyword = base.fg(ink::link());
    let number = base.fg(ink::text());
    let mut out: Vec<Run> = Vec::new();
    let mut plain = String::new();
    let mut at = 0;
    // What the previous run ended with, which is what says whether a digit begins a
    // number or sits inside a word.
    let mut before: Option<char> = None;
    while at < text.len() {
        let rest = &text[at..];
        let ch = rest.chars().next().expect("a position inside the text");
        // A comment runs to the end of the line and nothing after it is read: it is
        // what is said about the code rather than being it. `#[` is not one -- it is
        // the one shape a `#` leads in the code itself, an attribute or an array's
        // length -- and is left as it was written.
        if rest.starts_with("//") || (ch == '#' && !rest.starts_with("#[")) {
            if !plain.is_empty() {
                out.push((std::mem::take(&mut plain), base));
            }
            out.push((rest.to_string(), comment));
            break;
        }
        // A string is its two quotes and what is between them, or the rest of the
        // line when nothing closes it: an unclosed one is still the shape a string
        // has, and seeing the rest of the line go with it is how a reader notices.
        if let Some(len) = quoted(rest, before) {
            if !plain.is_empty() {
                out.push((std::mem::take(&mut plain), base));
            }
            out.push((rest[..len].to_string(), string));
            before = rest[..len].chars().next_back();
            at += len;
            continue;
        }
        // A number is a digit that begins one rather than sitting inside a word, and
        // it is taken whole -- `0x1f`, `1_000`, `3.5`, `2px` -- so no line is left
        // half a number.
        if ch.is_ascii_digit()
            && !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            let len = run_len(rest, |c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
            if !plain.is_empty() {
                out.push((std::mem::take(&mut plain), base));
            }
            out.push((rest[..len].to_string(), number));
            before = rest[..len].chars().next_back();
            at += len;
            continue;
        }
        // A word, in the keyword's colour when it is one of the few a reader knows
        // without knowing the language.
        if ch.is_alphabetic() || ch == '_' {
            let len = run_len(rest, |c| c.is_alphanumeric() || c == '_');
            let word = &rest[..len];
            if KEYWORDS.contains(&word) {
                if !plain.is_empty() {
                    out.push((std::mem::take(&mut plain), base));
                }
                out.push((word.to_string(), keyword));
            } else {
                plain.push_str(word);
            }
            before = word.chars().next_back();
            at += len;
            continue;
        }
        plain.push(ch);
        before = Some(ch);
        at += ch.len_utf8();
    }
    if !plain.is_empty() {
        out.push((plain, base));
    }
    out
}

/// How many bytes of `rest` one run of the characters `keeps` keeps takes.
fn run_len(rest: &str, keeps: impl Fn(char) -> bool) -> usize {
    rest.find(|c| !keeps(c)).unwrap_or(rest.len())
}

/// How many bytes of `rest` one literal takes: the quote, what is between, and the
/// quote that closes it, or the rest of the line when nothing closes it.
///
/// `None` for a single quote that does not open one: a `'` inside a word is an
/// apostrophe (`don't`) and a `'` that never closes is a lifetime or a lone quote.
/// Colouring the rest of `let x: &'a str` green is exactly the confidently wrong
/// line this is not here to draw.
fn quoted(rest: &str, before: Option<char>) -> Option<usize> {
    let quote = rest.chars().next()?;
    if !matches!(quote, '"' | '\'' | '`') {
        return None;
    }
    if quote == '\'' && before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let mut at = quote.len_utf8();
    let mut escaped = false;
    let mut closed = false;
    for ch in rest[at..].chars() {
        at += ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            closed = true;
            break;
        } else if quote == '\'' && matches!(ch, '"' | '`') {
            // A quote of another kind inside a `'` run: this is not a string, it is
            // a lifetime or an apostrophe meeting the quote that was really opening
            // one. `let x: &'a str = "it's here"` is the line this keeps legible, and
            // the cost is `'say "hi"'`, whose inner quotes are the ones drawn.
            return None;
        }
    }
    if closed || quote != '\'' {
        Some(at)
    } else {
        None
    }
}

/// The text of a heading line, or nothing if it is not one: `#` through `######`,
/// and only with the space after them that markdown asks for.
fn heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6)
        .contains(&hashes)
        .then(|| line[hashes..].strip_prefix(' '))
        .flatten()
}

/// A quote's depth and what is quoted: one `>` per level, spaces allowed between
/// them.
fn quote(line: &str) -> Option<(usize, &str)> {
    let mut level = 0;
    let mut rest = line;
    while let Some(after) = rest.strip_prefix('>') {
        level += 1;
        rest = after.trim_start_matches([' ', '\t']);
    }
    (level > 0).then_some((level, rest))
}

/// A task list item: whether the box is ticked, and what it says.
fn task(rest: &str) -> Option<(bool, &str)> {
    rest.strip_prefix("[ ] ").map(|r| (false, r)).or_else(|| {
        rest.strip_prefix("[x] ")
            .or_else(|| rest.strip_prefix("[X] "))
            .map(|r| (true, r))
    })
}

/// A rule: three or more of the same `-`, `*` or `_`, spaces allowed between them.
fn dashed(line: &str) -> bool {
    let marks: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    marks.len() >= 3 && "-*_".contains(marks[0]) && marks.iter().all(|c| *c == marks[0])
}

/// A markdown table as the lines a row draws: cells padded to the widest in their
/// column, the header bold, and the rule under it drawn like one.
///
/// ponytail: the columns are shared out of the row's width rather than measured
/// against the text, and a cell longer than its share is cut. A table that must
/// keep every character belongs in the overlay, where the width is not a row's.
fn table(block: &[String], style: Style, room: usize) -> Vec<Vec<Run>> {
    let mut rows: Vec<Vec<Vec<Run>>> = Vec::new();
    let mut rule = None;
    for (n, line) in block.iter().enumerate() {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        // The row of dashes under the first one is markdown for "this is a table",
        // and is drawn as the rule it stands for.
        if n == 1 && cells.iter().all(|c| !c.is_empty() && dashed(c)) {
            rule = Some(cells.len());
            continue;
        }
        let header = rule.is_none() && n == 0 && block.len() > 1;
        let style = if header {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        };
        rows.push(cells.iter().map(|cell| inline(cell, style)).collect());
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return rows
            .into_iter()
            .map(|row| row.into_iter().flatten().collect())
            .collect();
    }
    // Three columns of every four belong to the cells; the rest is the separators.
    let cap = ((room.saturating_sub(3 * columns)) / columns).max(4);
    let mut widths = vec![0usize; columns];
    for row in &rows {
        for (n, cell) in row.iter().enumerate() {
            widths[n] = widths[n].max(runs_width(cell).min(cap));
        }
    }
    let separator = Style::new().fg(ink::branch());
    let mut out = Vec::new();
    for (n, row) in rows.iter().enumerate() {
        let mut runs: Vec<Run> = Vec::new();
        for (col, width) in widths.iter().enumerate() {
            if col > 0 {
                push(&mut runs, " \u{2502} ", separator);
            }
            let cell = row
                .get(col)
                .map(|c| clip_runs(c, *width))
                .unwrap_or_default();
            let used = runs_width(&cell);
            runs.extend(cell);
            // The last column is not padded: a row ending in spaces is a row with a
            // ragged right edge nobody can see and every writer has to allow for.
            if col + 1 < columns {
                push(&mut runs, &" ".repeat(width.saturating_sub(used)), style);
            }
        }
        out.push(runs);
        if n == 0 && rule.is_some() {
            let across: usize = widths.iter().sum::<usize>() + 3 * (columns - 1);
            out.push(vec![("\u{2500}".repeat(across), separator)]);
        }
    }
    out
}

/// How wide runs draw: their text in columns, which is what the screen counts.
pub(crate) fn runs_width(runs: &[Run]) -> usize {
    runs.iter().map(|(text, _)| columns(text)).sum()
}

/// Runs cut to `most` columns, with a mark that something was cut.
pub(crate) fn clip_runs(runs: &[Run], most: usize) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut used = 0;
    for (text, style) in runs {
        if used >= most {
            break;
        }
        let room = most - used;
        let wide = columns(text);
        if wide <= room {
            push(&mut out, text, *style);
            used += wide;
            continue;
        }
        // One column of the room goes to the mark, so the ellipsis is inside the
        // width rather than one column past it. `split_at` answers in bytes, which
        // is what a prefix of the string needs.
        let cut = &text[..split_at(text, room.saturating_sub(1))];
        push(&mut out, &format!("{cut}\u{2026}"), *style);
        used = most;
    }
    out
}

/// A line with its inline markers gone: `***both***`, `**bold**`, `*italic*`,
/// `_italic_`, `~~struck~~`, `` `code` `` and `[a link](where)`.
///
/// An unclosed marker is left as written rather than swallowed.
fn inline(text: &str, base: Style) -> Vec<Run> {
    let marks: [(&str, Style); 6] = [
        ("***", base.add_modifier(Modifier::BOLD | Modifier::ITALIC)),
        ("**", base.add_modifier(Modifier::BOLD)),
        ("~~", base.add_modifier(Modifier::CROSSED_OUT)),
        ("`", base.fg(ink::code())),
        ("*", base.add_modifier(Modifier::ITALIC)),
        ("_", base.add_modifier(Modifier::ITALIC)),
    ];
    let mut out: Vec<Run> = Vec::new();
    let mut plain = String::new();
    let mut at = 0;
    while at < text.len() {
        // A link is two markers, not one: the label and where it goes. The address
        // is kept beside the label unless the label already is the address, because
        // a link you cannot read the target of is a dead end in a terminal.
        if let Some((label, url, used)) = link(&text[at..]) {
            if !plain.is_empty() {
                out.push((std::mem::take(&mut plain), base));
            }
            out.push((
                label.to_string(),
                base.fg(ink::link()).add_modifier(Modifier::UNDERLINED),
            ));
            if url != label {
                out.push((format!(" ({url})"), base.fg(ink::quiet())));
            }
            at += used;
            continue;
        }
        let found = marks.iter().find_map(|(mark, style)| {
            let after = text.get(at..)?.strip_prefix(mark)?;
            let end = after.find(mark)?;
            (end > 0).then(|| (mark.len(), *style, &after[..end]))
        });
        match found {
            Some((mark, style, inner)) => {
                if !plain.is_empty() {
                    out.push((std::mem::take(&mut plain), base));
                }
                out.push((inner.to_string(), style));
                at += mark + inner.len() + mark;
            }
            None => {
                let ch = text[at..]
                    .chars()
                    .next()
                    .expect("a position inside the text");
                plain.push(ch);
                at += ch.len_utf8();
            }
        }
    }
    if !plain.is_empty() {
        out.push((plain, base));
    }
    out
}

/// A `[label](where)` at the front of `rest`: the two, and how many bytes they took.
/// An image is the same shape with a `!` in front, and is drawn as its alt text and
/// its address: a terminal row has nowhere to put a picture.
fn link(rest: &str) -> Option<(&str, &str, usize)> {
    let bang = usize::from(rest.starts_with('!'));
    let inside = rest[bang..].strip_prefix('[')?;
    let close = inside.find("](")?;
    let after = &inside[close + 2..];
    let end = after.find(')')?;
    let (label, url) = (&inside[..close], &after[..end]);
    (!label.is_empty() && !url.is_empty()).then_some((label, url, bang + 1 + close + 2 + end + 1))
}

/// A call's head line, in the shape a call has: `write(src/main.rs)`,
/// `shell(date)`.
///
/// The row's own summary is one `key=value` and it is clipped to forty characters,
/// which leaves the reader guessing at what is actually being run. The call itself
/// is on the row, so the head names what the call did -- the action, when the tool
/// has one, which is why `file(action=write)` reads as `write(...)` -- and puts
/// the whole of the argument that says what it was into the brackets. Nothing is
/// cut: the head wraps like any other row rather than ending in an ellipsis, and
/// the argument it names is the whole of what the call is.
pub fn call_head(name: &str, args: &str, call: &Value) -> String {
    match call_name(name, args, call) {
        (verb, Some(hint)) => format!("{verb}({hint})"),
        (verb, None) => verb,
    }
}

/// A call's name in its two parts: the verb -- what it did -- and the argument
/// that says what it did it to, which is nothing when the verb is all there is.
///
/// Two parts rather than one string because a call's head is drawn in two
/// colours: the verb says a row is a call, and the argument is the thing being
/// called on. `call_head` is the same two parts written as one line, for the
/// places that only have room for one.
pub fn call_name(name: &str, args: &str, call: &Value) -> (String, Option<String>) {
    let verb = call
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or(name)
        .to_string();
    // The action is already the verb in the head, so its value is not said again
    // in the brackets: `file(action=write)` is `write(path)`, and a call whose
    // action is all it has -- `todo(action=plan)` -- is just `plan`.
    let key = summary_key(args);
    let hint = if key.is_empty() {
        None
    } else {
        // Nothing picked, or picked from something that was not an object: the
        // summary is all there is, and it is still better than the bare name.
        call.get(key)
            .and_then(Value::as_str)
            .or_else(|| (!args.is_empty()).then_some(args))
    };
    let hint = hint
        .map(|h| h.lines().next().unwrap_or_default().to_string())
        .map(|h| if is_path(&h) { short_path(&h) } else { h })
        .filter(|h| !h.is_empty());
    (verb, hint)
}

/// The key a call's own one-line summary picked, which is the shared rule for
/// what says what a call was about. It is the head that carries that key's value,
/// so the detail under the head leaves it out rather than saying it twice.
fn summary_key(args: &str) -> &str {
    match args.split('=').next().unwrap_or_default() {
        // The action is the verb in the head, so it names no argument there.
        "action" => "",
        key => key,
    }
}

/// Whether a string is a path rather than prose: one that starts at the root or
/// at home -- or, on Windows, at a drive -- and has no space in it. A command
/// that happens to contain a slash is not one, and is left as it was written.
fn is_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    // A drive is a letter and a colon. Any second colon - `12:30` - is prose.
    let drive = cfg!(windows) && bytes.len() > 1 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let rooted = matches!(text.chars().next(), Some('/') | Some('~')) || drive;
    rooted && !text.contains(char::is_whitespace)
}

/// A path as a row has room for it: relative to the directory this conversation
/// is in, `~` for home, and the middle of a long one dropped between its first
/// part and its last two.
///
/// The least informative part of a path is all of it but the end: someone reading
/// `write(~/.cuacode/sessions/20260917-172453-ea54/notebook/SESSION_BREAKDOWN.md)`
/// is looking for the file, and the directories on the way there are a wall of
/// grey that in a list is cut off at the edge anyway -- which is how a row came to
/// say `write(~/sessions/2026` and never name anything at all.
///
/// Nothing is lost: what a call was run with is in full in its panel, one
/// keypress away, and that is the copy with the room for it.
pub fn short_path(path: &str) -> String {
    let home = cuacode::home_dir().map(|p| p.display().to_string());
    let short = if let Some(rest) = inside(path, &session::working_dir()) {
        rest
    } else if let Some(rest) = home.as_deref().and_then(|home| inside(path, home)) {
        format!("~/{rest}")
    } else {
        path.to_string()
    };
    let parts: Vec<&str> = short.split('/').collect();
    // A path that fits is left whole, and one with nowhere to cut is not cut: the
    // answer to a long path is not a shorter lie about where the file is.
    if short.chars().count() <= PATH_MAX || parts.len() <= 4 {
        return short;
    }
    // Where it is, in the shape of a path still: the root or the home above it,
    // then the gap, then the last two -- the file, and the folder holding it.
    let head = if short.starts_with('/') { "" } else { parts[0] };
    format!(
        "{head}/\u{2026}/{}/{}",
        parts[parts.len() - 2],
        parts[parts.len() - 1]
    )
}

/// What is left of a path below a directory, or nothing when it is not below it.
/// Empty rather than a prefix of nothing: a path that *is* the directory has no
/// shorter name, and one that merely starts with the same letters is somewhere
/// else entirely.
fn inside(path: &str, root: &str) -> Option<String> {
    if root.is_empty() {
        return None;
    }
    let rest = path.strip_prefix(root)?;
    if !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }
    let rest = rest.trim_start_matches('/');
    (!rest.is_empty()).then(|| rest.to_string())
}

/// A call's argument as the two things it is when it names a path: the way there
/// and the file at the end of it. The file is what a reader is looking for, so it
/// is drawn in the row's own colour and the directories behind it are drawn in
/// `faint`, which is one rung quieter rather than a second colour.
///
/// One run for everything that is not one word with a slash in it: a command, a
/// flag, a number. A command is text with a meaning of its own, and splitting it
/// at its last slash would say `ls -la /var/` and `log` were two things.
pub fn arg_runs(hint: &str, colour: Color) -> Vec<Run> {
    let one_word = !hint.contains(char::is_whitespace) && !hint.ends_with('/');
    match one_word.then(|| hint.rfind('/')).flatten() {
        Some(at) if at + 1 < hint.len() => vec![
            (hint[..=at].to_string(), Style::new().fg(ink::faint())),
            (hint[at + 1..].to_string(), Style::new().fg(colour)),
        ],
        _ => vec![(hint.to_string(), Style::new().fg(colour))],
    }
}

/// The summary without the pair the head above it already carries.
///
/// A row's head names what a call was on -- `write(~/.cuacode/x.md)` -- and the
/// summary the worker sent repeats it, because the summary does not know what the
/// head had room for. Reading `created \u{b7} path ~/.cuacode/x.md \u{b7} written
/// 7977` under a head that says the path is reading it twice. The list behind
/// ctrl-o puts the same summary beside the same head, so it is left out there too.
pub fn call_result(args: &str, summary: &str) -> String {
    let key = summary_key(args);
    if key.is_empty() {
        return summary.to_string();
    }
    let said = format!("{key} ");
    summary
        .split(" \u{b7} ")
        // The directory a call ran in is the header's business, and the panel has
        // the whole result object: on a row it is the longest part of the line and
        // the least worth a column of it, and a path long enough to wrap turns one
        // row into two that read like separate results.
        .filter(|part| !part.starts_with(&said) && !part.trim_start().starts_with("cwd "))
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// A call's head as the conversation and the list behind ctrl-o both draw it: the
/// lead it is marked with, the style that lead is in, and the runs of the head
/// itself -- the verb, and what the call was run on.
///
/// One function for both, because the list behind ctrl-o is the conversation's own
/// rows read back. A call that read one way in a row and another way in a list of
/// rows would be two ideas of what a call is, which is the bug this file's header
/// warns about.
///
/// The verb wears ember's accent: the one hue that means "you can act on this",
/// and a call is the thing you act on. It is not replaced by the state, because
/// the mark at the front is already the state, and a row whose one colour is its
/// state is a row you read twice. On a failed call the accent and the red cross
/// beside it are 12.6 dE apart and are two shapes as well, which keeps the pair
/// two things even though the whole row is red.
pub fn call_lead(
    name: &str,
    args: &str,
    call: &Value,
    result: Option<&str>,
    live: bool,
    flash: bool,
) -> (String, Style, Vec<Run>) {
    // The lead is in the colour of what happened -- the tick's green, the cross's
    // red, the dim dot of a call that has said nothing -- except while this is the
    // row being written, which blinks instead.
    let mark_ink = if live && result.is_none() {
        blink(flash)
    } else {
        call_ink(result)
    };
    let colour = arg_ink(result);
    // The verb is the one coloured thing on the row: in a column of calls it is
    // what says which rows are calls at all.
    let (verb, hint) = call_name(name, args, call);
    let mut said = vec![(verb, Style::new().fg(ink::call()))];
    if let Some(hint) = hint {
        said.push(("(".to_string(), Style::new().fg(colour)));
        said.extend(arg_runs(&hint, colour));
        said.push((")".to_string(), Style::new().fg(colour)));
    }
    (
        format!("{} ", call_state(result).glyph()),
        Style::new().fg(mark_ink),
        said,
    )
}

/// The colour of that mark: green when the call worked, red when it failed, and
/// the dim nothing of a call that has not reported. The transcript's own rule,
/// so the list cannot drift from the rows it lists.
pub fn call_ink(result: Option<&str>) -> Color {
    match call_state(result) {
        Mark::Tick => ink::done(),
        Mark::Cross => ink::failed(),
        // A call never reports as a peer: the buddy mark belongs to the peer row, and
        // this arm is here so the two marks cannot be confused for each other.
        Mark::Dot | Mark::Buddy => ink::quiet(),
    }
}

/// What became of a call, from the one line the worker said about it. One place
/// decides, so the mark a row is led by and the colour it is led in agree.
fn call_state(result: Option<&str>) -> Mark {
    match result {
        Some(r) if failed(r) => Mark::Cross,
        Some(_) => Mark::Tick,
        None => Mark::Dot,
    }
}

/// The colour of what a call was called on: the failure's red, or the row's own
/// white. A call still running is white too: it has not failed yet.
fn arg_ink(result: Option<&str>) -> Color {
    if result.is_some_and(failed) {
        ink::failed()
    } else {
        ink::text()
    }
}

/// One side of a call -- what it was run with, or what came back -- as the lines
/// a detail shows them in.
///
/// The action is the verb in the title and is never drawn here, and `skip` is what
/// the other side has already said. What is left is the input the row itself never
/// had room for: a file's content, the edits to apply, a command's output.
///
/// The one-line facts come first and the boxes after them: a path under a wall of
/// its own code reads as a caption at the bottom of the page, and the document is
/// what the panel was opened for.
fn fields(value: &Value, skip: &[&str], room: usize) -> Vec<Vec<Run>> {
    let Value::Object(map) = value else {
        return value_lines(value, 0, room);
    };
    let (mut said, mut shown) = (Vec::new(), Vec::new());
    for (key, value) in map {
        if key == "action" || skip.contains(&key.as_str()) || value.as_str() == Some("") {
            continue;
        }
        if document(key, value) {
            shown.extend(pair(key, value, 0, room));
        } else {
            said.extend(pair(key, value, 0, room));
        }
    }
    said.extend(shown);
    said
}

/// Whether a key's value is a document, which is the one thing a box is for. The
/// list is the row's own (`transcript::is_bulky`), so a body cannot be a document
/// in its row and a measurement in its panel.
fn document(key: &str, value: &Value) -> bool {
    is_bulky(key) && matches!(value, Value::String(_))
}

/// One key and its value as the lines they take: on the key's line when it is a
/// fact, in a box of its own when it is a document, under it when it is a list or
/// an object.
fn pair(key: &str, value: &Value, indent: usize, room: usize) -> Vec<Vec<Run>> {
    let pad = " ".repeat(indent);
    let name = Style::new().fg(ink::branch());
    let code = Style::new().fg(ink::code());
    match value {
        // A flag that is off is the default and a command that exited zero has
        // nothing to report, so neither earns a line. The row's own `phrase` drops
        // the same two, which is what keeps `truncated  false` off every file that
        // was not cut short.
        Value::Bool(false) => return Vec::new(),
        Value::Number(n) if matches!(key, "code" | "exit_code") && n.as_i64() == Some(0) => {
            return Vec::new();
        }
        _ => {}
    }
    match value {
        // What a call changed comes back as a unified diff, and a diff is read by
        // its colours: a line that went is red, a line that came is green. The
        // one block whose lines are not all drawn in the same colour.
        Value::String(s) if key == "diff" => diff_block(s, room, indent),
        // A document is a box and never a bare line: a file's content, an edit's
        // old and new. `key  value` beside a boxed copy of the same kind of thing
        // is two shapes for one idea, and a one-line document is still a document.
        Value::String(s) if s.contains('\n') || is_bulky(key) => {
            let lines = document_lines(s);
            if lines.is_empty() {
                return Vec::new();
            }
            let title = format!("{key} \u{b7} {}", lines_n(lines.len()));
            code_block(&title, &lines, room, indent)
        }
        _ => match one_line(value) {
            Some(text) => vec![vec![(format!("{pad}{key}  "), name), (text, code)]],
            None => {
                let mut out = vec![vec![(format!("{pad}{key}"), name)]];
                out.extend(value_lines(value, indent + 2, room));
                out
            }
        },
    }
}

/// A document's own lines, with any trailing blank ones dropped: a file that ends
/// in a newline must not end in a line of box.
fn document_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// `3 lines`, or `1 line`, which a box's edge says about what it holds.
fn lines_n(n: usize) -> String {
    if n == 1 {
        "1 line".to_string()
    } else {
        format!("{n} lines")
    }
}

/// One line of a box: the number in its gutter, if it has one, and the runs it
/// draws in.
type Body = (Option<usize>, Vec<Run>);

/// A box with a title on its top edge and a body inside it, a line to a row.
///
/// `room` is how many columns it may take, `indent` how far it hangs in. The box
/// is as wide as its longest line, or as wide as the title needs, and never wider
/// than the room there is. A body that carries numbers gets a gutter down its
/// left, ruled off from the text, which is what makes a file read as a file and
/// not as a paragraph that happens to have digits in it.
///
/// Nothing is clipped. A line that runs past the box wraps inside it and keeps its
/// edge, because the point of showing a call's input is to read it, and an ellipsis
/// would be the same loss the head's was. Too little room for a box at all, the
/// body is drawn as plain lines rather than as a broken frame.
fn framed(title: &str, body: &[Body], room: usize, indent: usize) -> Vec<Vec<Run>> {
    let pad = " ".repeat(indent);
    let room = room.saturating_sub(indent);
    let frame = Style::new().fg(ink::rule());
    let label = Style::new().fg(ink::branch());
    let line_no = Style::new().fg(ink::faint());
    if room < MIN_BOX {
        return body
            .iter()
            .flat_map(|(_, runs)| wrap_runs(runs, room.max(1)))
            .map(|runs| {
                let mut out = vec![(pad.clone(), Style::new())];
                out.extend(runs);
                out
            })
            .collect();
    }
    // The gutter is as wide as the longest number in it, and three columns wider
    // for the rule and the spaces either side of it.
    let digits = body
        .iter()
        .filter_map(|(n, _)| *n)
        .max()
        .map_or(0, |n| n.to_string().len());
    let gutter = if digits == 0 { 0 } else { digits + 3 };
    let widest = body
        .iter()
        .map(|(_, runs)| runs_width(runs))
        .max()
        .unwrap_or(0);
    let width = (widest + gutter + 4).max(columns(title) + 5).min(room);
    let inside = width.saturating_sub(4 + gutter);
    // The title sits on the top edge, so the edge is not one long dash to nowhere.
    let caption = clip(title, width.saturating_sub(5));
    let mut out = vec![vec![
        (format!("{pad}\u{250c} "), frame),
        (caption.clone(), label),
        (
            format!(
                " {}\u{2510}",
                "\u{2500}".repeat(width.saturating_sub(columns(&caption) + 4))
            ),
            frame,
        ),
    ]];
    for (number, runs) in body {
        // Wrapped here rather than left to the row, so every line of the block
        // keeps its edge and the box stays a box. Only the first part carries the
        // number: a continuation is the same line of the file, and numbering it
        // again would say it was another one.
        let blank = " ".repeat(gutter);
        for (k, part) in wrap_runs(runs, inside.max(1)).into_iter().enumerate() {
            let head = match (number, k) {
                (Some(n), 0) => format!("{n:>digits$} \u{2502} "),
                _ => blank.clone(),
            };
            let gap = inside.saturating_sub(runs_width(&part));
            let mut line = vec![(format!("{pad}\u{2502} "), frame), (head, line_no)];
            line.extend(part);
            line.push((format!(" {}\u{2502}", " ".repeat(gap)), frame));
            out.push(line);
        }
    }
    out.push(vec![(
        format!("{pad}\u{2514}{}\u{2518}", "\u{2500}".repeat(width - 2)),
        frame,
    )]);
    out
}

/// A document as the lines of a box, each in the colours a line of code reads in,
/// numbered from one unless the text carries its own numbers.
///
/// The file tool answers a read with `7\tcode` on every line, and those digits are
/// not the file: they are its offset, and drawing them as part of the line puts
/// the number in the file's own colour and loses the column they were meant to
/// hold. They are taken out here and handed to the gutter.
fn body_of(lines: &[String]) -> Vec<Body> {
    // The whole block has to agree before the numbers are believed: a file of
    // tab-separated columns starting with one is not the file tool's answer, and
    // eating its first column would be worse than drawing a number it had.
    let numbered: Vec<Option<(&str, &str)>> = lines
        .iter()
        .map(|line| {
            line.split_once('\t').filter(|(n, _)| {
                !n.is_empty() && n.len() <= 6 && n.chars().all(|c| c.is_ascii_digit())
            })
        })
        .collect();
    let counts = numbered.iter().all(Option::is_some)
        && numbered.windows(2).all(|w| match (w[0], w[1]) {
            (Some((a, _)), Some((b, _))) => a
                .parse::<usize>()
                .ok()
                .zip(b.parse::<usize>().ok())
                .is_some_and(|(a, b)| b == a + 1),
            _ => false,
        });
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| match numbered[i].filter(|_| counts) {
            Some((n, text)) => (
                n.parse::<usize>().ok(),
                code_line(&untabbed(text), Style::new()),
            ),
            None => (Some(i + 1), code_line(&untabbed(line), Style::new())),
        })
        .collect()
}

/// A block of code: the box above, in the colours a line of code reads in.
fn code_block(title: &str, lines: &[String], room: usize, indent: usize) -> Vec<Vec<Run>> {
    framed(title, &body_of(lines), room, indent)
}

/// What a call changed, as a diff is read: the line that went in the colour of a
/// failure, the line that came in the colour of a thing that is done, which is
/// what those two lines are.
///
/// The file names and the hunk markers are dim, because they say what the change
/// is against rather than being the change, and the lines kept only for context
/// are fainter still. The two colours are the point.
fn diff_block(text: &str, room: usize, indent: usize) -> Vec<Vec<Run>> {
    let lines = document_lines(text);
    if lines.is_empty() {
        return Vec::new();
    }
    // A diff is not numbered: its line numbers are the hunk header's, and a gutter
    // counting the block would be a second set saying something else.
    let body: Vec<Body> = lines
        .iter()
        .map(|line| {
            (
                None,
                vec![(untabbed(line), Style::new().fg(diff_ink(line)))],
            )
        })
        .collect();
    framed(
        &format!("diff \u{b7} {}", lines_n(lines.len())),
        &body,
        room,
        indent,
    )
}

/// The colour of one line of a unified diff, from the marker it leads with.
fn diff_ink(line: &str) -> Color {
    if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
        ink::dim()
    } else if line.starts_with('-') {
        ink::failed()
    } else if line.starts_with('+') {
        ink::done()
    } else {
        ink::faint()
    }
}

/// A value that belongs on its key's own line: a short string, a number, a bool,
/// null. `None` for the three that are a block of their own.
fn one_line(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.contains('\n') => Some(s.clone()),
        Value::String(_) | Value::Array(_) | Value::Object(_) => None,
        other => Some(other.to_string()),
    }
}

/// One value as the lines a detail draws it in: a string is the lines it has, in
/// the code colour; an object is its keys and an array its items; a scalar is
/// itself.
///
/// Never JSON. A file's content escaped onto one line, with `\n` where the breaks
/// were, is the document this whole path exists to read, and pretty-printing only
/// makes it longer.
fn value_lines(value: &Value, indent: usize, room: usize) -> Vec<Vec<Run>> {
    let pad = " ".repeat(indent);
    let mark = Style::new().fg(ink::branch());
    let code = Style::new().fg(ink::code());
    match value {
        Value::String(s) => s
            .split('\n')
            .map(|line| vec![(format!("{pad}{line}"), code)])
            .collect(),
        Value::Array(items) if items.is_empty() => vec![vec![(format!("{pad}[]"), mark)]],
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                match one_line(item) {
                    Some(text) => out.push(vec![(format!("{pad}- {text}"), code)]),
                    None => {
                        out.push(vec![(format!("{pad}-"), mark)]);
                        out.extend(value_lines(item, indent + 2, room));
                    }
                }
            }
            out
        }
        Value::Object(map) => map
            .iter()
            .flat_map(|(k, v)| pair(k, v, indent, room))
            .collect(),
        other => vec![vec![(format!("{pad}{other}"), code)]],
    }
}

/// One line of text, cut to `most` characters with a mark that it was cut.
pub(crate) fn clip(text: &str, most: usize) -> String {
    let text = text.trim();
    if columns(text) <= most {
        return text.to_string();
    }
    let head = &text[..split_at(text, most.saturating_sub(1))];
    format!("{head}\u{2026}")
}

/// The reasoning row's own readout, as the design draws it: the round's thinking
/// tokens and rate, quoted from the worker and nowhere else. A round that has not
/// reported either says just "Reasoning".
fn reasoning(round: Round) -> String {
    if round.tokens <= 0 && round.tps <= 0.0 {
        return "Reasoning".to_string();
    }
    let mut out = String::from("Reasoning | ");
    if round.tokens > 0 {
        out.push_str(&format!("{} tokens", tokens(round.tokens, round.estimated)));
        if round.tps > 0.0 {
            out.push_str(", ");
        }
    }
    if round.tps > 0.0 {
        out.push_str(&format!("{:.0} tok/s", round.tps));
    }
    out.trim_end().to_string()
}

/// A call in full, as the worker keeps it: the arguments and the result, past the
/// summary a row carries.
///
/// The title carries the index the worker found it by and how many calls there are,
/// because that index is what a reader following a run would ask for next. `room`
/// is how wide the box it lands in is, which a code block is sized to; the lines
/// are wrapped by `wrap_lines` to the same width, so scrolling counts what is on
/// the screen.
///
/// The path the title carries is not drawn again under it: the panel is what the
/// title had no room for.
pub fn call_detail(data: &Value, room: usize) -> Opened {
    let name = data.get("name").and_then(Value::as_str).unwrap_or("?");
    let index = data.get("index").and_then(Value::as_i64).unwrap_or(-1);
    let total = data.get("total").and_then(Value::as_i64).unwrap_or(0);
    let args = data.get("args").cloned().unwrap_or(Value::Null);
    // The row's own head, so a panel and the row it was opened from say the same
    // thing about what the call was run on.
    let title = format!(
        "{} #{index} of {total}",
        call_head(name, &describe_args(&args), &args)
    );
    // A call the records no longer hold -- the run was rewound away -- says so
    // rather than showing another call's arguments.
    if let Some(why) = data.get("unavailable").and_then(Value::as_str) {
        return Opened {
            title,
            lines: vec![Line::from(why.to_string())],
            scroll: 0,
        };
    }
    let result = data.get("result").map(unwrap_result);
    // A document appears once, under the label that owns it. The path is the
    // title's, and a diff is the coloured copy of the very edits the arguments
    // spell out in prose, so the diff owns them where there is one.
    let mut skip = vec!["path"];
    if result.is_some_and(|r| r.get("diff").is_some()) {
        skip.extend(["content", "edits"]);
    }
    let mut lines = Vec::new();
    section(&mut lines, "args", fields(&args, &skip, room));
    if let Some(result) = result {
        // The wrapper every tool answers one level down is not a fact, the path is
        // the title's, and a document's line count is the box's own title. What is
        // left is what the call did.
        let result_skip = if document("content", result.get("content").unwrap_or(&Value::Null)) {
            vec!["path", "lines"]
        } else {
            vec!["path"]
        };
        section(&mut lines, "result", fields(result, &result_skip, room));
    }
    Opened {
        title,
        lines,
        scroll: 0,
    }
}

/// The same call, built from the row rather than from the worker.
///
/// The wire carries a call's arguments whole, so opening one can say what it was
/// run with at once, before the worker has been asked for its own record. It is
/// also the whole panel for a replay, where there is no worker to ask at all.
/// The result is the row's summary here and the worker's document when it lands.
pub fn call_from_row(
    name: &str,
    args: &str,
    call: &Value,
    result: Option<&str>,
    room: usize,
) -> Opened {
    let mut lines = Vec::new();
    // The path is the head's. A row's summary is one line of prose, so there is no
    // document here to own anything else.
    section(&mut lines, "args", fields(call, &["path"], room));
    if let Some(r) = result.filter(|r| !r.is_empty()) {
        section(
            &mut lines,
            "result",
            fields(&Value::String(r.to_string()), &[], room),
        );
    }
    Opened {
        title: call_head(name, args, call),
        lines,
        scroll: 0,
    }
}

/// Every tool answers one level down, as `{"result": {...}}`, so the shape worth
/// drawing is inside that key and not the wrapper: a panel that drew the wrapper
/// would say `result` twice and one of them would not be a fact. A string behind
/// that key is unwrapped too, or the same thing happens a line lower.
fn unwrap_result(result: &Value) -> &Value {
    match result.get("result") {
        Some(inner) if inner.is_object() || inner.is_string() => inner,
        _ => result,
    }
}

/// A labelled block of a panel: what it is, the lines it holds, and a blank line
/// under it. A block with nothing in it is left out rather than drawn empty.
fn section(lines: &mut Vec<Line<'static>>, label: &str, body: Vec<Vec<Run>>) {
    if body.is_empty() {
        return;
    }
    lines.push(Line::from(Span::styled(
        format!("{label}:"),
        Style::new().fg(ink::branch()).bold(),
    )));
    for line in body {
        lines.push(Line::from(
            line.into_iter()
                .map(|(text, style)| Span::styled(text, style))
                .collect::<Vec<_>>(),
        ));
    }
    lines.push(Line::from(""));
}

/// A panel's lines wrapped to a width, so its scroll offset counts what is on the
/// screen. Wrapped here rather than in the widget for the same reason the
/// transcript's own lines are: an offset that counted unwrapped lines would scroll
/// to the wrong place the moment a line ran past the box.
pub fn wrap_lines(lines: &[Line<'static>], width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for line in lines {
        let runs: Vec<Run> = line
            .spans
            .iter()
            .map(|s| (s.content.to_string(), s.style))
            .collect();
        for wrapped in wrap_runs(&runs, width) {
            out.push(Line::from(
                wrapped
                    .into_iter()
                    .map(|(text, style)| Span::styled(text, style))
                    .collect::<Vec<_>>(),
            ));
        }
    }
    out
}

/// The question the worker is blocked on, in full.
///
/// A field line can only carry a yes or a no. Everything needed to answer one is
/// here: why the gate flagged the call, the call itself as the model wrote it, and
/// what the call says it would do. Refusing on a clipped line is refusing blind.
pub fn ask_detail(name: &str, data: &Value) -> Opened {
    let mut lines = Vec::new();
    let label = |what: &str| {
        Line::from(Span::styled(
            format!("{what}:"),
            Style::new().fg(ink::code()).bold(),
        ))
    };
    if let Some(note) = data
        .get("note")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
    {
        lines.push(label("why"));
        lines.extend(note.lines().map(|line| Line::from(line.to_string())));
        lines.push(Line::from(""));
    }
    lines.push(label("the call"));
    let call = data.get("args").map(pretty).unwrap_or_default();
    lines.extend(call.lines().map(|line| Line::from(line.to_string())));
    lines.push(Line::from(""));
    if let Some(preview) = data.get("preview") {
        if let Some(summary) = preview.get("summary").and_then(Value::as_str) {
            lines.push(label("it would"));
            lines.extend(summary.lines().map(|line| Line::from(line.to_string())));
            lines.push(Line::from(""));
        }
        if let Some(diff) = preview.get("diff").and_then(Value::as_str) {
            lines.push(label("change"));
            // The colours the panel gives a diff, in the one place a person is
            // asked to read it before saying yes.
            lines.extend(diff.lines().map(|line| {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::new().fg(diff_ink(line)),
                ))
            }));
        }
    }
    Opened {
        title: format!("allow {name}?"),
        lines,
        scroll: 0,
    }
}

/// The keys, on request. `/help` is the one door to them.
///
/// Deliberately not on screen at startup: a row of shortcuts in the corner of
/// every frame is noise, and the one thing it helps with is learned once. It is a
/// panel rather than anything special, so what closes any other panel closes this
/// one, and the list lives here with the rest of what a row looks like.
///
/// Keys and nothing else. The commands are the `/` menu's own list, and naming
/// one here would put a single command on a screen with no room for the rest of
/// them: a key list that answers one command and not twenty is worse than one
/// that answers none. The three menu triggers are here because they are keys.
pub fn keys() -> Opened {
    let rows: &[(&str, &str)] = &[
        ("enter", "send"),
        ("shift-enter, ctrl-j", "a new line in the message"),
        ("ctrl-v", "attach the picture on the clipboard"),
        ("drop or paste a path", "attach the picture at it"),
        ("click a chip", "take that picture off the message"),
        (
            "\u{2190} \u{2192}",
            "move the caret; ctrl or alt for a word",
        ),
        (
            "cmd-\u{2190} \u{2192}, ctrl-a / ctrl-e",
            "the ends of the line, or of the conversation",
        ),
        ("backspace / delete", "a character, or a whole picture"),
        ("ctrl or alt + backspace / delete", "a word at a time"),
        (
            "cmd-backspace, ctrl-u",
            "everything before the caret on the line",
        ),
        ("cmd-delete, cmd-k / ctrl-k", "to the end of the line"),
        (
            "\u{2191} \u{2193}",
            "a line of the message, or of the conversation",
        ),
        ("pgup / pgdn", "ten lines of the conversation"),
        (
            "escape",
            "stop the round in flight, or empty what you typed",
        ),
        ("/ $ @", "the menus: commands, skills, files"),
        ("ctrl-t", "every step's calls; its thinking one line each"),
        ("ctrl-o", "the calls, to open one in full"),
        ("ctrl-b", "push the running call into the background"),
        (
            "double click",
            "one row: a call in full, a thinking opened or shut",
        ),
        ("ctrl-c", "quit"),
    ];
    Opened {
        title: "keys".to_string(),
        lines: rows
            .iter()
            .map(|(key, what)| {
                Line::from(vec![
                    Span::styled(format!("{key:<22}"), Style::new().fg(ink::link())),
                    Span::styled(*what, Style::new().fg(ink::text())),
                ])
            })
            .collect(),
        scroll: 0,
    }
}

/// A value as the document it is, one key per line.
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Whether a call's summary reads as a failure. The worker says so two ways: an
/// `error:` prefix, or a non-zero exit. A successful run is still a pass when the
/// summary carries on past the code -- the wire says `exit 0 \u{b7} stdout:7B` --
/// so it is the first word of the rest that decides, not the whole line.
fn failed(result: &str) -> bool {
    result.starts_with("error") || (result.starts_with("exit ") && !result.starts_with("exit 0"))
}

/// The turn's mark and readout: what the round has cost, and how long it has been
/// going.
///
/// Drawn at the right end of the field line rather than on a line of its own: a
/// round in flight is the one thing on the screen that changes by itself, and a
/// lone dim line above the rules reads as one more row of the transcript.
///
/// The wire carries no clock, so the elapsed figure is this frontend's own
/// stopwatch and says tenths rather than pretending to precision. The token count
/// is the worker's last report, with its `~` while it is an estimate. A round that
/// has not reported tokens says only the time: an absence is not a zero.
///
/// `ms` is this screen's own age, so the spinner turns at the same rate however
/// often the loop happens to come round.
pub fn turn(t: &Turn, ms: u128) -> Line<'static> {
    let mut said = Vec::new();
    if let Some(n) = t.tokens {
        said.push(format!("{} tok", tokens(n, t.estimated)));
    }
    said.push(format!("{:.1}s", t.secs()));
    // Spinning while it works, a dot once it is done: one slot either way, so
    // nothing on the line moves when the round ends.
    let mark = if t.ended.is_none() {
        Span::styled(
            SPINNER[ms as usize / 80 % SPINNER.len()],
            Style::new().fg(ink::text()),
        )
    } else {
        Span::styled(DOT, Style::new().fg(ink::done()))
    };
    Line::from(vec![
        mark,
        Span::raw(" "),
        Span::styled(said.join(" \u{b7} "), Style::new().fg(ink::text())),
    ])
}

/// A token count: `~2.2k`, `1.5k`, `845`. One decimal, because a round's count is
/// worth knowing to a tenth of a thousand and a truncated one says 1k for 1500,
/// which is off by half.
fn tokens(n: i64, est: bool) -> String {
    let mark = if est { "~" } else { "" };
    if n >= 1000 {
        format!("{mark}{:.1}k", n as f64 / 1000.0)
    } else {
        format!("{mark}{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuacode::transcript::Transcript;
    use cuacode::wire;

    fn rows(lines: &[&str]) -> Vec<Row> {
        let mut log = Transcript::default();
        for line in lines {
            log.apply(&wire::parse(line).unwrap());
        }
        log.rows
    }

    /// Nothing folded: the screen as the design draws it.
    fn all_open() -> impl Fn(usize, &Row) -> bool {
        |_, _| true
    }

    /// Every thought unfolded too, which is the reader who opened the lot.
    fn all_unfolded() -> impl Fn(usize) -> bool {
        |_| true
    }

    /// No figures filed: the screen at rest.
    fn no_figures() -> impl Fn(usize) -> Option<Round> {
        |_| None
    }

    /// The width a row draws at, unless the test is about the width.
    const ROOM: usize = 80;

    /// `pieces` at the width and the screen the tests do not care about. The
    /// thinking is open here: what an opened step holds is what these are about,
    /// and the folded line it opens as has its own test.
    fn pieces_at(
        rows: &[Row],
        expanded: &dyn Fn(usize, &Row) -> bool,
        figures: &dyn Fn(usize) -> Option<Round>,
        writing: Option<usize>,
        flash: bool,
    ) -> Vec<Held> {
        pieces(rows, expanded, &all_unfolded(), figures, writing, flash, ROOM, None)
    }

    /// A piece with no row behind it, for the tests that are about `draw` and not
    /// about where a piece came from.
    /// The band, for the demo that prints the peer rows as a terminal would.
    fn band(text: &str, phase: usize) -> Line<'static> {
        let runs = vec![vec![(text.to_string(), Style::new().fg(ink::quiet()))]];
        Line::from(
            shimmer_runs(runs, ink::quiet(), ink::text(), Motion::Pass, phase)
                .pop()
                .unwrap_or_default()
                .into_iter()
                .map(|(text, style)| Span::styled(text, style))
                .collect::<Vec<_>>(),
        )
    }

    fn held(piece: Piece) -> Held {
        Held { row: 0, head: 0, piece }
    }

    #[test]
    fn the_dot_says_what_happened() {
        let call = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"adb devices"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":127,"stderr":"zsh:1: command not found: adb\n"}}}}"#,
        ]);
        let after = pieces_at(&call, &all_open(), &no_figures(), None, true);
        assert_eq!(after[0].lead_style.fg, Some(ink::failed()));
        assert!(after[1].text().contains("command not found"), "{after:?}");

        // A call with no result yet: white while it is the row being written,
        // dim on the other half of the blink.
        let running = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"adb devices"}}]}}"#,
        ]);
        let written = Some(0);
        assert_eq!(
            pieces_at(&running, &all_open(), &no_figures(), written, true)[0]
                .lead_style
                .fg,
            Some(ink::text())
        );
        assert_eq!(
            pieces_at(&running, &all_open(), &no_figures(), written, false)[0]
                .lead_style
                .fg,
            Some(ink::quiet())
        );

        // A call that already failed is red whether or not it is the live row.
        assert_eq!(
            pieces_at(&call, &all_open(), &no_figures(), written, true)[0]
                .lead_style
                .fg,
            Some(ink::failed())
        );

        // The answer is white whatever else is true, blinking or not.
        let answered =
            rows(&[r#"{"type":"token","id":"a","data":{"state":"content","token":"done"}}"#]);
        for flash in [true, false] {
            assert_eq!(
                pieces_at(&answered, &all_open(), &no_figures(), written, flash)[0]
                    .lead_style
                    .fg,
                Some(ink::text())
            );
        }
    }

    #[test]
    fn the_models_own_text_is_drawn_as_the_markdown_it_is() {
        let answer = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"content","token":"**bold** and `code`\n\n# Heading\n\n- one\n- two\n\n```\nlet x = 1;\n```"}}"#,
        ]);
        let piece = &pieces_at(&answer, &all_open(), &no_figures(), None, true)[0];
        let text = piece.text();
        assert!(text.contains("bold and code"), "{text}");
        assert!(
            !text.contains('*') && !text.contains('`'),
            "markers are gone: {text}"
        );
        assert!(text.contains("Heading") && !text.contains('#'), "{text}");
        assert!(text.contains("\u{2022} one"), "{text}");
        assert!(text.contains("let x = 1;"), "{text}");

        // The styles the markers asked for, on the runs they named.
        let base = Style::new().fg(ink::text());
        let styled = |wanted: &str| {
            piece
                .lines
                .iter()
                .flatten()
                .find(|(text, _)| text == wanted)
                .map(|(_, style)| *style)
                .unwrap_or_else(|| panic!("no {wanted} run: {:?}", piece.lines))
        };
        assert_eq!(styled("bold"), base.add_modifier(Modifier::BOLD));
        assert_eq!(styled("code"), base.fg(ink::code()));
        assert_eq!(styled("Heading"), base.add_modifier(Modifier::BOLD));
        // A fenced line is read as the code it is, so `let` is no longer part of one
        // run of code colour. What the whole line is drawn as is checked in
        // `a_fenced_line_is_highlighted_and_a_plain_one_stays_one_colour`.
        assert_eq!(
            styled("let"),
            base.fg(ink::link()),
            "a keyword takes a tone of its own"
        );
    }

    /// A fenced line is read as code: a comment, a string, a keyword and a number take
    /// the tones the palette already has, and a line with none of them stays the one
    /// run in the code colour it has always been.
    ///
    /// The whole line is checked rather than a tone at a time, because what makes a
    /// highlighter wrong is run boundaries: a colour that took a comma with it, or a
    /// keyword that swallowed the identifier after it, still contains the words.
    #[test]
    fn a_fenced_line_is_highlighted_and_a_plain_one_stays_one_colour() {
        let base = Style::new().fg(ink::code());
        // One line, through the fence and the fence markers, which is the only path
        // the screen takes to it.
        let runs = |line: &str| {
            formatted(
                &format!("```\n{line}\n```"),
                Style::new().fg(ink::text()),
                ROOM,
            )
            .into_iter()
            .next()
            .expect("the one line inside the fence")
        };

        // A comment and a string, which is the pair a reader skims a fence for. The
        // comment runs to the end of the line and takes nothing before it with it.
        let line = "let s = \"hi\"; // say it";
        assert_eq!(
            runs(line)
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>(),
            line,
            "highlighting is a colour, not an edit"
        );
        assert_eq!(
            runs(line),
            vec![
                ("let".to_string(), base.fg(ink::link())),
                (" s = ".to_string(), base),
                ("\"hi\"".to_string(), base.fg(ink::done())),
                ("; ".to_string(), base),
                ("// say it".to_string(), base.fg(ink::faint())),
            ]
        );

        // A number is taken whole, and `2` is not a number when it is part of a word.
        assert_eq!(
            runs("if x2 < 0x1f { return 2; }"),
            vec![
                ("if".to_string(), base.fg(ink::link())),
                (" x2 < ".to_string(), base),
                ("0x1f".to_string(), base.fg(ink::text())),
                (" { ".to_string(), base),
                ("return".to_string(), base.fg(ink::link())),
                (" ".to_string(), base),
                ("2".to_string(), base.fg(ink::text())),
                ("; }".to_string(), base),
            ]
        );

        // Nothing in the vocabulary is one run, and a blank line draws nothing: an
        // empty run is not a run.
        assert_eq!(
            runs("widget.mount(theme);"),
            vec![("widget.mount(theme);".to_string(), base)]
        );
        assert!(runs("").is_empty(), "a blank fence line is no runs at all");

        // A `'` that opens nothing is left alone: a lifetime is not a string, and
        // reading `&'a` as one would swallow the real string later on the line.
        assert_eq!(
            runs("let p: &'a str = \"it's here\";"),
            vec![
                ("let".to_string(), base.fg(ink::link())),
                (" p: &'a str = ".to_string(), base),
                ("\"it's here\"".to_string(), base.fg(ink::done())),
                (";".to_string(), base),
            ]
        );
    }

    #[test]
    fn wrapping_keeps_each_words_style() {
        let answer = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"content","token":"**alpha beta gamma delta epsilon zeta**"}}"#,
        ]);
        let drawn = draw(
            &pieces_at(&answer, &all_open(), &no_figures(), None, true),
            20,
        );
        assert!(
            drawn.lines.iter().all(|line| line.width() <= 20),
            "{:?}",
            drawn.lines
        );
        let bold = Style::new().fg(ink::text()).add_modifier(Modifier::BOLD);
        for line in &drawn.lines {
            // The first span is the dot's lead; everything after it is the text.
            for span in line.spans.iter().skip(1) {
                assert_eq!(span.style, bold, "a wrapped run lost its style: {:?}", line);
            }
        }
    }
    #[test]
    fn the_rest_of_the_markdown_is_there_too() {
        let answer = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"content","token":"***both*** ~~gone~~ [docs](https://x)\n\n> quoted\n> > nested\n\n- [ ] todo\n- [x] done\n\n---\n\n| Name | What |\n|---|---|\n| a | first |\n| bb | second |"}}"#,
        ]);
        let text = pieces_at(&answer, &all_open(), &no_figures(), None, true)[0].text();
        // Nothing is left wearing its markers.
        assert!(
            !text.contains('*') && !text.contains('~') && !text.contains('['),
            "{text}"
        );
        assert!(text.contains("both") && text.contains("gone"), "{text}");
        // A link keeps its address, because a terminal cannot follow it otherwise.
        assert!(text.contains("docs (https://x)"), "{text}");
        // Quotes, nested, one bar per level.
        assert!(text.contains("\u{2502} quoted"), "{text}");
        assert!(text.contains("\u{2502} \u{2502} nested"), "{text}");
        // Task lists, ticked and not, with the box instead of the brackets.
        assert!(
            text.contains("\u{2610} todo") && text.contains("\u{2611} done"),
            "{text}"
        );
        // A rule, and a table whose columns line up with each other.
        assert!(text.contains("\u{2500}\u{2500}"), "{text}");
        assert!(text.contains("Name"), "{text}");
        let columns: Vec<usize> = text
            .lines()
            // A table's row, not a quote's: a quote's bar is its first character.
            .filter(|line| line.contains(" \u{2502} ") && !line.starts_with('\u{2502}'))
            .map(|line| line.find('\u{2502}').expect("the pipe just found"))
            .collect();
        assert!(columns.len() >= 3, "a header and its rows: {text}");
        assert!(
            columns.windows(2).all(|pair| pair[0] == pair[1]),
            "lined up: {text}"
        );
    }

    /// A path is read the way a person reads one: from the directory this
    /// conversation is in, from home, and with its middle dropped only when what
    /// is left still fits. What a row has no room for is in full in the panel.
    #[test]
    fn a_long_path_is_shortened_for_the_row() {
        let here = cuacode::session::working_dir();
        assert_eq!(short_path(&format!("{here}/src/view.rs")), "src/view.rs");
        let home = cuacode::home_dir()
            .expect("a home to shorten against")
            .display()
            .to_string();
        assert_eq!(
            short_path(&format!(
                "{home}/.cuacode/sessions/20260917-172453-ea54/notebook/SESSION_BREAKDOWN.md"
            )),
            "~/\u{2026}/notebook/SESSION_BREAKDOWN.md"
        );
        // One that fits is left where it is, and one that is nowhere near either is
        // cut in the middle rather than made up to be somewhere it is not.
        assert_eq!(short_path("/tmp/x.py"), "/tmp/x.py");
        assert_eq!(
            short_path("/opt/one/two/three/four/five/six/seven.txt"),
            "/\u{2026}/six/seven.txt"
        );
        // A sibling whose name starts the same way is not inside it.
        assert_eq!(inside("/a/bc/d", "/a/b"), None);
        assert_eq!(
            inside("/a/b", "/a/b"),
            None,
            "the directory itself has no shorter name"
        );
        assert_eq!(inside("/a/b/d", "/a/b").as_deref(), Some("d"));
    }

    /// A file's row names the file, not the way to it, and says the path once: the
    /// head carries it and the summary under the head leaves it out. The two parts
    /// of the head are the verb and what it ran on, which is where the file's own
    /// name gets to be the bright part of a long path.
    #[test]
    fn a_files_row_shortens_its_path_and_says_it_once() {
        let home = cuacode::home_dir()
            .expect("a home to shorten against")
            .display()
            .to_string();
        let path =
            format!("{home}/.cuacode/sessions/20260917-172453-ea54/notebook/SESSION_BREAKDOWN.md");
        // The path is a Windows path too, and a backslash inside JSON is an
        // escape: the test has to send the path the worker would send.
        let path_json = path.replace('\\', "\\\\");
        let call = rows(&[
            &format!(
                r#"{{"type":"token","id":"a","data":{{"state":"tool_calls","token":[{{"name":"file","args":{{"action":"write","path":"{path_json}","content":"x"}}}}]}}}}"#
            ),
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"file","result":{"result":{"created":true,"path":"/tmp/x","written":7977}}}}"#,
        ]);
        let after = pieces_at(&call, &all_open(), &no_figures(), None, true);
        assert_eq!(after[0].lead, "\u{25cf} ");
        assert_eq!(after[0].text(), "write(~/…/notebook/SESSION_BREAKDOWN.md)");
        // The way there is fainter than the file, and the file is the row's own
        // colour: what a person is looking for in a list of files is the file.
        assert_eq!(
            after[0].lines[0],
            vec![
                ("write".to_string(), Style::new().fg(ink::call())),
                ("(".to_string(), Style::new().fg(ink::text())),
                (
                    "~/\u{2026}/notebook/".to_string(),
                    Style::new().fg(ink::faint())
                ),
                (
                    "SESSION_BREAKDOWN.md".to_string(),
                    Style::new().fg(ink::text())
                ),
                (")".to_string(), Style::new().fg(ink::text())),
            ]
        );
        // The path is said once. The head has it; the summary under the head does
        // not repeat it, and keeps everything else it was told.
        assert_eq!(after[1].text(), "created \u{b7} written 7977");
    }

    /// The verb is what says a row is something that ran, so it is the one
    /// coloured thing on a head that is otherwise the row's own colour.
    #[test]
    fn a_calls_verb_is_the_coloured_part_of_its_head() {
        let call = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"file","args":{"action":"write","path":"a.py"}}]}}"#,
        ]);
        let head = &pieces_at(&call, &all_open(), &no_figures(), None, true)[0];
        assert_eq!(
            head.lines[0][0],
            ("write".to_string(), Style::new().fg(ink::call()))
        );
        assert_eq!(
            head.text(),
            "write(a.py)",
            "and a head with no path in it is one run of argument"
        );
        // No slash to split on, so the argument is the one run after the verb: a
        // call is never one colour, and the argument is never the verb's.
        let arg = head.lines[0].last().expect("an argument");
        assert_eq!(arg.0, ")");
        assert_ne!(arg.1.fg, Some(ink::call()), "the argument is not the verb");
    }

    /// A diff is read by its colours: a line that went is red, a line that came is
    /// green, and the file names and hunk markers are not the change.
    #[test]
    fn a_diff_is_drawn_in_its_two_colours() {
        let data = serde_json::json!({
            "name": "file",
            "index": 1,
            "total": 1,
            "args": {"action": "edit", "path": "a.py", "edits": [{"old": "x", "new": "y"}]},
            "result": {"result": {"path": "a.py", "edits": 1,
                "diff": "--- a/a.py\n+++ b/a.py\n@@ -1 +1 @@\n-x\n+y"}}
        });
        let panel = call_detail(&data, 60);
        let ink_of = |wanted: &str| {
            panel
                .lines
                .iter()
                .find(|line| line.to_string().contains(wanted))
                .and_then(|line| line.spans.iter().find(|s| s.content.contains(wanted)))
                .map(|s| s.style.fg)
                .unwrap_or_else(|| {
                    panic!(
                        "no line with {wanted}: {:?}",
                        panel.lines.iter().map(Line::to_string).collect::<Vec<_>>()
                    )
                })
        };
        assert_eq!(ink_of("-x"), Some(ink::failed()), "a line that went");
        assert_eq!(ink_of("+y"), Some(ink::done()), "a line that came");
        assert_eq!(ink_of("--- a/a.py"), Some(ink::dim()), "what it is against");
    }

    #[test]
    fn the_question_to_allow_a_call_carries_everything_needed_to_answer_it() {
        let data = serde_json::json!({
            "name": "file",
            "args": {"action": "write", "path": "src/main.rs", "content": "lots"},
            "note": "it writes outside the directory the run started in",
            "preview": {"summary": "write 6542 bytes to src/main.rs"},
        });
        let asked = ask_detail("file", &data);
        assert_eq!(asked.title, "allow file?");
        let text: String = asked
            .lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        // The gate's reason in full, the call as the model wrote it, and what the
        // call says it would do: a yes or a no on a clipped line is a blind one.
        assert!(
            text.contains("why:\nit writes outside the directory"),
            "{text}"
        );
        assert!(text.contains("the call:\n{"), "{text}");
        assert!(text.contains("\"path\": \"src/main.rs\""), "{text}");
        assert!(text.contains("it would:\nwrite 6542 bytes"), "{text}");
    }

    #[test]
    fn a_call_reads_as_a_call() {
        // The action is what it did, and the path is the whole of what the summary
        // would have cut short.
        let write = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"file","args":{"action":"write","path":"crates/quik/src/a/rather/long/name.rs","content":"lots of text"}}]}}"#,
        ]);
        let head = pieces_at(&write, &all_open(), &no_figures(), None, true)[0].text();
        assert_eq!(head, "write(crates/quik/src/a/rather/long/name.rs)");

        let shell = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"date"}}]}}"#,
        ]);
        assert_eq!(
            pieces_at(&shell, &all_open(), &no_figures(), None, true)[0].text(),
            "shell(date)"
        );

        // A call with nothing to put in the brackets is still its verb.
        let bare = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"todo","args":{}}]}}"#,
        ]);
        assert_eq!(
            pieces_at(&bare, &all_open(), &no_figures(), None, true)[0].text(),
            "todo"
        );

        // The action is the verb, so it is not also a bracketed argument: a call
        // whose action is all it has says it once.
        let planned = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"todo","args":{"action":"plan","steps":["one"]}}]}}"#,
        ]);
        assert_eq!(
            pieces_at(&planned, &all_open(), &no_figures(), None, true)[0].text(),
            "plan"
        );
    }

    /// A call's row is its head and what came back, and nothing else: the input
    /// the head had no room for -- a file's content -- is the panel's, so the row
    /// is not a second copy of the call. A call that has not reported yet is only
    /// its head.
    #[test]
    fn a_call_shows_what_came_back_and_not_its_input() {
        let write = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"file","args":{"action":"write","path":"a.py","content":"one\ntwo"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"file","result":{"result":{"created":true,"written":268}}}}"#,
        ]);
        let open = pieces_at(&write, &all_open(), &no_figures(), None, true);
        assert_eq!(
            open.len(),
            2,
            "the head and the result: {:?}",
            open.iter().map(|p| p.text()).collect::<Vec<_>>()
        );
        assert_eq!(open[0].text(), "write(a.py)");
        assert_eq!(open[1].lead, BODY_LEAD);
        assert_eq!(open[1].text(), "created \u{b7} written 268");
        // The input is nowhere on the row: no code block, no `content`.
        let all: String = open.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n");
        assert!(
            !all.contains("content") && !all.contains('\u{250c}'),
            "{all}"
        );

        // A call still running has nothing to show under its head.
        let running = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"date"}}]}}"#,
        ]);
        assert_eq!(
            pieces_at(&running, &all_open(), &no_figures(), None, true).len(),
            1
        );
    }

    /// The short result is on the row whatever ctrl-t is doing: a column of calls
    /// with nothing said about their results is a list you open one line at a time
    /// to read, and the fold is for thoughts.
    #[test]
    fn a_call_shows_its_result_whether_the_thoughts_are_folded_or_not() {
        let call = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"date"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0,"stdout":"Friday\n"}}}}"#,
        ]);
        let folded = |_: usize, _: &Row| false;
        let shut = pieces_at(&call, &folded, &no_figures(), None, true);
        assert_eq!(shut.len(), 2);
        assert_eq!(shut[0].text(), "shell(date)");
        assert_eq!(shut[1].text(), "stdout:7B \u{b7} +1");
    }

    /// The block is a real one: the title on its edge, the code inside it with an
    /// edge on every line, and a line too long for the box wrapped inside it and
    /// keeping its edge rather than running off or being cut short.
    #[test]
    fn a_code_block_is_a_box_that_keeps_its_edges() {
        let long = "x".repeat(40);
        let call = serde_json::json!({
            "action": "write",
            "path": "a.py",
            "content": format!("short\n{long}")
        });
        let panel = call_from_row("file", "path=a.py", &call, None, 44);
        let lines: Vec<String> = panel.lines.iter().map(Line::to_string).collect();
        let boxed: Vec<&String> = {
            // The block runs from the line that opens it to the line that closes it.
            // Found by its corners rather than by index: the `args` lines above it are
            // keyed by serde_json's map order, which is sorted on its own and insertion
            // order once another crate in the workspace asks for `preserve_order`, so a
            // fixed index here passes under `-p` and fails under `--workspace`.
            let start = lines
                .iter()
                .position(|l| l.starts_with('\u{250c}'))
                .expect("no box opens");
            let end = lines
                .iter()
                .position(|l| l.ends_with('\u{2518}'))
                .expect("no box closes");
            lines[start..=end].iter().collect()
        };
        assert!(
            boxed[0].starts_with("\u{250c} content \u{b7} 2 lines ")
                && boxed[0].ends_with('\u{2510}'),
            "{lines:?}"
        );
        assert!(boxed.len() >= 3, "a box holds something: {lines:?}");
        let widths: Vec<usize> = boxed.iter().map(|l| l.chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "{widths:?}\n{lines:?}"
        );
        // The wrapped parts of the long line keep the box's left edge, so the
        // block still reads as one block.
        assert!(
            boxed[1..boxed.len() - 1]
                .iter()
                .all(|l| l.starts_with('\u{2502}')),
            "{lines:?}"
        );
        // All of it is there: wrapping is not clipping.
        let xs: usize = lines.iter().map(|l| l.matches('x').count()).sum();
        assert_eq!(xs, 40, "{lines:?}");

        // No room for a box: plain lines, because a frame narrower than its own
        // contents is worse than no frame.
        let tight = call_from_row("file", "path=a.py", &call, None, 10);
        let text: String = tight
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !text.contains('\u{250c}') && !text.contains('\u{2502}'),
            "{text}"
        );
        assert!(text.contains("short") && text.contains("xxx"), "{text}");
    }

    /// A call in full, from the worker's record: the arguments and the result as
    /// the documents they are, and not as the JSON the wire carried.
    #[test]
    fn a_call_in_full_draws_documents_as_documents() {
        let data = serde_json::json!({
            "name": "file",
            "index": 2,
            "total": 5,
            "args": {"action": "write", "path": "a.py", "content": "one\ntwo"},
            "result": {"result": {"created": true, "written": 7}}
        });
        let panel = call_detail(&data, 60);
        assert_eq!(panel.title, "write(a.py) #2 of 5");
        let text: String = panel
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        // The head says the path, so the panel does not say it again.
        assert!(!text.contains("path"), "{text}");
        assert!(text.contains("\u{250c} content \u{b7} 2 lines "), "{text}");
        assert!(
            text.contains("\u{2502} one") && text.contains("\u{2502} two"),
            "{text}"
        );
        assert!(text.contains("result:"), "{text}");
        assert!(
            text.contains("created  true") && text.contains("written  7"),
            "{text}"
        );
        // A newline is a newline. An escaped one, and the braces around it, are
        // exactly what this replaced.
        assert!(!text.contains("\\n") && !text.contains('{'), "{text}");
    }

    /// A row's own copy is enough for a panel: that is what a replay has, and what
    /// a live one shows while the worker is being asked for the rest of it.
    #[test]
    fn a_call_from_the_row_is_a_panel_without_a_round_trip() {
        let call = serde_json::json!({
            "action": "edit",
            "path": "a.py",
            "edits": [{"old": "x", "new": "y", "all": false}]
        });
        let panel = call_from_row("file", "path=a.py", &call, Some("edited 1"), 60);
        assert_eq!(panel.title, "edit(a.py)");
        let text: String = panel
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("old  x") && text.contains("new  y"), "{text}");
        // A flag that is off says nothing, and the path is the title's.
        assert!(!text.contains("all  false"), "{text}");
        assert!(!text.contains("path"), "{text}");
        assert!(text.contains("result:\nedited 1"), "{text}");
    }

    /// A call that never reported has no result to draw, so the panel is the
    /// arguments and nothing else rather than an empty block named "result". A
    /// call whose only argument is the path its head already says has nothing to
    /// add, so it is the head alone.
    #[test]
    fn a_panel_leaves_out_a_block_it_has_nothing_for() {
        let call = serde_json::json!({"action": "read", "path": "a.py"});
        let panel = call_from_row("file", "path=a.py", &call, None, 60);
        let text: String = panel
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text, "", "the title is the whole panel: {text}");
        assert!(!text.contains("result:"), "{text}");

        // Something else in the arguments is drawn, and the path still is not.
        let ranged = serde_json::json!({"action": "read", "path": "a.py", "start": 10});
        let text: String = call_from_row("file", "path=a.py", &ranged, None, 60)
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("start  10"), "{text}");
        assert!(!text.contains("path"), "{text}");
    }

    /// A document's lines carry the file's own numbers, in a gutter ruled off from
    /// the text, and the file tool's `7\tcode` offset is read as that number rather
    /// than drawn as part of the code. Without it a read of a source file draws a
    /// wall of amber that happens to start with digits.
    #[test]
    fn a_documents_lines_are_numbered_in_a_gutter() {
        let body = body_of(&["1\tdef f():".to_string(), "2\t    return 1".to_string()]);
        let boxed: Vec<String> = framed("content", &body, 40, 0)
            .iter()
            .map(|r| r.iter().map(|(t, _)| t.as_str()).collect())
            .collect();
        assert!(boxed[1].contains("1 \u{2502} def f():"), "{boxed:?}");
        assert!(boxed[2].contains("2 \u{2502}     return 1"), "{boxed:?}");
        // The number is not code: it is drawn in the gutter's tone, and the line
        // after it is highlighted as code.
        let runs = framed("content", &body, 40, 0);
        let gutter = runs[1][1].0.as_str();
        assert!(
            gutter.contains('1'),
            "the gutter comes before the text: {gutter:?}"
        );
        assert!(
            runs[1][1].1.fg == Some(ink::faint()),
            "the number is the gutter's tone"
        );
        // A diff keeps the hunk header's own numbers and gets no gutter.
        let diff = diff_block("@@ -1 +1 @@\n-a\n+b", 40, 0);
        assert!(
            diff.iter()
                .all(|r| !r.iter().any(|(t, _)| t.contains("\u{2502} 1"))),
            "{diff:?}"
        );
    }

    /// A code block keeps the code's indentation, which is most of what makes it
    /// code. The row's wrapping and the block's own are both places it could be
    /// lost, so this is checked through a drawn panel rather than a piece.
    #[test]
    fn a_code_block_keeps_the_codes_indentation() {
        let call = serde_json::json!({
            "action": "write",
            "path": "a.py",
            "content": "def f():\n    if x:\n        return 1"
        });
        let panel = call_from_row("file", "path=a.py", &call, None, 40);
        let drawn = wrap_lines(&panel.lines, 40);
        let text: String = drawn
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("\u{2502} def f():"), "{text}");
        assert!(text.contains("\u{2502}     if x:"), "{text}");
        assert!(text.contains("\u{2502}         return 1"), "{text}");
    }

    /// The wrap always finishes, whatever the width: a split that cut nothing would
    /// be a loop that never ends, which is the shape of every wrapping bug that
    /// hangs a screen instead of drawing a wrong line.
    #[test]
    fn wrapping_always_finishes() {
        for width in [1usize, 2, 3] {
            let runs: Vec<Run> = vec![("\u{8868}\u{8868}ab".to_string(), Style::new())];
            let lines = wrap_runs(&runs, width);
            let text: String = lines.iter().flatten().map(|r| r.0.as_str()).collect();
            assert_eq!(
                text, "\u{8868}\u{8868}ab",
                "a width of {width} lost or repeated a character"
            );
        }
    }

    /// A terminal column is not a character, and a word is split to the room its
    /// line has left rather than to the whole width. Both mistakes draw past the
    /// width the line was wrapped to: the first because a wide glyph is two
    /// columns, the second because the indent was already spent.
    ///
    /// `Line::width` is ratatui's own measure, so this asserts what the terminal is
    /// handed and not a number this file decided for itself.
    #[test]
    fn a_wrapped_line_never_draws_past_its_width() {
        // Ten glyphs of U+8868 draw in twenty columns, not ten.
        let wide = Piece {
            lead: String::new(),
            lead_style: Style::new(),
            air_above: false,
            air_below: false,
            rail: false,
            lines: vec![vec![("\u{8868}".repeat(10), Style::new())]],
        };
        let drawn = draw(&[held(wide)], 10);
        assert!(
            drawn.lines.len() > 1,
            "twenty columns did not wrap in ten: {}",
            drawn.lines.len()
        );
        for line in &drawn.lines {
            assert!(
                line.width() <= 10,
                "a wide character was counted as one: {:?}",
                line.to_string()
            );
        }

        // A word after an indent is split to what the indent left it. Ten letters
        // behind four spaces on an eight-column row: four, then four, then two.
        let indented = Piece {
            lead: String::new(),
            lead_style: Style::new(),
            air_above: false,
            air_below: false,
            rail: false,
            lines: vec![vec![("    abcdefghij".to_string(), Style::new())]],
        };
        for line in &draw(&[held(indented)], 8).lines {
            assert!(
                line.width() <= 8,
                "a word was split to the row, not to the room left: {:?}",
                line.to_string()
            );
        }

        // A box around a wide body is a box, not a frame its text pokes through,
        // and a tab in a code line keeps the indent it stands for instead of being
        // dropped by the terminal and taking the indent with it.
        let body: Vec<Body> = ["\u{8868}\u{8868}", "\treturn 1"]
            .into_iter()
            .map(|l| (None, vec![(untabbed(l), Style::new())]))
            .collect();
        let edges: Vec<usize> = framed("t", &body, 40, 0)
            .iter()
            .map(|r| runs_width(r))
            .collect();
        assert!(edges.len() > 2, "a box around two lines: {edges:?}");
        assert!(
            edges.windows(2).all(|w| w[0] == w[1]),
            "the box was not square: {edges:?}"
        );
        let code: String = framed("t", &body, 40, 0)
            .iter()
            .flatten()
            .map(|r| r.0.as_str())
            .collect();
        assert!(
            code.contains("    return 1"),
            "a tab lost its indent: {code:?}"
        );

        // A cut measured in columns lands on a character boundary and inside the
        // width: taking the byte count as a character count splits a wide glyph in
        // half. Three glyphs and a mark in four columns is one glyph and the mark,
        // because half of the second glyph is not a cell.
        let cut = clip("\u{8868}\u{8868}\u{8868}", 4);
        assert!(columns(&cut) <= 4, "the mark is outside the width: {cut:?}");
        assert!(cut.ends_with('\u{2026}'), "the cut is not marked: {cut:?}");
        assert_eq!(clip("abcdef", 4), "abc\u{2026}");
    }

    /// The row's own wrapping must not eat a code line's indentation, and must
    /// not eat the spaces a box pads with either: whatever the block is drawn
    /// with has to survive being put on a row.
    #[test]
    fn wrapping_keeps_its_spaces() {
        let runs: Vec<Run> = vec![("    return 1".to_string(), Style::new())];
        assert_eq!(
            wrap_runs(&runs, 20)[0][0].0,
            "    return 1",
            "leading indent"
        );
        let runs: Vec<Run> = vec![("a      b".to_string(), Style::new())];
        assert_eq!(wrap_runs(&runs, 20)[0][0].0, "a      b", "interior run");
    }

    #[test]
    fn a_reasoning_row_keeps_its_figures() {
        let thought =
            rows(&[r#"{"type":"token","id":"a","data":{"state":"thinking","token":"hm"}}"#]);
        let mine = |i: usize| {
            (i == 0).then_some(Round {
                tokens: 2048,
                tps: 351.4,
                estimated: true,
            })
        };
        // The head is the thought's own name; the figures hang under it, on the
        // one line this screen gives a step.
        let live = pieces_at(&thought, &all_open(), &mine, Some(0), true);
        assert_eq!(live[0].text(), "hm");
        assert_eq!(live[1].text(), "Reasoning | ~2.0k tokens, 351 tok/s");
        assert_eq!(live[0].lead_style.fg, Some(ink::text()));
        // The round is over. The dot stops being live and the figures stay: they
        // are what that round cost, not what is happening now.
        let after = pieces_at(&thought, &all_open(), &mine, None, true);
        assert_eq!(after[1].text(), "Reasoning | ~2.0k tokens, 351 tok/s");
        assert_eq!(after[0].lead_style.fg, Some(ink::done()));
        // A thought with nothing filed is its name and no second line at all.
        let bare = pieces_at(&thought, &all_open(), &no_figures(), Some(0), true);
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].text(), "hm");
    }

    /// A merged step's rate is the rate of all of it: tokens over the time they
    /// took. Summing the rounds' own rates would say a step that spent two seconds
    /// at 50 and three at 100 was doing 150 a second.
    #[test]
    fn a_merged_step_reports_its_whole_rate() {
        let stream = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"first"}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"ls"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","index":0,"result":{"result":{"exit_code":0,"stdout":"a\n"}}}}"#,
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"second"}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"pwd"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","index":0,"result":{"result":{"exit_code":0,"stdout":"/\n"}}}}"#,
        ]);
        // Rows: thought, call, thought, call.
        let mine = |i: usize| match i {
            0 => Some(Round { tokens: 100, tps: 50.0, estimated: false }),
            2 => Some(Round { tokens: 300, tps: 100.0, estimated: false }),
            _ => None,
        };
        let ps = pieces_at(&stream, &|_: usize, _: &Row| false, &mine, None, true);
        assert_eq!(ps[1].text(), "Reasoning | 400 tokens, 80 tok/s  \u{b7}  Ran shell \u{d7}2");
    }

    /// The thought under a reasoning row is drawn dimmer than the answer. It is
    /// worth reading, and it is not what was asked for: at full white it
    /// out-shouts the reply it was thinking towards.
    #[test]
    fn an_open_thought_is_dimmer_than_the_answer() {
        // Two sentences, so the name is the first one and the detail under it is
        // the whole thought: a thought of one short sentence is its own name and
        // is not drawn twice.
        let thought = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"hm. one two"}}"#,
        ]);
        let open = pieces_at(&thought, &all_open(), &no_figures(), None, true);
        assert_eq!(open[0].text(), "hm");
        assert_eq!(open[1].lines[0][0].1.fg, Some(ink::dim()));
        let answer = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"content","token":"it is 04:56"}}"#,
        ]);
        assert_eq!(
            pieces_at(&answer, &all_open(), &no_figures(), None, true)[0].lines[0][0]
                .1
                .fg,
            Some(ink::text())
        );
    }

    /// A step opened is its calls. Its thinking is one line each -- the thought's
    /// own opening words, which is what says whether it is worth opening -- so a
    /// long stretch of work is a column of calls with the reasoning behind a rail
    /// rather than a wall of prose with the calls buried in it.
    #[test]
    fn an_opened_step_folds_its_thinking_to_one_line_each() {
        let stream = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"Look at the loader first. Then read the file."}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"ls"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","index":0,"result":{"result":{"exit_code":0,"stdout":"a\n"}}}}"#,
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"Now the loader is clear. Write the patch."}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"pwd"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","index":1,"result":{"result":{"exit_code":0,"stdout":"/\n"}}}}"#,
        ]);
        // Rows: thought, call, thought, call. The step is open -- ctrl-t -- and the
        // thinking inside it is not.
        let folded = pieces(&stream, &all_open(), &|_| false, &no_figures(), None, true, ROOM, None);
        let said: Vec<String> = folded.iter().map(|p| p.text()).collect();
        assert!(said[0].starts_with("Now the loader is clear"), "{said:?}");
        assert_eq!(folded[2].text(), "\u{25b8} Look at the loader first", "{said:?}");
        // The step is named for the second thought, so the line under it cannot say
        // the same words again: with nothing filed about that round it says what it
        // is and lets the triangle say the rest.
        assert_eq!(folded[4].text(), "\u{25b8} thinking", "{said:?}");
        // This is a column to scroll rather than a page to read: one line each,
        // whatever the thought's length, and the calls still a line each.
        for piece in &folded[1..] {
            assert_eq!(piece.lines.len(), 1, "{:?}", piece.text());
        }

        // The head's own thought says what its round cost, since whose words are on
        // the head above it is already answered.
        let mine = |i: usize| {
            (i == 2).then_some(Round {
                tokens: 820,
                tps: 0.0,
                estimated: false,
            })
        };
        let cost = pieces(&stream, &all_open(), &|_| false, &mine, None, true, ROOM, None);
        let said: Vec<String> = cost.iter().map(|p| p.text()).collect();
        assert!(said.iter().any(|t| t == "\u{25b8} 820 tokens"), "{said:?}");

        // Opened on the reader's own fold, the text is there again.
        let open = pieces_at(&stream, &all_open(), &no_figures(), None, true);
        let all: String = open.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n");
        assert!(all.contains("Then read the file"), "{all}");
        assert!(!all.contains('\u{25b8}'), "an opened thought is not a triangle: {all}");
    }

    /// The count is of tools. A run of one kind says its own count instead, where
    /// "Ran 2 tools" in front of "read ×2" would say two twice.
    #[test]
    fn a_run_of_tools_is_counted_as_tools() {
        let stream = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"Look, then read."}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"ls"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0,"stdout":"a\n"}}}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"file","args":{"action":"read","path":"a.py"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"file","result":{"result":{"content":"x"}}}}"#,
        ]);
        assert_eq!(ran(&[1, 2], &stream), "Ran 2 tools · read ×1, shell ×1");
        assert_eq!(ran(&[1, 1], &stream), "Ran shell ×2");
    }

    /// The sweep over a step's name runs in one of two ramps: warm while the model
    /// is thinking, blue while a tool is in flight. The two halves of a round are
    /// then told apart at a glance instead of by reading what the row says.
    #[test]
    fn the_sweep_is_blue_while_a_tool_runs() {
        let think = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"Reading the loader first. Then the patch."}}"#,
        ]);
        // The name is under the warm ramp, turned back on itself: at the top of the
        // cycle the band is centred on its first character, and the far end of the same
        // row is the tone the row always had.
        let warm = pieces(&think, &all_open(), &|_| false, &no_figures(), Some(0), true, ROOM, Some(0));
        assert_eq!(warm[0].lines[0][0].1.fg, Some(ink::text()), "the band starts on the name");
        assert_eq!(
            warm[0].lines[0].last().expect("a name").1.fg,
            Some(ink::dim()),
            "the thought's own ramp"
        );

        // The call it asked for, with nothing back yet: the blue one, resting at the
        // cooled grey rather than the warm one.
        let tool = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"Reading the loader first. Then the patch."}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"ls"}}]}}"#,
        ]);
        // `writing` is still the thought: the worker streams a thought, then asks for
        // the call, and the row it was writing is not closed until the answer starts.
        let cold = pieces(&tool, &all_open(), &|_| false, &no_figures(), Some(0), true, ROOM, Some(0));
        assert_eq!(cold[0].lines[0][0].1.fg, Some(ink::running_dim()), "the name cools with the band");
        // And that the step is live at all: the dot blinks, which a step the
        // frontend could not see being written was not doing.
        assert_eq!(cold[0].lead_style.fg, Some(blink(true)));

        // The band itself is the blue one, and only the band: a quarter of a cycle in,
        // the highlight is centred on the first character of the name.
        let lit = pieces(&tool, &all_open(), &|_| false, &no_figures(), Some(0), true, ROOM, Some(SHIMMER));
        let runs = &lit[0].lines[0];
        assert_eq!(runs[0].1.fg, Some(ink::running()), "the band is not at the blue");
        assert_eq!(
            runs.last().expect("a name").1.fg,
            Some(ink::running_dim()),
            "the band lit the whole name"
        );

        // And it sets past the right before the cycle takes it round again: the frame
        // before the wrap is the name's own colour, which is what keeps the pass from
        // snapping the band back onto the left edge.
        let name = lit[0].text().chars().count();
        let setting = pieces(
            &tool,
            &all_open(),
            &|_| false,
            &no_figures(),
            Some(0),
            true,
            ROOM,
            Some(name + 2 * SHIMMER - 1),
        );
        assert!(
            setting[0].lines[0].iter().all(|(_, s)| s.fg == Some(ink::running_dim())),
            "the band is still on the name when the cycle ends: {:?}",
            setting[0].lines[0]
        );

        // A call that has reported, with no thought after it yet: the wait between the
        // tool and the model. Still the tools' blue, because the worker is still in
        // them rather than thinking.
        let back = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"Reading the loader first. Then the patch."}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"ls"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0,"stdout":"a\n"}}}}"#,
        ]);
        let wait = pieces(&back, &all_open(), &|_| false, &no_figures(), Some(0), true, ROOM, Some(SHIMMER));
        assert_eq!(
            wait[0].lines[0][0].1.fg,
            Some(ink::running()),
            "the wait after a tool is not blue"
        );

        // Nothing in flight and nothing streaming: no sweep at all, and the name is
        // the white it always was.
        let idle = pieces(&tool, &all_open(), &|_| false, &no_figures(), None, true, ROOM, Some(40));
        assert_eq!(idle[0].lines[0][0].1.fg, Some(ink::text()));
    }

    /// Not an assertion: what a frame costs on a long conversation, so the clock the
    /// loop runs on is a decision rather than a guess. `QUIK_BENCH_ROUNDS` is how long
    /// the conversation is, four rows a round. Run with
    ///   cargo test -p cuacode-quik -- --ignored --nocapture measure_a_frame
    #[test]
    #[ignore]
    fn measure_a_frame() {
        let rounds: usize = std::env::var("QUIK_BENCH_ROUNDS")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(60);
        let mut log = Vec::new();
        for i in 0..rounds {
            log.push(format!(
                r#"{{"type":"token","id":"a","data":{{"state":"thinking","token":"Round {i}. Looking at the loader before the patch, because the last one was the wrong file and this one is the right one."}}}}"#
            ));
            log.push(format!(
                r#"{{"type":"token","id":"a","data":{{"state":"tool_calls","token":[{{"name":"shell","args":{{"command":"grep -n \"match(\" lumen/router.py"}}}}]}}}}"#
            ));
            log.push(format!(
                r#"{{"type":"token","id":"a","data":{{"state":"tool_output","token":"shell","result":{{"result":{{"exit_code":0,"stdout":"line {i}\n"}}}}}}}}"#
            ));
            log.push(format!(
                r#"{{"type":"token","id":"a","data":{{"state":"content","token":"Answer {i}: the router compiles a pattern per segment and the tests cover one of them."}}}}"#
            ));
        }
        let lines: Vec<&str> = log.iter().map(String::as_str).collect();
        let rows = rows(&lines);
        let (width, frames) = (100usize, 60);
        for (what, open) in [("folded", false), ("opened", true)] {
            let expanded = |_: usize, _: &Row| open;
            let mut made = std::time::Duration::ZERO;
            let mut drawn = std::time::Duration::ZERO;
            let start = std::time::Instant::now();
            for phase in 0..frames {
                let at = std::time::Instant::now();
                let ps = pieces(&rows, &expanded, &|_| false, &no_figures(), Some(0), true, width, Some(phase));
                made += at.elapsed();
                let at = std::time::Instant::now();
                let _ = draw(&ps, width);
                drawn += at.elapsed();
            }
            let each = start.elapsed() / frames as u32;
            println!(
                "{:>6}: {:?} a frame at {width} columns, {} rows (pieces {:?}, draw {:?})",
                what,
                each,
                rows.len(),
                made / frames as u32,
                drawn / frames as u32
            );
        }
    }

    /// Not an assertion: prints the peer rows as a terminal would draw them, and the
    /// shimmer at four points along its sweep. Run with
    ///   cargo test -p cuacode-quik -- --ignored --nocapture show_the_peer_rows
    #[test]
    #[ignore]
    fn show_the_peer_rows() {
        let log = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"user","token":"check function_a for me"}}"#,
            r#"{"type":"token","id":"b","data":{"state":"peer","token":"peer message from work: clearing rust code (20261014-123418-7044)\nfunction_a looks fine on this worktree. I would change line 11."}}"#,
            r#"{"type":"token","id":"c","data":{"state":"notice","token":"recall: 2 memories outside this scope"}}"#,
            r#"{"type":"token","id":"d","data":{"state":"peer","token":"you to work: building a\nis function_a valid in main.py?"}}"#,
        ]);
        for (i, row) in log.iter().enumerate() {
            let open = |j: usize, _: &Row| j == i;
            let ps = pieces(
                std::slice::from_ref(row),
                &open,
                &|_| true,
                &no_figures(),
                None,
                true,
                68,
                None,
            );
            for line in draw(&ps, 68).lines {
                let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                println!("{i} |{}|", text.trim_end());
            }
        }
        println!();
        println!(
            "the sweep, on the peer body, at four moments. Each cell is its red channel:\n'ea' is the bright end, 'a8' the row's own dim, and the run between them is the ramp."
        );
        let msg = std::slice::from_ref(&log[1]);
        for (n, phase) in [0usize, 9, 18, 27].into_iter().enumerate() {
            let ps = pieces(msg, &all_open(), &|_| true, &no_figures(), None, true, 68, Some(phase));
            // Every cell as the red channel it will be painted, so the falloff is visible
            // in a plain pipe: a block would be a run of one value beside a run of another.
            let d = draw(&ps, 68);
            let body = d
                .lines
                .iter()
                .skip(1)
                .map(|l| {
                    l.spans
                        .iter()
                        .flat_map(|s| {
                            let cell = match s.style.fg {
                                Some(Color::Rgb(r, _, _)) => format!("{r:02x}"),
                                other => format!("{other:?}"),
                            };
                            std::iter::repeat_n(cell, s.content.chars().count())
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join("\n    ");
            println!("  t{:<2} {}", n, body.trim_end());
        }
        println!();
        println!("the status line, swept at the same four moments (quiet 9a -> dim a8):");
        let said = " running  \u{b7}  ctx 12k/200k  \u{b7}  waiting on work: clearing rust code";
        for (n, phase) in [0usize, 9, 18, 27].into_iter().enumerate() {
            let line = band(said, phase);
            let reds: Vec<String> = line
                .spans
                .iter()
                .flat_map(|s| {
                    let cell = match s.style.fg {
                        Some(Color::Rgb(r, _, _)) => format!("{r:02x}"),
                        other => format!("{other:?}"),
                    };
                    std::iter::repeat_n(cell, s.content.chars().count())
                })
                .collect();
            println!("  t{:<2} {}", n, reds.join(" "));
        }
        // The text is the same text, whatever the highlight is doing to its colour.
        let plain: String = band(said, 0)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(plain, said, "a sweep recolours, it does not rewrite");
    }

    #[test]
    fn a_peer_message_is_drawn_apart_from_a_note_and_from_the_person() {
        // The buddy mark, in its own colour, with the head naming both ends. A tick or
        // a cross would say the message finished or failed, and neither is true of words.
        let msg = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"peer","token":"peer message from work: building a (20261014-123501-046f)\nfunction_a looks valid"}}"#,
        ]);
        let ps = pieces_at(&msg, &all_open(), &no_figures(), None, true);
        assert_eq!(ps[0].lead, "\u{2726} ", "the buddy leads it");
        assert_eq!(
            ps[0].lead_style.fg,
            Some(ink::peer()),
            "in the peer colour, not a state's"
        );
        assert!(
            ps[0]
                .text()
                .starts_with("peer message from work: building a")
        );
        assert_eq!(ps.len(), 2, "the body hangs under the head");
        assert_eq!(ps[1].text(), "function_a looks valid");

        // Folded, it is the head alone: the same shape a thought folds into.
        let kept = pieces_at(&msg, &|_, _| false, &no_figures(), None, true);
        assert_eq!(kept.len(), 1);
        assert!(foldable(&msg[0]), "a peer message has a body to open");

        // And it is not the user's own line, which is the whole point of the row.
        assert!(!ps[0].text().contains("\u{276f}"));
        assert_ne!(ps[0].lead_style.bg, Some(ink::bar()));
    }

    #[test]
    fn a_peer_message_is_swept_once_and_then_left_alone() {
        let msg = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"peer","token":"peer message from work: building a\nfunction_a looks valid"}}"#,
        ]);
        let body = |phase: Option<usize>| {
            pieces(&msg, &all_open(), &|_| true, &no_figures(), None, true, ROOM, phase)[1]
                .lines
                .clone()
        };
        // Characters, not runs: two neighbours sharing a colour are merged into one
        // run, so counting runs would be counting how the text happened to split.
        let lit = |lines: &Vec<Vec<Run>>| {
            lines
                .iter()
                .flatten()
                .filter(|(_, st)| st.fg == Some(ink::text()))
                .map(|(t, _)| t.chars().count())
                .sum::<usize>()
        };
        let all = |lines: &Vec<Vec<Run>>| {
            lines
                .iter()
                .flatten()
                .map(|(t, _)| t.chars().count())
                .sum::<usize>()
        };
        let text = |lines: &Vec<Vec<Run>>| {
            lines
                .iter()
                .map(|l| l.iter().map(|(t, _)| t.as_str()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Two phases where the highlight is over a character rather than off the end: a
        // pass starts a whole ramp off the left of the words, so the first phases after
        // the wrap light nothing at all. A stride per step, so these land two characters
        // apart on the first line.
        let a = body(Some(SHIMMER));
        let b = body(Some(SHIMMER + 4));
        assert!(lit(&a) > 0 && lit(&b) > 0, "the band is lit at both phases");
        assert!(lit(&a) < all(&a), "and the rest of the line is not");
        // The band travels rather than the same cells blinking.
        let where_lit = |lines: &Vec<Vec<Run>>| {
            let mut at = 0usize;
            let mut out = Vec::new();
            for (t, st) in lines.iter().flatten() {
                if st.fg == Some(ink::text()) {
                    out.push(at..at + t.chars().count());
                }
                at += t.chars().count();
            }
            out
        };
        assert_ne!(
            where_lit(&a),
            where_lit(&b),
            "the band moves along the line"
        );
        // The words are untouched: a shimmer recolours, it does not rewrite.
        assert_eq!(text(&a), text(&b));
        assert!(text(&a).contains("function_a looks valid"));

        // Nothing to sweep: every cell is the settled colour, nothing lit and nothing
        // lost. This is the frame every second after the first one draws.
        let still = body(None);
        assert_eq!(lit(&still), 0, "no highlight once the sweep is over");
        assert_eq!(all(&still), all(&a), "and no character went with it");
        assert_eq!(text(&still), text(&a), "the same words, in the same order");
    }

    #[test]
    fn a_sent_message_is_drawn_the_same_way() {
        let sent = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"peer","token":"you to work: building a\nwhat is the word?"}}"#,
        ]);
        let ps = pieces_at(&sent, &all_open(), &no_figures(), None, true);
        assert_eq!(ps[0].lead, "\u{2726} ", "the same mark as a received one");
        assert!(ps[0].text().starts_with("you to work: building a"));
        assert_eq!(ps[1].text(), "what is the word?");
    }

    /// The worker's small model names the step, and its name is what the row says.
    /// The thought's own first words are only the stand-in until it arrives.
    /// A call's row does not spend its columns on the directory it ran in.
    #[test]
    fn a_call_result_leaves_out_the_directory_it_ran_in() {
        assert_eq!(
            call_result(r#"{"command":"wc -l x"}"#, "bytes 30 \u{b7} cwd /Users/me/work/thing \u{b7} 0.4s"),
            "bytes 30 \u{b7} 0.4s"
        );
    }

    /// The band is a band: a few characters brighter than the line they cross, and
    /// the rest of the line its own colour. A shimmer that recoloured everything
    /// would be a colour change, not a highlight moving.
    #[test]
    fn the_band_lights_a_few_characters_and_leaves_the_rest() {
        let line = shimmer_runs(
            vec![vec![("the status line".to_string(), Style::new().fg(ink::quiet()))]],
            ink::quiet(),
            ink::text(),
            Motion::Pass,
            2,
        )
        .pop()
        .unwrap_or_default();
        let colours: Vec<Color> = line.iter().filter_map(|(_, s)| s.fg).collect();
        assert!(colours.iter().any(|c| *c == ink::quiet()), "{colours:?}");
        assert!(colours.iter().any(|c| *c != ink::quiet()), "{colours:?}");
    }

    #[test]
    fn a_named_step_says_the_name_the_worker_gave_it() {
        let log = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"The router needs pattern segments. Let me see how the app calls it first."}}"#,
            r#"{"type":"token","id":"a","data":{"state":"step_name","token":"Tracing router callers","of":"The router needs pattern"}}"#,
        ]);
        let folded = |_: usize, _: &Row| false;
        let shut = pieces_at(&log, &folded, &no_figures(), None, true);
        assert_eq!(shut[0].text(), "Tracing router callers");
        // Opened, the thought behind the name is still there to read.
        let open = pieces_at(&log, &all_open(), &no_figures(), None, true);
        assert_eq!(open[0].text(), "Tracing router callers");
        assert!(open[1].text().starts_with("The router needs pattern"), "{open:?}");
    }

    #[test]
    fn folding_keeps_the_head_and_drops_the_detail() {
        let thought =
            rows(&[r#"{"type":"token","id":"a","data":{"state":"thinking","token":"one\ntwo"}}"#]);
        // The head, and the whole thought as one piece: the corner marks the block
        // rather than every line of it.
        let open = pieces_at(&thought, &all_open(), &no_figures(), None, true);
        assert_eq!(open.len(), 2);
        assert_eq!(open[1].text(), "one\ntwo");
        let kept = pieces_at(&thought, &|_, _| false, &no_figures(), None, true);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].text(), "one");
    }

    #[test]
    fn the_bar_under_what_you_typed_spans_the_width() {
        let short = rows(&[r#"{"type":"token","id":"a","data":{"state":"user","token":"hi"}}"#]);
        let drawn = draw(
            &pieces_at(&short, &all_open(), &no_figures(), None, true),
            20,
        );
        assert_eq!(drawn.lines[0].width(), 20);
        assert_eq!(drawn.spans[0], (0, 1));

        // A line longer than the screen wraps onto the next one instead of
        // running off the edge, and every line of the bar is still a bar.
        let long = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"user","token":"a line far too long to fit in twenty columns"}}"#,
        ]);
        let drawn = draw(
            &pieces_at(&long, &all_open(), &no_figures(), None, true),
            20,
        );
        assert!(drawn.lines.len() > 1);
        assert!(
            drawn.lines.iter().all(|l| l.width() == 20),
            "{:?}",
            drawn.lines
        );
    }

    #[test]
    fn a_wrapped_detail_line_starts_under_its_own_text() {
        let thought = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"alpha bravo. charlie delta echo foxtrot golf"}}"#,
        ]);
        let drawn = draw(
            &pieces_at(&thought, &all_open(), &no_figures(), None, true),
            24,
        );
        assert!(drawn.lines.len() > 3, "{:?}", drawn.lines);
        assert!(
            drawn.lines.iter().all(|l| l.width() <= 24),
            "{:?}",
            drawn.lines
        );
        // A railed piece keeps its rail on every line it takes, including the ones
        // it wrapped to: a paragraph that stops looking quoted halfway down is a
        // paragraph you read as prose.
        for line in &drawn.lines[1..] {
            assert_eq!(line.spans[0].content.as_ref(), "  \u{2502} ", "{:?}", drawn.lines);
        }
    }

    /// A row showing its detail is spaced from what comes next and not from
    /// itself: the head and the thought under it are one thing, joined by the
    /// corner, and the gap belongs after the last line of it.
    #[test]
    fn an_open_detail_hangs_off_its_head_and_stands_off_the_next_row() {
        let stream = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"thinking","token":"one two"}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"date"}}]}}"#,
        ]);
        let drawn = draw(
            &pieces_at(&stream, &all_open(), &no_figures(), None, true),
            40,
        );
        let screen: Vec<String> = drawn.lines.iter().map(Line::to_string).collect();
        assert_eq!(
            screen,
            // One call, still running: nothing to say under the head yet, and
            // the call itself is the line under it.
            vec!["\u{25cf} one two", "  \u{25cf} shell(date)"]
        );
    }

    /// A file tool's panel draws its document once. The row already said the path,
    /// and the panel used to say it again as a bare `path  ...` line beside the
    /// box holding the same file's content: one shape for the thing, not two.
    #[test]
    fn a_file_calls_document_is_drawn_once() {
        let data = serde_json::json!({
            "name": "file", "index": 2, "total": 5,
            "args": {"action": "read", "path": "a.py"},
            "result": {"result": {"path": "/tmp/a.py", "content": "one\ntwo\n", "lines": 2, "truncated": false}}
        });
        let panel = call_detail(&data, 60);
        let text: String = panel
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            panel.title.contains("a.py"),
            "the head says the path: {}",
            panel.title
        );
        assert_eq!(
            text.matches('\u{250c}').count(),
            1,
            "one document, one box: {text}"
        );
        assert!(
            !text.contains("path"),
            "and the path is not said again: {text}"
        );
        assert!(
            !text.contains("truncated"),
            "a flag that is off is not a fact: {text}"
        );
        assert!(
            !text.contains("lines  "),
            "the box's own title counts the lines: {text}"
        );
        assert!(
            !text.contains("result  "),
            "the tool's wrapper is not a field: {text}"
        );
    }

    /// The answer is what the run was for rather than a step through it, so it is
    /// given a blank line on both sides -- calls above and below it, each with its
    /// own short result, which would otherwise read as one column of steps.
    #[test]
    fn the_answer_stands_off_the_calls_around_it() {
        let stream = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"date"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0,"stdout":"Friday\n"}}}}"#,
            r#"{"type":"token","id":"a","data":{"state":"content","token":"it is Friday"}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"ls"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0,"stdout":"notes.txt\n"}}}}"#,
        ]);
        let nothing_open = |_: usize, _: &Row| false;
        let drawn = draw(
            &pieces_at(&stream, &nothing_open, &no_figures(), None, true),
            40,
        );
        let screen: Vec<String> = drawn.lines.iter().map(Line::to_string).collect();
        assert_eq!(
            screen,
            vec![
                "\u{25cf} shell(date)",
                "  \u{2514} stdout:7B \u{b7} +1",
                "",
                "\u{25cf} it is Friday",
                "",
                "\u{25cf} shell(ls)",
                "  \u{2514} stdout:10B \u{b7} +1",
            ]
        );

        // Two calls with no answer between them are one step: the verbs and the
        // count on one line, and the two results behind it when it is opened.
        let calls = rows(&[
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"date"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0,"stdout":"Friday\n"}}}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"ls"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":0,"stdout":"notes.txt\n"}}}}"#,
        ]);
        let drawn = draw(
            &pieces_at(&calls, &nothing_open, &no_figures(), None, true),
            40,
        );
        let screen: Vec<String> = drawn.lines.iter().map(Line::to_string).collect();
        assert_eq!(screen, vec!["\u{25cf} shell \u{d7}2"]);
    }
}
