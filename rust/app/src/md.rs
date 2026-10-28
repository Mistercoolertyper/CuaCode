//! Markdown, parsed into the blocks and spans a window can actually draw.
//!
//! The transcript used to be one `Div` of raw text, so an answer arrived wearing
//! its own syntax: `**bold**` drawn as four asterisks and a word, `` `code` ``
//! drawn with its backticks. That is the "text formatting is shit" this exists to
//! fix, and it is not a renderer that was missing - it is that the model writes
//! markdown and the window was showing the source.
//!
//! What is here is the subset a model actually writes, and no more: paragraphs,
//! headings, list items, fenced code, quotes, rules, and the inline spans that
//! carry meaning (**bold**, *italic*, `code`, ~~strike~~, [links](url)).
//!
//! Two decisions worth naming:
//!
//! - **A single newline is a line break**, which is what `remark-breaks` does in
//!   t3code and what a chat needs: a model that writes three lines on three lines
//!   meant three lines. Joining them into one paragraph reflows them away.
//! - **Nothing is invented.** A fence with no closing fence is still a code
//!   block, because a streaming answer is an unterminated fence most of the time
//!   it exists. A `**` with no closer stays literal text rather than swallowing
//!   the rest of the answer as bold.
//!
//! `parse` is pure and has a self-check at the bottom: run
//! `cargo test -p cuacode-app md::` to fail loudly if the scanner breaks.

/// How a run of text is said, not what it says. Every flag is a face, and a
/// span carries all of them because markdown nests (`**a *b* c**`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    /// A code span: mono, and drawn as a chip by the view.
    pub code: bool,
    /// The label of a link. The address is not drawn (see `Span::url`).
    pub link: bool,
    /// A footnote reference: `[^1]` in the prose, drawn as the mark at the foot.
    pub note: bool,
}

/// A stretch of text with one style. `Break` is a line break inside a block, kept
/// as a span so the view can force a wrap in the middle of a flex row.
#[derive(Clone, PartialEq, Debug)]
pub enum Span {
    Text { text: String, style: Style },
    Break,
}

/// One block of a document. Blocks are what a paragraph is: the thing that gets
/// its own vertical space.
#[derive(Clone, PartialEq, Debug)]
pub enum Block {
    /// A paragraph, or several lines of one: the breaks inside are `Span::Break`.
    Para(Vec<Span>),
    /// `#` through `######`. The level is kept because a size per level is a
    /// heading hierarchy, and flattening it makes every heading a bold line.
    Head { level: u8, spans: Vec<Span> },
    /// A list item. `depth` is the nesting, `mark` is what leads it (`•` for a
    /// bullet, `3.` for an ordered one), and the text follows the mark.
    Item { depth: usize, mark: String, spans: Vec<Span> },
    /// A fenced block. `lang` is the info string, drawn as a quiet label.
    Code { lang: Option<String>, text: String },
    /// A `>` quote, which is one block however many lines it spans.
    Quote(Vec<Span>),
    /// `---`: a horizontal rule.
    Rule,
    /// A GFM table: the head row, the body rows, and one alignment per column.
    ///
    /// A model writes tables constantly, and drawn as prose they were unreadable:
    /// the pipes stayed in the text and every column ran together.
    Table { head: Vec<Vec<Span>>, rows: Vec<Vec<Vec<Span>>>, aligns: Vec<Align> },
    /// The footnote definitions, collected to the foot of the document, which is
    /// where a footnote lives (`index.css:2090`-`:2096`).
    Footnotes(Vec<Footnote>),
    /// `<details><summary>..</summary>..</details>`: a disclosure with a summary
    /// row and a body of its own blocks.
    Details { summary: String, body: Vec<Block> },
}

/// One column's alignment, from its `:---:` marker (`index.css:2213`-`:2218`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// A footnote: its mark and its text.
#[derive(Clone, PartialEq, Debug)]
pub struct Footnote {
    pub id: String,
    pub spans: Vec<Span>,
}

/// A document: the blocks, in order.
pub fn parse(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    // Footnote definitions are lifted out of the flow and set at the foot, one
    // block, in the order they were defined.
    let mut notes: Vec<Footnote> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // `<details>` opens a disclosure: its summary row, then its body parsed as
        // the markdown it is. A model writes these for a long aside, and dropped they
        // were the tags in the prose with the text loose under them.
        if trimmed.starts_with("<details") {
            let mut summary = String::new();
            let mut body: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() {
                let next = lines[i].trim();
                if next.starts_with("</details") {
                    i += 1;
                    break;
                }
                if let Some(rest) = next.strip_prefix("<summary>") {
                    summary = rest.trim_end_matches("</summary>").trim().to_string();
                } else if !next.is_empty() {
                    body.push(next);
                }
                i += 1;
            }
            // Their fallback for a missing summary is the literal `Details`
            // (`MarkdownDetails.tsx:30`-`:33`).
            if summary.is_empty() {
                summary = "Details".into();
            }
            out.push(Block::Details { summary, body: parse(&body.join("\n")) });
            continue;
        }

        // A fence opens a code block and runs until the closing fence, or to the
        // end of the answer: a streaming model is mid-fence most of the time it
        // has one open, and dropping the block until it closes loses the code.
        if let Some(rest) = fence(trimmed) {
            let lang = rest.trim();
            let mut body: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() {
                if fence(lines[i].trim()).is_some() {
                    i += 1;
                    break;
                }
                body.push(lines[i]);
                i += 1;
            }
            out.push(Block::Code {
                lang: (!lang.is_empty()).then(|| lang.to_string()),
                text: body.join("\n"),
            });
            continue;
        }

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if is_rule(trimmed) {
            out.push(Block::Rule);
            i += 1;
            continue;
        }

        // `[^1]: text` is a definition, not a paragraph: it is collected and drawn
        // at the foot of the document.
        if let Some((id, rest)) = footnote_def(trimmed) {
            let mut body = vec![rest];
            i += 1;
            // An indented continuation line belongs to the note, the way markdown
            // folds a wrapped definition.
            while i < lines.len() {
                let next = lines[i];
                if next.starts_with("  ") && !next.trim().is_empty() && footnote_def(next.trim()).is_none() {
                    body.push(next.trim());
                    i += 1;
                } else {
                    break;
                }
            }
            notes.push(Footnote { id, spans: lines_of(&body) });
            continue;
        }

        if let Some((level, rest)) = heading(trimmed) {
            out.push(Block::Head { level, spans: inline(rest) });
            i += 1;
            continue;
        }

        // A quote is every consecutive `>` line, as one block.
        if let Some(rest) = trimmed.strip_prefix('>') {
            let mut body = vec![rest.strip_prefix(' ').unwrap_or(rest)];
            i += 1;
            while i < lines.len() {
                match lines[i].trim().strip_prefix('>') {
                    Some(next) => {
                        body.push(next.strip_prefix(' ').unwrap_or(next));
                        i += 1;
                    }
                    None => break,
                }
            }
            out.push(Block::Quote(lines_of(&body)));
            continue;
        }

        // A table: a row of cells over a delimiter row. Before the list and the
        // paragraph, because `| a | b |` is neither.
        if let Some((block, next)) = table(&lines, i) {
            out.push(block);
            i = next;
            continue;
        }

        if let Some((depth, mark, rest)) = item(line) {
            out.push(Block::Item { depth, mark, spans: inline(rest) });
            i += 1;
            continue;
        }

        // Everything else is a paragraph, and it runs until a blank line or a
        // line that is some other block. Single newlines inside it are kept.
        let mut body: Vec<&str> = vec![line.trim_end()];
        i += 1;
        while i < lines.len() {
            let next = lines[i];
            let t = next.trim();
            if t.is_empty()
                || fence(t).is_some()
                || is_rule(t)
                || heading(t).is_some()
                || t.starts_with('>')
                || item(next).is_some()
                || footnote_def(t).is_some()
                || table(&lines, i).is_some()
            {
                break;
            }
            body.push(next.trim_end());
            i += 1;
        }
        out.push(Block::Para(lines_of(&body)));
    }
    if !notes.is_empty() {
        out.push(Block::Footnotes(notes));
    }
    out
}

/// Several source lines as one run of spans: the text, with a `Break` between.
fn lines_of(lines: &[&str]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        if n > 0 {
            spans.push(Span::Break);
        }
        spans.extend(inline(line));
    }
    spans
}

/// ```` ```lang ```` or `~~~`, and the info string after it.
fn fence(line: &str) -> Option<&str> {
    for marker in ["```", "~~~"] {
        if let Some(rest) = line.strip_prefix(marker) {
            // The whole line is the fence: ``` followed by more backticks only.
            return Some(rest);
        }
    }
    None
}

/// `#` through `######`, with the level and the text after it.
fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    // `#tag` is not a heading, and neither is a bare `#`.
    if !rest.starts_with(' ') {
        return None;
    }
    Some((hashes as u8, rest.trim_start()))
}

/// `[^1]: the note`, with the mark and the text after it.
///
/// The mark is kept as written (`1`, `a`, `note`) because a footnote's mark is what
/// the reference in the prose points at, and renumbering it would break that link.
fn footnote_def(line: &str) -> Option<(String, &str)> {
    let rest = line.strip_prefix("[^")?;
    let end = rest.find("]:")?;
    let id = rest[..end].to_string();
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    Some((id, rest[end + 2..].trim_start()))
}

/// A GFM table at `i`, and where the line after it starts.
///
/// The shape is a head row over a **delimiter row** - `|---|:--:|---:|` - and the
/// delimiter is what makes it a table rather than a paragraph with pipes in it. The
/// columns' alignment comes from that row too (`index.css:2213`-`:2218`).
fn table(lines: &[&str], i: usize) -> Option<(Block, usize)> {
    let head_line = *lines.get(i)?;
    if !head_line.contains('|') {
        return None;
    }
    let aligns = delimiters(lines.get(i + 1)?)?;
    let head: Vec<Vec<Span>> = cells(head_line).iter().map(|c| inline(c)).collect();
    // A head with no cells is not a table: `|` alone is a character.
    if head.iter().all(|c| c.is_empty()) {
        return None;
    }
    let mut rows: Vec<Vec<Vec<Span>>> = Vec::new();
    let mut at = i + 2;
    while let Some(line) = lines.get(at) {
        if line.trim().is_empty() || !line.contains('|') {
            break;
        }
        let mut row: Vec<Vec<Span>> = cells(line).iter().map(|c| inline(c)).collect();
        // Every row is as wide as the head: a short row is padded rather than
        // making the columns jump on one line.
        while row.len() < head.len() {
            row.push(Vec::new());
        }
        row.truncate(head.len());
        rows.push(row);
        at += 1;
    }
    let mut aligns = aligns;
    aligns.resize(head.len(), Align::Start);
    Some((Block::Table { head, rows, aligns }, at))
}

/// The cells of one row, without the outer pipes.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// The alignments a delimiter row declares, or `None` if the line is not one.
///
/// Every cell must be dashes with optional colons, so `| a | 1 | b |` - which also
/// has pipes - is not mistaken for the row under a head.
fn delimiters(line: &str) -> Option<Vec<Align>> {
    if !line.contains('|') {
        return None;
    }
    let mut out = Vec::new();
    for cell in cells(line) {
        let c = cell.trim();
        if c.is_empty() {
            return None;
        }
        let body = c.strip_prefix(':').unwrap_or(c);
        let body = body.strip_suffix(':').unwrap_or(body);
        if body.is_empty() || !body.chars().all(|ch| ch == '-') {
            return None;
        }
        out.push(match (c.starts_with(':'), c.ends_with(':')) {
            (true, true) => Align::Center,
            (false, true) => Align::End,
            _ => Align::Start,
        });
    }
    (!out.is_empty()).then_some(out)
}

/// Three or more of `-`, `*` or `_`, and nothing else on the line.
fn is_rule(line: &str) -> bool {
    let mut marker = None;
    let mut count = 0;
    for c in line.chars() {
        if c == ' ' {
            continue;
        }
        if !matches!(c, '-' | '*' | '_') {
            return false;
        }
        match marker {
            Some(m) if m != c => return false,
            None => marker = Some(c),
            _ => {}
        }
        count += 1;
    }
    count >= 3
}

/// The bullet a level of nesting wears: their `ul` is a disc, `ul ul` a circle and
/// `ul ul ul` a square (`index.css:1994`-`:1998`). Depth is capped at three by
/// `item`, so the square repeats rather than running off the end.
const BULLETS: [&str; 4] = ["\u{2022}", "\u{25e6}", "\u{25aa}", "\u{25aa}"];

/// A list item: its nesting, what leads it, and its text.
///
/// **Nesting changes the marker, not the indent** (`index.css:1922`-`:1944`; the
/// comment over the `ul` rule says the gutter is reset on purpose so nested lists
/// do not walk right). The indent is the view's: one `SPACE_LG` gutter per item.
fn item(line: &str) -> Option<(usize, String, &str)> {
    let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
    // Four spaces is one level in markdown; two is what a model actually writes.
    let depth = (indent / 2).min(3);
    let rest = &line[indent..];

    for bullet in ["- ", "* ", "+ "] {
        if let Some(text) = rest.strip_prefix(bullet) {
            return Some((depth, BULLETS[depth].to_string(), text.trim_start()));
        }
    }
    // `1. ` and `1) `, and the number is kept: a list that counts is a list whose
    // order is part of what it says. A nested list counts in letters and then in
    // roman numerals - `ol ol` is lower-alpha and `ol ol ol` lower-roman
    // (`index.css:2002`, `:2006`) - and it counts from the number the source gave.
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let after = &rest[digits..];
        let n: u32 = rest[..digits].parse().unwrap_or(1);
        for mark in [". ", ") "] {
            if let Some(text) = after.strip_prefix(mark) {
                let lead = match depth {
                    0 => n.to_string(),
                    1 => alpha(n),
                    _ => roman(n),
                };
                return Some((depth, format!("{lead}."), text.trim_start()));
            }
        }
    }
    None
}

/// `1` as `a`, `26` as `z`, and past that as `aa`, `ab`: the marker a list one
/// level in wears.
fn alpha(n: u32) -> String {
    let mut out = String::new();
    let mut n = n.max(1);
    while n > 0 {
        let r = (n - 1) % 26;
        out.insert(0, (b'a' + r as u8) as char);
        n = (n - 1) / 26;
    }
    out
}

/// `1` as `i` and `4` as `iv`: the marker a list two levels in wears. Past 3999
/// it stays decimal rather than growing a bar over the glyph.
fn roman(n: u32) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    const RUNGS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    let mut n = n;
    for (value, glyph) in RUNGS {
        while n >= value {
            out.push_str(glyph);
            n -= value;
        }
    }
    out
}

/// The inline spans of one run of text: the emphasis, the code, and the links.
///
/// A marker with no closer is left as the text it is. That is the honest
/// fallback: a half-typed `**` in a streaming answer is two asterisks, and
/// treating it as a style would bold the rest of the answer while it arrives.
pub fn inline(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut plain = String::new();
    let mut i = 0;

    // A plain run is flushed as one span, so the view draws one item per word
    // rather than one per character.
    macro_rules! flush {
        () => {
            if !plain.is_empty() {
                out.push(Span::Text { text: std::mem::take(&mut plain), style: Style::default() });
            }
        };
    }

    while i < chars.len() {
        let c = chars[i];

        // `code`: everything to the next backtick, and nothing else applies
        // inside it, which is the whole point of a code span.
        if c == '`' {
            if let Some(end) = find(&chars, i + 1, '`') {
                flush!();
                let inner: String = chars[i + 1..end].iter().collect();
                out.push(Span::Text {
                    text: inner.trim_matches(' ').to_string(),
                    style: Style { code: true, ..Default::default() },
                });
                i = end + 1;
                continue;
            }
        }

        if c == '~' && peek(&chars, i + 1) == Some('~') {
            if let Some(end) = find_pair(&chars, i + 2, '~', '~') {
                flush!();
                let inner: String = chars[i + 2..end].iter().collect();
                out.push(Span::Text {
                    text: inner,
                    style: Style { strike: true, ..Default::default() },
                });
                i = end + 2;
                continue;
            }
        }

        if c == '[' {
            // A footnote reference is `[^1]`, and it is not a link: there is no
            // address after it. The mark is drawn where it stands, small, and the
            // note is at the foot of the document.
            if peek(&chars, i + 1) == Some('^') {
                if let Some(end) = find(&chars, i + 2, ']') {
                    let id: String = chars[i + 2..end].iter().collect();
                    if !id.is_empty() && !id.contains(char::is_whitespace) {
                        flush!();
                        out.push(Span::Text {
                            text: id,
                            style: Style { note: true, ..Default::default() },
                        });
                        i = end + 1;
                        continue;
                    }
                }
            }
            if let Some((label, url, next)) = link(&chars, i) {
                flush!();
                out.push(Span::Text {
                    text: label,
                    style: Style { link: true, ..Default::default() },
                });
                // The address is not drawn: a url under a label is noise in a
                // sentence. It is kept alive here so a click can be wired later.
                let _ = url;
                i = next;
                continue;
            }
        }

        if c == '*' || c == '_' {
            // A marker is only a marker at a word boundary, so `snake_case` is
            // one word and not an italic `case`.
            let boundary = i == 0
                || chars[i - 1].is_whitespace()
                || matches!(chars[i - 1], '(' | '[' | '"' | '\'' | '-' | '>');
            if boundary {
                let double = peek(&chars, i + 1) == Some(c);
                let at = if double { i + 2 } else { i + 1 };
                if let Some(end) = find_emphasis(&chars, at, c, double) {
                    flush!();
                    let inner: Vec<char> = chars[at..end].to_vec();
                    let style = Style {
                        bold: double,
                        italic: !double,
                        ..Default::default()
                    };
                    // The inside can carry its own spans (`**a `b` c**`), so it
                    // is parsed and the style folded over what it found.
                    for span in inline(&inner.iter().collect::<String>()) {
                        out.push(match span {
                            Span::Text { text, style: inner_style } => Span::Text {
                                text,
                                style: Style {
                                    bold: style.bold || inner_style.bold,
                                    italic: style.italic || inner_style.italic,
                                    ..inner_style
                                },
                            },
                            other => other,
                        });
                    }
                    i = if double { end + 2 } else { end + 1 };
                    continue;
                }
            }
        }

        plain.push(c);
        i += 1;
    }
    flush!();
    out
}

/// The next index of `c` at or after `from`.
fn find(chars: &[char], from: usize, c: char) -> Option<usize> {
    (from..chars.len()).find(|i| chars[*i] == c)
}

/// The closing pair of emphasis: `**` for a double marker, `*` for a single.
///
/// A single `*` does not close on the first byte of a `**`, or `*a**b*` would
/// end its italic one marker early and leave a stray asterisk behind.
fn find_emphasis(chars: &[char], from: usize, c: char, double: bool) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == c {
            if double {
                if peek(chars, i + 1) == Some(c) {
                    return Some(i);
                }
            } else if peek(chars, i + 1) != Some(c) {
                // The closer must be at a boundary too, so `2 * 3 * 4` is maths
                // and not an italic `3`.
                let after = peek(chars, i + 1);
                let before = if i > from { Some(chars[i - 1]) } else { None };
                if after.is_none_or(|a| a.is_whitespace() || is_punct(a))
                    && before.is_some_and(|b| !b.is_whitespace())
                {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// `**`, `~~` and friends: two of `c` in a row at or after `from`.
fn find_pair(chars: &[char], from: usize, a: char, b: char) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|i| chars[*i] == a && chars[*i + 1] == b)
}

/// A link at `i`: `[label](url)`, returning the label, the url, and where the
/// link ends.
fn link(chars: &[char], i: usize) -> Option<(String, String, usize)> {
    let close = find(chars, i + 1, ']')?;
    if peek(chars, close + 1) != Some('(') {
        return None;
    }
    let end = find(chars, close + 2, ')')?;
    let label: String = chars[i + 1..close].iter().collect();
    let url: String = chars[close + 2..end].iter().collect();
    if label.is_empty() {
        return None;
    }
    Some((label, url, end + 1))
}

fn peek(chars: &[char], i: usize) -> Option<char> {
    chars.get(i).copied()
}

/// Whether a character is punctuation that can end an italic word.
fn is_punct(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\'')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spans as `text|flags`, so a test says what it means without naming
    /// every field.
    fn flat(spans: &[Span]) -> Vec<String> {
        spans
            .iter()
            .map(|s| match s {
                Span::Break => "\\n".to_string(),
                Span::Text { text, style } => {
                    let mut flags = String::new();
                    if style.bold {
                        flags.push('b');
                    }
                    if style.italic {
                        flags.push('i');
                    }
                    if style.code {
                        flags.push('c');
                    }
                    if style.strike {
                        flags.push('s');
                    }
                    if style.link {
                        flags.push('l');
                    }
                    if style.note {
                        flags.push('n');
                    }
                    format!("{text}|{flags}")
                }
            })
            .collect()
    }

    #[test]
    fn the_markers_become_styles_and_stop_being_text() {
        assert_eq!(flat(&inline("a **b** c")), vec!["a |", "b|b", " c|"]);
        assert_eq!(flat(&inline("an *i* word")), vec!["an |", "i|i", " word|"]);
        assert_eq!(flat(&inline("a `code` span")), vec!["a |", "code|c", " span|"]);
        assert_eq!(flat(&inline("~~gone~~")), vec!["gone|s"]);
        assert_eq!(flat(&inline("[label](https://x)")), vec!["label|l"]);
    }

    #[test]
    fn a_style_nests_rather_than_being_eaten() {
        // `**bold with a `code` span**`: the code keeps its own face inside bold.
        assert_eq!(flat(&inline("**a `b` c**")), vec!["a |b", "b|bc", " c|b"]);
    }

    #[test]
    fn an_unclosed_marker_stays_text() {
        // A streaming answer is full of these, and swallowing the rest of it as
        // bold is worse than showing two asterisks.
        assert_eq!(flat(&inline("half **typed")), vec!["half **typed|"]);
        assert_eq!(flat(&inline("a `tick")), vec!["a `tick|"]);
    }

    #[test]
    fn underscores_inside_a_word_are_not_emphasis() {
        assert_eq!(flat(&inline("snake_case_name")), vec!["snake_case_name|"]);
        // ...but a word-boundary pair is.
        assert_eq!(flat(&inline("an _em_ word")), vec!["an |", "em|i", " word|"]);
    }

    #[test]
    fn a_paragraph_keeps_its_single_newlines() {
        let blocks = parse("one\ntwo");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Para(spans) => assert_eq!(flat(spans), vec!["one|", "\\n", "two|"]),
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    #[test]
    fn a_fence_that_never_closes_is_still_a_code_block() {
        let blocks = parse("text\n\n```rust\nfn main() {}\n");
        assert_eq!(
            blocks,
            vec![
                Block::Para(vec![Span::Text { text: "text".into(), style: Style::default() }]),
                Block::Code { lang: Some("rust".into()), text: "fn main() {}".into() },
            ]
        );
    }

    #[test]
    fn headings_lists_and_rules_are_their_own_blocks() {
        let blocks = parse("# Title\n\n- one\n- two\n\n1. first\n\n---\n\n> quoted");
        assert!(matches!(&blocks[0], Block::Head { level: 1, .. }));
        assert!(matches!(&blocks[1], Block::Item { mark, .. } if mark == "\u{2022}"));
        assert!(matches!(&blocks[2], Block::Item { mark, .. } if mark == "\u{2022}"));
        assert!(matches!(&blocks[3], Block::Item { mark, .. } if mark == "1."));
        assert_eq!(blocks[4], Block::Rule);
        assert!(matches!(&blocks[5], Block::Quote(_)));
    }

    #[test]
    fn nesting_changes_the_marker_and_not_the_indent() {
        // `ul` / `ul ul` / `ul ul ul` are disc, circle, square, and `ol` counts in
        // numbers, letters and then roman numerals - the *mark* is the level's,
        // the indent is the view's one gutter (`index.css:1922`-`:1944`).
        let blocks = parse("- one\n  - two\n    - three\n1. first\n  1. second\n    1. third");
        let marks: Vec<&str> = blocks
            .iter()
            .map(|b| match b {
                Block::Item { mark, .. } => mark.as_str(),
                other => panic!("expected a list item, got {other:?}"),
            })
            .collect();
        assert_eq!(marks, vec!["\u{2022}", "\u{25e6}", "\u{25aa}", "1.", "a.", "i."]);
        assert_eq!(alpha(27), "aa");
        assert_eq!(roman(4), "iv");
        assert_eq!(roman(9), "ix");
    }

    #[test]
    fn a_table_is_a_head_a_delimiter_row_and_its_body() {
        let blocks = parse("| what | value |\n|---|:--:|\n| a | 1 |\n| b | 2 |");
        assert_eq!(blocks.len(), 1, "one table, not four paragraphs: {blocks:?}");
        match &blocks[0] {
            Block::Table { head, rows, aligns } => {
                assert_eq!(head.len(), 2);
                assert_eq!(flat(&head[1]), vec!["value|"]);
                assert_eq!(rows.len(), 2);
                assert_eq!(flat(&rows[1][0]), vec!["b|"]);
                assert_eq!(*aligns, vec![Align::Start, Align::Center]);
            }
            other => panic!("expected a table, got {other:?}"),
        }
        // A row shorter than the head is padded, so the columns do not jump.
        let blocks = parse("| a | b | c |\n|---|---|---|\n| 1 |");
        match &blocks[0] {
            Block::Table { rows, .. } => assert_eq!(rows[0].len(), 3),
            other => panic!("expected a table, got {other:?}"),
        }
        // Pipes with no delimiter row under them are just text.
        assert!(matches!(&parse("a | b")[0], Block::Para(_)));
        assert!(matches!(&parse("| a | 1 |\n| b | 2 |")[0], Block::Para(_)));
    }

    #[test]
    fn a_footnote_is_lifted_to_the_foot_and_its_ref_is_a_mark() {
        let blocks = parse("a claim[^1]\n\n[^1]: the note");
        assert_eq!(flat(&inline("a claim[^1]")), vec!["a claim|", "1|n"]);
        assert_eq!(blocks.len(), 2, "the definition is not a paragraph: {blocks:?}");
        assert!(matches!(&blocks[0], Block::Para(_)));
        match &blocks[1] {
            Block::Footnotes(notes) => {
                assert_eq!(notes.len(), 1);
                assert_eq!(notes[0].id, "1");
                assert_eq!(flat(&notes[0].spans), vec!["the note|"]);
            }
            other => panic!("expected the footnotes, got {other:?}"),
        }
        // A note that wraps keeps its second line.
        match &parse("[^a]: one\n  and two")[0] {
            Block::Footnotes(notes) => assert_eq!(flat(&notes[0].spans), vec!["one|", "\\n", "and two|"]),
            other => panic!("expected the footnotes, got {other:?}"),
        }
    }

    #[test]
    fn a_hashtag_is_not_a_heading_and_a_date_is_not_a_rule() {
        assert!(heading("#tag").is_none());
        assert!(heading("#").is_none());
        assert!(!is_rule("--"), "a rule is three markers, not two");
        assert!(is_rule("---"));
        assert!(!is_rule("- not a rule"));
        assert!(!is_rule("2026-10-12"));
    }
}
