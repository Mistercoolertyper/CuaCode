"""Is this shell command one that only looks at the machine?

A yes here skips the permission prompt, so anything not understood (an unknown command, a construct the tokenizer cannot take apart, a flag not on the list) comes back unsafe and the user is asked as before, and the list grows only by deliberate addition.
Three checks, in order: the raw text for redirects and command substitutions, which make the tokens meaningless; each segment split on its separators, since `date && rm -rf /` is not the `date` its first token says; each command's name against the read-only set, then its flags, since half of them have a mode that writes.
`destructive` asks the opposite question with the same tokenizer, because bypass mode asks nothing and this is then the only thing between the user and the machine, so it names the few calls there is nothing to undo after.
"""
import re
import shlex

from tools._safety import paths, rules

# Commands whose every invocation only reports: a command belongs here when no flag
# makes it write, and in a CHECKS entry when one does.
READ_ONLY = {
    # where am i / who am i
    "pwd", "date", "whoami", "id", "groups", "hostname", "uname", "uptime",
    "cal", "locale", "arch", "sw_vers",
    # looking at the filesystem
    "ls", "tree", "stat", "file", "du", "df", "basename", "dirname",
    "realpath", "readlink", "wc",
    # reading
    "cat", "bat", "head", "tail", "nl", "less", "more", "column",
    # filtering
    "grep", "egrep", "fgrep", "ag", "ack", "cut", "tr", "uniq", "comm", "join",
    "paste", "fold", "rev", "tac", "diff", "cmp", "strings",
    # trivia
    "echo", "printf", "seq", "true", "false",
    # what is installed / what is running
    "which", "whereis", "type", "ps", "pgrep", "shasum", "md5", "md5sum",
    "sha1sum", "sha256sum",
}

# Constructs that make the token list a lie, checked against the raw string, quotes and
# all: a `>` inside a quoted grep pattern is still flagged, one needless prompt the price.
_RAW_BAD = (
    (">", "redirects output"),
    ("<", "redirects input"),
    ("`", "runs a command substitution"),
    ("$(", "runs a command substitution"),
    ("${", "expands a variable"),
    ("$[", "expands an expression"),
)

# The separators a segment may end on. Anything else the tokenizer hands back as
# punctuation ends the analysis rather than being skipped over.
_SEPARATORS = {"|", "||", "&&", ";", "&", "|&", ";;"}

def _git(argv: list[str]) -> bool:
    """git is not one program, and its subcommands are different answers."""
    args = [a for a in argv[1:] if not a.startswith("-")]
    if not args: return True                      # bare `git`, prints usage
    sub, rest = args[0], args[1:]
    if sub in {"status", "log", "diff", "show", "blame", "shortlog", "ls-files",
               "ls-tree", "rev-parse", "describe", "whatchanged", "cat-file",
               "count-objects", "reflog", "grep", "annotate", "var", "help",
               "diff-tree", "name-rev", "check-ignore", "verify-commit"}:
        return True
    # `git config x y` writes the config; only the readers are safe.
    if sub == "config":
        return any(f in argv for f in ("--get", "--get-all", "--get-regexp", "--list", "-l"))
    # `git branch -d`, `git tag -d`, `git remote add` -- the listing forms have
    # no operand and no flag beyond the display ones.
    if sub in {"branch", "tag", "remote", "worktree"}:
        return not rest and all(a in {"-a", "-v", "-vv", "-r", "-l", "--list", "--all",
                                      "--verbose", "--remote", "--sort"} or a.startswith("--sort=")
                                for a in argv[2:])
    if sub == "stash":
        return bool(rest) and rest[0] in {"list", "show"}
    return False

def _find(argv: list[str]) -> bool:
    """find walks, and then find runs whatever you tell it on what it found."""
    bad = {"-delete", "-exec", "-execdir", "-ok", "-okdir", "-fls", "-fprint",
           "-fprint0", "-fprintf", "-x", "-X", "--exec", "--exec-batch"}
    return not any(a in bad for a in argv[1:])

def _sed(argv: list[str]) -> bool:
    return not any(a == "--in-place" or (a.startswith("-") and not a.startswith("--") and "i" in a)
                   for a in argv[1:])

def _no_flags(*bad):
    """For the readers that grew one flag that writes: `sort -o`, `yq -i`, `rg --pre`, which hands rg a
    program to run on every file it opens. Clusters count, since `-uo out.txt` is `sort -o` with company
    and a check comparing whole arguments alone would wave it through."""
    shorts = {b[1] for b in bad if len(b) == 2 and b[0] == "-"}
    def check(argv: list[str]) -> bool:
        for a in argv[1:]:
            if a in bad or any(a.startswith(f"{b}=") for b in bad): return False
            if a.startswith("-") and not a.startswith("--") and shorts & set(a[1:]): return False
        return True
    return check

def _awk(argv: list[str]) -> bool:
    """awk is a language with a shell in it. The redirect forms are already
    gone with the raw `>` check; these are the two that are left."""
    return not any(re.search(r"system\s*\(|\||getline|ENVIRON", a) for a in argv[1:])

def _gh(argv: list[str]) -> bool:
    """`gh pr list` reports and `gh pr create` opens a pull request, so the
    noun alone does not decide it."""
    args = [a for a in argv[1:] if not a.startswith("-")]
    if len(args) < 2: return False
    return args[0] in {"pr", "issue", "repo", "run", "release", "workflow", "cache"} \
        and args[1] in {"list", "view", "status", "diff", "checks"}

def _subcommand_only(*allowed):
    """For the package managers, where the read-only half is a handful of
    named subcommands and everything else installs something."""
    def check(argv: list[str]) -> bool:
        args = [a for a in argv[1:] if not a.startswith("-")]
        return bool(args) and args[0] in allowed
    return check

CHECKS = {
    "git": _git,
    "gh": _gh,
    "find": _find,
    "fd": _find,
    "sed": _sed,
    "awk": _awk, "gawk": _awk, "mawk": _awk,
    "sort": _no_flags("-o", "--output"),
    "base64": _no_flags("-o", "--output"),
    "jq": _no_flags("-i", "--in-place"),
    "yq": _no_flags("-i", "--in-place", "--inplace"),
    "rg": _no_flags("--pre", "--hostname-bin", "--search-zip", "-z"),
    "npm": _subcommand_only("ls", "list", "view", "info", "outdated", "why", "root", "prefix"),
    "pnpm": _subcommand_only("ls", "list", "why", "outdated", "root"),
    "yarn": _subcommand_only("list", "info", "why"),
    "pip": _subcommand_only("list", "show", "freeze", "check"),
    "pip3": _subcommand_only("list", "show", "freeze", "check"),
    "brew": _subcommand_only("list", "info", "outdated", "config", "--version"),
    "docker": _subcommand_only("ps", "images", "logs", "inspect", "version", "info"),
    "kubectl": _subcommand_only("get", "describe", "logs", "top", "version", "explain"),
    "go": _subcommand_only("version", "env", "list", "doc"),
    "cargo": _subcommand_only("--version", "tree", "metadata"),
}

def _allowed_name(name: str) -> bool:
    deny = rules.extra("shell_deny")
    if name in deny: return False
    return name in READ_ONLY or name in CHECKS or name in rules.extra("shell_allow")

def _segments(line: str) -> list[list[str]] | None:
    """The line as a list of commands, or None when it cannot be read as one.

    punctuation_chars is what makes `a|b` inside quotes stay one token while a bare pipe becomes its
    own; splitting the string first would cut a grep pattern in half."""
    lx = shlex.shlex(line, posix=True, punctuation_chars=True)
    lx.whitespace_split = True
    try: tokens = list(lx)
    except ValueError: return None            # unbalanced quote
    out, cur = [], []
    for t in tokens:
        if t in _SEPARATORS:
            if cur: out.append(cur)
            cur = []
        elif re.fullmatch(r"[|&;<>()]+", t):
            return None                       # punctuation with a meaning we do not model
        else:
            cur.append(t)
    if cur: out.append(cur)
    return out

def is_read_only(command: str) -> tuple[bool, str]:
    """(safe, reason). The reason is for a log or a test, not for the model, since an
    unsafe answer only ever becomes a prompt the user was going to get."""
    cmd = (command or "").strip()
    if not cmd: return False, "empty"
    for needle, why in _RAW_BAD:
        if needle in cmd: return False, f"{needle} {why}"
    # Newlines are separators too, and shlex would glue the lines together into
    # one command whose first token vouches for all of them.
    for line in cmd.splitlines():
        line = line.strip()
        if not line or line.startswith("#"): continue
        segs = _segments(line)
        if segs is None: return False, "cannot be parsed as a plain command"
        for argv in segs:
            name = argv[0].rsplit("/", 1)[-1]
            if not _allowed_name(name): return False, f"{name} is not on the read-only list"
            check = CHECKS.get(name)
            if check and not check(argv): return False, f"this form of {name} can change things"
            for a in argv[1:]:
                if a.startswith("-"): continue
                if paths.is_sensitive(a): return False, f"touches {a}"
    return True, "read-only"

# What a disk-erasing diskutil subcommand is called. Kept as words rather than a
# prefix match: `diskutil list` and `diskutil info` touch nothing.
_DISKUTIL_WIPES = {"eraseDisk", "eraseVolume", "reformat", "partitionDisk",
                   "zeroDisk", "secureErase", "randomDisk"}

def _rm_wipes(argv: list[str]) -> bool:
    """`rm` with recursion and force both set: the pair that means the answer to "is it
    gone" is yes, whatever it was. Either one alone is recoverable in practice (`rm -r`
    asks about a read-only file, `rm -f` takes files) and neither is worth a prompt."""
    rec = force = False
    for a in argv[1:]:
        if a == "--recursive": rec = True
        elif a == "--force": force = True
        elif a.startswith("-") and a != "-":
            rec = rec or "r" in a[1:] or "R" in a[1:]
            force = force or "f" in a[1:]
    return rec and force

def destructive(command: str) -> str:
    """Why bypass mode must not wave this command through, or "".

    A floor, not a screen: a recursive forced delete, a write straight onto a device, a reformat, and nothing else. Read by
    shape, and a delete hidden inside another command's own string (`bash -c "rm -rf /"`) is not seen."""
    for line in (command or "").splitlines():
        for argv in (_segments(line) or []):
            # `sudo rm -rf /` is the same call wearing a prefix: step over the
            # wrappers rather than let one of them vouch for what follows.
            i = 0
            while i < len(argv) and argv[i].rsplit("/", 1)[-1] in ("sudo", "doas"): i += 1
            argv = argv[i:]
            if not argv: continue
            name = argv[0].rsplit("/", 1)[-1]
            if name == "rm" and _rm_wipes(argv): return "rm -rf"
            if name == "dd" and any(a.startswith("of=/dev/") for a in argv[1:]):
                return "dd writing straight to a device"
            if name.startswith("mkfs"): return "mkfs reformats a filesystem"
            if name == "diskutil" and any(a in _DISKUTIL_WIPES for a in argv[1:]):
                return "diskutil erases a disk"
    return ""

def _self_check():
    """Run: PYTHONPATH=. python3 tools/_safety/shell.py"""
    wipes = ["rm -rf /", "rm -rf build", "rm -fr x", "rm -Rf x", "sudo rm -rf /tmp/x",
             "date && rm -rf /", "dd if=/dev/zero of=/dev/disk2", "mkfs.ext4 /dev/sda1",
             "diskutil eraseDisk JHFS+ Free /dev/disk2"]
    keeps = ["ls -la", "rm -f a.txt", "rm -r a", "dd if=x of=out.bin", "git status",
             "echo rm -rf /", "diskutil list", "rm --recursive --force x"]
    for c in wipes: assert destructive(c), c
    for c in keeps[:-1]: assert not destructive(c), c
    assert destructive(keeps[-1]) == "rm -rf", keeps[-1]
    assert is_read_only("ls -la")[0] and not is_read_only("rm -rf /")[0]

if __name__ == "__main__":
    _self_check()
    print("ok")
