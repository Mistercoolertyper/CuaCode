"""Fetch one page, hand back either the whole thing or what a subagent made of it.

The fetch is ours and only the html-to-markdown step is trafilatura's: the
redirect budget, byte cap, timeout and reachable hosts are policy.
"""
import ipaddress, os, re, socket
from urllib.parse import urlparse

from handler.agent.subagent import AgentSpec, run as run_agent

UA = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/124.0 Safari/537.36")
TIMEOUT = 20.0
MAX_BYTES = 5 * 1024 * 1024
MAX_CHARS = 50_000
MAX_REDIRECTS = 5

# Not a page. Checked before the body is read: content-length is no help.
TEXTUAL = ("text/html", "application/xhtml", "text/plain", "text/markdown",
           "application/json", "application/xml", "text/xml")

DIGEST_PROMPT = """You read one web page and report what it says about a single goal.

You have no tools and no way to fetch anything else -- the page is already in
front of you, in full. Work only from it.

Rules:
- Answer the goal, not the page. Skip everything the goal did not ask for.
- Quote verbatim for anything exact: numbers, flags, names, code, limits.
  Paraphrase is fine for explanation and wrong for values.
- If the page does not address the goal, say so plainly and set found false.
  A confident answer assembled out of a page that never said it is the one
  failure that costs more than fetching nothing.
- Never guess at what a truncated page probably said next."""

DIGEST = AgentSpec(
    name="digest", tools=[], effort="low", max_rounds=3, system=DIGEST_PROMPT,
    schema={"properties": {
        "answer": {"type": "string",
                   "description": "What this page says about the goal. Empty if it says nothing."},
        "found": {"type": "boolean",
                  "description": "False if the page does not address the goal at all."},
        "quotes": {"type": "array", "items": {"type": "string"},
                   "description": "Up to 3 verbatim lines backing the answer. Exact numbers, flags and code go here."},
        "next_urls": {"type": "array", "items": {"type": "string"},
                      "description": "Links on this page that look more likely to answer the goal than this page did."}},
     "required": ["answer", "found"]})

def _private(host: str) -> bool:
    """Whether a hostname lands anywhere on this machine or its network.

    The permission prompt covers fetching a url, not the user's router admin
    page or a cloud metadata endpoint, and the model picks the host. Resolved
    rather than pattern matched, because a public name may point at 127.0.0.1.
    """
    try: infos = socket.getaddrinfo(host, None)
    except OSError: return False                # unresolvable: the request fails on its own
    for info in infos:
        try: ip = ipaddress.ip_address(info[4][0])
        except ValueError: continue
        if (ip.is_private or ip.is_loopback or ip.is_link_local or ip.is_reserved
                or ip.is_multicast or ip.is_unspecified):
            return True
    return False

def _fetch(url: str) -> tuple:
    """(text, content_type, final_url). Raises ValueError with something worth
    reading on any refusal."""
    import httpx
    with httpx.Client(follow_redirects=True, timeout=TIMEOUT,
                      max_redirects=MAX_REDIRECTS, headers={"User-Agent": UA}) as c:
        with c.stream("GET", url) as r:
            if r.status_code >= 400:
                raise ValueError(f"{r.status_code} {r.reason_phrase} from {r.url}")
            ctype = (r.headers.get("content-type") or "").split(";")[0].strip().lower()
            if ctype and not any(ctype.startswith(t) for t in TEXTUAL):
                raise ValueError(f"not a readable page: content-type {ctype}")
            # Capped while reading, not after: content-length is optional and a
            # server may lie about it.
            chunks, size = [], 0
            for chunk in r.iter_bytes():
                size += len(chunk)
                if size > MAX_BYTES:
                    raise ValueError(f"page over {MAX_BYTES // 1024 // 1024}MB, refusing to load it")
                chunks.append(chunk)
            body = b"".join(chunks)
            enc = r.encoding or "utf-8"
    return body.decode(enc, errors="replace"), ctype, str(r.url)

_IMG = re.compile(r"!\[([^\]]*)\]\([^)]*\)")
_LINK = re.compile(r"(?<!!)\[([^\]]*)\]\(([^)\s]*)\)")

def _delink(md: str, base: str) -> str:
    """Unlink same-page anchors.

    A `#section` href resolves against the site, not the page, so it comes out
    as a plausible url that is not the page it came from. The links returned
    here are fetch targets, so the text stays and the target goes.
    """
    root = f"{urlparse(base).scheme}://{urlparse(base).hostname}"
    def sub(m):
        text, href = m.group(1), m.group(2)
        head, _, frag = href.partition("#")
        if frag and head in ("", base, root, root + "/"): return text
        return m.group(0)
    return _LINK.sub(sub, md)

def _markdown(html: str, url: str) -> str:
    import trafilatura
    md = trafilatura.extract(html, output_format="markdown", url=url, include_links=True,
                             include_images=True, include_tables=True, with_metadata=False)
    if not md:
        # Boilerplate removal sometimes judges the whole page to be boilerplate;
        # an empty result costs a refetch, so retry with recall favored.
        md = trafilatura.extract(html, output_format="markdown", url=url, favor_recall=True,
                                 include_links=True, include_tables=True)
    if not md: raise ValueError("nothing extractable on the page (js-rendered, or not an article)")
    # Images become alt text: the model cannot follow an image url to anything
    # it can see.
    md = _IMG.sub(lambda m: f"[image: {m.group(1)}]" if m.group(1) else "[image]", md)
    return _delink(md, url)

def run(args: dict, ctx) -> dict:
    url, goal = args["url"], args["goal"]
    mode = args.get("mode", "digest")

    u = urlparse(url)
    if u.scheme not in ("http", "https") or not u.hostname:
        return {"error": "url must be absolute and http(s), e.g. https://example.com/page"}
    if _private(u.hostname) and os.environ.get("CUACODE_ALLOW_PRIVATE_FETCH") != "1":
        return {"error": f"{u.hostname} resolves to a private or local address; refusing to fetch it"}

    try:
        text, ctype, final = _fetch(url)
    except Exception as e:
        return {"error": str(e)}

    if ctype.startswith("text/html") or ctype.startswith("application/xhtml") or not ctype:
        try: body = _markdown(text, final)
        except Exception as e: return {"error": str(e)}
    else:
        body = text                                  # already plain text, json or xml

    truncated = len(body) > MAX_CHARS
    if truncated:
        body = body[:MAX_CHARS] + f"\n\n[truncated: {len(body) - MAX_CHARS} more characters]"

    if mode == "full":
        return {"website": final, "output": body, "truncated": truncated}

    r = run_agent(DIGEST, f"Goal: {goal}\n\n<page url=\"{final}\">\n{body}\n</page>", ctx=ctx)
    if r.get("error"):
        # The page was fetched either way, so hand back the markdown rather than
        # making the caller fetch it again.
        return {"website": final, "output": body, "truncated": truncated,
                "note": f"digest failed ({r['error']}), returning the full page instead"}
    return {"website": final, "output": r["output"], "truncated": truncated, "rounds": r["rounds"]}
