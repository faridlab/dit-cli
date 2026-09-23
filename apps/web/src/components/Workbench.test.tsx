// The activity bar and the side panel are how every other screen is reached,
// so every icon and every sub-menu row has to go where its label says. These
// click each one and check what it asks for — a row that no longer navigates
// looks exactly like a row that does.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { Route } from "../lib/router";
import type { ActivityId } from "../lib/workbench";

vi.mock("../lib/queries", () => ({
  useOpenPool: () => ({
    data: {
      items: [
        { id: "a", assignees: [], labels: [], status: "todo" },
        { id: "b", assignees: ["farid"], labels: ["context:api"], status: "todo" },
      ],
      total: 17,
    },
  }),
  useSchema: () => ({ data: undefined }),
  useStatus: () => ({ data: { me: "farid", repo: "/home/farid/kyntati" } }),
}));

vi.mock("../lib/starred", () => ({
  useStarred: () => new Set<string>(["x", "y"]),
}));

const { ActivityBar } = await import("./ActivityBar");
const { SidePanel } = await import("./SidePanel");
const { PaneSection } = await import("./PaneSection");

let container: HTMLDivElement;
let root: Root;
let went: Route[] = [];
let activated: ActivityId[] = [];

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  went = [];
  activated = [];
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function click(el: Element | null | undefined) {
  expect(el, "the control under test is not on screen").toBeTruthy();
  act(() => {
    el!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

function bar(active: ActivityId = "work", inbox: number | null = 5) {
  act(() =>
    root.render(
      <ActivityBar
        active={active}
        panelOpen
        badges={{ work: inbox }}
        onActivate={(id) => activated.push(id)}
        workspaceMenu={[]}
        workspace="kyntati"
      />,
    ),
  );
}

function panel(activity: ActivityId, route: Route) {
  act(() =>
    root.render(
      <SidePanel
        activity={activity}
        route={route}
        open
        width={272}
        onWidth={() => undefined}
        onNavigate={(r) => went.push(r)}
        onFold={() => undefined}
        onOpenPalette={() => undefined}
      >
        <PaneSection id="test.one" title="One">
          <p>first body</p>
        </PaneSection>
      </SidePanel>,
    ),
  );
}

const icon = (label: string) => container.querySelector(`.ab-i[aria-label="${label}"]`);
const subRow = (label: string) =>
  [...container.querySelectorAll(".sp-row")].find((a) => a.querySelector(".lbl")?.textContent === label);

describe("the activity bar", () => {
  it("offers every kind of place, with Settings pinned last", () => {
    bar();
    const labels = [...container.querySelectorAll(".ab-i")].map((b) => b.getAttribute("aria-label"));
    expect(labels).toEqual(["Home", "Docs", "Morse", "Work", "Flow", "Plan", "Search", "Settings"]);
  });

  it("asks for the activity each icon names", () => {
    bar();
    for (const label of ["Home", "Docs", "Morse", "Work", "Flow", "Plan", "Search", "Settings"]) click(icon(label));
    expect(activated).toEqual(["home", "docs", "morse", "work", "flow", "plan", "search", "settings"]);
  });

  it("lights the active icon and badges Work with the inbox", () => {
    bar("work", 163);
    expect(icon("Work")?.getAttribute("aria-current")).toBe("page");
    expect(icon("Work")?.querySelector(".ab-badge")?.textContent).toBe("163");
    expect(icon("Docs")?.getAttribute("aria-current")).toBeNull();
  });

  it("keeps each icon's name and shortcut in its tooltip, since the bar shows none", () => {
    bar();
    expect(icon("Docs")?.querySelector(".ab-tip")?.textContent).toContain("⌘5");
    expect(icon("Morse")?.querySelector(".ab-tip")?.textContent).toContain("⌘0");
  });
});

describe("the side panel's sub-menu", () => {
  it("lists Work's places with the counts the lists read, and goes to each", () => {
    panel("work", { name: "board" });
    expect(subRow("Issues")?.querySelector(".cnt")?.textContent).toBe("17");
    expect(subRow("Inbox")?.querySelector(".cnt")?.textContent).toBe("1");
    expect(subRow("My issues")?.querySelector(".cnt")?.textContent).toBe("1");
    expect(subRow("Starred")?.querySelector(".cnt")?.textContent).toBe("2");
    expect(subRow("Board")?.getAttribute("aria-current")).toBe("page");

    for (const label of ["Board", "Issues", "Inbox", "Starred"]) click(subRow(label));
    expect(went).toEqual([
      { name: "board" },
      { name: "issues", q: null },
      { name: "issues", q: null, inbox: true },
      { name: "issues", q: null, starred: true },
    ]);
  });

  it("lists Plan's three views", () => {
    panel("plan", { name: "gantt" });
    for (const label of ["Timeline", "Roadmap", "Gantt"]) click(subRow(label));
    expect(went.map((r) => r.name)).toEqual(["timeline", "roadmap", "gantt"]);
    expect(subRow("Gantt")?.getAttribute("aria-current")).toBe("page");
  });

  it("has no sub-menu where there is one place, only its sections", () => {
    panel("docs", { name: "docs", p: null });
    expect(container.querySelector(".sp-menu")).toBeNull();
    expect(container.textContent).toContain("first body");
  });
});

describe("a section in the lower split", () => {
  it("folds from its heading and stays folded next time", () => {
    panel("home", { name: "home" });
    click(container.querySelector(".ps-t"));
    expect(container.textContent).not.toContain("first body");
    act(() => root.unmount());
    root = createRoot(container);
    panel("home", { name: "home" });
    expect(container.textContent, "folding is remembered per section").not.toContain("first body");
    click(container.querySelector(".ps-t"));
    expect(container.textContent).toContain("first body");
  });
});
