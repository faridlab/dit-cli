// Which workspace this page belongs to (ADR 0028). One server serves every
// workspace on the machine at `/w/<name>/`; a page loaded there talks to
// `/w/<name>/api/…`, and everything it remembers in this browser is kept
// apart from the other workspaces'. A server started for one workspace
// (`dit-server --workspace`) serves at `/`, and nothing is prefixed.
//
// Switching workspace is a full page load, so the answer never changes
// during a page's life and may be read once, at module load.

const PREFIX = /^\/w\/([^/]+)(?:\/|$)/;

/** The workspace named by the page's path, or `null` at `/`. */
export function workspaceName(pathname: string = currentPath()): string | null {
  const match = PREFIX.exec(pathname);
  if (!match?.[1]) return null;
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return null;
  }
}

/** The path every workspace request starts with: `/w/<name>`, or "". */
export function workspaceBase(pathname: string = currentPath()): string {
  const name = workspaceName(pathname);
  return name === null ? "" : `/w/${encodeURIComponent(name)}`;
}

/** The page of workspace `name` — where the switcher sends the browser. */
export function workspaceHref(name: string): string {
  return `/w/${encodeURIComponent(name)}/`;
}

/** A browser-storage key kept apart per workspace. Per-browser preferences
 *  (theme, panel width) do not use this; anything naming issues, pages or
 *  scenarios does — `#12` in one workspace is not `#12` in another. */
export function scopedKey(key: string, pathname: string = currentPath()): string {
  const name = workspaceName(pathname);
  return name === null ? key : `${key}@${name}`;
}

function currentPath(): string {
  return typeof window === "undefined" ? "/" : window.location.pathname;
}
