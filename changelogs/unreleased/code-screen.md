# The code map in the browser

`dit ui` has a **Code** screen.

- **Folder view:** one folder at a time, its subfolders and files sized by what they
  hold, the heaviest imports drawn and the rest on hover, large folders laid out in
  layers so names never collide. Wheel to zoom, drag to pan, double-click to fit.
- **Focus view:** one file between the files that import it and the files it imports,
  what it defines, and every API path it calls — the operation it reaches and where it
  was proven, or a warning that no registered spec describes it.
- **Any repository:** run `dit ui` (or `dit-server`) in a repository that is not a DIT
  workspace and it opens the Code screen alone, read-only; nothing is written into the
  repository.
