"""Fan-out for agents. parallel() is a barrier, waiting for everything before returning
anything. pipeline() is not: each item walks every stage on its own, so the wall clock is the
slowest chain, not the sum of the slowest stage. A barrier is right only when a stage needs every
result of the one before it at once, as in a dedup. A failure comes back as None, so a rate limit
costs only its own item.
"""
import os
from concurrent.futures import ThreadPoolExecutor
from contextvars import copy_context

# Concurrent model requests, not cpu work: the threads sleep on a socket, so the cap is
# about not opening thirty billable streams because a list happened to be long.
MAX_WORKERS = min(8, max(2, (os.cpu_count() or 4) - 2))

def _safe(fn, on_error):
    try: return fn()
    except Exception as e:
        if on_error:
            try: on_error(e)
            except Exception: pass
        return None

def parallel(thunks, on_error=None) -> list:
    """Run every thunk, return results in the order given. Barrier. Takes zero-argument callables
    rather than (fn, args) pairs, so a caller binds with a lambda and this never has to know
    anything about arguments.
    """
    thunks = list(thunks)
    if not thunks: return []
    if len(thunks) == 1: return [_safe(thunks[0], on_error)]
    with ThreadPoolExecutor(max_workers=min(MAX_WORKERS, len(thunks))) as ex:
        # Copy the context here, on the calling thread: a worker otherwise starts from
        # ContextVar defaults, which resets the nesting depth. One per thunk, entered once.
        futures = [ex.submit(copy_context().run, _safe, t, on_error) for t in thunks]
        return [f.result() for f in futures]

def pipeline(items, *stages, on_error=None) -> list:
    """Every item through every stage, independently. No barrier between stages. Each stage is
    called (previous_result, original_item, index), the last two so a stage three deep can label its
    work without stage one threading the original through, and the first sees the item as both. A
    raise drops that item to None and skips its later stages, and the other items carry on.
    """
    items = list(items)
    def chain(index, item):
        value = item
        for stage in stages:
            value = stage(value, item, index)
        return value
    # Default arguments, not closure capture: a lambda closing over the loop
    # variable would see whatever it ended on by the time a thread ran it.
    return parallel([lambda i=i, it=it: chain(i, it) for i, it in enumerate(items)], on_error)
