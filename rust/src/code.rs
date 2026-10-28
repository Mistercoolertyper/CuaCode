//! A line of code in a fence, read as the tones a terminal draws it in.
//!
//! A reader's convenience and not a parser: a comment, a string, a keyword, a
//! number, and the code colour for everything else. There is no grammar behind it,
//! only the shapes a line takes in the languages these transcripts are written in,
//! which is why it is this file and not a dependency.
//!
//! Two things it does carry. What the fence declared, because a `#` is a comment in
//! Python and an attribute in Rust, and `//` is floor division in Python and a
//! comment in C. And what the line before it left open, because a block comment or
//! a triple-quoted string that opens on one line is still open on the next.
//!
//! ponytail: no grammar, no scope, no cross-file knowledge. A line coloured
//! confidently wrong reads worse than a line in one colour, so anything this cannot
//! tell (a heredoc, a raw string with a hash fence, a macro body holding something
//! else entirely) is left in the code tone. The upgrade path is a real lexer with a
//! tree, behind this same `Fence` interface.

/// What a run of a line is, in the few tones a terminal tells code apart by. Which
/// colour each of these is belongs to the frontend's palette; nothing here knows one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    /// The code itself.
    Plain,
    /// What is said about the code rather than being it.
    Comment,
    /// A literal.
    String,
    /// A word the language reserves.
    Keyword,
    /// A number, taken whole.
    Number,
}

/// The comment syntaxes, grouped by what they share. A language is in exactly one,
/// and the ones this does not know are `Generic`, which reads both of the two most
/// common markers rather than guessing.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Family {
    /// Nothing said which language this is: `//`, `#` and `/* */` all read as
    /// comments, which is what a fence with no info string has always done.
    #[default]
    Generic,
    /// `//` to the end of the line, `/* */` across them: rust, c, js, go, java, css.
    Slash,
    /// `#` to the end of the line: python, sh, toml, yaml, make.
    Hash,
    /// `--` to the end of the line: sql, lua, haskell, ada.
    Dash,
    /// No comment of its own but `<!-- -->`: html, xml, markdown. Json is here for
    /// having none at all, which the same absence says correctly.
    Markup,
}

/// The family a fence's info string names: ```` ```rust title="x" ```` is `rust`,
/// and case does not matter.
fn family(info: &str) -> Family {
    let word: String = info
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_'))
        .collect::<String>()
        .to_ascii_lowercase();
    match word.as_str() {
        "rs" | "rust" | "c" | "h" | "cc" | "c++" | "cpp" | "cxx" | "hpp" | "hh" | "m" | "mm"
        | "objc" | "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "java" | "go" | "swift" | "kt"
        | "kts" | "kotlin" | "cs" | "csharp" | "zig" | "dart" | "scala" | "css" | "scss"
        | "less" | "php" | "glsl" | "wgsl" | "proto" | "graphql" => Family::Slash,
        "py" | "python" | "sh" | "bash" | "zsh" | "shell" | "console" | "rb" | "ruby" | "toml"
        | "yaml" | "yml" | "nix" | "r" | "pl" | "perl" | "make" | "makefile" | "dockerfile"
        | "ini" | "conf" | "cfg" | "env" | "ex" | "exs" | "elixir" | "cr" | "nim" | "jl"
        | "julia" | "ps1" | "powershell" | "awk" | "cmake" => Family::Hash,
        "sql" | "lua" | "hs" | "haskell" | "ada" | "vhdl" | "asm" | "elm" | "applescript" => {
            Family::Dash
        }
        "html" | "xml" | "svg" | "md" | "markdown" | "vue" | "svelte" | "json" | "json5" => {
            Family::Markup
        }
        _ => Family::Generic,
    }
}

impl Family {
    /// What opens a comment that runs to the end of the line.
    fn line_comment(self) -> &'static [&'static str] {
        match self {
            Family::Slash => &["//"],
            Family::Hash => &["#"],
            Family::Dash => &["--"],
            Family::Markup => &[],
            Family::Generic => &["//", "#"],
        }
    }

    /// What opens a comment that runs until its own closer, for the languages that
    /// have one.
    fn block_comment(self) -> Option<(&'static str, &'static str)> {
        match self {
            Family::Slash | Family::Generic => Some(("/*", "*/")),
            Family::Markup => Some(("<!--", "-->")),
            Family::Hash | Family::Dash => None,
        }
    }
}

/// A fenced block as it is being drawn, one line at a time: what the fence said it
/// was written in, and what the last line left open.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fence {
    family: Family,
    /// The closer of a comment or a string that is still open, and the tone it is in.
    open: Option<(Tone, &'static str)>,
}

impl Fence {
    /// A fence from its info string. A block with no language is `Generic`, which is
    /// what a file read's box is: the wire gives it no name to be read by.
    pub fn new(info: &str) -> Self {
        Fence {
            family: family(info),
            open: None,
        }
    }

    /// One line of it, as the runs it draws. The runs are the line's own slices, in
    /// order, and they join back into it: highlighting is a colour, not an edit.
    pub fn line<'a>(&mut self, text: &'a str) -> Vec<(&'a str, Tone)> {
        let mut out: Vec<(&'a str, Tone)> = Vec::new();
        // Where the plain run being accumulated began, which is what keeps a stretch
        // of code that is nothing in particular one run and not one per word.
        let mut plain = 0;
        let mut at = 0;
        let family = self.family;
        // What the previous line left open is read first, and it swallows everything
        // up to its closer: an unclosed one is still the shape it is.
        if let Some((tone, closer)) = self.open {
            let end = match text.find(closer) {
                Some(i) => i + closer.len(),
                None => text.len(),
            };
            if end > 0 {
                out.push((&text[..end], tone));
            }
            if end == text.len() {
                return out;
            }
            self.open = None;
            at = end;
            plain = end;
        }
        // What the previous character was, which is what says whether a digit begins a
        // number or sits inside a word.
        let mut before = text[..at].chars().next_back();
        while at < text.len() {
            let rest = &text[at..];
            let ch = rest.chars().next().expect("a position inside the text");
            // A comment runs to the end of the line and nothing after it is read.
            if line_comment(family, rest).is_some() {
                if plain < at {
                    out.push((&text[plain..at], Tone::Plain));
                }
                out.push((rest, Tone::Comment));
                return out;
            }
            // A block comment runs until its closer, or to the end of the line with
            // the block left open for the next one.
            if let Some((opener, closer)) = family.block_comment() {
                if rest.starts_with(opener) {
                    if plain < at {
                        out.push((&text[plain..at], Tone::Plain));
                    }
                    let (end, closed) = match rest[opener.len()..].find(closer) {
                        Some(i) => (opener.len() + i + closer.len(), true),
                        None => (rest.len(), false),
                    };
                    out.push((&rest[..end], Tone::Comment));
                    if !closed {
                        self.open = Some((Tone::Comment, closer));
                        return out;
                    }
                    at += end;
                    plain = at;
                    before = text[..at].chars().next_back();
                    continue;
                }
            }
            // A triple-quoted string is the one string that outlives its line: a
            // docstring, or a blob of sql in a shell script.
            if let Some(delim) = triple(rest) {
                if plain < at {
                    out.push((&text[plain..at], Tone::Plain));
                }
                match rest[3..].find(delim) {
                    Some(i) => {
                        let end = 3 + i + 3;
                        out.push((&rest[..end], Tone::String));
                        at += end;
                        plain = at;
                    }
                    None => {
                        out.push((rest, Tone::String));
                        self.open = Some((Tone::String, delim));
                        return out;
                    }
                }
                before = text[..at].chars().next_back();
                continue;
            }
            // A string is its two quotes and what is between them, or the rest of the
            // line when nothing closes it.
            if let Some(len) = quoted(rest, before) {
                if plain < at {
                    out.push((&text[plain..at], Tone::Plain));
                }
                out.push((&rest[..len], Tone::String));
                at += len;
                plain = at;
                before = text[..at].chars().next_back();
                continue;
            }
            // A number is a digit that begins one rather than sitting inside a word,
            // and it is taken whole -- `0x1f`, `1_000`, `3.5`, `2px` -- so no line is
            // left half a number.
            if ch.is_ascii_digit()
                && !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                let len = run_len(rest, |c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
                if plain < at {
                    out.push((&text[plain..at], Tone::Plain));
                }
                out.push((&rest[..len], Tone::Number));
                at += len;
                plain = at;
                before = text[..at].chars().next_back();
                continue;
            }
            // A word, in the keyword's tone when it is one of the few a reader knows
            // without knowing the language.
            if ch.is_alphabetic() || ch == '_' {
                let len = run_len(rest, |c| c.is_alphanumeric() || c == '_');
                let word = &rest[..len];
                if KEYWORDS.contains(&word) {
                    if plain < at {
                        out.push((&text[plain..at], Tone::Plain));
                    }
                    out.push((word, Tone::Keyword));
                    plain = at + len;
                }
                at += len;
                before = text[..at].chars().next_back();
                continue;
            }
            at += ch.len_utf8();
            before = Some(ch);
        }
        if plain < at {
            out.push((&text[plain..at], Tone::Plain));
        }
        out
    }
}

/// The comment marker `rest` begins with, if it begins with one.
///
/// `#[` is not a comment: it is an attribute or an array's length, which is the one
/// shape a `#` leads in the code itself. It is only an exception where nothing said
/// which language this is, because a language that comments with `#` has no use for
/// the shape.
fn line_comment(family: Family, rest: &str) -> Option<&'static str> {
    family.line_comment().iter().copied().find(|mark| {
        rest.starts_with(mark) && !(family == Family::Generic && rest.starts_with("#["))
    })
}

/// The triple-quote delimiter `rest` begins with, if it begins with one.
fn triple(rest: &str) -> Option<&'static str> {
    if rest.starts_with("\"\"\"") {
        Some("\"\"\"")
    } else if rest.starts_with("'''") {
        Some("'''")
    } else {
        None
    }
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
/// Colouring the rest of `let x: &'a str` green is exactly the confidently wrong line
/// this is not here to draw.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tones(info: &str, lines: &[&str]) -> Vec<Vec<(String, Tone)>> {
        let mut fence = Fence::new(info);
        lines
            .iter()
            .map(|line| {
                let runs = fence.line(line);
                assert_eq!(
                    runs.iter().map(|(s, _)| *s).collect::<String>(),
                    *line,
                    "highlighting is a colour, not an edit"
                );
                runs.into_iter().map(|(s, t)| (s.to_string(), t)).collect()
            })
            .collect()
    }

    #[test]
    fn the_language_a_fence_declares_decides_what_a_hash_is() {
        // In Rust `#[derive]` is code; the same line in Python is a comment.
        let rust = tones("rust", &["#[derive(Debug)]"]);
        assert_eq!(rust[0], vec![("#[derive(Debug)]".to_string(), Tone::Plain)]);
        let python = tones("py", &["# a note"]);
        assert_eq!(python[0], vec![("# a note".to_string(), Tone::Comment)]);
        // `//` is floor division in Python and a comment everywhere this knows.
        let python = tones("py", &["x = 7 // 2"]);
        assert_eq!(
            python[0],
            vec![
                ("x = ".to_string(), Tone::Plain),
                ("7".to_string(), Tone::Number),
                (" // ".to_string(), Tone::Plain),
                ("2".to_string(), Tone::Number),
            ]
        );
        // Nothing said: both markers are comments, which is what a bare fence has
        // always been drawn as.
        let bare = tones("", &["// say it"]);
        assert_eq!(bare[0], vec![("// say it".to_string(), Tone::Comment)]);
    }

    #[test]
    fn a_comment_or_a_string_left_open_carries_to_the_next_line() {
        let c = tones("c", &["int x; /* open", "still here", "done */ int y;"]);
        assert_eq!(
            c[0].last().unwrap(),
            &("/* open".to_string(), Tone::Comment)
        );
        assert_eq!(c[1], vec![("still here".to_string(), Tone::Comment)]);
        assert_eq!(
            c[2],
            vec![
                ("done */".to_string(), Tone::Comment),
                (" int y;".to_string(), Tone::Plain)
            ]
        );
        let py = tones("py", &["s = \"\"\"open", "still", "\"\"\" + x"]);
        assert_eq!(py[1], vec![("still".to_string(), Tone::String)]);
        assert_eq!(
            py[2],
            vec![
                ("\"\"\"".to_string(), Tone::String),
                (" + x".to_string(), Tone::Plain)
            ]
        );
        // A closed string on the same line closes it: nothing carries over.
        let one = tones("py", &["s = \"\"\"a\"\"\"", "x = 1"]);
        assert_eq!(
            one[1],
            vec![
                ("x = ".to_string(), Tone::Plain),
                ("1".to_string(), Tone::Number)
            ]
        );
    }

    #[test]
    fn a_blank_line_draws_nothing_and_a_plain_one_is_one_run() {
        assert!(tones("rust", &[""]).remove(0).is_empty());
        assert_eq!(
            tones("rust", &["widget.mount(theme);"]).remove(0),
            vec![("widget.mount(theme);".to_string(), Tone::Plain)]
        );
    }
}
