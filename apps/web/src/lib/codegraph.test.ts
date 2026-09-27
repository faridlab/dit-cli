// The whole-root picture has thousands of nodes, so everything the screen
// decides about it is decided here, where it can be checked: which files
// the filters keep, which cluster a file joins, that the pointer finds the
// right node without scanning all of them, and that the layout lands the
// same way every time.
import { describe, expect, it } from "vitest";
import {
  adjacency,
  blitTransform,
  buildGrid,
  centreOn,
  clusterAnchors,
  clusterLabel,
  clustersOf,
  commonFolder,
  DEFAULT_FILTERS,
  degrees,
  filterGraph,
  formatCount,
  graphRadius,
  hitTest,
  isTestPath,
  labelled,
  placeLabels,
  layoutKey,
  positionBounds,
  ROOT_CLUSTER,
  searchGraph,
  topFolderOf,
} from "./codegraph";
import { runLayout, startLayout, type LayoutInput, type LayoutMessage } from "./codegraphLayout";
import type { CodeGraphFileDto } from "./types";

const f = (path: string, users = 0, generated = false): CodeGraphFileDto => ({ path, users, generated });

const GRAPH = {
  files: [
    f("src/a.ts", 3),
    f("src/components/Button.tsx", 5),
    f("src/components/Button.test.tsx", 0),
    f("src/generated/Entity.schema.ts", 9, true),
    f("src/lib/__tests__/util.ts", 0),
  ],
  edges: [
    [0, 1],
    [2, 1],
    [1, 3],
    [4, 0],
  ] as Array<[number, number]>,
};

describe("filtering the graph", () => {
  it("recognises tests by the usual conventions", () => {
    expect(isTestPath("src/x.test.tsx")).toBe(true);
    expect(isTestPath("src/x.spec.ts")).toBe(true);
    expect(isTestPath("src/lib/__tests__/util.ts")).toBe(true);
    expect(isTestPath("app/tests/api.rs")).toBe(true);
    expect(isTestPath("tests/api.rs")).toBe(true);
    expect(isTestPath("src/testing/Harness.tsx")).toBe(false);
  });

  it("hides generated files by default, and every edge that touched them", () => {
    const g = filterGraph(GRAPH, DEFAULT_FILTERS);
    expect(g.files.map((x) => x.path)).not.toContain("src/generated/Entity.schema.ts");
    expect(g.edges).toHaveLength(3);
    expect(g.total).toEqual({ files: 5, edges: 4 });
    // Indexes are renumbered into the kept list, and point back to the source.
    for (const [a, b] of g.edges) {
      expect(g.files[a]).toBeDefined();
      expect(g.files[b]).toBeDefined();
    }
    expect(g.source).toEqual([0, 1, 2, 4]);
  });

  it("drops tests and rarely-used files on request", () => {
    const g = filterGraph(GRAPH, { generated: true, tests: false, minUsers: 3 });
    expect(g.files.map((x) => x.path)).toEqual(["src/a.ts", "src/components/Button.tsx", "src/generated/Entity.schema.ts"]);
    expect(g.edges).toEqual([
      [0, 1],
      [1, 2],
    ]);
  });

  it("keys a layout by root, filters and what is actually drawn", () => {
    const a = filterGraph(GRAPH, DEFAULT_FILTERS);
    const b = filterGraph(GRAPH, { ...DEFAULT_FILTERS, generated: true });
    expect(layoutKey("web", DEFAULT_FILTERS, a)).toBe(layoutKey("web", DEFAULT_FILTERS, filterGraph(GRAPH, DEFAULT_FILTERS)));
    expect(layoutKey("web", DEFAULT_FILTERS, a)).not.toBe(layoutKey("web", { ...DEFAULT_FILTERS, generated: true }, b));
    const renamed = { ...GRAPH, files: GRAPH.files.map((x, i) => (i === 0 ? f("src/b.ts", 3) : x)) };
    expect(layoutKey("web", DEFAULT_FILTERS, filterGraph(renamed, DEFAULT_FILTERS))).not.toBe(layoutKey("web", DEFAULT_FILTERS, a));
  });
});

describe("clusters", () => {
  it("finds the folder every file shares, in whole segments", () => {
    expect(commonFolder(["src/a/x.ts", "src/b/y.ts"])).toBe("src");
    expect(commonFolder(["src/ab/x.ts", "src/ac/y.ts"])).toBe("src");
    expect(commonFolder(["a/x.ts", "b/y.ts"])).toBe("");
    expect(commonFolder(["src/a/x.ts"])).toBe("src/a");
    expect(commonFolder([])).toBe("");
  });

  it("groups a file by the first folder under that", () => {
    expect(topFolderOf("src/components/table/Cell.tsx", "src")).toBe("components");
    expect(topFolderOf("src/main.tsx", "src")).toBe(ROOT_CLUSTER);
    expect(topFolderOf("crates/core/src/lib.rs", "")).toBe("crates");
  });

  it("names the files directly in the common folder after it", () => {
    expect(clusterLabel(ROOT_CLUSTER, "src")).toBe("src (root files)");
    expect(clusterLabel(ROOT_CLUSTER, "apps/web/src")).toBe("src (root files)");
    expect(clusterLabel(ROOT_CLUSTER, "")).toBe("top level (root files)");
    expect(clusterLabel("components", "src")).toBe("components");
    expect(clustersOf([f("src/a.ts"), f("src/b/c.ts")]).common).toBe("src");
  });

  it("ranks clusters by size and shares the last colour past the palette", () => {
    const files = [
      ...Array.from({ length: 3 }, (_, i) => f(`src/big/${i}.ts`)),
      ...Array.from({ length: 12 }, (_, i) => f(`src/c${String(i).padStart(2, "0")}/x.ts`)),
    ];
    const c = clustersOf(files);
    expect(c.names[0]).toBe("big");
    expect(c.counts[0]).toBe(3);
    expect(c.of[0]).toBe(0);
    expect(c.colour[0]).toBe(0);
    expect(c.colour[12]).toBe(9);
    expect(Math.max(...c.colour)).toBe(9);
  });

  it("gives a cluster of generated files no colour, and keeps the others' colours", () => {
    const files = [
      ...Array.from({ length: 5 }, (_, i) => f(`src/generated/${i}.ts`, 0, true)),
      f("src/a/x.ts"),
      f("src/a/y.ts"),
      f("src/b/z.ts"),
    ];
    const c = clustersOf(files);
    expect(c.names).toEqual(["generated", "a", "b"]);
    expect(c.colour).toEqual([-1, 0, 1]);
    // Without the generated files, a and b keep the same colours.
    expect(clustersOf(files.filter((x) => !x.generated)).colour).toEqual([0, 1]);
  });

  it("anchors clusters round a circle that grows with the graph", () => {
    const small = clusterAnchors(4, 100);
    const big = clusterAnchors(4, 10_000);
    expect(small[0]?.x).toBeCloseTo(0);
    expect(small[0]?.y).toBeLessThan(0);
    expect(Math.hypot(big[1]?.x ?? 0, big[1]?.y ?? 0)).toBeGreaterThan(Math.hypot(small[1]?.x ?? 0, small[1]?.y ?? 0));
    expect(clusterAnchors(1, 50)).toEqual([{ x: 0, y: 0 }]);
  });
});

describe("nodes", () => {
  it("sizes by users on a square root, 2 to 14", () => {
    expect(graphRadius(0)).toBe(2);
    expect(graphRadius(100)).toBe(14);
    expect(graphRadius(4)).toBeGreaterThan(graphRadius(1));
  });

  it("labels the most-used files, ties by path", () => {
    const files = [f("b", 1), f("a", 1), f("c", 7)];
    expect([...labelled(files, 2)].sort()).toEqual([1, 2]);
  });

  it("counts imports each way and lists neighbours both ways", () => {
    const { ins, outs } = degrees(3, [
      [0, 1],
      [2, 1],
    ]);
    expect([...ins]).toEqual([0, 2, 0]);
    expect([...outs]).toEqual([1, 0, 1]);
    expect(adjacency(3, [[0, 1]])).toEqual([[1], [0], []]);
  });
});

describe("finding the node under the pointer", () => {
  it("finds the nearest disc that contains the point, and nothing in empty space", () => {
    const xs = [0, 30, 1000];
    const ys = [0, 0, 1000];
    const rs = [5, 5, 10];
    const grid = buildGrid(xs, ys, 32);
    expect(hitTest(grid, xs, ys, rs, 2, 1)).toBe(0);
    expect(hitTest(grid, xs, ys, rs, 28, 0)).toBe(1);
    expect(hitTest(grid, xs, ys, rs, 15, 0)).toBe(-1);
    expect(hitTest(grid, xs, ys, rs, 15, 0, 12)).toBe(0);
    expect(hitTest(grid, xs, ys, rs, 1005, 995)).toBe(2);
  });

  it("agrees with a full scan on a crowd of nodes", () => {
    const n = 2000;
    const xs = Array.from({ length: n }, (_, i) => ((i * 7919) % 997) * 3);
    const ys = Array.from({ length: n }, (_, i) => ((i * 104729) % 991) * 3);
    const rs = Array.from({ length: n }, (_, i) => 2 + (i % 12));
    const grid = buildGrid(xs, ys);
    for (let probe = 0; probe < 200; probe += 1) {
      const x = (probe * 37) % 3000;
      const y = (probe * 53) % 3000;
      let best = -1;
      let bestD = Number.POSITIVE_INFINITY;
      for (let i = 0; i < n; i += 1) {
        const d = Math.hypot((xs[i] ?? 0) - x, (ys[i] ?? 0) - y);
        if (d <= (rs[i] ?? 0) && d < bestD) {
          best = i;
          bestD = d;
        }
      }
      expect(hitTest(grid, xs, ys, rs, x, y)).toBe(best);
    }
  });
});

describe("search", () => {
  it("ranks an exact name first, then a name that starts with it, then by users", () => {
    const files = [f("src/lib/hooks-extra.ts", 9), f("src/crud/hooks.ts", 2), f("src/x/usehooks.ts", 50), f("src/y/other.ts")];
    expect(searchGraph(files, "hooks")).toEqual([1, 0, 2]);
    expect(searchGraph(files, "  ")).toEqual([]);
    expect(searchGraph(files, "OTHER")).toEqual([3]);
  });
});

describe("the view", () => {
  it("bounds the positions and centres a point", () => {
    expect(positionBounds([0, 10], [0, 20], 5)).toEqual({ x: -5, y: -5, w: 20, h: 30 });
    expect(centreOn(10, 20, 800, 600, 2)).toEqual({ k: 2, x: 380, y: 260 });
  });

  it("prints counts with thousands separators", () => {
    expect(formatCount(17104)).toBe("17,104");
  });
});

describe("the layout", () => {
  const n = 120;
  const input: LayoutInput = {
    paths: Array.from({ length: n }, (_, i) => `src/c${i % 4}/f${i}.ts`),
    radii: Array.from({ length: n }, (_, i) => graphRadius(i % 9)),
    cluster: Array.from({ length: n }, (_, i) => i % 4),
    anchors: clusterAnchors(4, n),
    edges: Array.from({ length: n - 1 }, (_, i) => [i + 1, Math.floor(i / 3)] as [number, number]),
  };

  it("lands the same input in the same place after the same ticks", () => {
    const a = startLayout(input);
    const b = startLayout(input);
    a.step(60);
    b.step(60);
    expect([...b.positions()]).toEqual([...a.positions()]);
    expect(a.ticks()).toBe(60);
    expect(a.progress()).toBeGreaterThan(0);
  });

  it("pulls each cluster toward its own anchor", () => {
    const run = startLayout(input);
    run.step(200);
    const p = run.positions();
    const mean = (c: number) => {
      let x = 0;
      let y = 0;
      let k = 0;
      for (let i = c; i < n; i += 4) {
        x += p[i * 2] ?? 0;
        y += p[i * 2 + 1] ?? 0;
        k += 1;
      }
      return { x: x / k, y: y / k };
    };
    // Cluster 0's anchor is at twelve o'clock, cluster 2's at six.
    expect(mean(0).y).toBeLessThan(mean(2).y);
  });

  it("streams pictures while it runs and ends with a done message", () => {
    const messages: LayoutMessage[] = [];
    runLayout({ id: 7, input }, (m) => messages.push(m));
    const last = messages[messages.length - 1];
    expect(last?.kind).toBe("done");
    expect(messages.filter((m) => m.kind === "tick").length).toBeGreaterThan(2);
    expect(messages.every((m) => m.id === 7 && m.positions.length === n * 2)).toBe(true);
  });
});

describe("copying the painted layer", () => {
  it("lands every world point where the new view would paint it", () => {
    const from = { x: 30, y: -10, k: 0.5 };
    const to = { x: -120, y: 40, k: 1.25 };
    const b = blitTransform(from, to);
    for (const [wx, wy] of [
      [0, 0],
      [100, -40],
      [-333, 77],
    ] as const) {
      // Where the layer has the point, carried through the copy…
      const lx = wx * from.k + from.x;
      const ly = wy * from.k + from.y;
      // …is where the new view puts it.
      expect(lx * b.s + b.dx).toBeCloseTo(wx * to.k + to.x);
      expect(ly * b.s + b.dy).toBeCloseTo(wy * to.k + to.y);
    }
    expect(blitTransform(from, from)).toEqual({ s: 1, dx: 0, dy: 0 });
  });
});

describe("placing labels", () => {
  it("keeps the first of two overlapping labels and every label that fits", () => {
    const boxes = [
      { x: 0, y: 0, w: 50, h: 12 },
      { x: 30, y: 4, w: 50, h: 12 },
      { x: 0, y: 30, w: 50, h: 12 },
      { x: 200, y: 0, w: 80, h: 12 },
    ];
    expect(placeLabels(boxes)).toEqual([true, false, true, true]);
    expect(placeLabels([])).toEqual([]);
  });
});
