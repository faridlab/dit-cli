// Session token handling. The token is the only thing standing between a
// malicious webpage and this server, so it lives in sessionStorage (dies with
// the tab), never in localStorage, and is never logged or put in a URL —
// except the one WebSocket endpoint where the browser cannot set headers.

const STORAGE_KEY = "dit-token";

export function getToken(): string | null {
  try {
    return window.sessionStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function setToken(token: string): void {
  try {
    window.sessionStorage.setItem(STORAGE_KEY, token);
  } catch {
    // Storage disabled (private mode quirks): the session simply won't
    // survive a reload, which is acceptable for a local tool.
  }
}

export function clearToken(): void {
  try {
    window.sessionStorage.removeItem(STORAGE_KEY);
  } catch {
    // Nothing to do — see setToken.
  }
}

/**
 * `dit serve` opens the browser at `#token=<token>`. Grab it, persist it,
 * and scrub the fragment so the token never lingers in the address bar,
 * history entries, or copy-pasted links.
 */
export function captureTokenFromLocation(): boolean {
  const hash = window.location.hash;
  if (!hash.startsWith("#token=")) return false;
  const raw = hash.slice("#token=".length);
  // The token is the whole fragment; tolerate a stray trailing "/".
  const token = decodeURIComponent(raw.split("/")[0] ?? "").trim();
  if (!token) return false;
  setToken(token);
  window.history.replaceState(null, "", window.location.pathname + window.location.search);
  return true;
}
