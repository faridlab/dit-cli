// Search: one box that takes either DQL or plain words. Words become a
// full-text query (`~ "words"`), DQL runs as typed, and the server's parse
// error is shown verbatim — those messages are written for people. Pages
// are searched here too, by title, path and (for word searches) body, so
// one box answers "where did we write that down" for issues and docs alike.

import { useMemo, useState, type KeyboardEvent, type ReactNode } from "react";
import { useQueries } from "@tanstack/react-query";
import { FileText, Search } from "lucide-react";
import * as api from "../lib/api";
import { ApiError } from "../lib/api";
import { IssueHandle, TypeBadge } from "../components/badges";
import { HeadingNote, SectionHeading } from "../components/chrome";
import { ErrorBox, Loading } from "../components/states";
import { looksLikeDql } from "../lib/dql";
import { relativeTime } from "../lib/format";
import { useRegisterPeekList } from "../lib/peeklist";
import { queryKeys, useDocs, useIssues, useOpenPool, useSchema } from "../lib/queries";
import { navigate, routeToHash } from "../lib/router";
import { snippet } from "../lib/snippet";
import type { DocEntryDto, IssueDto } from "../lib/types";

const RESULT_LIMIT = 200;
// Page bodies are fetched one request each; a cap keeps a big docs tree
// from turning one search into hundreds of requests.
const DOC_BODY_LIMIT = 100;
// Below this length the index cannot use its trigram FTS and falls back to
// LIKE — slower and less precise, so the count says so.
const FTS_MIN_LENGTH = 3;

/** Words wrapped as a full-text query. Double quotes are escaped so a quoted
 *  phrase cannot end the string early. */
function fullText(words: string): string {
  return `~ "${words.replace(/"/g, '\\"')}"`;
}

/** The parser's own message, or null for any other failure. */
function parseError(error: unknown): string | null {
  return error instanceof ApiError && error.status === 400 ? error.message : null;
}

/** `text` with every occurrence of `needle` wrapped in <mark>. */
function marked(text: string, needle: string): ReactNode {
  if (needle.length === 0) return text;
  const pattern = new RegExp(needle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "ig");
  const out: ReactNode[] = [];
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    const at = match.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    out.push(<mark key={at}>{match[0]}</mark>);
    last = at + match[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

/** A page's display name: its first heading, else the file name. */
function docTitle(entry: DocEntryDto, body: string | undefined): string {
  const heading = body?.match(/^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$/m)?.[1];
  if (heading) return heading.trim();
  return entry.path.split("/").pop()?.replace(/\.md$/, "") ?? entry.path;
}

export function SearchView({ q, onOpen }: { q: string; onOpen: (id: string) => void }) {
  const schema = useSchema();
  const statuses = schema.data?.workflow.statuses ?? [];
  const pool = useOpenPool();
  const [text, setText] = useState(q);
  // A route change (example chip, palette, saved view) replaces the box's
  // text; typing does not touch the route until Enter.
  const [seen, setSeen] = useState(q);
  if (seen !== q) {
    setSeen(q);
    setText(q);
  }

  const trimmed = q.trim();
  const active = trimmed.length > 0;
  const isDql = looksLikeDql(trimmed);
  const effective = isDql ? trimmed : fullText(trimmed);

  const primary = useIssues({ q: effective, limit: RESULT_LIMIT }, active);
  // Bare `~ "words"` is newer than `body ~ "words"`; a server that rejects
  // the bare form gets the field-qualified one, without telling the user.
  const primaryParse = parseError(primary.error);
  const needFallback = active && !isDql && primaryParse !== null;
  const fallback = useIssues({ q: `body ${effective}`, limit: RESULT_LIMIT }, needFallback);
  const results = needFallback ? fallback : primary;
  const usedFallback = needFallback && fallback.data !== undefined;
  const shownParseError = active && results.isError ? parseError(results.error) : null;

  const hits = results.data?.items ?? [];
  useRegisterPeekList(useMemo(() => hits.map((issue) => issue.short_ref), [hits]));

  // -- pages: titles and paths always; bodies for word searches
  const docs = useDocs(active);
  const entries = useMemo(() => (docs.data ?? []).slice(0, DOC_BODY_LIMIT), [docs.data]);
  const bodies = useQueries({
    queries: entries.map((entry) => ({
      queryKey: queryKeys.doc(entry.path),
      queryFn: () => api.getDoc(entry.path),
      enabled: active,
      staleTime: 15_000,
    })),
  });
  const bodyOf = new Map<string, string>();
  bodies.forEach((result, index) => {
    const entry = entries[index];
    if (entry && result.data) bodyOf.set(entry.path, result.data.body);
  });
  const needle = trimmed.toLowerCase();
  const pages = active
    ? entries.filter((entry) => {
        const title = docTitle(entry, bodyOf.get(entry.path)).toLowerCase();
        if (title.includes(needle) || entry.path.toLowerCase().includes(needle)) return true;
        return !isDql && (bodyOf.get(entry.path)?.toLowerCase().includes(needle) ?? false);
      })
    : [];

  // -- example queries, spelled with this workspace's status ids
  const doing = statuses.find((status) => status.category === "doing")?.id ?? "in_progress";
  const done = statuses.find((status) => status.category === "done")?.id ?? "done";
  const firstEpic = pool.data?.items.find((issue) => issue.type === "story");
  const examples = [
    `status = ${doing} and assignee = @me`,
    "type = bug and priority <= p1",
    `due <= +7d and status != ${done}`,
    "label = context:meeting",
    '~ "merge driver"',
    ...(firstEpic ? [`epic = "${firstEpic.id}"`] : []),
  ];

  const run = (value: string) => navigate({ name: "search", q: value.trim() });
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      run(text);
    }
  };

  const statusLabel = (issue: IssueDto) => statuses.find((status) => status.id === issue.status)?.label ?? issue.status;

  return (
    <div className="search">
      <div className="sbox">
        <Search className="i" aria-hidden />
        <input
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={onKeyDown}
          placeholder='DQL or words — status = review and assignee = @me · "merge driver"'
          aria-label="Search issues and pages"
          spellCheck={false}
        />
        <span style={{ fontSize: 11, color: "var(--faint)" }}>↵ run</span>
      </div>
      <div className="ex">
        {examples.map((example) => (
          <button key={example} type="button" className="exq" onClick={() => run(example)} title="Run this query">
            {example}
          </button>
        ))}
      </div>

      {active ? (
        <>
          {shownParseError !== null ? (
            <ErrorBox tone="warn" error={new Error(shownParseError)} title="The query could not be parsed" />
          ) : null}
          {results.isError && shownParseError === null ? (
            <ErrorBox error={results.error} title="Search failed" onRetry={() => void results.refetch()} />
          ) : null}
          <section>
            <SectionHeading>
              Issues{" "}
              <HeadingNote>
                {hits.length} · trigram FTS over title{usedFallback ? " and body" : ", body and comments"}
                {trimmed.length < FTS_MIN_LENGTH ? (
                  <>
                    {" · "}
                    <span style={{ color: "var(--warn)" }}>short query — LIKE fallback, degraded</span>
                  </>
                ) : null}
              </HeadingNote>
            </SectionHeading>
            <div className="res">
              {results.isPending || (needFallback && fallback.isPending) ? <Loading label="Searching…" /> : null}
              {hits.map((issue) => {
                const lines = isDql ? [] : snippet(issue.body, trimmed);
                return (
                  <button key={issue.id} type="button" className="rrow open" onClick={() => onOpen(issue.short_ref)}>
                    <TypeBadge type={issue.type} />
                    <div>
                      <div className="t">
                        <IssueHandle shortRef={issue.short_ref} number={issue.number} />{" "}
                        {isDql ? issue.title : marked(issue.title, trimmed)}
                      </div>
                      <div className="sn">
                        {lines.map((segment, index) =>
                          segment.match ? <mark key={index}>{segment.text}</mark> : segment.text,
                        )}
                      </div>
                    </div>
                    <span className="meta">
                      {statusLabel(issue)} · {relativeTime(issue.updated)}
                    </span>
                  </button>
                );
              })}
              {results.data && hits.length === 0 ? <p className="empty my-[1em]">No issues match.</p> : null}
            </div>
          </section>
          <section>
            <SectionHeading>Pages</SectionHeading>
            <div className="res">
              {pages.map((entry) => (
                <a key={entry.path} className="rrow" href={routeToHash({ name: "docs", p: entry.path })}>
                  <FileText className="i" aria-hidden />
                  <div>
                    <div className="t">{docTitle(entry, bodyOf.get(entry.path))}</div>
                    <div className="sn mono">{entry.path}</div>
                  </div>
                  <span className="meta">page</span>
                </a>
              ))}
              {docs.isPending ? <Loading label="Reading pages…" /> : null}
              {docs.data && pages.length === 0 ? <p className="empty my-[1em]">No pages match.</p> : null}
            </div>
          </section>
        </>
      ) : (
        <p className="empty my-[1em]">
          Search runs the same DQL the sidebar filters compose and the palette speaks. Queries shorter than 3
          characters fall back to LIKE and are marked degraded.
        </p>
      )}
    </div>
  );
}
