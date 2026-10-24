"""The provider registry. One module per wire dialect -- openai
chat-completions, anthropic messages -- with the shared plumbing in base.py.
A host with its own key lookup (ollama.com) subclasses the dialect it speaks.

A registry key is the provider you pick in settings; the class behind it is
whichever wire dialect that provider speaks."""

import copy

from handler.agent.providers.base import (
    Delta, CallAssembler, usage_of, attachment_note, append_user_text, _daemon_up)
from handler.agent.providers.ollama import OllamaOpenAI
from handler.agent.providers.openai import OpenAI
from handler.agent.providers.anthropic import Anthropic

PROVIDERS = {
    # ollama.com over the openai schema. models_url is /api/tags because /v1/models lists bare
    # names and no window; show_url is where one model's window and abilities come from, which
    # is what /context and the vision check read. The dict is a shortlist, best first: it orders
    # /models and its top entry is the model used when settings name none. The scores are yours.
    "ollama":     OllamaOpenAI({"deepseek-v4.1-flash": 10, "glm-5.3-flash": 9, "glm-5.3": 7},
                               "https://ollama.com/v1", "OLLAMA_API_KEY",
                               models_url="https://ollama.com/api/tags",
                               show_url="https://ollama.com/api/show"),
    "openai":     OpenAI("gpt-5.6-sol"),
    "anthropic":  Anthropic(),
    "nvidia":     OpenAI("minimaxai/minimax-m3", "https://integrate.api.nvidia.com/v1", "NVIDIA_API_KEY"),
    "openrouter": OpenAI("poolside/laguna-s-2.1:free", "https://openrouter.ai/api/v1", "OPENROUTER_API_KEY", vision=False),
    "deepseek":   OpenAI("DeepSeek-V4-Flash-Vision-Exp", "https://api.deepseek.com", "DEEPSEEK_API_KEY", vision=True),
    "together":   OpenAI("MiniMaxAI/MiniMax-M3", "https://api.together.xyz/v1", "TOGETHER_API_KEY"),
    "qubrain":    OpenAI("glm-5.2", "https://qubrain.org/v1", "QB_API_KEY"),
    "Synthetic":  OpenAI("hf:zai-org/GLM-5.3-Flash", "https://api.synthetic.new/openai/v1", "SYNTHETIC_API_KEY")
    # Any endpoint speaking the OpenAI schema can be added here, local or hosted.
    #
    # A provider reads its own catalog by default: /v1/models for this dialect, /api/tags for
    # ollama.com. When that endpoint is missing or answers in a shape nobody can read, models_url
    # (a constructor argument on the openai class, so it sits on the entry like base_url) names
    # the URL that does, and models() reads that instead:
    #     "somehost": OpenAI("model", "https://host/v1", "HOST_KEY", models_url="https://host/api/tags")
    #
    # show_url names where to ask about one model when the dialect cannot say: a POST of
    # {"model": name} answering with capabilities, and ollama's model_info.<arch>.context_length for
    # the window. It is what /context's "N left of M" and the vision check read.
    #     "somehost": OpenAI("model", "https://host/v1", "HOST_KEY", show_url="https://host/api/show")
    #
    # In place of one default_model, the first argument takes a dict of names to scores. The
    # highest score is the default, /models lists the shortlist above the host's own catalog,
    # and equal scores keep the order they were written in.
    #     "somehost": OpenAI({"big": 9, "small": 4}, "https://host/v1", "HOST_KEY")
}

def get(name: str):
    p = PROVIDERS.get(name)
    if p is None: raise ValueError(f"unknown provider: {name!r} (have {sorted(PROVIDERS)})")
    return p

# Instance state a copy must not inherit, and what it resets to. The client is
# rebuilt rather than shared: it is cheap to rebuild and the key it was built
# with may not be this run's.
_FRESH = {"_c": None, "_thinking": [], "_key": ""}

def new(name: str):
    """An instance of the same provider that shares nothing mutable.

    The registry holds one instance per provider and they carry per-turn state:
    Anthropic hands thinking blocks from stream() to assistant_message() through
    self._thinking, and every class caches a client. Two loops at once is a data
    race: agent A's thinking blocks, signature and all, land in agent B's
    message and the request is rejected.

    Copied rather than reconstructed because the openai class carries its
    identity in constructor arguments -- base_url, key_env, default_model,
    vision -- and there is no registry of what each entry was built with.
    """
    p = copy.copy(get(name))
    for attr, val in _FRESH.items():
        if hasattr(p, attr): setattr(p, attr, list(val) if isinstance(val, list) else val)
    return p