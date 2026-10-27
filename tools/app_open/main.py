from tools import _window


def run(args: dict, ctx) -> dict:
    app = args["app"]
    m = _window.backend()

    # Park this session's own window, not whatever is frontmost: mid-run that is
    # usually the app being driven, and the two swap places. A handle identifies
    # the window; a name only identifies the executable it belongs to.
    self_snapped = _window.park_self(getattr(ctx, "self_identity", None),
                                getattr(ctx, "self_handle", None))

    # Taken before the launch so anything it drags up (a helper process, a second
    # app it hands the request to) reads as new, not as always having been there.
    _window.baseline()

    handle = m.open_app(app)
    if not handle:
        return {"ok": True, "app": app, "snapped": False, "self_snapped": self_snapped,
                "note": "app opened but no window was detected in time; not snapped"}

    # The one snap that raises: the agent is about to drive this app, so it
    # has to end the call holding keyboard focus.
    app_snapped = m.snap_region(handle, *_window.APP_REGION, focus=True)
    if app_snapped:
        _window.remember(handle, app)
    # Accounted for now, so the next caller asking what is new does not park it again.
    _window.baseline()
    return {"ok": True, "app": app, "snapped": app_snapped, "self_snapped": self_snapped}
