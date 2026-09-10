## Summary

<!-- The user-visible outcome in product terms, grounded in what exists
     today. If prototypes or screens drive the story, show the divergence
     concretely — a two-column "what each surface shows" table beats prose.
     Name real routes/tables/files (path:line) so every claim is checkable. -->

## Model

<!-- The data contract. Field tables (| Field | Note |) with one row per
     field; enums and their members listed out with a line each on who sets
     them and when they fire. If the model is embedded vs standalone, or
     append-only vs mutable, say it here — those are the decisions a reader
     needs first. -->

## Behaviour

<!-- Numbered rules, one per line, each testable on its own. Cover the
     edges that matter: visibility, ordering, who may write, what is
     derived-at-write-time vs read-time (and why). If another ticket raises
     or consumes part of this contract, reference it by number. -->

## Why on X, rather than derived from Y

<!-- The rejected alternative, written down. Example shape: the audit log
     has no notion of customer visibility, carries no customer-facing copy,
     and would need a per-row scan on render — so display data lives where
     it is read. One paragraph, one deciding property. -->

## Acceptance criteria

<!-- Assertable checkboxes. Where a rule is the point of the story (a
     visibility fence, an ordering, an append-only guarantee), assert it
     against the response the consumer actually sees — the JSON, not the
     schema definition. -->

- [ ]

## Tests

<!-- The file/suite, and which assertions are load-bearing. If the
     load-bearing test is an API-body assertion rather than a unit check,
     say so explicitly — that is the one that catches silent regressions. -->
