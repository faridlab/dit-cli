# Git fixtures

Synthetic repositories that reproduce the traps found while designing DIT.
See `DESIGN.md` §19.1 and `ARCHITECTURE.md` §6.3.

**The rule: every bug fix ships with a fixture that reproduces it.** This
directory only grows.

Each fixture is a builder function that constructs a real git repo in a tempdir.
Never a mock — the whole point is that these behaviors are surprising until you
run them.

| Fixture | Reproduces |
|---|---|
| `merge_resolved_by_driver` | `git log -p` without `-m` yields zero events for merge commits |
| `treesame_after_merge` | `git log` without `--full-history` hides legitimate commits |
| `archived_issue_rename` | History breaks when keyed to path instead of `issue_id` |
| `rebased_history` | Rebase rewrites committer dates |
| `backward_clock` | `ts` ordering vs `seq` ordering diverge |
| `root_commit_null_parent` | NULL in a SQLite PRIMARY KEY does not prevent duplicates |
| `driver_binary_missing` | File left with no conflict markers — Risk #0 |
| `half_merged_into_fmt` | `dit fmt` eats conflict markers |
| `delete_modify_conflict` | The merge driver is never invoked |
| `rename_modify_conflict` | The merge driver is never invoked |
| `df_conflict_branch_names` | `dit` + `dit/<x>` cannot coexist as refs |
