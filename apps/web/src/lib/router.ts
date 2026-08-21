// A hash router, small enough to own. The app runs same-origin inside the
// server binary, so there is no server-side routing to cooperate with — the
// fragment is the whole routing story and it survives reloads for free.

import { useCallback, useEffect, useState } from "react";

export type Route =
  | { name: "home" }
  | { name: "board" }
  | { name: "issues"; q: string | null }
  | { name: "docs"; p: string | null }
  | { name: "search"; q: string }
  | { name: "issue"; id: string }
  | { name: "new-issue" }
  | { name: "settings" };

export function routeToHash(route: Route): string {
  switch (route.name) {
    case "home":
      return "#/home";
    case "board":
      return "#/board";
    case "issues":
      return route.q === null ? "#/issues" : `#/issues?q=${encodeURIComponent(route.q)}`;
    case "docs":
      return route.p === null ? "#/docs" : `#/docs?p=${encodeURIComponent(route.p)}`;
    case "search":
      return `#/search?q=${encodeURIComponent(route.q)}`;
    case "issue":
      return `#/issue/${encodeURIComponent(route.id)}`;
    case "new-issue":
      return "#/new";
    case "settings":
      return "#/settings";
  }
}

export function parseHash(hash: string): Route {
  const path = hash.replace(/^#/, "");
  const [head = "", query] = path.split("?", 2);
  const segments = head.split("/").filter((s) => s.length > 0);
  const first = segments[0] ?? "";
  if (first === "issue" && segments[1]) {
    return { name: "issue", id: decodeURIComponent(segments[1]) };
  }
  if (first === "search") {
    const q = new URLSearchParams(query ?? "").get("q") ?? "";
    return { name: "search", q };
  }
  if (first === "home") return { name: "home" };
  if (first === "board") return { name: "board" };
  if (first === "issues") {
    // The filter the side pane composes rides in `q` — a filtered list is
    // a shareable, reloadable thing, not private view state.
    const q = new URLSearchParams(query ?? "").get("q");
    return { name: "issues", q: q === null || q.length === 0 ? null : q };
  }
  if (first === "docs") {
    // The selected page rides in `p` as the full `docs/…` path — kept in
    // the URL so a reload (or a shared link) reopens the same page.
    const p = new URLSearchParams(query ?? "").get("p");
    return { name: "docs", p: p === null || p.length === 0 ? null : p };
  }
  if (first === "new") return { name: "new-issue" };
  if (first === "settings") return { name: "settings" };
  // Home is the landing view: capture, triage, orient — the board is one
  // click away for people who want to go straight to moving cards.
  return { name: "home" };
}

export function navigate(route: Route): void {
  // Setting the hash fires hashchange; the listener updates state. If the
  // hash is already current nothing happens, which is what we want.
  window.location.hash = routeToHash(route);
}

export function useRoute(): Route {
  const [route, setRoute] = useState<Route>(() => parseHash(window.location.hash));

  useEffect(() => {
    const onChange = () => setRoute(parseHash(window.location.hash));
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);

  return route;
}

// Imperative navigation for non-click callers (palette, post-create jumps).
// Re-reading the hash keeps this stable across renders.
export function useNavigate(): (route: Route) => void {
  return useCallback((route: Route) => navigate(route), []);
}
