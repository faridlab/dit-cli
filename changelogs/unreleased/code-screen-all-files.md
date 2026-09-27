# Every file at once

The Code screen has a third view, **All**: the whole root as one network. Every
file is a dot sized by how many files import it and coloured by its top folder, so
the clusters show; every import is a line. It is drawn on a canvas and laid out in
a background worker, so a root of thousands of files stays responsive while it
settles.

- Generated files are hidden at first; turn them on to see everything.
- Hide tests, or show only files with at least N importers.
- Hover a file to light up what it touches; click it to open it in focus.
- Click a folder in the legend to pick out its cluster; search to find a file and
  zoom to it.
