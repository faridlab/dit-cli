// The rail is the one surface every other screen is reached through, so
// every row in it has to go where its label says. These click each one and
// check the route it asks for — the grouping into Work, Plan and the saved
// queries under Issues rearranged all of them at once, and a row that no
// longer navigates would look exactly like a row that does.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { Route } from "../lib/router";

vi.mock("../lib/queries", () => ({
  useOpenPool: () => ({ data: { items: [], total: 17 } }),
  useSchema: () => ({ data: undefined }),
  useStatus: () => ({ data: { me: "farid", repo: "/home/farid/kyntati" } }),
}));

vi.mock("../lib/starred", () => ({
  useStarred: () => ({ starred: new Set<string>(["x"]) }),
}));

const { Sidebar, SHORTCUT_VIEWS } = await import("./Sidebar");

let container: HTMLDivElement;
let root: Root;
let went: Route[] = [];

function render(route: Route = { name: "home" }) {
  act(() => {
    root.render(
      <Sidebar
        route={route}
        mode="expanded"
        section={null}
        onNavigate={(r) => went.push(r)}
        onOpenPalette={() => undefined}
        workspaceMenu={[]}
      />,
    );
  });
}

/** The nav row whose visible label is exactly this. */
function row(label: string): HTMLAnchorElement {
  const found = [...container.querySelectorAll<HTMLAnchorElement>("nav.nav a")].find(
    (a) => a.querySelector(".lbl")?.textContent === label,
  );
  expect(found, `no rail row labelled ${label}`).toBeTruthy();
  return found!;
}

function click(element: Element) {
  act(() => {
    element.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  });
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  went = [];
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the rail", () => {
  it("sends every row to the view its label names", () => {
    render();
    for (const [label, expected] of [
      ["Home", { name: "home" }],
      ["Docs", { name: "docs", p: null }],
      ["Board", { name: "board" }],
      ["Issues", { name: "issues", q: null }],
      ["Flow", { name: "flow" }],
      ["Timeline", { name: "timeline" }],
      ["Roadmap", { name: "roadmap" }],
      ["Gantt", { name: "gantt" }],
    ] as Array<[string, Route]>) {
      went = [];
      click(row(label));
      expect(went, label).toEqual([expected]);
    }
  });

  it("sends the saved queries to the issue list with their filter", () => {
    render();
    went = [];
    click(row("Inbox"));
    expect(went).toEqual([{ name: "issues", q: null, inbox: true }]);

    went = [];
    click(row("Starred"));
    expect(went).toEqual([{ name: "issues", q: null, starred: true }]);

    went = [];
    click(row("My issues"));
    expect(went[0]?.name).toBe("issues");
    // It is a query, not a flag: real DQL rides in `q`, and it names the
    // actor symbolically so a shared link shows the reader *their* work.
    expect((went[0] as { q: string | null }).q).toContain("assignee = @me");
  });

  it("draws the saved queries as children of the list, not as peers of it", () => {
    render();
    for (const label of ["Inbox", "My issues", "Starred"]) {
      expect(row(label).classList.contains("q"), label).toBe(true);
    }
    for (const label of ["Board", "Issues", "Flow"]) {
      expect(row(label).classList.contains("q"), label).toBe(false);
    }
  });

  it("groups the lenses under headings rather than listing eleven equals", () => {
    render();
    const headings = [...container.querySelectorAll("nav.nav .nav-h")].map((h) => h.textContent);
    expect(headings).toEqual(["Work", "Plan"]);
  });

  it("marks exactly one row current, and never the list when a query is open", () => {
    render({ name: "issues", q: null, inbox: true });
    const current = [...container.querySelectorAll("nav.nav a")].filter(
      (a) => a.getAttribute("aria-current") === "page",
    );
    expect(current).toHaveLength(1);
    expect(current[0]?.querySelector(".lbl")?.textContent).toBe("Inbox");
  });

  it("draws New issue once, and not here — the header already owns it", () => {
    render();
    const creates = [...container.querySelectorAll("button, a")].filter((el) =>
      (el.textContent ?? "").includes("New issue"),
    );
    expect(creates).toHaveLength(0);
  });

  it("pins Settings and nothing else to the bottom edge", () => {
    render();
    const pinned = [...container.querySelectorAll(".sb-bottom a, .sb-bottom button")];
    expect(pinned.map((el) => el.querySelector(".lbl")?.textContent)).toEqual(["Settings"]);
  });

  it("keeps the nav out of the scrolling region so a long list cannot push it away", () => {
    render({ name: "board" });
    const nav = container.querySelector("nav.nav");
    const section = container.querySelector(".sb-section");
    expect(nav).toBeTruthy();
    // Structural, because this is a structural fix: the part that grows
    // without limit is the view's section, and it must be the only thing
    // that scrolls.
    expect(nav!.closest(".sb-section")).toBeNull();
    expect(section?.contains(nav!)).not.toBe(true);
  });

  it("keeps the shortcut table and the rail agreeing on the order", () => {
    // AppShell binds ⌘1..⌘8 to this list by index; a row that moved without
    // the table moving sends the reader somewhere they did not ask for.
    expect(SHORTCUT_VIEWS.map((r) => r.name)).toEqual([
      "home",
      "search",
      "board",
      "issues",
      "docs",
      "timeline",
      "roadmap",
      "gantt",
    ]);
    render();
    for (const [index, label] of [
      [0, "Home"],
      [2, "Board"],
      [3, "Issues"],
      [4, "Docs"],
      [5, "Timeline"],
      [6, "Roadmap"],
      [7, "Gantt"],
    ] as Array<[number, string]>) {
      expect(row(label).querySelector("kbd")?.textContent, label).toBe(`⌘${index + 1}`);
    }
  });
});
