// The line of body text that explains why a search result matched. The
// palette shows titles, and a title alone often does not say why an issue
// came back — the match was three paragraphs down.
//
// The body is markdown, so this strips the markup people do not need to
// read in a one-line preview and returns plain segments, marked or not, for
// the caller to render. Segments rather than HTML: nothing here builds
// markup, so nothing here can inject any.

export interface SnippetSegment {
  text: string;
  match: boolean;
}

const DEFAULT_WIDTH = 90;

/** Markdown reduced to the words: fences, inline code, links, emphasis and
 *  heading markers dropped, whitespace collapsed. */
export function plainText(markdown: string): string {
  return markdown
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+/gm, "")
    .replace(/^\s{0,3}>\s?/gm, "")
    .replace(/^\s{0,3}[-*+]\s+/gm, "")
    .replace(/[*_~]{1,3}/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/** The window of body text around the first match, split into segments so
 *  the matched run can be highlighted. Returns an empty array when the
 *  query does not appear in the body — the caller shows nothing rather than
 *  a misleading first line. */
export function snippet(
  markdown: string,
  query: string,
  width: number = DEFAULT_WIDTH,
): SnippetSegment[] {
  const text = plainText(markdown);
  const needle = query.trim();
  if (text.length === 0 || needle.length === 0) return [];

  const at = text.toLowerCase().indexOf(needle.toLowerCase());
  if (at === -1) return [];

  // Keep a little more before the match than after: the words leading up to
  // a term are usually what make it make sense.
  const before = Math.floor(width / 3);
  const start = Math.max(0, at - before);
  const end = Math.min(text.length, at + needle.length + (width - before));

  const segments: SnippetSegment[] = [];
  const push = (value: string, match: boolean) => {
    if (value.length > 0) segments.push({ text: value, match });
  };

  push((start > 0 ? "…" : "") + text.slice(start, at), false);
  push(text.slice(at, at + needle.length), true);
  push(text.slice(at + needle.length, end) + (end < text.length ? "…" : ""), false);
  return segments;
}
