# Brand marks

The provider logos, so a provider chip wears **the provider's own mark** rather
than a Lucide approximation. Lucide has no brand glyphs at all, which is why the
first pass here was a pile of lookalikes: a "server" for ollama, a "bot" for
openai, a "hexagon" for a company whose mark is nothing like one.

Vendored from **lobe-icons** (`github.com/lobehub/lobe-icons`, MIT), whose
`static-svg/icons` set is the one that carries these marks at all: Simple Icons
has no OpenAI or Together entry, and no brand marks in any Lucide-style set.

```text
assets/brands/<key>.svg   one file per mark, named for its key
```

Registered with the running application through `assets::Brands` in `main.rs`,
which serves `brands/<key>.svg` out of the crate's own bytes. Nothing is fetched
at runtime: a provider list that needs the network to draw its own icons is a
list that is wrong when the network is.

## Which key a provider gets

`state::brand(provider)` is the one place that decides, and it falls back to
nothing rather than to a lookalike: a provider with no mark of its own gets the
letter, because a wrong logo is worse than no logo.

| provider | key | | provider | key |
|---|---|---|---|---|
| ollama | `ollama` | | openrouter | `openrouter` |
| openai | `openai` | | together | `together` |
| anthropic | `anthropic` | | deepseek | `deepseek` |
| Synthetic | `huggingface` | | nvidia | `nvidia` |
| qubrain | `glmv` | | | |

The keys are a vendor's own name in most cases, so a provider the user adds to
their `config.json` that happens to match one gets its mark for free.

## Licence

lobe-icons is MIT. The marks themselves belong to their owners and are used here
to identify the service being configured, which is what a brand mark is for. Do
not recolor them into this palette: a logo that has been tinted to match a theme
is no longer the recognisable thing it was there to be.
