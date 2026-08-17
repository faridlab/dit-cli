// Server-rendered markdown host. The browser never parses or sanitizes
// markdown: the server owns rendering, so the only HTML that ever enters
// this component already passed the server's sanitizer. No other component
// may call dangerouslySetInnerHTML.

import { cn } from "../lib/cn";

export function Markdown({ html, className }: { html: string; className?: string }) {
  return (
    <div
      className={cn("dit-md", className)}
      // Server output only — see the comment above before "fixing" this.
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
