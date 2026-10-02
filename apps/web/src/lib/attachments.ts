// Pictures in documents (ADR 0026). The markdown holds a relative link —
// `attachments/x.png`, or `../attachments/x.png` from a comment — exactly as
// GitHub reads it; only at display time does it become a URL on this
// server. The bytes on disk never name the server or carry the token.

import { getToken } from "./auth";

/** What an upload is attached to, as the server's query names it. */
export type AttachTarget = { doc: string } | { issue: string } | { comment: string };

/** Everything an editor needs to take a pasted picture and show its own:
 *  where uploads go, and the folder its relative links resolve against
 *  (content-root relative — `docs`, `issues/2026/10/<folder>/comments`). */
export type AttachContext = { target: AttachTarget; baseDir: string };

/** The folder part of a content-root path. */
export function dirOf(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut < 0 ? "" : path.slice(0, cut);
}

/** Turn an image `src` as written in markdown into one the browser can
 *  load. Absolute URLs and `data:` pass through untouched (the CSP decides
 *  what loads); a relative link into an `attachments/` folder becomes the
 *  attachments route; anything else is left as written. */
export function resolveAttachmentSrc(src: string, baseDir: string, token: string | null = getToken()): string {
  if (src === "" || /^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith("/") || src.startsWith("#")) return src;
  const parts = baseDir.split("/").filter(Boolean);
  for (const segment of (src.split(/[?#]/, 1)[0] ?? "").split("/")) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      if (parts.length === 0) return src; // climbs out of the workspace
      parts.pop();
    } else {
      parts.push(segment);
    }
  }
  if (parts.length < 3 || parts[parts.length - 2] !== "attachments") return src;
  const path = parts.map(encodeURIComponent).join("/");
  return token ? `/api/attachments/${path}?token=${encodeURIComponent(token)}` : `/api/attachments/${path}`;
}

/** Server-rendered HTML (a comment) with every relative attachment `src`
 *  pointed at the attachments route. The HTML already passed the server's
 *  sanitizer; DOMParser builds an inert document, so nothing here runs. */
export function withAttachmentSrcs(html: string, baseDir: string, token: string | null = getToken()): string {
  if (!html.includes("<img")) return html;
  const doc = new DOMParser().parseFromString(html, "text/html");
  for (const img of Array.from(doc.querySelectorAll("img[src]"))) {
    const src = img.getAttribute("src") ?? "";
    const resolved = resolveAttachmentSrc(src, baseDir, token);
    if (resolved !== src) img.setAttribute("src", resolved);
  }
  return doc.body.innerHTML;
}

/** The query string that names an upload's target. */
export function targetQuery(target: AttachTarget): string {
  if ("doc" in target) return `doc=${encodeURIComponent(target.doc)}`;
  if ("issue" in target) return `issue=${encodeURIComponent(target.issue)}`;
  return `comment=${encodeURIComponent(target.comment)}`;
}

/** Alt text from an uploaded file's name — "Screen Shot 2026-10-03.png"
 *  reads as words; a clipboard's anonymous "image.png" says nothing. */
export function altFrom(name: string): string {
  const words = name.replace(/\.[a-z0-9]+$/i, "").replace(/[-_]+/g, " ").trim();
  return /^image( \d+)?$/i.test(words) ? "" : words;
}

/** Image files the editor should upload rather than paste as text. */
export function imageFiles(list: FileList | null | undefined): File[] {
  return Array.from(list ?? []).filter((file) => /^image\/(png|jpe?g|gif|webp)$/.test(file.type));
}
