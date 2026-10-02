// Server-rendered markdown host. The browser never parses or sanitizes
// markdown: the server owns rendering, so the only HTML that ever enters
// this component already passed the server's sanitizer. No other component
// may call dangerouslySetInnerHTML.

import { useMemo } from "react";

import { withAttachmentSrcs } from "../lib/attachments";
import { cn } from "../lib/cn";
import { useDocs } from "../lib/queries";
import { navigate } from "../lib/router";
import { resolveWikiTarget } from "../lib/wikilinks";

export function Markdown({
  html,
  className,
  baseDir,
}: {
  html: string;
  className?: string;
  /** The folder of the file this HTML came from: relative attachment links
   *  resolve against it (ADR 0026). */
  baseDir?: string;
}) {
  const shown = useMemo(() => (baseDir === undefined ? html : withAttachmentSrcs(html, baseDir)), [html, baseDir]);
  const docs = useDocs(html.includes("data-wikilink"));
  return (
    <div
      className={cn("dit-md", className)}
      // A wiki link's href is its target (`docs/flows/auth`), which as a
      // URL would leave the app; follow it here instead.
      onClick={(event) => {
        const link = event.target instanceof Element ? event.target.closest("a[data-wikilink]") : null;
        if (!link) return;
        event.preventDefault();
        const route = resolveWikiTarget(link.getAttribute("href") ?? "", docs.data ?? []);
        if (route) navigate(route);
      }}
      // Server output only — see the comment above before "fixing" this.
      dangerouslySetInnerHTML={{ __html: shown }}
    />
  );
}
