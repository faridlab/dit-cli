// The code map's picture has to be the same picture every time: a link
// pasted into a thread, or a screenshot, is only worth something if the
// folder it names lands where it did for the sender. These pin the seeding,
// the cut at the unit cap, and the labels.
import { describe, expect, it } from "vitest";
import {
  boxExit,
  capUnits,
  capNeighbours,
  connector,
  EDGE_CAP,
  FOCUS_GEO,
  fitViewBox,
  focusGeometry,
  edgeWidth,
  folderCrumbs,
  fullyGenerated,
  halfOf,
  hashString,
  lastSegment,
  layoutUnits,
  parentFolder,
  LAYER,
  layeredLayout,
  middleTruncate,
  orderRanks,
  rankUnits,
  seedPosition,
  shortenPath,
  topEdges,
  unitSize,
  unitWeights,
} from "./codemap";
import type { CodeUnitDto, CodeUnitEdgeDto } from "./types";

function unit(path: string, over: Partial<CodeUnitDto> = {}): CodeUnitDto {
  return { path, folder: true, files: 4, generated: 0, inbound: 0, outbound: 0, ...over };
}

describe("seeding the layout", () => {
  it("hashes a path the same way every time, and different paths differently", () => {
    expect(hashString("src/crud")).toBe(hashString("src/crud"));
    expect(hashString("src/crud")).not.toBe(hashString("src/lib"));
    expect(hashString("")).toBe(0x811c9dc5);
  });

  it("seeds inside the box, from the path alone", () => {
    const a = seedPosition("src/crud", 900, 600);
    expect(seedPosition("src/crud", 900, 600)).toEqual(a);
    expect(Math.abs(a.x)).toBeLessThanOrEqual(360);
    expect(Math.abs(a.y)).toBeLessThanOrEqual(240);
  });

  it("lays the same folder out identically twice", () => {
    const units = ["src/a", "src/b", "src/c", "src/d.ts"].map((p) => unit(p, { folder: !p.endsWith(".ts") }));
    const edges: CodeUnitEdgeDto[] = [
      { from: "src/a", to: "src/b", imports: 3 },
      { from: "src/c", to: "src/b", imports: 1 },
      { from: "src/d.ts", to: "src/a", imports: 2 },
    ];
    const first = layoutUnits(units, edges).map((p) => [p.x, p.y]);
    const second = layoutUnits(units, edges).map((p) => [p.x, p.y]);
    expect(second).toEqual(first);
    // …and actually spreads them out rather than stacking them.
    const distinct = new Set(first.map(([x, y]) => `${Math.round(x ?? 0)},${Math.round(y ?? 0)}`));
    expect(distinct.size).toBe(4);
  });

  it("places a lone unit at the centre", () => {
    const [only] = layoutUnits([unit("src")], []);
    expect(only).toMatchObject({ x: expect.any(Number), y: expect.any(Number) });
  });
});

describe("the unit cap", () => {
  it("keeps every unit under the cap and drops edges to units it does not know", () => {
    const units = [unit("a"), unit("b")];
    const edges = [
      { from: "a", to: "b", imports: 1 },
      { from: "a", to: "zz", imports: 1 },
    ];
    expect(capUnits(units, edges, 5)).toEqual({ units, edges: [edges[0]], hidden: 0 });
  });

  it("keeps the heaviest units, counting boundary imports and edge weight", () => {
    const units = [unit("a", { inbound: 1 }), unit("b"), unit("c", { outbound: 9 }), unit("d")];
    const edges = [{ from: "b", to: "d", imports: 5 }];
    expect(unitWeights(units, edges).get("d")).toBe(5);
    const cut = capUnits(units, edges, 3);
    expect(cut.units.map((u) => u.path)).toEqual(["b", "c", "d"]);
    expect(cut.hidden).toBe(1);
    expect(cut.edges).toEqual(edges);
  });

  it("breaks a tie by path, so the cut never flickers", () => {
    const units = [unit("c"), unit("a"), unit("b")];
    expect(capUnits(units, [], 2).units.map((u) => u.path)).toEqual(["a", "b"]);
  });
});

describe("sizes and marks", () => {
  it("marks a unit generated only when all of it is", () => {
    expect(fullyGenerated({ files: 3, generated: 3 })).toBe(true);
    expect(fullyGenerated({ files: 3, generated: 2 })).toBe(false);
    expect(fullyGenerated({ files: 0, generated: 0 })).toBe(false);
  });

  it("grows folders with their file count, within bounds", () => {
    const small = unitSize({ folder: true, files: 1 });
    const big = unitSize({ folder: true, files: 400 });
    const huge = unitSize({ folder: true, files: 40_000 });
    expect(big.w).toBeGreaterThan(small.w);
    expect(huge).toEqual(big);
    expect(unitSize({ folder: false, files: 1 }).h).toBeLessThan(small.h);
  });

  it("widens a file pill to fit its name, up to a cap", () => {
    const short = unitSize({ folder: false, files: 1, path: "src/vite-env.d.ts" });
    expect(short.w).toBeGreaterThanOrEqual("vite-env.d.ts".length * 7 + 16);
    const long = unitSize({ folder: false, files: 1, path: `src/${"x".repeat(80)}.tsx` });
    expect(long.w).toBe(180);
  });

  it("thickens edges logarithmically and caps them", () => {
    expect(edgeWidth(1)).toBe(1);
    expect(edgeWidth(4)).toBe(3);
    expect(edgeWidth(10_000)).toBe(6);
    expect(edgeWidth(0)).toBe(1);
  });

  it("lands an arrow on the edge of the box it points at", () => {
    expect(boxExit(0, 0, 100, 40, 200, 0)).toEqual({ x: 50, y: 0 });
    expect(boxExit(0, 0, 100, 40, 0, -200)).toEqual({ x: 0, y: -20 });
    expect(boxExit(5, 5, 10, 10, 5, 5)).toEqual({ x: 5, y: 5 });
  });
});

describe("paths as labels", () => {
  it("names a unit by its last segment", () => {
    expect(lastSegment("src/crud/hooks.ts")).toBe("hooks.ts");
    expect(lastSegment("src/crud/")).toBe("crud");
  });

  it("builds the breadcrumb from the root down, each step a folder", () => {
    expect(folderCrumbs("web", "")).toEqual([{ label: "web", folder: "" }]);
    expect(folderCrumbs("web", "src/desks/people")).toEqual([
      { label: "web", folder: "" },
      { label: "src", folder: "src" },
      { label: "desks", folder: "src/desks" },
      { label: "people", folder: "src/desks/people" },
    ]);
  });

  it("finds the folder a file sits in", () => {
    expect(parentFolder("src/crud/hooks.ts")).toBe("src/crud");
    expect(parentFolder("main.tsx")).toBe("");
  });
});

describe("fitting the picture to the screen", () => {
  const box = { x: -50, y: -20, w: 100, h: 40 };

  it("never blows a small folder up past the scale cap, keeping it centred", () => {
    const v = fitViewBox(box, 1000, 500, 1);
    expect(v).toEqual({ x: -500, y: -250, w: 1000, h: 500 });
  });

  it("leaves a big folder's box alone, and an unmeasured viewport too", () => {
    const big = { x: 0, y: 0, w: 4000, h: 3000 };
    expect(fitViewBox(big, 1000, 500)).toEqual(big);
    expect(fitViewBox(box, 0, 0)).toEqual(box);
  });
});

describe("shortening in the middle", () => {
  it("keeps the start and the end of a name, the end at least its extension", () => {
    expect(middleTruncate("RecordChangesPage.tsx", 17)).toBe("RecordCh…Page.tsx");
    expect(middleTruncate("RecordChangesPage.tsx", 17)).toHaveLength(17);
    expect(middleTruncate("EmployeeContract-guard.test.tsx", 14)).toBe("Employ…est.tsx");
    expect(middleTruncate("hooks.ts", 20)).toBe("hooks.ts");
    expect(middleTruncate("abc", 1)).toBe("…");
  });

  it("never loses the whole start to a long extension", () => {
    const out = middleTruncate("a.verylongextension", 6);
    expect(out).toHaveLength(6);
    expect(out.startsWith("a")).toBe(true);
  });

  it("shortens a path by its middle segments first, then to the name", () => {
    expect(shortenPath("src/crud/hooks.ts", 40)).toBe("src/crud/hooks.ts");
    expect(shortenPath("src/desks/people/Page.tsx", 21)).toBe("src/…/people/Page.tsx");
    expect(shortenPath("src/desks/people/self/Page.tsx", 16)).toBe("src/…/Page.tsx");
    expect(shortenPath("src/desks/people/RecordChangesPage.tsx", 23)).toBe("…/RecordChangesPage.tsx");
    expect(shortenPath("src/desks/people/RecordChangesPage.tsx", 22)).toBe("RecordChangesPage.tsx");
    expect(shortenPath("src/desks/people/RecordChangesPage.tsx", 17)).toBe("RecordCh…Page.tsx");
  });
});

describe("which edges are drawn", () => {
  const e = (from: string, to: string, imports: number): CodeUnitEdgeDto => ({ from, to, imports });

  it("draws every edge when there are few", () => {
    const edges = [e("a", "b", 1), e("b", "c", 2)];
    expect(topEdges(edges).size).toBe(2);
  });

  it("keeps the heaviest, ties by from then to", () => {
    const edges = [e("a", "b", 1), e("c", "d", 5), e("b", "c", 1), e("a", "c", 1)];
    const top = topEdges(edges, 2);
    expect([...top].sort()).toEqual(["a\u0000b", "c\u0000d"].sort());
    expect(topEdges(Array.from({ length: 50 }, (_, i) => e(`u${i}`, "z", i))).size).toBe(EDGE_CAP);
  });
});

describe("the layered layout", () => {
  const e = (from: string, to: string, imports = 1): CodeUnitEdgeDto => ({ from, to, imports });

  it("ranks importers left of what they import, by longest path", () => {
    const { rank } = rankUnits(["a", "b", "c", "d"], [e("a", "b"), e("b", "c"), e("a", "c")]);
    expect(rank.get("a")).toBe(0);
    expect(rank.get("b")).toBe(1);
    expect(rank.get("c")).toBe(2);
    expect(rank.get("d")).toBe(-1);
  });

  it("pulls an importer next to its nearest import rather than leaving it first", () => {
    // a → b → c → d, and e imports only d: e belongs just left of d.
    const { rank } = rankUnits(["a", "b", "c", "d", "e"], [e("a", "b"), e("b", "c"), e("c", "d"), e("e", "d")]);
    expect(rank.get("d")).toBe(3);
    expect(rank.get("e")).toBe(2);
    expect(rank.get("a")).toBe(0);
  });

  it("breaks a cycle at the back edge a path-ordered search finds", () => {
    const { rank, back } = rankUnits(["a", "b", "c"], [e("a", "b"), e("b", "c"), e("c", "a")]);
    expect([...back]).toEqual(["c\u0000a"]);
    expect([rank.get("a"), rank.get("b"), rank.get("c")]).toEqual([0, 1, 2]);
  });

  it("orders a rank by where its neighbours sit", () => {
    // x feeds the bottom of the next rank, w the top; the sweep uncrosses them.
    const layers = [["w", "x"], ["p", "q"]];
    const order = orderRanks(layers, [e("w", "q"), e("x", "p")]);
    expect(order[0]).toEqual(["w", "x"]);
    expect(order[1]).toEqual(["q", "p"]);
  });

  it("gives every unit its own slot, and the same slot every time", () => {
    const units = Array.from({ length: 40 }, (_, i) => unit(`src/p/F${String(i).padStart(2, "0")}.tsx`, { folder: false }));
    const edges = units.slice(1).map((u, i) => e(units[Math.floor(i / 3)]?.path ?? "", u.path, (i % 4) + 1));
    const first = layeredLayout(units, edges);
    const second = layeredLayout([...units].reverse(), [...edges].reverse());
    const pos = (list: typeof first) => new Map(list.map((p) => [p.unit.path, `${p.x},${p.y}`]));
    expect(pos(second)).toEqual(pos(first));
    // No two boxes share a slot.
    expect(new Set(first.map((p) => `${p.x},${p.y}`)).size).toBe(units.length);
    // Columns step by a fixed amount, rows by a fixed amount.
    for (const p of first) {
      expect((p.x - LAYER.nodeW / 2) % (LAYER.nodeW + LAYER.colGap)).toBe(0);
      expect(p.w).toBe(LAYER.nodeW);
    }
  });

  it("stands unconnected units in their own trailing columns", () => {
    const units = [unit("a"), unit("b"), unit("lonely")];
    const placed = layeredLayout(units, [e("a", "b")]);
    const x = new Map(placed.map((p) => [p.unit.path, p.x]));
    expect((x.get("lonely") ?? 0) > (x.get("b") ?? 0)).toBe(true);
  });
});

describe("the focus view", () => {
  it("caps each side at twelve, most depended-on first, ties by path", () => {
    const list = Array.from({ length: 15 }, (_, i) => ({ path: `f${String(i).padStart(2, "0")}`, users: i % 3 }));
    const cut = capNeighbours(list);
    expect(cut.shown).toHaveLength(12);
    expect(cut.hidden).toBe(3);
    expect(cut.all[0]).toEqual({ path: "f02", users: 2 });
    expect(cut.all.map((n) => n.users)).toEqual([...cut.all.map((n) => n.users)].sort((a, b) => b - a));
  });

  it("splits the width into two sides and a card, and centres both sides on the anchor", () => {
    const g = focusGeometry(1200, 13, 3);
    expect(g.width).toBeLessThanOrEqual(1200);
    expect(g.centerW).toBeGreaterThanOrEqual(FOCUS_GEO.minCenter);
    expect(g.leftTop).toBe(0);
    expect(g.anchorY).toBe((13 * FOCUS_GEO.rowH) / 2);
    expect(g.rightTop).toBe(g.anchorY - (3 * FOCUS_GEO.rowH) / 2);
    expect(g.pillY("right", 1)).toBe(g.anchorY);
    expect(g.cardTop).toBe(g.anchorY - FOCUS_GEO.anchor);
  });

  it("keeps a usable side column on a narrow screen and room for a lone card", () => {
    const g = focusGeometry(500, 0, 0);
    expect(g.sideW).toBe(FOCUS_GEO.minSide);
    expect(g.cardTop).toBe(0);
  });

  it("draws a connector as a horizontal S-curve", () => {
    expect(connector(0, 0, 100, 50)).toBe("M0,0 C50,0 50,50 100,50");
  });
});

describe("which half of the screen shows", () => {
  it("follows the link, and otherwise shows the file when one is in focus", () => {
    expect(halfOf("folder", "src/a.ts")).toBe("folder");
    expect(halfOf(null, "src/a.ts")).toBe("focus");
    expect(halfOf(undefined, null)).toBe("folder");
  });
});
