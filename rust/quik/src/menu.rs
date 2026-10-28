//! The `/`, `$` and `@` menus: one list over the field, filtered as it is typed.
//!
//! The three work the same way and differ only in where the rows come from and
//! what choosing one writes. A command and a skill are named, a file is
//! path'd, and the worker parses the finished line rather than being told what
//! was chosen: `/name` by `handler/agent/inline.py`, `$name` by
//! `integrations/skills/loader.py`, a path as prose for the model to read.
//!
//! That is the point of writing text instead of sending a command -- the worker
//! already knows which commands it routes and which skills exist, so a menu
//! that only writes what it was handed cannot fall out of step with it.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use cuacode::hosts::Host;
use cuacode::transcript::Row;

use crate::view::{call_ink, call_lead, call_result, clip, clip_runs, ink, runs_width, thought_name, Run};

/// How many rows a menu shows at once. Past this the window follows the
/// selection rather than the menu growing: the field it sits above has to stay
/// on screen, and the row the cursor is on is the one worth seeing.
const ROWS: usize = 8;

/// Which list a menu is showing, and so what choosing a row writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The worker's inline commands, written as `/name `.
    Commands,
    /// The skills a user may invoke by name, written as `$name `.
    Skills,
    /// One directory of the conversation's own tree, written as its path.
    Files,
    /// A directory anywhere on the worker's machine, written as its path. The
    /// list behind `/cwd`: where the conversation stands is a place on a disk,
    /// and a path is picked from what is there rather than spelled from memory.
    Dirs,
    /// The configured providers. A picker rather than a word: choosing one
    /// switches, and nothing goes on the line.
    Providers,
    /// The models the current provider offers. A picker too.
    Models,
    /// The conversations already on disk. Opened by the launch rather than by a
    /// trigger, and the only menu with nothing on the line behind it.
    Sessions,
    /// The permission modes. A picker too, and the frontend's own list: the
    /// worker holds the mode, not a table of them.
    Permissions,
    /// The calls this conversation has made, newest first. A picker, and one the
    /// frontend fills itself: the rows are already in the transcript, and the
    /// worker has no list of them to ask for.
    Calls,
    /// The machines a conversation can run on. A picker, and the only one that
    /// replaces the worker rather than talking to it: choosing a row starts a
    /// new conversation over there, because a conversation belongs to the machine
    /// its worker runs on.
    Hosts,
}

impl Kind {
    /// The menu a trigger character opens, or nothing when it opens none.
    pub fn of(c: char) -> Option<Kind> {
        match c {
            '/' => Some(Kind::Commands),
            '$' => Some(Kind::Skills),
            '@' => Some(Kind::Files),
            _ => None,
        }
    }

    /// The word naming the menu in its frame.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Commands => "command",
            Kind::Skills => "skill",
            Kind::Files => "file",
            Kind::Dirs => "directory",
            Kind::Providers => "provider",
            Kind::Models => "model",
            Kind::Sessions => "session",
            Kind::Permissions => "permission",
            Kind::Calls => "call",
            Kind::Hosts => "host",
        }
    }
}

/// One row of a menu: what it says, the line of help beside it, and what
/// choosing it writes onto the message.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub hint: String,
    pub value: String,
    /// The label cut into the runs it is drawn in, when it is more than one
    /// colour. Empty for every row that is drawn as one string, which is all of
    /// them but a call: a call's mark, the verb it ran and what it ran on are
    /// three things, and the conversation's own row says so in colour already.
    runs: Vec<Run>,
    /// The tone the help beside it is drawn in, when the row's own state has one.
    /// `None` for the rows whose help is only a description, which is all of them
    /// but a call: a call that failed says so in red here the way its row in the
    /// conversation does, and one grey help line for every outcome is what makes a
    /// list of calls read as one colour.
    hint_ink: Option<Color>,
}

impl Item {
    pub fn new(label: impl Into<String>, hint: impl Into<String>, value: impl Into<String>) -> Item {
        Item {
            label: label.into(),
            hint: hint.into(),
            value: value.into(),
            runs: Vec::new(),
            hint_ink: None,
        }
    }

    /// A row whose own text is drawn in more than one colour. The label is that
    /// same text as one string, built here rather than passed in, so what the
    /// filter reads and what the screen draws cannot drift apart.
    fn painted(lead: String, lead_style: Style, said: Vec<Run>, hint: String, value: String) -> Item {
        let mut runs = vec![(lead, lead_style)];
        runs.extend(said);
        let label = runs.iter().map(|(text, _)| text.as_str()).collect();
        Item { label, hint, value, runs, hint_ink: None }
    }

    /// The same row with its help line in a state's own colour.
    fn tinted(mut self, colour: Color) -> Item {
        self.hint_ink = Some(colour);
        self
    }

    /// The row's own text as the spans it is drawn in, and how wide what is left
    /// comes to, which is what leaves room for the hint beside it.
    ///
    /// A row that arrived with colours keeps them; one drawn as a single string
    /// takes the row's own white. Either way the row the cursor is on is bold,
    /// because the cursor is what says which row a keystroke is about.
    fn cell(&self, selected: bool, room: usize) -> (Vec<Span<'static>>, usize) {
        let bold = |style: Style| {
            if selected {
                style.add_modifier(Modifier::BOLD)
            } else {
                style
            }
        };
        let runs = if self.runs.is_empty() {
            vec![(self.label.clone(), Style::new().fg(ink::text()))]
        } else {
            self.runs.clone()
        };
        let cut = clip_runs(&runs, room);
        let wide = runs_width(&cut);
        (
            cut.into_iter().map(|(text, style)| Span::styled(text, bold(style))).collect(),
            wide,
        )
    }
}

/// The verbs the frontend answers itself.
///
/// The `/` menu is the worker's own list, read off the parser that routes it, so
/// there is one owner of what the commands are. Neither of these is down there:
/// no worker command is called help (it is the keys, and the frontend is the side
/// that knows them), and the permission mode is session state the worker holds
/// but has no command for. A `/help` or a `/permissions` that went to the model
/// as prose would be worse than two rows kept here.
pub fn local() -> Vec<Item> {
    vec![
        Item::new("help", "the keys", "/help"),
        Item::new("permissions", "ask, auto, bypass, or off", "/permissions"),
    ]
}

/// The machines a conversation can run on, this one first.
///
/// The rows are `hosts.json` read once rather than a list kept here, and the
/// empty value is this machine: every install without that file has exactly one
/// row, which is the way this screen has always looked. The help beside a row is
/// the `ssh` destination, because that string is the whole answer to "how does it
/// get there" and a person debugging one should not have to open the file to see
/// what was used.
pub fn hosts(list: &[Host], now: Option<&str>) -> Vec<Item> {
    let mut rows = vec![Item::new("this machine", acting("", now.is_none()), "")];
    for host in list {
        rows.push(Item::new(
            host.name.clone(),
            acting(&host.ssh, now == Some(host.name.as_str())),
            host.name.clone(),
        ));
    }
    rows
}

/// The conversation list, with the row that is not a conversation on top: a new
/// one, standing in `where_new`.
///
/// It leads because that is the row someone who has just moved to a box wants, and
/// the list under it is that box's own. The value is empty, which is not a session
/// id and cannot be: `session.new` is the command behind that row, and the worker
/// creates the folder with its first record, in the directory its own entry put it
/// in. `where_new` is that directory, said out loud, because a new session on a
/// machine you are not sitting at is a location before it is anything else.
pub fn sessions(where_new: &str, items: Vec<Item>) -> Vec<Item> {
    let mut rows = vec![Item::new("new session", where_new.to_string(), String::new())];
    rows.extend(items);
    rows
}

/// The value of the row a **typed path** adds to a conversation picker. Not a
/// session id and not a value one could be: the rows are `new session` for the
/// empty value and a session's id for everything else.
pub const NEW_IN: &str = "\u{0}new-in";

/// The permission modes as a picker draws them: the four the worker knows, in
/// the order it takes them, with the one in use marked. A picker that only lists
/// is one you have to leave to find out where you are.
pub fn modes(now: &str) -> Vec<Item> {
    ["ask", "auto", "bypass", "off"]
        .iter()
        .map(|mode| Item::new(*mode, acting("", *mode == now), *mode))
        .collect()
}

/// The conversation's own calls as a picker's rows, newest first.
///
/// The list is the transcript read back, not a second record of it: a call is
/// named by the same head its row carries and led by the same mark, and the value
/// is the row's index, which is what opening one is asked for by. Newest first,
/// because the call just watched go past is the one usually wanted.
///
/// The head is drawn by `view::call_lead`, the same function the row in the
/// conversation is drawn by, colours and all: a list of calls that read one way
/// here and another way in the transcript would be two ideas of what a call is.
pub fn calls(rows: &[Row]) -> Vec<Item> {
    rows.iter()
        .enumerate()
        .rev()
        .filter_map(|(i, row)| match row {
            Row::Tool { name, args, call, result, .. } => {
                // Nothing here is live: the list is the conversation read back,
                // so no row blinks and no row is "the one being written".
                let (lead, lead_style, said) = call_lead(name, args, call, result.as_deref(), false, false);
                // The help beside a call is what came back, without the one thing
                // the head already said: a row that names the file twice has room
                // for nothing else. It is drawn in the tone the call's own mark is,
                // so a failure is red here too and the list says at a glance which
                // of these rows is the one that went wrong.
                let help = result.as_deref().map(|r| call_result(args, r)).unwrap_or_default();
                Some(
                    Item::painted(lead, lead_style, said, help, i.to_string())
                        .tinted(call_ink(result.as_deref())),
                )
            }
            // A thought is in the list too, because a step with its thinking opened
            // is a thing you want to close again, and hunting for the row on the
            // screen to double-click is not a way to close anything.
            Row::Thinking { text, .. } => {
                let said = vec![(thought_name(text), Style::new().fg(ink::text()))];
                Some(Item::painted(format!("\u{25cf} "), Style::new().fg(ink::done()), said,
                                   "thinking".to_string(), i.to_string())
                    .tinted(ink::done()))
            }
            _ => None,
        })
        .collect()
}

/// The worker's rows with the local ones in front, and any name the worker sent
/// for itself kept as the worker sent it rather than doubled.
pub fn with_local(items: Vec<Item>) -> Vec<Item> {
    let mut out = local();
    for item in items {
        if !out.iter().any(|l| l.label == item.label) {
            out.push(item);
        }
    }
    out
}

/// A menu's rows, as the worker sent them: which list they are, the rows, and
/// for a model catalog the provider it was asked of, or for a directory why it
/// could not be read.
pub struct Reply {
    pub kind: Kind,
    pub items: Vec<Item>,
    pub provider: String,
    pub error: String,
}

/// The rows a menu reply carries, or nothing when the event was not one.
///
/// The worker answers each list request with an envelope named for it, and the
/// field names are those names -- `command.list` with `commands`, `fs.list`
/// with `entries`, and so on. One reader for all five, so a frontend never
/// carries a second idea of what a list looks like.
pub fn from_reply(kind: &str, data: &serde_json::Value) -> Option<Reply> {
    let (key, kind) = match kind {
        "commands" => ("commands", Kind::Commands),
        "skills" => ("skills", Kind::Skills),
        "files" => ("entries", Kind::Files),
        "dirs" => ("entries", Kind::Dirs),
        "providers" => ("providers", Kind::Providers),
        "models" => ("models", Kind::Models),
        "sessions" => ("sessions", Kind::Sessions),
        _ => return None,
    };
    let rows = data.get(key)?.as_array()?;
    let items = match kind {
        Kind::Commands => rows
            .iter()
            .filter_map(|row| {
                let name = row.get("name")?.as_str()?;
                let hint = row.get("description").and_then(|d| d.as_str()).unwrap_or("");
                Some(Item::new(name, hint, format!("/{name}")))
            })
            .collect(),
        Kind::Skills => rows
            .iter()
            .filter_map(|row| {
                let name = row.get("name")?.as_str()?;
                let hint = brief(row.get("description").and_then(|d| d.as_str()).unwrap_or(""));
                Some(Item::new(name, hint, format!("${name}")))
            })
            .collect(),
        Kind::Files => rows
            .iter()
            .filter_map(|row| {
                let path = row.get("path")?.as_str()?;
                let name = row.get("name").and_then(|n| n.as_str()).unwrap_or(path);
                // A directory keeps its slash, so choosing one carries on
                // walking instead of naming a file that cannot exist.
                let dir = row.get("dir").and_then(|d| d.as_bool()).unwrap_or(false);
                let value = if dir { format!("{path}/") } else { path.to_string() };
                Some(Item::new(name, "", value))
            })
            .collect(),
        // A directory anywhere on that machine, which is what `/cwd`'s argument
        // is. The row that finishes is the directory being listed -- choosing
        // it is the answer, and it carries no trailing slash for exactly that
        // reason -- and every other row is a way in, so it does.
        Kind::Dirs => {
            let here = data.get("dir").and_then(|d| d.as_str()).unwrap_or("");
            let mut items: Vec<Item> = Vec::with_capacity(rows.len() + 2);
            if !here.is_empty() {
                items.push(Item::new("use this directory", here.to_string(), here.to_string()));
            }
            if let Some(up) = data.get("parent").and_then(|p| p.as_str()).filter(|p| !p.is_empty()) {
                items.push(Item::new("..", up.to_string(), format!("{up}/")));
            }
            items.extend(rows.iter().filter_map(|row| {
                let path = row.get("path")?.as_str()?;
                let name = row.get("name").and_then(|n| n.as_str()).unwrap_or(path);
                Some(Item::new(name, "", format!("{path}/")))
            }));
            items
        }
        Kind::Providers => rows
            .iter()
            .filter_map(|row| {
                let name = row.get("name")?.as_str()?;
                let model = row.get("model").and_then(|m| m.as_str()).unwrap_or("");
                let now = row.get("active").and_then(|a| a.as_bool()).unwrap_or(false);
                Some(Item::new(name, acting(model, now), name))
            })
            .collect(),
        // The catalog, as ids. One the account cannot see is worth saying so
        // about: the picker is where that choice gets made.
        Kind::Models => {
            let now = data.get("active").and_then(|a| a.as_str()).unwrap_or("");
            let blind = data.get("blind").and_then(|b| b.as_array());
            let blind = |id: &str| {
                blind.is_some_and(|b| b.iter().any(|v| v.as_str() == Some(id)))
            };
            rows.iter()
                .filter_map(|row| {
                    let id = row.as_str()?;
                    let hint = if blind(id) { "cannot see" } else { "" };
                    Some(Item::new(id, acting(hint, id == now), id))
                })
                .collect()
        }
        // A conversation on disk. The name when it has one and its own id when
        // it does not: an unnamed session showing its id is telling the truth
        // about itself, where a label made from "hi" would not be.
        Kind::Sessions => {
            let now = data.get("active").and_then(|a| a.as_str()).unwrap_or("");
            rows.iter()
                .filter_map(|row| {
                    let id = row.get("id")?.as_str()?;
                    let title = row.get("title").and_then(|t| t.as_str()).unwrap_or("").trim();
                    let label = if title.is_empty() { id } else { title };
                    let when = stamp(row.get("updated").and_then(|u| u.as_str()).unwrap_or(""));
                    let there = base(row.get("cwd").and_then(|c| c.as_str()).unwrap_or(""));
                    let hint = format!("{when}  {there}").trim().to_string();
                    Some(Item::new(label, acting(&hint, id == now), id))
                })
                .collect()
        }
        // Never arrives: the four permission modes are the frontend's own, and
        // the calls are read off the transcript. Here so the match stays
        // exhaustive over what a menu can be.
        Kind::Permissions | Kind::Calls | Kind::Hosts => Vec::new(),
    };
    let provider = data.get("provider").and_then(|p| p.as_str()).unwrap_or("").to_string();
    let error = data.get("error").and_then(|e| e.as_str()).unwrap_or("").to_string();
    Some(Reply { kind, items, provider, error })
}

/// A row's help line, marked when the row is the thing already in use: a picker
/// that only lists is one you have to leave to find out where you are.
fn acting(text: &str, now: bool) -> String {
    match (text.is_empty(), now) {
        (true, false) => String::new(),
        (true, true) => "acting now".to_string(),
        (false, false) => text.to_string(),
        (false, true) => format!("{text}  \u{b7}  acting now"),
    }
}

/// The date and time out of an ISO stamp, as a picker reads it: `10-03 08:54`.
/// Nothing at all when the stamp is not the shape it should be, rather than a
/// slice of whatever happened to be there.
fn stamp(iso: &str) -> String {
    match iso.get(0..16) {
        Some(s) if s.as_bytes().get(10) == Some(&b'T') => format!("{} {}", &s[5..10], &s[11..16]),
        _ => String::new(),
    }
}

/// The last piece of a path, which is what names a directory to a person.
fn base(path: &str) -> String {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string()
}

/// A description cut at its first sentence: a row has one line for help, and a
/// skill's first sentence is the one saying when it applies.
fn brief(description: &str) -> String {
    match description.split_once(['.', '\n']) {
        Some((head, _)) if !head.is_empty() => head.to_string(),
        _ => description.to_string(),
    }
}

/// An open menu. The filter is not a buffer of its own: it is the tail of the
/// message, from the trigger on, read back.
pub struct Menu {
    pub kind: Kind,
    /// The trigger's index in the message, counted in characters. Choosing a
    /// row replaces everything from here on.
    pub anchor: usize,
    /// What has been typed after the trigger, and so what the rows are filtered
    /// by.
    pub filter: String,
    /// Files only: the directory the rows on hand came from, and `None` until
    /// one has been asked for. A directory rather than a plain string, because
    /// the root's own name is the empty string and a menu that starts out
    /// "already at the root" would never ask for it.
    pub dir: Option<String>,
    /// Models only: the provider the catalog came from, as the worker echoed
    /// it. What a chosen model is set on, rather than whatever the frontend
    /// believes is current.
    pub provider: String,
    /// Directories only: why the worker could not read the one asked for, in its
    /// own words. Empty when it could. A menu that drew an empty frame for both
    /// an empty directory and a path that is not there is a menu telling the
    /// reader nothing about the thing it was asked for.
    pub error: String,
    /// Whether the filter is also the tail of the message, and so whether a
    /// keystroke is a keystroke in the field.
    ///
    /// True for a menu a trigger opened: the trigger and everything after it
    /// are on the line, and the menu is a view of them. False for a picker,
    /// which was opened by a command or by the launch and has no trigger on the
    /// line to type after -- a filter that wrote itself into the message there
    /// would be editing something the user never asked it to touch.
    on_line: bool,
    all: Vec<Item>,
    shown: Vec<usize>,
    sel: usize,
}

impl Menu {
    pub fn open(kind: Kind, anchor: usize) -> Menu {
        Menu {
            kind,
            anchor,
            filter: String::new(),
            dir: None,
            provider: String::new(),
            error: String::new(),
            on_line: true,
            all: Vec::new(),
            shown: Vec::new(),
            sel: 0,
        }
    }

    /// A menu opened by something other than a trigger: a picker, or the
    /// question the launch asks. Nothing on the line belongs to it, so its
    /// filter is its own and typing at it types nothing into the message.
    pub fn open_apart(kind: Kind) -> Menu {
        Menu { on_line: false, ..Menu::open(kind, 0) }
    }

    /// Whether typing at this menu also types into the field.
    pub fn on_line(&self) -> bool {
        self.on_line
    }

    /// Whether any rows are in hand at all. A picker asks once and a list of
    /// providers is a network question, so this is what keeps a keystroke from
    /// asking it again.
    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// Take the rows that arrived. The selection is kept where it is rather
    /// than reset: a list landing under a cursor already moving should not jump.
    pub fn fill(&mut self, items: Vec<Item>) {
        self.all = items;
        self.refilter();
    }

    /// A file filter is a path, and only its last part names an entry: what has
    /// been typed after the last `/`, and the directory walked to get there. A
    /// filter that begins at the root walks from `/` rather than from the
    /// conversation, which is the difference between `/cwd /etc` and `/cwd etc`.
    pub fn walked(&self) -> (&str, &str) {
        match self.filter.rsplit_once('/') {
            Some(("", name)) if self.filter.starts_with('/') => ("/", name),
            Some((dir, name)) => (dir, name),
            None => ("", self.filter.as_str()),
        }
    }

    pub fn chosen(&self) -> Option<&Item> {
        self.shown.get(self.sel).map(|i| &self.all[*i])
    }

    /// The word naming the menu in its frame, and how many rows are on hand.
    ///
    /// The window shows eight whatever the list holds, so a long list without a
    /// count is a place with no way to tell how big it is or where in it you are.
    /// The count is what the filter left, because that is what the keys can reach.
    pub fn title(&self) -> String {
        let said = match self.shown.len() {
            0 => self.kind.title().to_string(),
            n => format!("{} \u{b7} {n}", self.kind.title()),
        };
        // Why a list is empty, when the worker said why: a directory with
        // nothing in it and a directory that is not there draw the same frame.
        match self.error.is_empty() {
            true => said,
            false => format!("{said} \u{b7} {}", self.error),
        }
    }

    /// Move the selection, wrapping at both ends: the list is short, and the
    /// keyboard should not have to be told where it stops.
    pub fn move_sel(&mut self, by: isize) {
        if self.shown.is_empty() {
            return;
        }
        let n = self.shown.len() as isize;
        self.sel = (((self.sel as isize + by) % n + n) % n) as usize;
    }

    /// Keep the rows the filter names, best match first.
    ///
    /// Ranked rather than merely kept: a prefix beats a word inside the name
    /// beats the letters being in it in order. Within a rank the list's own
    /// order stands, so rows do not shuffle under the cursor as characters
    /// arrive.
    pub fn refilter(&mut self) {
        let needle = match self.kind {
            Kind::Files | Kind::Dirs => self.walked().1,
            _ => self.filter.as_str(),
        }
        .to_lowercase();
        // A path typed into a conversation picker is not a filter, it is where the new
        // conversation should stand: a row appears for it, so the choice is a row like
        // every other one and enter takes it. Without this the picker only knew how to
        // open what was already on that disk.
        self.all.retain(|item| item.value != NEW_IN);
        let typed = self.filter.trim().to_string();
        if self.kind == Kind::Sessions && (typed.starts_with('/') || typed.starts_with('~')) {
            self.all
                .insert(0, Item::new(format!("new session in {typed}"), "", NEW_IN));
        }
        let mut ranked: Vec<(u8, usize)> = Vec::new();
        for (i, item) in self.all.iter().enumerate() {
            let hay = item.label.to_lowercase();
            let rank = if needle.is_empty() || hay.starts_with(&needle) {
                0
            } else if hay.contains(&needle) {
                1
            } else if subsequence(&hay, &needle) {
                2
            } else {
                continue;
            };
            ranked.push((rank, i));
        }
        ranked.sort_by_key(|(rank, _)| *rank);
        self.shown = ranked.into_iter().map(|(_, i)| i).collect();
        self.sel = self.sel.min(self.shown.len().saturating_sub(1));
    }

    /// The menu as the lines a screen draws: the rows that matched, and the one
    /// line saying what the keys do.
    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        if self.shown.is_empty() {
            out.push(Line::from(Span::styled("  nothing matches", Style::new().fg(ink::quiet()))));
            return out;
        }
        let n = self.shown.len().min(ROWS);
        let start = if self.sel < n { 0 } else { self.sel - n + 1 };
        for k in start..start + n {
            let item = &self.all[self.shown[k]];
            let selected = k == self.sel;
            // The row's own text, in the colours it was given and cut to the
            // room there is, and how wide what is left draws.
            let (runs, drawn) = item.cell(selected, width.saturating_sub(6).max(1));
            let used = 2 + drawn;
            // A hint is dropped rather than cut to nothing when there is no room
            // for it: three characters of a sentence is not help.
            let spare = width.saturating_sub(used + 3);
            let hint = if spare >= 4 { clip(&item.hint, spare) } else { String::new() };
            let pad = width.saturating_sub(used + hint.chars().count());
            // The cursor's mark is the accent, and nothing else in a menu is:
            // ember's one hue means "you can act on this", and the row the enter
            // key is about is the one thing here you can act on.
            let tone = if selected { ink::call() } else { ink::quiet() };
            let mut spans = vec![Span::styled(
                if selected { "\u{203a} " } else { "  " },
                Style::new().fg(tone),
            )];
            spans.extend(runs);
            spans.push(Span::raw(" ".repeat(pad)));
            // A row that carries a state's tone says so in its help as well as in
            // its mark, which is what makes the one that failed findable without
            // reading a column of sentences.
            spans.push(Span::styled(hint, Style::new().fg(item.hint_ink.unwrap_or_else(ink::quiet))));
            out.push(Line::from(spans));
        }
        out.push(Line::from(Span::styled(
            "\u{2191}\u{2193} choose  \u{b7}  enter select  \u{b7}  esc cancel",
            Style::new().fg(ink::faint()),
        )));
        out
    }
}

/// Whether `needle`'s letters are in `hay` in order, with gaps allowed: `sn`
/// finds `session`. What makes a menu feel like it is reading the name rather
/// than demanding it.
fn subsequence(hay: &str, needle: &str) -> bool {
    let mut left = hay.chars();
    needle.chars().all(|n| left.any(|h| h == n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(labels: &[&str]) -> Vec<Item> {
        labels.iter().map(|l| Item::new(*l, "", *l)).collect()
    }

    fn labels(menu: &Menu) -> Vec<&str> {
        menu.shown.iter().map(|i| menu.all[*i].label.as_str()).collect()
    }

    /// A menu's rows, and what choosing one writes onto the message.
    ///
    /// The host picker is the one list a worker never answers, so it is checked
    /// here: this machine is always a row, and a host's help is the `ssh` string
    /// that is the whole answer to how the conversation gets there.
    #[test]
    fn the_machines_are_this_one_first_and_the_file_after_it() {
        let list = vec![
            Host { name: "box".into(), ssh: "me@box".into(), dir: "/srv".into(), ..Host::default() },
        ];
        let rows = hosts(&list, None);
        assert_eq!(rows[0].label, "this machine");
        assert_eq!(rows[0].value, "", "the empty value is this machine");
        assert_eq!(rows[0].hint, "acting now");
        assert_eq!(rows[1].label, "box");
        assert_eq!(rows[1].hint, "me@box", "the ssh destination, so a broken one is visible");
        assert_eq!(rows[1].value, "box");

        // On a host, the mark moves to it and this machine is only a destination.
        let rows = hosts(&list, Some("box"));
        assert_eq!(rows[0].hint, "");
        assert!(rows[1].hint.contains("acting now"));
    }

    /// The filter is the tail of the message, and what a menu does with it. A
    /// prefix is the best match, not merely a match: someone typing `mo` means
    /// the row that starts with it, wherever it sits in the list.
    #[test]
    fn a_prefix_beats_a_word_inside_beats_the_letters_in_order() {
        let mut menu = Menu::open(Kind::Commands, 0);
        menu.fill(named(&["old_model", "model", "mod.rs", "context"]));
        assert_eq!(labels(&menu), ["old_model", "model", "mod.rs", "context"]);
        menu.filter = "mo".into();
        menu.refilter();
        assert_eq!(labels(&menu), ["model", "mod.rs", "old_model"]);
        // Letters in order and nowhere else: `cxt` is nothing but context.
        menu.filter = "cxt".into();
        menu.refilter();
        assert_eq!(labels(&menu), ["context"]);
        menu.filter = "zzz".into();
        menu.refilter();
        assert!(labels(&menu).is_empty());
    }

    /// The selection wraps, and it is clamped down rather than reset when the
    /// list shrinks: a cursor that jumped to the top of its own accord would
    /// make the next enter choose the wrong row.
    #[test]
    fn the_selection_wraps_and_is_clamped_not_reset() {
        let mut menu = Menu::open(Kind::Commands, 0);
        menu.fill(named(&["one", "two", "three"]));
        menu.move_sel(-1);
        assert_eq!(menu.chosen().unwrap().label, "three");
        menu.move_sel(1);
        assert_eq!(menu.chosen().unwrap().label, "one");
        menu.move_sel(1);
        menu.move_sel(1);
        assert_eq!(menu.chosen().unwrap().label, "three");
        menu.filter = "t".into();
        menu.refilter();
        assert_eq!(menu.sel, 1);
        menu.filter = "three".into();
        menu.refilter();
        assert_eq!(menu.sel, 0);
    }

    /// A file filter is a path. The directory part is what the worker has to be
    /// asked for, and only the part after the last `/` names an entry.
    #[test]
    fn a_file_filter_is_a_path_and_only_its_last_part_names_a_row() {
        let mut menu = Menu::open(Kind::Files, 0);
        menu.fill(named(&["main.rs", "mod.rs"]));
        assert_eq!(menu.walked(), ("", ""));
        menu.filter = "src/main".into();
        assert_eq!(menu.walked(), ("src", "main"));
        menu.refilter();
        assert_eq!(labels(&menu), ["main.rs"]);
        menu.filter = "src/".into();
        assert_eq!(menu.walked(), ("src", ""));
        menu.refilter();
        assert_eq!(labels(&menu), ["main.rs", "mod.rs"]);
    }

    /// `/cwd`'s list: a directory anywhere on the machine. The row that
    /// finishes is the directory being read, and it carries no slash because a
    /// slash means "walk into this"; the entry above it says where walking up
    /// goes, and every row under it is a way in.
    #[test]
    fn the_directory_list_says_where_you_are_and_how_to_leave_it() {
        let reply = from_reply(
            "dirs",
            &serde_json::json!({"dir": "/home/me/work", "parent": "/home/me",
                 "error": "", "entries": [
                {"name": "src", "dir": true, "path": "/home/me/work/src"}]}),
        )
        .unwrap();
        assert_eq!(reply.kind, Kind::Dirs);
        assert_eq!(reply.items[0], Item::new("use this directory", "/home/me/work", "/home/me/work"));
        assert_eq!(reply.items[1].value, "/home/me/", "the row above walks up");
        assert_eq!(reply.items[2].value, "/home/me/work/src/", "an entry walks in");

        // A path that is not there says why, rather than drawing an empty frame
        // that reads as an empty directory.
        let reply = from_reply(
            "dirs",
            &serde_json::json!({"dir": "/nope", "parent": "/", "entries": [],
                 "error": "No such file or directory"}),
        )
        .unwrap();
        let mut menu = Menu::open(Kind::Dirs, 0);
        menu.fill(reply.items);
        menu.error = reply.error;
        assert!(menu.title().contains("No such file or directory"), "{}", menu.title());
    }

    /// An absolute filter walks from the root, which is what makes `/cwd /etc`
    /// different from `/cwd etc`.
    #[test]
    fn an_absolute_filter_starts_at_the_root() {
        let mut menu = Menu::open(Kind::Dirs, 0);
        menu.filter = "/etc/".into();
        assert_eq!(menu.walked(), ("/etc", ""));
        menu.filter = "/".into();
        assert_eq!(menu.walked(), ("/", ""));
        menu.filter = "etc".into();
        assert_eq!(menu.walked(), ("", "etc"), "a bare word is still the conversation's own");
    }

    /// Every list the worker sends becomes rows, and choosing one writes the
    /// thing itself: the worker parses the finished line rather than being told.
    #[test]
    fn a_reply_becomes_rows_and_a_row_becomes_what_it_writes() {
        let reply = from_reply(
            "commands",
            &serde_json::json!({"commands": [{"name": "model", "description": "which model"}]}),
        )
        .unwrap();
        assert_eq!(reply.kind, Kind::Commands);
        assert_eq!(reply.items, vec![Item::new("model", "which model", "/model")]);

        let reply = from_reply(
            "skills",
            &serde_json::json!({"skills": [{"name": "unslop", "description": "Cut AI tells. Always."}]}),
        )
        .unwrap();
        // Cut at the first sentence, because a row has one line for it.
        assert_eq!(reply.items, vec![Item::new("unslop", "Cut AI tells", "$unslop")]);

        let reply = from_reply(
            "files",
            &serde_json::json!({"dir": "", "entries": [
                {"name": "src", "dir": true, "path": "src"},
                {"name": "run.sh", "dir": false, "path": "run.sh"}]}),
        )
        .unwrap();
        assert_eq!(reply.kind, Kind::Files);
        // A directory keeps its slash, so the next thing typed keeps walking.
        assert_eq!(reply.items[0].value, "src/");
        assert_eq!(reply.items[1].value, "run.sh");

        assert!(from_reply("status", &serde_json::json!({})).is_none());
    }

    /// A picker says where it is: the row already in use is marked, and the
    /// model catalog is marked for the provider it was asked of rather than for
    /// whichever one the frontend last heard about.
    #[test]
    fn a_picker_marks_the_row_it_is_already_on() {
        let reply = from_reply(
            "providers",
            &serde_json::json!({"providers": [
                {"name": "ollama", "active": true, "model": "deepseek-v4.1-flash"},
                {"name": "openai", "active": false, "model": "gpt-5"}]}),
        )
        .unwrap();
        assert_eq!(reply.kind, Kind::Providers);
        assert_eq!(reply.items[0].value, "ollama");
        assert!(reply.items[0].hint.ends_with("acting now"), "{}", reply.items[0].hint);
        assert!(!reply.items[1].hint.contains("acting now"));

        let reply = from_reply(
            "models",
            &serde_json::json!({"provider": "ollama", "models": ["a", "b", "c"],
                                "active": "b", "blind": ["c"]}),
        )
        .unwrap();
        assert_eq!(reply.kind, Kind::Models);
        // Which provider the catalog belongs to travels with it, so a choice is
        // set on the provider that was listed and not on a guess.
        assert_eq!(reply.provider, "ollama");
        assert_eq!(reply.items[1].value, "b");
        assert!(reply.items[1].hint.contains("acting now"));
        assert!(reply.items[2].hint.contains("cannot see"));
        assert_eq!(reply.items[0].hint, "");
    }

    /// A fresh file menu has asked for nothing yet, so listing the root is a
    /// change even though the root's name is the empty string. Without the
    /// two being different, the first `@` opens a menu that never asks and so
    /// never fills.
    #[test]
    fn a_fresh_file_menu_has_not_asked_for_the_root_yet() {
        let menu = Menu::open(Kind::Files, 0);
        assert_eq!(menu.walked(), ("", ""));
        assert_ne!(menu.dir.as_deref(), Some(""));
    }

    /// A menu says which list it is and how much is in it: the window shows eight
    /// rows whatever the list holds, so without the count there is no telling how
    /// far the list goes or where in it the cursor is.
    #[test]
    fn a_menu_says_what_it_is_and_how_much_is_in_it() {
        let mut menu = Menu::open(Kind::Commands, 0);
        assert_eq!(menu.title(), "command", "nothing in hand yet");
        menu.fill(named(&["one", "two", "three"]));
        assert_eq!(menu.title(), "command \u{b7} 3");
        menu.filter = "tw".into();
        menu.refilter();
        assert_eq!(menu.title(), "command \u{b7} 1", "what the filter left");
    }

    /// A call's row in a menu is the conversation's own head, colours and all: the
    /// mark in the colour of what happened to the call, the verb in the call's
    /// colour, and the file standing out from the way to it. Every other row in
    /// every other menu is one string in one colour, as it was.
    #[test]
    fn a_calls_row_is_drawn_in_the_colours_of_the_row_it_lists() {
        let mut log = cuacode::transcript::Transcript::default();
        for line in [
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"file","args":{"action":"read","path":"src/main.rs"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"file","result":{"result":{"path":"src/main.rs","lines":10}}}}"#,
        ] {
            log.apply(&cuacode::wire::parse(line).unwrap());
        }
        let items = calls(&log.rows);
        assert_eq!(items.len(), 1);
        // The label is the runs as one string, so what the filter reads is what
        // the screen draws.
        assert_eq!(items[0].label, "\u{2713} read(src/main.rs)");

        let mut menu = Menu::open_apart(Kind::Calls);
        menu.fill(items);
        let row = &menu.lines(60)[0];
        // Runs that share a style are drawn as one span, so a span is looked for by
        // what it says rather than by being exactly one run.
        let ink_of = |text: &str| {
            row.spans
                .iter()
                .find(|span| span.content.contains(text))
                .map(|span| span.style.fg)
                .unwrap_or_else(|| panic!("no {text} run in {row:?}"))
        };
        assert_eq!(ink_of("\u{2713}"), Some(ink::done()), "the mark of a call that ran");
        assert_eq!(ink_of("read"), Some(ink::call()), "the verb is a call's colour");
        assert_eq!(ink_of("src/"), Some(ink::faint()), "the way there is fainter");
        assert_eq!(ink_of("main.rs"), Some(ink::text()), "the file is the bright part");
        assert_eq!(ink_of("lines"), Some(ink::done()), "the help carries the mark's tone");
    }

    /// A call that failed says so in its help as well as in its mark. One grey help
    /// line for every outcome is what made a list of calls read as one colour, and
    /// the red one is the row a person opens.
    #[test]
    fn a_failed_calls_help_is_red_like_its_mark() {
        let mut log = cuacode::transcript::Transcript::default();
        for line in [
            r#"{"type":"token","id":"a","data":{"state":"tool_calls","token":[{"name":"shell","args":{"command":"nope"}}]}}"#,
            r#"{"type":"token","id":"a","data":{"state":"tool_output","token":"shell","result":{"result":{"exit_code":127,"stderr":"zsh: command not found: nope\n"}}}}"#,
        ] {
            log.apply(&cuacode::wire::parse(line).unwrap());
        }
        let items = calls(&log.rows);
        let mut menu = Menu::open_apart(Kind::Calls);
        menu.fill(items);
        let row = &menu.lines(60)[0];
        let red: Vec<&str> = row
            .spans
            .iter()
            .filter(|span| span.style.fg == Some(ink::failed()))
            .map(|span| span.content.as_ref())
            .collect();
        // The cross, the whole `shell(...)` head, and the message it came back with.
        assert!(red.iter().any(|s| s.contains('\u{2717}')), "the mark: {row:?}");
        assert!(!red.contains(&"shell"), "the verb is the accent, not the state: {row:?}");
        assert!(red.iter().any(|s| s.contains("command not found")), "the help: {row:?}");
        let verb = row.spans.iter().find(|span| span.content.as_ref() == "shell").expect("the verb");
        assert_eq!(verb.style.fg, Some(ink::call()), "and the accent is what it wears");
    }

    /// A conversation on disk becomes a row: its name, when it was last touched
    /// and where it lives, with the one already open marked.
    #[test]
    fn a_session_row_reads_as_a_conversation() {
        let reply = from_reply(
            "sessions",
            &serde_json::json!({"active": "20261003-083142-b1ea", "sessions": [
                {"id": "20261003-085129-b013", "title": "add spacing before text output",
                 "updated": "2026-10-03T08:54:37+00:00", "cwd": "/home/dev/Work/021026-cuacode-rs"},
                {"id": "20261003-083142-b1ea", "title": "",
                 "updated": "2026-10-03T08:31:42+00:00", "cwd": "/tmp"}]}),
        )
        .unwrap();
        assert_eq!(reply.kind, Kind::Sessions);
        assert_eq!(reply.items[0].label, "add spacing before text output");
        assert_eq!(reply.items[0].value, "20261003-085129-b013");
        assert!(reply.items[0].hint.contains("10-03 08:54"), "{}", reply.items[0].hint);
        assert!(reply.items[0].hint.contains("021026-cuacode-rs"), "{}", reply.items[0].hint);
        // Unnamed shows its own id, which is the truth about it.
        assert_eq!(reply.items[1].label, "20261003-083142-b1ea");
        assert!(reply.items[1].hint.ends_with("acting now"), "{}", reply.items[1].hint);
        // A stamp that is not a stamp leaves no room for invention.
        assert_eq!(stamp("whenever"), "");
    }

    /// A picker's filter is its own. Typing at one must not put characters on
    /// the message, which is a line the user never gave it.
    #[test]
    fn a_picker_filters_without_writing_on_the_line() {
        assert!(Menu::open(Kind::Skills, 4).on_line());
        assert!(!Menu::open_apart(Kind::Sessions).on_line());
    }

    /// A menu never draws taller than it was told to, whatever the list holds.
    #[test]
    fn a_long_list_is_windowed_over_the_selection() {
        let mut menu = Menu::open(Kind::Commands, 0);
        menu.fill(named(&["row"; 30]));
        let lines = menu.lines(40);
        assert_eq!(lines.len(), ROWS + 1);
        for _ in 0..20 {
            menu.move_sel(1);
        }
        assert_eq!(menu.sel, 20);
        assert_eq!(menu.lines(40).len(), ROWS + 1);
        // The row under the cursor is in the window, not scrolled off it.
        assert!(menu.lines(40).iter().any(|l| l.to_string().contains('\u{203a}')));
    }
}
