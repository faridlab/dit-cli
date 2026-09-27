# A map of the code, from git

`dit code` answers the questions an agent otherwise greps for, from the code at
HEAD, for TypeScript and Rust.

- **Register a root** under `code:` in `.dit/config.yaml` — this repository or a
  linked one, with `include`, `exclude` and `generated` globs.
- **Ask it.** `dit code users <file|symbol>` lists who imports it, followed through
  path aliases and barrel re-exports; `uses`, `explain`, `path`, `where` and `hubs`
  answer the rest. Each command refreshes first and re-reads only changed files: a
  6,000-file app maps cold in about twelve seconds and refreshes in a quarter of one.
- **Write down what the code cannot say.** A `dit-map` fence names, per task, the
  paths to change, the file to copy and the paths never to touch. `dit code check`
  says whether each entry holds, is unconfirmed, is stale or is broken, and
  `dit code map confirm <map>` pins it once someone has read it against the code.
- **The agent guide points at it**, and `dit ai spec code` goes deeper.
