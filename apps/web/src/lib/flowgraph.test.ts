// The reading tools are only as good as their answers about the graph:
// which nodes a selection lights up, which route joins two picks, what a
// typed query lands on, and how a long title survives a small box.
import { describe, expect, it } from "vitest";
import {
  buildGraph,
  chainEdges,
  layoutBoard,
  reach,
  routeBetween,
  searchNodes,
  semanticOf,
  wrapLabel,
  paintKeyOf,
  paintLegend,
  paintSlot,
  detailAt,
  columnLabel,
  groupFrames,
  storyChapters,
  METRICS,
} from "./flowgraph";
import type { FlowBoardDto, FlowNodeDto } from "./types";

function node(id: string, over: Partial<FlowNodeDto> = {}): FlowNodeDto {
  return {
    id,
    short_ref: id,
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

/** a -> b -> d, and a -> c (a diamond missing one side). */
function board(): FlowBoardDto {
  return {
    name: "launch",
    lanes: [{ id: null, label: "Unlaned" }],
    phases: [],
    groups: [],
    unphased: false,
    shape_problem: null,
    stages: 3,
    nodes: [
      node("a", { title: "Design the endpoint", number: 1 }),
      node("b", { title: "Build the endpoint", number: 2, stage: 1 }),
      node("c", { title: "Draft the comms plan", number: 3, stage: 1, row: 1 }),
      node("d", { title: "Announce it", number: 4, stage: 2 }),
    ],
    edges: [
      { from: "a", to: "b", disposition: "unsatisfied", gating: true, backward: false, label: null },
      { from: "a", to: "c", disposition: "unsatisfied", gating: true, backward: false, label: null },
      { from: "b", to: "d", disposition: "unsatisfied", gating: true, backward: false, label: null },
    ],
    main_path: ["a", "b", "d"],
  };
}

describe("reach", () => {
  it("lights up everything a node waits on, and everything waiting on it", () => {
    const graph = buildGraph(board());
    expect([...reach(graph, "d", "up")].sort()).toEqual(["a", "b"]);
    expect([...reach(graph, "a", "down")].sort()).toEqual(["b", "c", "d"]);
    expect([...reach(graph, "c", "down")]).toEqual([]);
  });

  it("terminates on a dependency cycle instead of walking forever", () => {
    const cyclic = board();
    cyclic.edges.push({ from: "d", to: "a", disposition: "unsatisfied", gating: true, backward: true, label: null });
    const graph = buildGraph(cyclic);
    expect([...reach(graph, "a", "down")].sort()).toEqual(["a", "b", "c", "d"].filter((i) => i !== "a"));
  });

  it("ignores edges pointing at nodes this board does not draw", () => {
    const partial = board();
    partial.edges.push({ from: "a", to: "elsewhere", disposition: "unsatisfied", gating: true, backward: false, label: null });
    const graph = buildGraph(partial);
    expect(graph.out.get("a")).toEqual(["b", "c"]);
  });
});

describe("routeBetween", () => {
  it("traces the chain in the direction the arrows run", () => {
    const graph = buildGraph(board());
    expect(routeBetween(graph, "a", "d")).toEqual(["a", "b", "d"]);
  });

  it("accepts the two ends in either order", () => {
    const graph = buildGraph(board());
    expect(routeBetween(graph, "d", "a")).toEqual(["a", "b", "d"]);
  });

  it("is null when nothing connects the two", () => {
    const graph = buildGraph(board());
    expect(routeBetween(graph, "c", "d")).toBeNull();
  });

  it("names the edges of the chain it returns", () => {
    expect([...chainEdges(["a", "b", "d"])]).toEqual(["a>b", "b>d"]);
  });
});

describe("searchNodes", () => {
  const nodes = board().nodes;

  it("puts an exact issue number first", () => {
    expect(searchNodes(nodes, "#3")[0]?.id).toBe("c");
  });

  it("prefers a title that starts with the query over one that contains it", () => {
    expect(searchNodes(nodes, "draft")[0]?.id).toBe("c");
    expect(searchNodes(nodes, "endpoint").map((n) => n.id).sort()).toEqual(["a", "b"]);
  });

  it("returns nothing for an empty query rather than everything", () => {
    expect(searchNodes(nodes, "   ")).toEqual([]);
  });
});

describe("wrapLabel", () => {
  it("breaks on words instead of mid-word", () => {
    expect(wrapLabel("Rebuild the people desk", 14, 2)).toEqual(["Rebuild the", "people desk"]);
  });

  it("marks a title that did not fit", () => {
    const lines = wrapLabel("Declare relations on HR setup entities", 14, 2);
    expect(lines).toHaveLength(2);
    expect(lines[1]?.endsWith("…")).toBe(true);
  });

  it("cuts a single word longer than the line", () => {
    expect(wrapLabel("supercalifragilistic", 10, 1)).toEqual(["supercali…"]);
  });

  it("leaves a title that fits alone", () => {
    expect(wrapLabel("Short one", 14, 2)).toEqual(["Short one"]);
  });
});

describe("semanticOf", () => {
  it("reads done from the status category and the rest from readiness", () => {
    expect(semanticOf(node("x", { category: "done", readiness: "not_pickable" }))).toBe("done");
    expect(semanticOf(node("x", { readiness: "ready" }))).toBe("ready");
    expect(semanticOf(node("x", { readiness: "blocked" }))).toBe("blocked");
    expect(semanticOf(node("x", { readiness: "not_pickable" }))).toBe("doing");
  });
});

describe("layoutBoard", () => {
  it("drops lanes with no members so the diagram has no empty bands", () => {
    const empty = board();
    empty.lanes = [
      { id: "ghost", label: "Ghost" },
      { id: null, label: "Unlaned" },
    ];
    expect(layoutBoard(empty).lanes.map((l) => l.key)).toEqual([""]);
  });

  it("places every node and spreads a fan-out across the source's side", () => {
    const layout = layoutBoard(board());
    expect(layout.node.size).toBe(4);
    expect(layout.node.get("a")!.x).toBeLessThan(layout.node.get("b")!.x);
    // Two edges leave `a`, so they exit at a third and two thirds of its
    // height rather than from one point.
    expect(layout.port.get("a>b")!.fromY).toBeCloseTo(1 / 3);
    expect(layout.port.get("a>c")!.fromY).toBeCloseTo(2 / 3);
    expect(layout.port.get("b>d")!.fromY).toBeCloseTo(0.5);
  });
});

describe("painting", () => {
  const nodes = [
    node("a", { lane: "backend", kind: "bug", priority: "p0" }),
    node("b", { lane: "backend", kind: "task", priority: "p1" }),
    node("c", { lane: null, kind: "task", priority: null }),
  ];

  it("gives a missing value its own bucket instead of dropping the node", () => {
    expect(paintKeyOf(nodes[2]!, "lane")).toBe("unlaned");
    expect(paintKeyOf(nodes[2]!, "priority")).toBe("none");
  });

  it("counts every key present on the board", () => {
    expect(paintLegend(nodes, "lane")).toEqual([
      { key: "backend", count: 2 },
      { key: "unlaned", count: 1 },
    ]);
  });

  it("keeps the designed order for state rather than sorting by count", () => {
    const mixed = [
      node("x", { readiness: "blocked" }),
      node("y", { readiness: "ready" }),
      node("z", { readiness: "ready" }),
    ];
    expect(paintLegend(mixed, "state").map((r) => r.key)).toEqual(["ready", "blocked"]);
  });

  it("gives a key the same swatch every time", () => {
    const legend = paintLegend(nodes, "lane");
    expect(paintSlot(legend, "backend")).toBe(0);
    expect(paintSlot(legend, "unlaned")).toBe(1);
    expect(paintSlot(legend, "never-seen")).toBe(0);
  });

  it("drops detail as the diagram shrinks", () => {
    expect(detailAt(0.4)).toBe("map");
    expect(detailAt(1)).toBe("read");
    expect(detailAt(2)).toBe("full");
  });
});

describe("the authored shape", () => {
  function shaped(): FlowBoardDto {
    const b = board();
    b.phases = [
      { id: "intake", label: "Intake" },
      { id: "build", label: "Build" },
    ];
    b.groups = [{ id: "g", label: "Loop", lane: null, phases: ["intake", "build"] }];
    b.unphased = true;
    b.stages = 3;
    return b;
  }

  it("names the columns from the fence, and the trailing one Unphased", () => {
    const b = shaped();
    expect(columnLabel(b, 0)).toBe("Intake");
    expect(columnLabel(b, 2)).toBe("unphased");
    // No fence: the computed ranks stand, exactly as before ADR 0020.
    expect(columnLabel(board(), 1)).toBe("stage 1");
  });

  it("frames a group across the columns it spans, inside its own lane", () => {
    const b = shaped();
    const frames = groupFrames(b, layoutBoard(b));
    expect(frames).toHaveLength(1);
    expect(frames[0]!.label).toBe("Loop");
    expect(frames[0]!.w).toBeGreaterThan(METRICS.nodeW);
  });

  it("draws nothing for a group whose lane is not on this board", () => {
    const b = shaped();
    b.groups = [{ id: "ghost", label: "Ghost", lane: "nowhere", phases: ["intake"] }];
    expect(groupFrames(b, layoutBoard(b))).toEqual([]);
  });
});

describe("storyChapters", () => {
  it("tells the phases when the flow has a fence", () => {
    const b = board();
    b.phases = [
      { id: "intake", label: "Intake" },
      { id: "ship", label: "Ship" },
    ];
    b.nodes[0]!.stage = 0;
    b.nodes[1]!.stage = 1;
    b.nodes[2]!.stage = 1;
    b.nodes[3]!.stage = 1;
    const chapters = storyChapters(b);
    expect(chapters.map((c) => c.label)).toEqual(["Intake", "Ship"]);
    expect(chapters[1]!.focus).toHaveLength(3);
    expect(chapters[1]!.note).toContain("3 issues");
  });

  it("falls back to the critical path, which needs nobody to write it", () => {
    const chapters = storyChapters(board());
    expect(chapters.map((c) => c.key)).toEqual(["a", "b", "d"]);
    expect(chapters[0]!.note).toContain("Design the endpoint");
    expect(chapters[0]!.focus).toEqual(["a"]);
  });

  it("says a phase is empty rather than skipping it", () => {
    const b = board();
    b.phases = [{ id: "ghost", label: "Ghost" }];
    for (const n of b.nodes) n.stage = 1;
    expect(storyChapters(b)[0]!.note).toContain("Nothing sits");
  });
});
