# Smaller answers, questions in words, and branches

- **Smaller answers.** With one root, answers no longer repeat its name on every line.
  `dit code uses` lists each import once by the file it reaches, libraries on one line;
  its calls moved behind `--calls`.
- **`dit code where token refresh`** takes a question in words and answers with the
  files to read, ranked by how many of the words they answer and how much of the code
  leans on them.
- **Branch switches are nearly free.** What the extractor read is cached by content, so
  a file seen on any branch is never parsed again.
- **`ref:` on a code root** maps that branch or tag instead of whatever the checkout has
  on HEAD — useful for a linked repository someone switched to a feature branch.
- **`dit code hook install`** refreshes the map in the background after every commit,
  merge, checkout and rebase. Opt-in; it keeps existing hooks intact and refuses hooks
  that are committed files.
