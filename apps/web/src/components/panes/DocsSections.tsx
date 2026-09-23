// The two views under the pages tree: the open page's outline, and the
// pages edited most recently. Both are reads — the outline of the body the
// server stored, the listing's own modification times — and neither writes.

import { useMemo } from "react";
import { Clock, Hash } from "lucide-react";
import { useDoc, useDocs } from "../../lib/queries";
import { outlineOf } from "../../lib/outline";
import { relativeTime } from "../../lib/format";
import { cn } from "../../lib/cn";
import { Row } from "../chrome";
import { PaneSection } from "../PaneSection";

/** Scroll the page to a heading. The editor renders headings as real
 *  `h1`–`h4` elements, so the one whose text matches is the target. */
function jumpTo(text: string, nth: number) {
  const matches = [...document.querySelectorAll<HTMLElement>(".content h1, .content h2, .content h3, .content h4")].filter(
    (el) => el.textContent?.trim() === text,
  );
  const target = matches[nth] ?? matches[0];
  if (!target) return;
  target.scrollIntoView({ behavior: "smooth", block: "start" });
  target.animate?.([{ background: "var(--active)" }, { background: "transparent" }], { duration: 900 });
}

export function DocsOutline({ p }: { p: string | null }) {
  const doc = useDoc(p);
  const entries = useMemo(() => outlineOf(doc.data?.body ?? ""), [doc.data?.body]);
  return (
    <PaneSection id="docs.outline" title="Outline" count={entries.length || null}>
      {p === null ? (
        <p className="empty" style={{ padding: "4px 8px" }}>
          Open a page to see its headings.
        </p>
      ) : entries.length === 0 ? (
        <p className="empty" style={{ padding: "4px 8px" }}>
          {doc.isPending ? "Reading the page…" : "This page has no headings."}
        </p>
      ) : (
        entries.map((entry, index) => {
          const nth = entries.slice(0, index).filter((e) => e.text === entry.text).length;
          return (
            <Row
              key={`${index}:${entry.text}`}
              className={cn("ol", `ol-${entry.level}`)}
              onClick={() => jumpTo(entry.text, nth)}
              title={entry.text}
            >
              <Hash className="i" aria-hidden />
              <span className="lbl">{entry.text}</span>
            </Row>
          );
        })
      )}
    </PaneSection>
  );
}

const RECENT = 8;

export function DocsRecent({ p, onSelect }: { p: string | null; onSelect: (path: string) => void }) {
  const docs = useDocs();
  const recent = useMemo(
    () => [...(docs.data ?? [])].sort((a, b) => b.updated_ms - a.updated_ms).slice(0, RECENT),
    [docs.data],
  );
  return (
    <PaneSection id="docs.recent" title="Recently edited" count={recent.length || null}>
      {recent.map((entry) => (
        <Row key={entry.path} on={entry.path === p} onClick={() => onSelect(entry.path)} title={entry.path}>
          <Clock className="i" aria-hidden />
          <span className="lbl">{entry.path.split("/").pop()}</span>
          <span className="cnt">{relativeTime(new Date(entry.updated_ms).toISOString())}</span>
        </Row>
      ))}
      {docs.data && recent.length === 0 ? (
        <p className="empty" style={{ padding: "4px 8px" }}>
          No pages yet.
        </p>
      ) : null}
    </PaneSection>
  );
}
