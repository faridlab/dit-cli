// Wiki links between issues and pages (DESIGN §4.3, §13): `[[Q2R7VN8]]`
// names an issue by its short ref, `[[docs/flows/auth-session]]` a page by
// its path without `.md` — or by any unique suffix of it, so moving a page
// into a space breaks no link. This module is the "[[" menu's catalogue and
// the click that follows a link; both are pure over the lists the app has.

import type { Route } from "./router";

export type WikiItem = { kind: "doc" | "issue"; target: string; label: string; detail: string };

type DocLike = { path: string };
type IssueLike = { short_ref: string; title: string; number: number | null };

const SHORT_REF = /^[0-9A-HJKMNP-TV-Z]{7}$/;

/** Everything a "[[" can link to: pages by path, issues by short ref. */
export function wikiItemsFrom(docs: readonly DocLike[], issues: readonly IssueLike[]): WikiItem[] {
  return [
    ...issues.map((issue) => ({
      kind: "issue" as const,
      target: issue.short_ref,
      label: issue.title.replace(/[|\]\n]/g, " ").trim() || issue.short_ref,
      detail: issue.number !== null ? `#${issue.number}` : issue.short_ref,
    })),
    ...docs.map((doc) => {
      const target = doc.path.replace(/\.md$/, "");
      return { kind: "doc" as const, target, label: target, detail: "page" };
    }),
  ];
}

/** The best few for what was typed after "[[": a word that starts a title,
 *  a path or a ref ranks above one found inside it; "#12" finds issue 12. */
export function matchWikiItems(items: readonly WikiItem[], query: string, limit = 8): WikiItem[] {
  const q = query.trim().toLowerCase();
  if (!q) return items.slice(0, limit);
  const scored: Array<[number, WikiItem]> = [];
  for (const item of items) {
    const fields = [item.label, item.target, item.detail].map((f) => f.toLowerCase());
    let score = -1;
    if (fields.some((f) => f === q)) score = 0;
    else if (fields.some((f) => f.startsWith(q) || f.split(/[\s/-]/).some((w) => w.startsWith(q)))) score = 1;
    else if (fields.some((f) => f.includes(q))) score = 2;
    if (score >= 0) scored.push([score, item]);
  }
  scored.sort((a, b) => a[0] - b[0]);
  return scored.slice(0, limit).map(([, item]) => item);
}

/** Where a link goes: an issue by its short ref, or the one page whose path
 *  ends with the target. Null when nothing — or more than one page — fits:
 *  following a guess is how a link quietly opens the wrong page. */
export function resolveWikiTarget(target: string, docs: readonly DocLike[]): Route | null {
  const t = target.trim().replace(/\.md$/, "");
  if (SHORT_REF.test(t)) return { name: "issue", id: t };
  const hits = docs.filter((doc) => {
    const bare = doc.path.replace(/\.md$/, "");
    return bare === t || bare.endsWith(`/${t}`);
  });
  return hits.length === 1 && hits[0] ? { name: "docs", p: hits[0].path } : null;
}
