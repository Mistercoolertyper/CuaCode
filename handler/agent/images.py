"""Keeping old screenshots out of the request. Every round resends the whole transcript, and
ten gridded captures at roughly 850KB base64'd each is 8.5MB up the wire before the first token.
Prompt caching does not help: cache_control saves the server re-reading the prompt, not the
client re-sending it. Nothing is lost, the tool archives every capture anyway, and eviction is
permanent and in place, so the cached prefix ahead of a rewritten message still matches.
"""

# Left where the image was, not deleted: a model that sees the gap asks for a fresh
# capture, one that finds a tool result it remembers as empty concludes the tool is broken.
NOTE = ("[screenshot removed from the transcript -- only the most recent captures are kept. "
        "Take a new screenshot if you need to see the screen.]")

DEFAULT_KEEP = 2


class PinnedUser(dict):
    """A user turn whose images the user attached by hand. Exempt from the window above, which
    exists for screenshots of a screen that has changed: an attached image is the subject, never
    goes stale, and no tool can take another. The exemption travels with the type, a plain dict.
    """


def _slots(msg: dict) -> list[tuple]:
    """Every encoded image in one message, in the order it appears. An image is a content block
    in both dialects here, and a tool result nests one a level down: (container, key) pairs, so
    the caller blanks one without knowing which shape it came from.
    """
    slots = []
    content = msg.get("content")
    if isinstance(content, list):
        for i, block in enumerate(content):
            if not isinstance(block, dict): continue
            kind = block.get("type")
            if kind in ("image", "image_url"):
                slots.append((content, i))
            elif kind == "tool_result" and isinstance(block.get("content"), list):
                # anthropic puts the image inside the tool result itself.
                for j, inner in enumerate(block["content"]):
                    if isinstance(inner, dict) and inner.get("type") in ("image", "image_url"):
                        slots.append((block["content"], j))
    return slots


def _blank(container: list, key: int) -> None:
    container[key] = {"type": "text", "text": NOTE}


def evict(messages: list[dict], keep: int = DEFAULT_KEEP) -> int:
    """Blank every image except the newest `keep`. Returns how many went. keep=0 is a real
    answer: a blind model's transcript replayed onto a sighted one has images nothing will ever
    look at again.
    """
    if keep < 0: return 0
    slots = [s for msg in messages if not isinstance(msg, PinnedUser) for s in _slots(msg)]
    doomed = slots[:-keep] if keep else slots
    for container, key in doomed:
        _blank(container, key)
    return len(doomed)
