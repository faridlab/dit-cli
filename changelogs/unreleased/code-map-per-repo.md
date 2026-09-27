# Every repository maps itself

`dit code` now works in any git repository, not only in a DIT workspace. Run it in
a code repository and it maps that repository — no `dit init`, no config — with its
index in `.dit/code/`, where you can see it. A `.gitignore` inside that directory
keeps it out of every commit, so the repository's own `.gitignore` is not touched.

Generated files are recognised by path (`generated/`, `*.gen.*`, …) and, in every
repository, by a header that says so (`@generated`, `DO NOT EDIT`). The seam link
(`dit code api`) and `dit-map` still live in the workspace, where specs and callers
meet.
