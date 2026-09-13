// The handful of DQL queries the shell itself runs. Real DQL, not a private
// filter language: every one of these is what a person could type into the
// search box, and the UI shows them verbatim wherever it runs them.
//
// `~` is full-text over title/body, so set membership uses `=`. Urgent-first
// is ASC: priority is the text p0..p4, so p0 sorts before p4.

import type { StatusDto } from "./types";

/** Home's "next actions": what @me has flagged as next, hottest first. */
export const NEXT_QUERY = "label = next AND assignee = @me ORDER BY priority ASC";

/** The inbox: captured, not yet triaged. Newest first so triage reads top-down. */
export const INBOX_QUERY = "label = inbox ORDER BY created DESC";

/** Everything assigned to @me, hottest first. Done work drops out via
 *  `openQuery`, which needs the workflow to know what "done" is called. */
export const MINE_FRAGMENT = "assignee = @me";

/** Statuses are configurable, so "open" is spelled out from the workflow:
 *  every status whose category is not `done`. With no schema loaded yet the
 *  query is unfiltered — a count that is briefly too high beats a query
 *  that names a status the workspace does not have. */
export function openFragment(statuses: readonly StatusDto[] | undefined): string | null {
  const done = (statuses ?? []).filter((status) => status.category === "done");
  if (done.length === 0) return null;
  return done.map((status) => `status != ${status.id}`).join(" AND ");
}

export function mineQuery(statuses: readonly StatusDto[] | undefined): string {
  const open = openFragment(statuses);
  return `${MINE_FRAGMENT}${open ? ` AND ${open}` : ""} ORDER BY priority ASC`;
}

export function openQuery(statuses: readonly StatusDto[] | undefined): string | null {
  return openFragment(statuses);
}

// A comparison, or one of the connectives as a whole word. Deliberately
// narrow: the cost of a false positive (handing the parser a sentence) is
// an error message, and the cost of a false negative is only that the
// palette offers a full-text search, which is what the words wanted anyway.
const COMPARISON = /(^|\s)[\w.]+\s*(=|!=|<=|>=|<|>|~)\s*\S/;
const CONNECTIVE = /(^|\s)(and|or)(\s|$)/i;
// `label IN (auth, api)` has no comparison operator at all, so set
// membership needs its own test or it reads as words to search for.
const MEMBERSHIP = /(^|\s)(not\s+)?in\s*\(/i;
// A leading `~` is full text spelled as a query — "~ \"merge driver\"".
const BARE_MATCH = /^~\s*\S/;

/** Does this text read as a query to run rather than words to search for? */
export function looksLikeDql(text: string): boolean {
  const trimmed = text.trim();
  if (trimmed.length === 0) return false;
  return (
    COMPARISON.test(trimmed) ||
    CONNECTIVE.test(trimmed) ||
    MEMBERSHIP.test(trimmed) ||
    BARE_MATCH.test(trimmed)
  );
}
