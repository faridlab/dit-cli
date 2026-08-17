// A hash router, small enough to own. The app runs same-origin inside the
// server binary, so there is no server-side routing to cooperate with — the
// fragment is the whole routing story and it survives reloads for free.

import { useCallback, useEffect, useState } from "react";

export type Route =
  | { name: "board" }
  | { name: "issues" }
  | { name: "search"; q: string }
  | { name: "issue"; id: string };

export function routeToHash(route: Route): string {
  switch (route.name) {
    case "board":
      return "#/board";
    case "issues":
      return "#/issues";
    case "search":
      return `#/search?q=${encodeURIComponent(route.q)}`;
    case "issue":
      return `#/issue/${encodeURIComponent(route.id)}`;
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
  if (first === "issues") return { name: "issues" };
  return { name: "board" };
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
