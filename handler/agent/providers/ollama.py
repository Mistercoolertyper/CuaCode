"""ollama.com over the openai schema, which is the only way this provider is wired:
the native /api/chat dialect is gone. This file owns one thing the generic class
must not know: the key the desktop app leaves on disk.
"""

import os

from handler.agent.providers.openai import OpenAI

def _signed_in_key(path: str = "~/.ollama/keys") -> str:
    """The key `ollama signin` left behind, or "". The desktop app never shows a key anyone
    pastes: it writes one to this file as tab-separated name and key lines, the same bearer
    token the web console hands out, first line winning.
    """
    try:
        for line in open(os.path.expanduser(path)):
            parts = line.strip().split("\t")
            if len(parts) == 2 and parts[1]: return parts[1]
    except OSError: pass
    return ""

class OllamaOpenAI(OpenAI):
    """ollama.com over the openai schema. The key `ollama signin` leaves behind is this host's
    own business and is looked for here, never in the generic class: stored_key is the hook
    config.api_key and setup() both read.
    """
    def stored_key(self) -> str:
        return _signed_in_key()
