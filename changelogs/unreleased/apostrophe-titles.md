# An apostrophe no longer hides an issue

An issue titled `Work plan's lane` was committed and then skipped by the
indexer: every `'` was read as an opening quote, so the title looked
unterminated, and no command could find the issue. One YAML-aware quote scanner
now serves the frontmatter, fence and config parsers — a quote opens a quoted
scalar only where a scalar begins. `dit reindex` also names each file it could
not read, with the reason, instead of only counting them.
