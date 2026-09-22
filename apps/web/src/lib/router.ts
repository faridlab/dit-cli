// A hash router, small enough to own. The app runs same-origin inside the
// server binary, so there is no server-side routing to cooperate with — the
// fragment is the whole routing story and it survives reloads for free.
//
// An open issue is part of the route, not private state: the list views
// carry `?issue=<short_ref>` while the panel is open, so a reload or a
// shared link reopens the same panel over the same list. The full page is
// its own route and remembers where it was opened from.

import { useCallback, useEffect, useState } from "react";

/** The views an issue panel can sit over. */
export type PeekHost =
  | "home"
  | "board"
  | "issues"
  | "search"
  | "timeline"
  | "roadmap"
  | "gantt"
  | "flow";

const PEEK_HOSTS: readonly string[] = [
  "home",
  "board",
  "issues",
  "search",
  "timeline",
  "roadmap",
  "gantt",
  "flow",
];

export type Route =
  | { name: "home"; issue?: string | null }
  | { name: "board"; issue?: string | null }
  | {
      name: "issues";
      q: string | null;
      issue?: string | null;
      /** This browser's shortlist — not a server query, so it rides as a flag. */
      starred?: boolean;
      /** The untriaged list (no owner or no @context). Computed client-side
       *  from the open pool because DQL has no "is empty" test, so it too
       *  rides as a flag rather than pretending to be a query. */
      inbox?: boolean;
    }
  | { name: "docs"; p: string | null }
  | { name: "search"; q: string; issue?: string | null }
  /** The three plan views. Each hosts the issue panel like any list. */
  | { name: "timeline"; issue?: string | null; seq?: number | null }
  | { name: "roadmap"; issue?: string | null }
  | { name: "gantt"; issue?: string | null }
  /** The flow diagram (ADR 0019): issues as nodes, blocked_by as edges.
   *  A reading of the diagram — which flow, what is selected, the traced
   *  route, the isolated colours, the paint dimension — rides in the URL so
   *  it can be pasted into a thread and reopened exactly (ADR 0021). */
  | {
      name: "flow";
      issue?: string | null;
      /** Flow name, or `__all__` for the union. */
      f?: string | null;
      /** The selected node's issue id. */
      n?: string | null;
      /** The route probe's two ends, `from~to`. */
      r?: string | null;
      /** Isolated semantics, comma-separated. */
      k?: string | null;
      /** Which dimension the node colours mean. */
      paint?: string | null;
    }
  | { name: "issue"; id: string; from?: PeekHost | null }
  /** The composer; `type` preselects the issue type (the roadmap's "New epic"). */
  | { name: "new-issue"; type?: string | null }
  | { name: "settings" };

/** Query string from pairs, skipping empties, in a stable order so the same
 *  route always produces the same URL (and never a spurious history entry). */
function query(pairs: Array<[string, string | null | undefined]>): string {
  const parts = pairs
    .filter((pair): pair is [string, string] => pair[1] !== null && pair[1] !== undefined && pair[1].length > 0)
    .map(([key, value]) => `${key}=${encodeURIComponent(value)}`);
  return parts.length > 0 ? `?${parts.join("&")}` : "";
}

export function routeToHash(route: Route): string {
  switch (route.name) {
    case "home":
      return `#/home${query([["issue", route.issue]])}`;
    case "board":
      return `#/board${query([["issue", route.issue]])}`;
    case "issues":
      return `#/issues${query([
        ["q", route.q],
        // A private shortlist cannot be a server query, so it rides as its
        // own flag rather than pretending to be DQL.
        ["starred", route.starred === true ? "1" : null],
        ["inbox", route.inbox === true ? "1" : null],
        ["issue", route.issue],
      ])}`;
    case "docs":
      return `#/docs${query([["p", route.p]])}`;
    case "search":
      return `#/search${query([
        ["q", route.q],
        ["issue", route.issue],
      ])}`;
    case "timeline":
      return `#/timeline${query([
        // Where you are standing in history, as a position in the commit
        // graph — a date only maps to one through an author's clock.
        ["seq", route.seq === null || route.seq === undefined ? null : String(route.seq)],
        ["issue", route.issue],
      ])}`;
    case "roadmap":
      return `#/roadmap${query([["issue", route.issue]])}`;
    case "gantt":
      return `#/gantt${query([["issue", route.issue]])}`;
    case "flow":
      return `#/flow${query([
        ["f", route.f],
        ["n", route.n],
        ["r", route.r],
        ["k", route.k],
        ["paint", route.paint],
        ["issue", route.issue],
      ])}`;
    case "issue":
      return `#/issue/${encodeURIComponent(route.id)}${query([["from", route.from]])}`;
    case "new-issue":
      return `#/new${query([["type", route.type]])}`;
    case "settings":
      return "#/settings";
  }
}

export function parseHash(hash: string): Route {
  const path = hash.replace(/^#/, "");
  const [head = "", queryString] = path.split("?", 2);
  const segments = head.split("/").filter((s) => s.length > 0);
  const first = segments[0] ?? "";
  const params = new URLSearchParams(queryString ?? "");
  const nonEmpty = (key: string): string | null => {
    const value = params.get(key);
    return value === null || value.length === 0 ? null : value;
  };
  // The open panel rides in `issue` as the issue's permanent short ref.
  const issue = nonEmpty("issue");

  if (first === "issue" && segments[1]) {
    const from = params.get("from");
    return {
      name: "issue",
      id: decodeURIComponent(segments[1]),
      from: from !== null && PEEK_HOSTS.includes(from) ? (from as PeekHost) : null,
    };
  }
  if (first === "search") return { name: "search", q: params.get("q") ?? "", issue };
  if (first === "home") return { name: "home", issue };
  if (first === "board") return { name: "board", issue };
  if (first === "issues") {
    // The filter the sidebar composes rides in `q` — a filtered list is
    // a shareable, reloadable thing, not private view state. `starred` is
    // the exception: it names this browser's own shortlist, so a link
    // carrying it shows the recipient *their* stars, not the sender's.
    return {
      name: "issues",
      q: nonEmpty("q"),
      issue,
      starred: params.get("starred") === "1",
      inbox: params.get("inbox") === "1",
    };
  }
  if (first === "docs") {
    // The selected page rides in `p` as the full `docs/…` path — kept in
    // the URL so a reload (or a shared link) reopens the same page.
    return { name: "docs", p: nonEmpty("p") };
  }
  if (first === "timeline") {
    const raw = nonEmpty("seq");
    const seq = raw === null ? null : Number.parseInt(raw, 10);
    return { name: "timeline", issue, seq: seq === null || Number.isNaN(seq) ? null : seq };
  }
  if (first === "roadmap") return { name: "roadmap", issue };
  if (first === "gantt") return { name: "gantt", issue };
  if (first === "flow") {
    return {
      name: "flow",
      issue,
      f: nonEmpty("f"),
      n: nonEmpty("n"),
      r: nonEmpty("r"),
      k: nonEmpty("k"),
      paint: nonEmpty("paint"),
    };
  }
  if (first === "new") return { name: "new-issue", type: nonEmpty("type") };
  if (first === "settings") return { name: "settings" };
  // Home is the landing view: capture, triage, orient — the board is one
  // click away for people who want to go straight to moving cards.
  return { name: "home", issue: null };
}

/** The view an issue panel would open over: the current one where it can
 *  host a panel, otherwise the one the full page was opened from. */
export function peekHost(route: Route): PeekHost {
  if (PEEK_HOSTS.includes(route.name)) return route.name as PeekHost;
  if (route.name === "issue") return route.from ?? "issues";
  return "issues";
}

/** The issue whose panel is open over this route, if any. */
export function peekOf(route: Route): string | null {
  switch (route.name) {
    case "home":
    case "board":
    case "issues":
    case "search":
    case "timeline":
    case "roadmap":
    case "gantt":
    case "flow":
      return route.issue ?? null;
    default:
      return null;
  }
}

/** The same route with the panel opened on `id` (or closed, for `null`).
 *  From anywhere that cannot host a panel — the full page, docs, settings —
 *  this lands on the list the panel belongs over. */
export function withPeek(route: Route, id: string | null): Route {
  switch (route.name) {
    case "home":
      return { name: "home", issue: id };
    case "board":
      return { name: "board", issue: id };
    case "issues":
      return { name: "issues", q: route.q, starred: route.starred, inbox: route.inbox, issue: id };
    case "search":
      return { name: "search", q: route.q, issue: id };
    case "timeline":
      return { name: "timeline", seq: route.seq, issue: id };
    case "roadmap":
      return { name: "roadmap", issue: id };
    case "gantt":
      return { name: "gantt", issue: id };
    case "flow":
      // Opening an issue must not throw away the reading the diagram is in.
      return { ...route, issue: id };
    default: {
      const host = peekHost(route);
      if (host === "home") return { name: "home", issue: id };
      if (host === "board") return { name: "board", issue: id };
      if (host === "search") return { name: "search", q: "", issue: id };
      if (host === "timeline") return { name: "timeline", issue: id };
      if (host === "roadmap") return { name: "roadmap", issue: id };
      if (host === "gantt") return { name: "gantt", issue: id };
      return { name: "issues", q: null, issue: id };
    }
  }
}

export function navigate(route: Route): void {
  // Setting the hash fires hashchange; the listener updates state. If the
  // hash is already current nothing happens, which is what we want.
  window.location.hash = routeToHash(route);
}

/** Rewrite the current history entry instead of pushing a new one, and tell
 *  the listeners. Reading state — a selection, a traced route, an isolated
 *  colour — changes many times a minute: every one of those is worth putting
 *  in a shareable link, and none of them is worth a press of Back. */
export function replaceRoute(route: Route): void {
  const hash = routeToHash(route);
  if (window.location.hash === hash) return;
  window.history.replaceState(null, "", hash);
  // replaceState fires nothing, and the app reads the route from this event.
  window.dispatchEvent(new Event("hashchange"));
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
