"""AppKit, with the worker kept out of the Dock.

The first call into AppKit registers the process with LaunchServices, and the
framework Python this runs under carries no LSUIElement, so the worker came up as a
regular app: a Dock tile named "Python", next to the frontend, whose Quit killed the
session. Prohibited is right for a process that drives other apps and owns no windows,
and it keeps the worker from taking focus away from the app being driven.

Every AppKit user goes through here rather than importing it directly, because the
policy has to be set on the first connection: set late, the tile appears and is then
withdrawn.
"""

_APPKIT = None


def appkit():
    """The AppKit module, policy already set. Raises if pyobjc is missing."""
    global _APPKIT
    if _APPKIT is None:
        import AppKit
        try:
            AppKit.NSApplication.sharedApplication().setActivationPolicy_(
                AppKit.NSApplicationActivationPolicyProhibited)
        except Exception:
            # An older pyobjc without the constant, or a policy change refused; not worth
            # failing the capture over.
            pass
        _APPKIT = AppKit
    return _APPKIT
