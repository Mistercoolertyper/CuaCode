---
name: ask_user
output:
  answered: list
  error: str
active: True
require_permissions: False
backgroundable: False
---
Ask the user a question and wait for the answer, without ending your turn.

Use it when you genuinely cannot continue without a decision: which of two
approaches, which account, which file, whether to overwrite something. The turn
stays open, so the answer comes straight back and you carry on.

Give `options` whenever the answer is a choice rather than a sentence. A person
answering "a / b / c" with one keypress is a different experience from typing a
sentence, and a model that offers options gets a faster yes.

Do not use it as a progress report or to ask permission for something you were
already asked to do. And do not use it for a question you could answer yourself by
reading a file or running a command -- asking costs the person their attention,
which is the one thing here that is not free.
