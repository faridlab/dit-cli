# The code map

`dit code` answers the questions an agent — or a person — asks before changing code:
who imports this, what does it depend on, how does one file reach another, which files
answer a question. It reads the code from git, keeps an index next to it, and answers in
one short reply instead of a page of grep matches. The design and its trade-offs are in
[ADR 0025](adr/0025-code-map-derived-graph-and-pinned-intent.md); this page is how to use
it and what it costs, measured.

## Two ways to run it

- **In any repository.** Run `dit code …` inside a git repository that is not a DIT
  workspace and it maps that repository: no `dit init`, no config. The index lives in
  `.dit/code/`, which carries its own `.gitignore`, so nothing is committed and the
  repository's `.gitignore` is left alone.
- **In a DIT workspace, across repositories.** Register code roots in
  `.dit/config.yaml` and the workspace maps them together — which is what the seam link
  (`dit code api`, calls in one repository read against the API specs of another) and the
  map of intent (`dit-map` fences) need.

```yaml
repos:
  - { name: webapp, remote: ../webapp }
code:
  - { id: web, repo: webapp, include: ["src/**"], generated: ["src/generated/**"], ref: main }
```

`ref:` is optional. Without it the map follows whatever the linked checkout has on HEAD;
with it, that branch or tag is read through git without checking it out.

## Commands

| Command | Answers |
|---|---|
| `dit code users <file\|symbol>` | Who imports it, through path aliases and barrel re-exports — what breaks if it changes. |
| `dit code uses <file> [--calls]` | What it imports, by the file each import reaches; `--calls` adds the calls it makes. |
| `dit code path <a> <b>` | The shortest import chain between two files or symbols. |
| `dit code explain <name>` | What a file defines, whether it is generated, and who uses it. |
| `dit code where <words…>` | The files to read for a question in words, best first. |
| `dit code hubs` | The most depended-on files, generated ones left out. |
| `dit code api [--all]` | Path literals in the code against the registered specs: orphan calls, unproven seams. |
| `dit code check` / `dit code map confirm <map>` | The map of intent, judged against the code; confirm it after reading it. |
| `dit code refresh [--full]` | Bring the map up to HEAD now (every command above does this first). |
| `dit code hook install` | Opt in: refresh in the background after every commit, merge, checkout and rebase. |

Languages: TypeScript, TSX and JavaScript, Rust, Kotlin. The map covers committed code:
a file written but not committed appears after its commit.

## What it costs, measured

`bench/code-map/bench.py` asks four questions an agent asks before a change, answers each
with grep, graphify and `dit code`, and judges every answer against a key the script
computes from the source itself — never from any tool's output. Tokens are estimated as
bytes ÷ 4, the same for every tool. A row's winner is the cheapest answer that is
correct and complete: an answer that misses part of the key cannot win, however small.

The run below: serpa-webapp-admin (6,371 tracked files, React and TypeScript), dit 0.9.0,
dit 0.8.0 as the baseline, graphify's code graph (`graphify update`), 2026-09-27 on a
shared machine at load average 4–14. The raw output is
[`bench/code-map/RESULTS.md`](../bench/code-map/RESULTS.md).

### The questions

| Question | grep | graphify | dit code |
|---|---|---|---|
| Who imports `useResourceList` | 1,889 tokens, 2 calls — all 30 importers plus 4 files that only mention the name | `query` 1,598 tokens, 28 of 30; `explain` 462 tokens, 17 of 30 | **286 tokens, 30 of 30** |
| What `PayrollRunsPage.tsx` imports | 358 tokens, all 16, specifiers unresolved | 481 tokens, 13 of 16 — packages not recorded | **201 tokens, 16 of 16, resolved to files** |
| How `SerpaShell` reaches `tokenStore` | — | 37 tokens | **21 tokens** |
| Where token refresh is handled | 5,283 tokens | 1,609 tokens | **154 tokens**, both files a person reads in the top five |

`dit code` gave the cheapest correct answer to all four. graphify's `query` answers from
a breadth-first slice under a ~2,000-token budget and says when it truncated — here 66 of
709 nodes — so a complete answer can take another call; `explain` on the symbol listed
17 of the 30 importing files among its edges.

### Keeping the map current

Seconds, each tool in a fresh clone of its own, run back to back:

| | Cold build | Warm, nothing changed | 40 commits back | And return |
|---|---:|---:|---:|---:|
| **dit 0.9.0** | **3.2** | **0.15** | **0.32** | **0.27** |
| dit 0.8.0 | 13.2 | 0.23 | 1.01 | 1.09 |
| `graphify update` | 73.3 | 84.4 | 78.2 | 80.8 |

Every `dit code` command refreshes first, reading only blobs it has not seen, so the map is
never older than the last commit. graphify rebuilds on `update` and keeps the graph from
the last time someone ran it; in the repositories this was measured beside, the graphs
agents were reading were two and three weeks old.

### Per call

graphify's search hook, installed as a Claude Code `PreToolUse` hook on `Bash|Grep`, adds
67 tokens to every such call and tells the agent it must run `graphify query` before
grepping. `dit code` installs nothing into the agent: no cost until it is asked.

### What the benchmark changed

Running it found three costs in `dit code` itself, fixed in 0.9.0:

- `dit code uses` repeated every specifier beside its target and listed every call:
  525 tokens for the file above, now 201, with calls behind `--calls`.
- A single root printed its name on every line of every answer.
- Four per-file tables had no `(root, path)` index, so replacing one file's rows scanned
  whole tables: a build grew with the square of the file count. Indexing them took a cold
  build of the same 6,299 files from 11.0 s to 2.5 s.

## What it does not do

- Answer a question in prose. `dit code where` returns the files to read; reading them is
  still the work.
- See a path built at runtime (`dit code api` reads literals), or tell calls apart by
  HTTP method.
- Map files it has no grammar for — schema YAML, SQL, Markdown. They are not indexed.
- Draw the graph. A rendered view in `dit ui` is planned; the index already holds what it
  needs.

## Re-running it

```bash
python3 bench/code-map/bench.py --repo <repo> --dit target/release/dit \
  --dit-baseline <older dit> --graphify <graphify> --out bench/code-map/RESULTS.md
```

The questions are written for serpa-webapp-admin; point `QUESTIONS` in the script at
another repository's names to reuse it. [`bench/code-map/README.md`](../bench/code-map/README.md)
explains the judging and its limits.
