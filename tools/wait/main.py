import time

# The loop is blocked for as long as this sleeps, so it is capped. A model that
# asks for a minute gets the cap, and is told what it actually got: reporting the
# number it asked for made a one-second wait indistinguishable from a ten-second
# one to anything reading the result.
CAP = 10.0

def run(args: dict, ctx) -> dict:
    asked = args.get("seconds", 1.0)
    waited = min(asked, CAP)
    time.sleep(waited)
    out = {"waited": waited}
    if waited < asked: out["asked"] = asked
    return out
