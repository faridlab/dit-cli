## Summary

<!-- The problem in product terms, grounded in what exists today. Name real
     routes, tables, services, files (path:line) so a reader with the repo
     open can verify every claim. If a prototype, report, or incident drives
     the work, say what it shows. 2–6 sentences; a wall of text hides the
     point. -->

## Evidence

<!-- What the codebase actually holds right now. A small table works well:
     | Thing | Where | What it is |
     If inspection corrects the request's premise, say so here — a ticket is
     allowed to correct its own request. Skip this section only when the
     Summary already carries the pointers. -->

## Plan

<!-- The change, in order. For data contracts use field tables
     (| Field | Note |); for endpoints give the signature and the response
     shape. Behaviour rules are numbered, one rule per line, each testable
     on its own — "1. Events are append-only. Nothing edits or removes one."
     not "handle events properly". -->

## Why this shape, and not the alternative

<!-- Record the design decision AND the rejected option in one short
     paragraph: what was considered, what was chosen, the single property
     that decided it. Readers (human and agent) re-litigate less when the
     trade-off is on record. -->

## Acceptance criteria

<!-- Each box must be assertable by a test or a one-minute manual check.
     Name the exact assertion where it matters ("asserted on the JSON for
     each of X, Y, Z"), never just "works". -->

- [ ]

## Tests

<!-- Where the tests live (file or suite) and which assertions are
     load-bearing — the ones whose silent pass would still leave the feature
     broken. Include the command that runs them. -->

## Do not

<!-- The scope fence: adjacent work this ticket deliberately avoids, and
     where that work goes instead (a follow-up issue number when known). -->
