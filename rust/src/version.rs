//! Which code this is, and whether origin has moved on from it.
//!
//! The id comes from `build.rs`, which stamps the commit the build ran on:
//! `<commits>.<short sha>`, so `412.52738a5` says both which build and which
//! commit, and two builds of one source agree. Nothing here reads a number out
//! of a manifest, because that number is the one nobody bumps.
//!
//! `check` is the other half: it asks origin, which means it needs the network
//! and it takes a moment. Both frontends run it on a thread beside the screen
//! and draw nothing until it answers, and `cuacode doctor` runs it on the spot
//! and prints the whole of what it found.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// The tree this was built from, as a version id.
pub const ID: &str = env!("CUACODE_VERSION");

/// The commit `ID` names, in full. Empty when the build had no git.
pub const COMMIT: &str = env!("CUACODE_COMMIT");

/// Long enough for a fetch over a slow line, short enough that a frontend's
/// thread is never left holding a question nobody will answer.
const LIMIT: Duration = Duration::from_secs(8);

/// What origin said, in the three shapes a screen can act on.
#[derive(Clone, Debug, PartialEq)]
pub enum News {
    /// origin's branch carries commits this tree does not, and this is how many.
    Behind(usize),
    /// Asked, and this is the tip.
    Current,
    /// Could not be asked: no git, no checkout, no network. The string is why,
    /// for `doctor` to print. A screen with one line for news says nothing at
    /// all, since none of these is something the reader can act on.
    Unknown(String),
}

impl News {
    /// The count when there is one, which is what a badge draws.
    pub fn behind(&self) -> Option<usize> {
        match self {
            News::Behind(n) => Some(*n),
            _ => None,
        }
    }
}

/// Ask how far behind this tree is.
///
/// The fetch is the point and not a side effect to be avoided: `ls-remote` says
/// the tip moved but not by how much, and "you are behind" without a number is a
/// nag. A fetch writes the remote's own refs and nothing else, so a checkout
/// with work in it is untouched.
///
/// Any remote will do, `origin` first: a fork renames it, and a tree with one
/// remote needs no guessing. The comparison is against the remote's **default**
/// branch rather than the local one, which is what makes this answer work for
/// the three checkouts a person actually builds from - a fork on a feature
/// branch, a tag checked out, and a plain clone.
pub fn check() -> News {
    // The cap has to be a thread: a `Command` has no timeout of its own, and a
    // fetch on a dead network would otherwise hang the caller for as long as the
    // kernel feels like holding the socket.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(ask());
    });
    match rx.recv_timeout(LIMIT) {
        Ok(news) => news,
        Err(_) => News::Unknown("the remote did not answer in time".into()),
    }
}

fn ask() -> News {
    let Some(root) = root() else {
        return News::Unknown("no git checkout above this directory or binary".into());
    };
    let remotes = git(&root, &["remote"]).unwrap_or_default();
    let Some(remote) = remotes.lines().find(|r| *r == "origin").or_else(|| remotes.lines().next())
    else {
        return News::Unknown("this checkout has no remote to ask".into());
    };
    // One round trip and no objects: `ls-remote --symref` answers both questions
    // at once, the remote's default branch and its tip. Up to date is the answer
    // most launches get, and it must not cost a fetch - a fetch negotiates and
    // can pull a pack to answer what comparing two shas already answered.
    let said = match git(&root, &["ls-remote", "--symref", remote, "HEAD"]) {
        Ok(said) => said,
        Err(why) => return News::Unknown(why),
    };
    let (branch, tip) = match parse_symref(&said) {
        Some(parsed) => parsed,
        None => return News::Unknown(format!("{remote} said nothing about its head")),
    };
    let Some(head) = git(&root, &["rev-parse", "HEAD"]).ok() else {
        return News::Unknown("this checkout has no head to compare".into());
    };
    if head == tip {
        return News::Current;
    }
    // Behind, or ahead of it, and only now is a count worth a fetch: the number
    // is the thing a fetch is for.
    if let Err(why) = git(&root, &["fetch", "--quiet", remote]) {
        return News::Unknown(why);
    }
    let target = format!("{remote}/{branch}");
    match git(&root, &["rev-list", "--count", &format!("HEAD..{target}")]) {
        Err(_) => News::Unknown(format!("{target} is not a ref here")),
        Ok(count) => match count.parse::<usize>() {
            Ok(0) => News::Current,
            Ok(n) => News::Behind(n),
            Err(_) => News::Unknown(format!("git said {count:?}, which is not a count")),
        },
    }
}

/// `ls-remote --symref <remote> HEAD` says a branch line and a sha line. A
/// remote that is a path on disk - a clone of a clone, a file:// URL - answers
/// with **four** lines, because it lists the remote's own `origin/HEAD` as well:
///
/// ```text
/// ref: refs/heads/main\tHEAD
/// 52738a5...\tHEAD
/// ref: refs/remotes/origin/main\trefs/remotes/origin/HEAD
/// 52738a5...\trefs/remotes/origin/HEAD
/// ```
///
/// So both halves are read off the lines that name `HEAD` and nothing else:
/// taking the last `ref:` seen names a remote-tracking ref, which strips to no
/// branch at all, and the answer would be "nothing about its head" for a checkout
/// that is perfectly fine.
fn parse_symref(said: &str) -> Option<(String, String)> {
    let mut branch = None;
    let mut tip = None;
    for line in said.lines() {
        if let Some(rest) = line.strip_prefix("ref:") {
            let mut fields = rest.split_whitespace();
            let path = fields.next()?;
            if fields.next() == Some("HEAD") {
                branch = path.strip_prefix("refs/heads/").map(str::to_string);
            }
        } else if let Some((sha, what)) = line.split_once('\t') {
            if what.trim() == "HEAD" && !sha.trim().is_empty() {
                tip = Some(sha.trim().to_string());
            }
        }
    }
    Some((branch?, tip?))
}

/// The checkout this binary belongs to: the nearest `.git` above the working
/// directory, then above the binary itself. The binary first would be wrong for
/// a checkout being run from a build inside it, and the directory first would be
/// wrong for an installed binary launched from anywhere.
pub fn root() -> Option<PathBuf> {
    let mut from = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        from.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.canonicalize().unwrap_or(exe);
        if let Some(dir) = exe.parent() {
            from.push(dir.to_path_buf());
        }
    }
    for start in from {
        for dir in start.ancestors() {
            if dir.join(".git").exists() {
                return Some(dir.to_path_buf());
            }
        }
    }
    None
}

/// Run one git command in `dir`. The last line of stderr is the reason, because
/// git puts the useful sentence last and the rest of a failed command is context
/// nobody reads on one line.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        // A repo that wants a password must fail rather than sit on a prompt no
        // one is looking at.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        let why = text.trim().lines().last().unwrap_or("").to_string();
        return Err(if why.is_empty() { format!("git {} failed", args[0]) } else { why });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The id is a build stamp, so it has a shape: digits, a dot, a sha. A blank
    /// or a bare manifest number means `build.rs` never ran.
    #[test]
    fn the_id_names_a_build() {
        let (count, sha) = ID.split_once('.').expect("id is <count>.<sha>");
        assert!(!count.is_empty() && sha.len() >= 7, "id was {ID}");
    }

    /// Discovery has to find the checkout this test is running in, or every
    /// answer about updates is "unknown" no matter what the remote says.
    #[test]
    fn the_checkout_is_found() {
        let root = root().expect("no checkout above the crate");
        assert!(root.join("rust").is_dir(), "found {} which is not the tree", root.display());
    }

    /// The one round trip is read as the two answers it carries, and only off
    /// the lines that name `HEAD`. A remote that says only a sha has not named
    /// its branch, and the count would have nothing to compare against.
    #[test]
    fn a_symref_names_the_branch_and_the_tip() {
        let said = "ref: refs/heads/main\tHEAD\n52738a5923bc2bf0228811fe5529d95f0c8f6b93\tHEAD\n";
        assert_eq!(
            parse_symref(said),
            Some(("main".into(), "52738a5923bc2bf0228811fe5529d95f0c8f6b93".into()))
        );
        // A remote that is a path on disk answers with its own origin/HEAD too,
        // and that ref is not a branch: reading it as one loses the answer.
        let local = "ref: refs/heads/main\tHEAD\n52738a5\tHEAD\n\
                     ref: refs/remotes/origin/main\trefs/remotes/origin/HEAD\n52738a5\trefs/remotes/origin/HEAD\n";
        assert_eq!(parse_symref(local), Some(("main".into(), "52738a5".into())));
        assert_eq!(parse_symref("52738a5\tHEAD\n"), None);
        assert_eq!(parse_symref(""), None);
    }
}
