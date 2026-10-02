// Server-rendered markdown host. The browser never parses or sanitizes
// markdown: the server owns rendering, so the only HTML that ever enters
// this component already passed the server's sanitizer. No other component
// may call dangerouslySetInnerHTML.

import { useMemo } from "react";

import { withAttachmentSrcs } from "../lib/attachments";
import { cn } from "../lib/cn";

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
  return (
    <div
      className={cn("dit-md", className)}
      // Server output only — see the comment above before "fixing" this.
      dangerouslySetInnerHTML={{ __html: shown }}
    />
  );
}
