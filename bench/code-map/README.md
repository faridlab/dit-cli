# Code map benchmark

What an agent pays to answer the questions it asks a codebase, with grep, graphify
and `dit code` — and whether the answer it gets is right.

```bash
python3 bench/code-map/bench.py \
  --repo ../serpa-webapp-admin \
  --dit target/release/dit \
  --dit-baseline /path/to/older/dit \
  --graphify ~/.local/bin/graphify \
  --out bench/code-map/RESULTS.md
```

The repository is cloned into a temporary directory and nothing in it is touched.
`--dit-baseline` and `--graphify` are optional. The run takes several minutes when
graphify is included, because each graphify build re-reads the whole tree.

## How a row is judged

- **Tokens** are bytes / 4, the usual rule of thumb for code and paths. It is an
  estimate, the same for every tool, so the comparison holds even where the
  absolute number is off.
- **Correct** is checked against a key the script computes itself from the source
  files, never from any tool's output. Who imports a symbol: the files whose import
  statements take it from its defining file, directly or through a barrel. What a
  file imports: its import statements. Where a question is answered: the files a
  person reads to answer it, named in `QUESTIONS`.
- **Calls** are the commands it takes to reach a complete answer. grep's list of
  files that mention a name mixes importers with mentions, so telling them apart is
  counted as a second call.
- **The winner** is the cheapest correct answer, by tokens × calls. A cheaper answer
  that misses part of the key cannot win.

## What it does not measure

- The reading after the answer. Every tool leaves files to read for a question in
  words; the benchmark counts the list, not the reading.
- Answer quality beyond the key — graphify's community labels or `dit code explain`'s
  export list are not scored.
- Timings on a busy machine move by tens of percent; the load average is worth
  noting beside a run, and the refresh rows compare tools run back to back.

The questions are written for serpa-webapp-admin (React and TypeScript). To point
it at another repository, change `QUESTIONS` to that repository's names.
