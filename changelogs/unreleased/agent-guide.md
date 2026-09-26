# The agent guide teaches how to work here

`dit ai init` wrote a guide to DIT's rules. It now tells an agent what it
cannot learn from the code, and stays short enough to be read whole.

- **Every rules file, and which one wins.** The guide lists each committed
  `CLAUDE.md` and `AGENTS.md` in the workspace and in every linked code
  repository, read at HEAD, with the rule that the nearest file governs.
- **The pointer carries the rules that cost most when broken** — never edit an
  issue by hand, pick with `dit ready` and claim first, move the status as you
  work, run `dit morse check` before building on an endpoint.
- **Recipes and topics.** Common tasks as the commands that do them, in order;
  `dit ai spec issues|flow|morse` goes deeper on one subject.
- **Seams.** How to find out whether an endpoint was proven, on which
  environment, and what holds an issue back.
- **`dit ai init` refuses outside a workspace**, naming `dit init`, instead of
  writing a guide to issues no command could create; `dit init --ai` creates
  the workspace and installs the guide in one step.
