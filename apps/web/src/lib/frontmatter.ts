// A page's frontmatter (`kind`, `title`, `order`, …) is not Markdown the rich
// editor can hold: read as Markdown, its opening `---` is a rule and its
// fields a heading, and saving would write them back as one. The editor edits
// the body; the frontmatter is kept apart and put back exactly as it was.
// Source mode still shows and edits the whole file.

/** The frontmatter block — delimiters and final newline included — and the
 *  body after it. No frontmatter, or an opening `---` that never closes:
 *  everything is body. */
export function splitFrontmatter(text: string): { front: string; body: string } {
  const open = /^---\r?\n/.exec(text);
  if (!open) return { front: "", body: text };
  const close = /^---[ \t]*(\r?\n|$)/m;
  const rest = text.slice(open[0].length);
  const match = close.exec(rest);
  if (!match) return { front: "", body: text };
  const end = open[0].length + match.index + match[0].length;
  return { front: text.slice(0, end), body: text.slice(end) };
}

export function joinFrontmatter(front: string, body: string): string {
  return front + body;
}

/** The top-level `key: value` fields of a frontmatter block, for display. */
export function propertiesOf(front: string): [string, string][] {
  const out: [string, string][] = [];
  for (const line of front.split(/\r?\n/)) {
    const m = /^([A-Za-z_][\w-]*):\s*(.*)$/.exec(line);
    if (!m?.[1]) continue;
    let value = (m[2] ?? "").trim();
    if (value.length >= 2 && (value[0] === '"' || value[0] === "'") && value.endsWith(value[0])) {
      value = value.slice(1, -1);
    }
    out.push([m[1], value]);
  }
  return out;
}
