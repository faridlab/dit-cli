# Code map benchmark

Repository: `serpa-webapp-admin` (6371 tracked files). Tokens are bytes / 4. A row's winner is the cheapest answer that is correct and complete; the others are shown with what they missed.

## Who imports `useResourceList` (blast radius before a change)

| Tool | Tokens | Calls | Correct | Notes |
|---|---:|---:|:---:|---|
| grep | 1889 | 2 | yes | 30/30 importers, 4 files that only mention it |
| dit **(winner)** | 286 | 1 | yes | 30/30 importers |
| graphify query | 1598 | 1 | no | 28/30 importers |
| graphify explain | 462 | 1 | no | 17/30 importers |

## What does `PayrollRunsPage.tsx` import

| Tool | Tokens | Calls | Correct | Notes |
|---|---:|---:|:---:|---|
| grep | 358 | 1 | yes | 16/16 imports, specifiers unresolved |
| dit **(winner)** | 201 | 1 | yes | 16/16 imports, resolved to files |
| graphify explain | 481 | 1 | no | 13/16 imports (internal files only) |

## How `SerpaShell` reaches `tokenStore`

| Tool | Tokens | Calls | Correct | Notes |
|---|---:|---:|:---:|---|
| dit **(winner)** | 21 | 1 | yes | a chain of 2 hops |
| graphify | 37 | 1 | yes | a chain of 2 hops |

## Where is token refresh handled (a question in words)

| Tool | Tokens | Calls | Correct | Notes |
|---|---:|---:|:---:|---|
| grep | 5283 | 1 | yes | 2/2 of the files to read |
| dit **(winner)** | 154 | 1 | yes | 2/2 of the files to read (2 in the top 5) |
| graphify query | 1609 | 1 | yes | 2/2 of the files to read |

## Keeping the map current

Seconds, each tool in a fresh clone of its own: build from nothing, refresh with nothing changed, check out 40 commits back, and return.

| Tool | Cold | Warm | Branch away | Branch back |
|---|---:|---:|---:|---:|
| dit (this build) | 3.21 | 0.15 | 0.32 | 0.27 |
| dit (baseline) | 13.22 | 0.23 | 1.01 | 1.09 |
| graphify update | 73.27 | 84.35 | 78.19 | 80.76 |

## Per-call overhead

graphify's search hook, when installed as a `PreToolUse` hook on `Bash|Grep`, adds **67 tokens** to every such call, and tells the agent it MUST run `graphify query` before grepping. dit installs no agent hook: 0 tokens per call.

## Tally

- dit: 4 of 4 questions
