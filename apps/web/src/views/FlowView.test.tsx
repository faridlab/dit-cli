// Every control on the Flow screen has to actually do something. These
// render the canvas and its sidebar section against one small board and
// exercise the reading tools the way a person does: click a node, isolate a
// colour, find one by name, trace a route between two.
//
// The data hooks are stubbed — what is under test is the screen's behaviour,
// not the index behind it.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { FlowBoardDto, FlowNodeDto } from "../lib/types";

function node(id: string, over: Partial<FlowNodeDto> = {}): FlowNodeDto {
  return {
    id,
    short_ref: `ref-${id}`,
    number: null,
    title: id,
    kind: "task",
    status: "todo",
    status_label: "To Do",
    category: "todo",
    priority: "p1",
    lane: null,
    stage: 0,
    row: 0,
    readiness: "ready",
    outside_blockers: [],
    phases: [],
    commits: 0,
    claim: null,
    ...over,
  };
}

const BOARD: FlowBoardDto = {
  name: "launch",
  lanes: [{ id: null, label: "Unlaned" }],
  phases: [],
  groups: [],
  unphased: false,
  shape_problem: null,
  stages: 3,
  nodes: [
    node("a", { title: "Design the endpoint", number: 1 }),
    node("b", { title: "Build the endpoint", number: 2, stage: 1, readiness: "blocked" }),
    node("c", { title: "Draft the comms plan", number: 3, stage: 1, row: 1 }),
    node("d", {
      title: "Announce the endpoint",
      number: 4,
      stage: 2,
      readiness: "blocked",
      outside_blockers: [
        {
          id: "x",
          short_ref: "ref-x",
          number: 9,
          title: "Legal sign-off",
          status_label: "In Review",
          satisfied: false,
          gone: false,
        },
      ],
    }),
  ],
  edges: [
    { from: "a", to: "b", disposition: "unsatisfied", gating: true, backward: false, label: null },
    { from: "a", to: "c", disposition: "unsatisfied", gating: true, backward: false, label: "record result" },
    { from: "b", to: "d", disposition: "unsatisfied", gating: true, backward: false, label: null },
  ],
  main_path: ["a", "b", "d"],
};

vi.mock("../lib/queries", () => ({
  useFlows: () => ({
    data: [{ name: "launch", issues: 4 }],
    isPending: false,
    isError: false,
    error: null,
    refetch: () => undefined,
  }),
  useFlowBoard: () => ({
    data: BOARD,
    isPending: false,
    isError: false,
    error: null,
    refetch: () => undefined,
  }),
  useFieldEvents: () => ({
    data: [
      { seq: 1, field: "status", old_value: "todo", new_value: "review", author: "farid", ts: "", commit_sha: "abc1234def" },
      { seq: 2, field: "title", old_value: null, new_value: "x", author: "budi", ts: "", commit_sha: "abc1234def" },
      { seq: 3, field: "lane", old_value: null, new_value: "backend", author: "farid", ts: "", commit_sha: "9876543210" },
    ],
    isPending: false,
  }),
}));

class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= NoopResizeObserver as unknown as typeof ResizeObserver;

const { FlowView } = await import("./FlowView");
const { FlowPane } = await import("../components/panes/FlowPane");
const { ViewOptionsProvider } = await import("../lib/viewopts");

let container: HTMLDivElement;
let root: Root;
let opened: string[] = [];

function render() {
  act(() => {
    root.render(
      <ViewOptionsProvider>
        <FlowView onOpen={(id) => opened.push(id)} />
        <FlowPane onOpen={(id) => opened.push(id)} />
      </ViewOptionsProvider>,
    );
  });
}

function click(element: Element | null | undefined) {
  expect(element, "the control under test is not on screen").toBeTruthy();
  act(() => {
    element!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

/** The node group whose accessible label carries this title. */
function nodeGroup(title: string): SVGGElement {
  const found = [...container.querySelectorAll<SVGGElement>("g.fnode")].find((g) =>
    (g.getAttribute("aria-label") ?? "").includes(title),
  );
  expect(found, `no node for ${title}`).toBeTruthy();
  return found!;
}

/** The sidebar button whose text starts with this label. */
function paneButton(text: string): HTMLButtonElement | undefined {
  return [...container.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => (b.textContent ?? "").trim() === text,
  );
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  window.location.hash = "#/flow";
  opened = [];
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the Flow screen", () => {
  it("draws every node and the arrows between them", () => {
    render();
    expect(container.querySelectorAll("g.fnode")).toHaveLength(4);
    // An edge is a group now: the path, plus its caption when the fence
    // gives it one.
    expect(container.querySelectorAll("g.fedge")).toHaveLength(3);
    expect(container.textContent).toContain("record result");
    // The critical path is emphasized, the rest is not.
    expect(container.querySelectorAll("path.wf-a-emph")).toHaveLength(2);
  });

  it("lights a node's whole dependency reach and dims the rest", () => {
    render();
    click(nodeGroup("Build the endpoint"));
    // a (blocker), b (itself) and d (dependent) stay lit; c is off the chain.
    expect(nodeGroup("Draft the comms plan").classList.contains("dim")).toBe(true);
    expect(nodeGroup("Design the endpoint").classList.contains("dim")).toBe(false);
    expect(nodeGroup("Announce the endpoint").classList.contains("dim")).toBe(false);
    expect(nodeGroup("Build the endpoint").classList.contains("selected")).toBe(true);
    // The sidebar is looking at the same selection.
    expect(container.textContent).toContain("1 up · 1 down");
  });

  it("clears the selection when the same node is clicked again", () => {
    render();
    click(nodeGroup("Build the endpoint"));
    click(nodeGroup("Build the endpoint"));
    expect(container.querySelectorAll("g.fnode.dim")).toHaveLength(0);
  });

  it("names the blockers that live outside the flow", () => {
    render();
    click(nodeGroup("Announce the endpoint"));
    expect(container.textContent).toContain("Legal sign-off");
    expect(container.textContent).toContain("Outside this flow");
  });

  it("opens the issue from the passport rather than on a single click", () => {
    render();
    click(nodeGroup("Design the endpoint"));
    expect(opened).toEqual([]);
    click(paneButton("Open issue"));
    expect(opened).toEqual(["ref-a"]);
  });

  it("isolates a colour when the legend is clicked", () => {
    render();
    click([...container.querySelectorAll("button.ndb")].find((b) => b.textContent?.includes("blocked")));
    expect(nodeGroup("Build the endpoint").classList.contains("dim")).toBe(false);
    expect(nodeGroup("Design the endpoint").classList.contains("dim")).toBe(true);
  });

  it("finds a node by title and selects it", () => {
    render();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "f" }));
    });
    const input = container.querySelector<HTMLInputElement>(".ffind-input");
    expect(input).toBeTruthy();
    act(() => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      setter.call(input, "comms");
      input!.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const hits = container.querySelectorAll(".ffind-hit");
    expect(hits).toHaveLength(1);
    click(hits[0]);
    expect(container.querySelector(".ffind-input")).toBeNull();
    expect(nodeGroup("Draft the comms plan").classList.contains("selected")).toBe(true);
  });

  it("traces the route between two picks", () => {
    render();
    click(nodeGroup("Design the endpoint"));
    click(paneButton("From"));
    click(nodeGroup("Announce the endpoint"));
    click(paneButton("To"));
    // The chain a -> b -> d is lit; the node off it is not.
    expect(nodeGroup("Draft the comms plan").classList.contains("dim")).toBe(true);
    expect(nodeGroup("Build the endpoint").classList.contains("dim")).toBe(false);
    expect(container.textContent).toContain("#1 → #4");
  });

  it("walks the critical path with the sidebar stepper", () => {
    render();
    const steps = [...container.querySelectorAll<HTMLButtonElement>(".fstep .btn")];
    expect(steps).toHaveLength(2);
    click(steps[1]);
    expect(nodeGroup("Design the endpoint").classList.contains("selected")).toBe(true);
    click(steps[1]);
    expect(nodeGroup("Build the endpoint").classList.contains("selected")).toBe(true);
    click(steps[0]);
    expect(nodeGroup("Design the endpoint").classList.contains("selected")).toBe(true);
  });

  it("zooms and fits from the floating controls", () => {
    render();
    const percent = () => container.querySelector(".fpct")?.textContent;
    const buttons = [...container.querySelectorAll<HTMLButtonElement>(".fnav button")];
    const zoomIn = buttons.find((b) => b.title.startsWith("Zoom in"));
    const fit = buttons.find((b) => b.title.startsWith("Fit"));
    const before = percent();
    click(zoomIn);
    expect(percent()).not.toBe(before);
    click(fit);
    expect(percent()).toBe(before);
  });
});

describe("the reading layer", () => {
  it("switches what the colours mean, and the legend follows", () => {
    render();
    // At rest the legend names states.
    expect(container.textContent).toContain("in flight");
    const lens = [...container.querySelectorAll<HTMLButtonElement>("button.lensb")];
    expect(lens.map((b) => b.textContent)).toEqual(["state", "lane", "type", "priority"]);
    click(lens.find((b) => b.textContent === "type"));
    // Now it names issue types, with counts, and the nodes carry palette
    // classes instead of semantic ones.
    expect(container.textContent).toContain("task");
    expect(container.querySelector("rect.wf-p-0")).toBeTruthy();
    expect(container.querySelector("rect.wf-c-ready")).toBeNull();
  });

  it("isolates by whatever the colours currently mean", () => {
    render();
    click([...container.querySelectorAll<HTMLButtonElement>("button.lensb")].find((b) => b.textContent === "priority"));
    const swatch = [...container.querySelectorAll<HTMLButtonElement>("button.ndb")][0];
    click(swatch);
    // Every node shares p1 in this fixture, so isolating it dims nothing —
    // what matters is that the isolation is keyed on the new dimension and
    // not silently still on state.
    expect(container.querySelectorAll("g.fnode.dim")).toHaveLength(0);
  });

  it("opens the guide from the keyboard and from the nav, and closes on Escape", () => {
    render();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "?" }));
    });
    expect(container.querySelector(".fguide")).toBeTruthy();
    expect(container.querySelector(".fguide")?.textContent).toContain("Walk the critical path");
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(container.querySelector(".fguide")).toBeNull();

    const help = [...container.querySelectorAll<HTMLButtonElement>(".fnav button")].find((b) =>
      b.title.startsWith("What this screen"),
    );
    click(help);
    expect(container.querySelector(".fguide")).toBeTruthy();
  });

  it("previews a node's immediate neighbours on hover, and only while nothing louder is being read", () => {
    render();
    const hovered = nodeGroup("Build the endpoint");
    act(() => {
      hovered.dispatchEvent(new MouseEvent("pointerover", { bubbles: true }));
    });
    // a and d touch b; c does not.
    expect(nodeGroup("Draft the comms plan").classList.contains("dim")).toBe(true);
    expect(nodeGroup("Design the endpoint").classList.contains("dim")).toBe(false);

    act(() => {
      hovered.dispatchEvent(new MouseEvent("pointerout", { bubbles: true }));
    });
    expect(container.querySelectorAll("g.fnode.dim")).toHaveLength(0);
  });

  it("has a presentation-stage control wired to the fullscreen API", () => {
    const calls: string[] = [];
    (document.documentElement as unknown as { requestFullscreen: () => Promise<void> }).requestFullscreen =
      () => {
        calls.push("enter");
        return Promise.resolve();
      };
    render();
    const stage = [...container.querySelectorAll<HTMLButtonElement>(".fnav button")].find((b) =>
      b.title.startsWith("Presentation stage"),
    );
    click(stage);
    expect(calls).toEqual(["enter"]);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "F" }));
    });
    expect(calls).toEqual(["enter", "enter"]);
  });

  it("puts the reading in the address bar so a link reopens it", () => {
    render();
    click(nodeGroup("Build the endpoint"));
    expect(window.location.hash).toContain("n=b");
    click([...container.querySelectorAll<HTMLButtonElement>("button.lensb")].find((b) => b.textContent === "lane"));
    expect(window.location.hash).toContain("paint=lane");
  });
});

describe("the authored shape on screen", () => {
  function shape() {
    BOARD.phases = [
      { id: "intake", label: "Intake" },
      { id: "build", label: "Build + verify" },
    ];
    BOARD.groups = [{ id: "g", label: "Planning loop", lane: null, phases: ["intake", "build"] }];
    BOARD.unphased = true;
    BOARD.stages = 3;
    BOARD.nodes[3]!.phases = ["build", "intake"];
    BOARD.edges[2]!.backward = true;
  }
  function unshape() {
    BOARD.phases = [];
    BOARD.groups = [];
    BOARD.unphased = false;
    BOARD.shape_problem = null;
    BOARD.stages = 3;
    BOARD.nodes[3]!.phases = [];
    BOARD.edges[2]!.backward = false;
  }

  afterEach(unshape);

  it("captions the columns from the fence and the trailing one Unphased", () => {
    shape();
    render();
    expect(container.textContent).toContain("Intake");
    expect(container.textContent).toContain("Build + verify");
    expect(container.textContent).toContain("unphased");
    expect(container.textContent).not.toContain("stage 0");
  });

  it("frames a group and marks a node that claims two phases", () => {
    shape();
    render();
    expect(container.querySelector("rect.wf-c-group")).toBeTruthy();
    expect(container.textContent).toContain("Planning loop");
    expect(nodeGroup("Announce the endpoint").classList.contains("split")).toBe(true);
    expect(container.textContent).toContain("in 2 phases");
  });

  it("marks an arrow that runs against the authored order", () => {
    shape();
    render();
    expect(container.querySelector("path.backward")).toBeTruthy();
  });

  it("keeps drawing when the fence is broken, and names the document and line", () => {
    BOARD.shape_problem = { path: "docs/r.md", line: 4, detail: "line 6: bad indentation" };
    render();
    expect(container.querySelectorAll("g.fnode")).toHaveLength(4);
    const banner = container.querySelector(".fproblem");
    expect(banner?.textContent).toContain("docs/r.md:4");
    expect(banner?.textContent).toContain("bad indentation");
  });
});

describe("release 3: non-gating arrows, commits, and the story", () => {
  afterEach(() => {
    BOARD.edges = BOARD.edges.filter((e) => e.gating);
    BOARD.nodes[0]!.commits = 0;
  });

  it("draws a fed_by arrow that can never be read as a dependency", () => {
    BOARD.edges.push({
      from: "c",
      to: "a",
      disposition: "unsatisfied",
      gating: false,
      backward: false,
      label: "record result",
    });
    render();
    const feed = container.querySelector("path.wf-a-feed");
    expect(feed).toBeTruthy();
    // It never borrows the critical path's emphasis.
    expect(feed?.classList.contains("wf-a-emph")).toBe(false);
  });

  it("marks a node that has real commits behind it", () => {
    BOARD.nodes[0]!.commits = 3;
    render();
    expect(nodeGroup("Design the endpoint").textContent).toContain("3 commits");
    expect(nodeGroup("Build the endpoint").textContent).not.toContain("commit");
  });

  it("walks the story, focuses each beat, and hands the whole diagram back", () => {
    render();
    const bar = container.querySelector(".fstory");
    expect(bar).toBeTruthy();
    expect(bar?.textContent).toContain("3 beats");

    const next = [...container.querySelectorAll<HTMLButtonElement>(".fstory button")].find(
      (b) => b.title === "Next beat",
    );
    click(next);
    // Beat one is the first step of the critical path; everything else dims.
    expect(container.querySelector(".fstory")?.textContent).toContain("Design the endpoint");
    expect(nodeGroup("Draft the comms plan").classList.contains("dim")).toBe(true);
    expect(nodeGroup("Design the endpoint").classList.contains("dim")).toBe(false);

    const close = [...container.querySelectorAll<HTMLButtonElement>(".fstory button")].find(
      (b) => b.title.startsWith("Show the whole"),
    );
    click(close);
    expect(container.querySelectorAll("g.fnode.dim")).toHaveLength(0);
  });
});

describe("source evidence", () => {
  afterEach(() => {
    BOARD.nodes[0]!.commits = 0;
  });

  it("lists one row per commit, not per field changed", () => {
    BOARD.nodes[0]!.commits = 2;
    render();
    click(nodeGroup("Design the endpoint"));
    const rows = [...container.querySelectorAll(".fplist-row")].filter((r) =>
      /^[0-9a-f]{7}/.test(r.textContent ?? ""),
    );
    // Two field events share one commit; the reader asked about commits.
    expect(rows).toHaveLength(2);
    expect(container.textContent).toContain("abc1234");
  });

  it("says nothing at all when no commit has touched the issue", () => {
    render();
    click(nodeGroup("Build the endpoint"));
    expect(container.textContent).not.toContain("abc1234");
  });
});
