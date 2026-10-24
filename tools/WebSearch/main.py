"""Search the web and hand back ranked results (title, url, snippet).

The ddgs import is lazy, so launching the worker never pays for it.
"""


def _search(query: str, max_results: int) -> list[dict]:
    """Run the query. Extracted so a fake backend can slot in for tests."""
    from ddgs import DDGS

    with DDGS() as ddgs:
        raw = ddgs.text(query, max_results=max_results, region="us-en")

    results = []
    for r in raw:
        results.append({
            "title": r.get("title", ""),
            "url": r.get("href", ""),
            "snippet": r.get("body", "")[:1000],
        })
    return results


def run(args: dict, ctx) -> dict:
    query = (args.get("query") or "").strip()
    if not query: return {"error": "query must be non-empty"}

    max_results = min(int(args.get("max_results", 5)), 35)

    try: results = _search(query, max_results)
    except Exception as exc: return {"error": f"search failed: {exc}"}

    if not results:return {"results": [], "note": "no results for query"}

    return {
        "results": results,
        "count": len(results),
        "truncated": len(results) >= 35,
    }
