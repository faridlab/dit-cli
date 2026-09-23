// A page's outline: its ATX headings, read from the markdown the server
// stored. Pure, so the rule "a `#` inside a code fence is not a heading" is
// pinned by a test rather than rediscovered in a document full of shell
// comments.

export interface OutlineEntry {
  level: 1 | 2 | 3 | 4;
  text: string;
}

export function outlineOf(markdown: string): OutlineEntry[] {
  const out: OutlineEntry[] = [];
  let fence: string | null = null;
  for (const line of markdown.split("\n")) {
    const marker = line.match(/^\s{0,3}(`{3,}|~{3,})/)?.[1];
    if (marker) {
      if (fence === null) fence = marker[0] ?? null;
      else if (marker[0] === fence) fence = null;
      continue;
    }
    if (fence !== null) continue;
    const heading = line.match(/^\s{0,3}(#{1,4})\s+(.+?)\s*#*\s*$/);
    if (!heading?.[1] || !heading[2]) continue;
    out.push({ level: heading[1].length as OutlineEntry["level"], text: plain(heading[2]) });
  }
  return out;
}

/** The heading as it reads on screen: inline marks and link targets gone. */
function plain(text: string): string {
  return text
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[*_`~]/g, "")
    .trim();
}
