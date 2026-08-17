#!/usr/bin/env node
/**
 * Bundle budget gate.
 *
 * The UI ships embedded inside the `dit` binary (ADR 0002), which is ~4 MB.
 * A bundle that doubles the download is a product regression, not a detail —
 * so it is gated here rather than noticed six months later.
 *
 * Per ARCHITECTURE.md: every rule is machine-checkable, or it is a hope.
 */
import { readdirSync, statSync, existsSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Resolved from this script's own location, because npm runs it with the
// package directory as cwd — a cwd-relative path would never find dist.
const DIST = fileURLToPath(new URL("../apps/web/dist/assets", import.meta.url));

// Gzipped, because rust-embed stores compressed and that is what ships.
const BUDGET_KB = {
  "entry (js)": 350,   // React + Radix + TanStack + shell
  "entry (css)": 60,   // Tailwind output is tiny; a blowup means a runtime CSS lib crept in
};

// Lazy chunks are exempt from the entry budget but capped individually.
// mermaid is 83 MB unpacked — it must never be reachable from the entry graph.
const LAZY_CAP_KB = 900;

if (!existsSync(DIST)) {
  console.log("no build output — run `npm run build --prefix apps/web` first");
  process.exit(0);
}

const gz = (p) => gzipSync(readFileSync(p)).length / 1024;
const files = readdirSync(DIST).map((f) => ({ f, p: join(DIST, f) }));

// Vite names the entry chunk `index-<hash>.js`.
const entryJs = files.filter(({ f }) => /^index-.*\.js$/.test(f));
const entryCss = files.filter(({ f }) => /^index-.*\.css$/.test(f));
const lazy = files.filter(
  ({ f, p }) => f.endsWith(".js") && !/^index-/.test(f) && statSync(p).isFile(),
);

let failed = false;
const check = (label, list, budget) => {
  const kb = list.reduce((n, { p }) => n + gz(p), 0);
  const ok = kb <= budget;
  failed ||= !ok;
  console.log(`${ok ? "ok  " : "FAIL"} ${label.padEnd(14)} ${kb.toFixed(0).padStart(5)} KB gz  (budget ${budget})`);
};

check("entry (js)", entryJs, BUDGET_KB["entry (js)"]);
check("entry (css)", entryCss, BUDGET_KB["entry (css)"]);

for (const { f, p } of lazy) {
  const kb = gz(p);
  if (kb > LAZY_CAP_KB) {
    failed = true;
    console.log(`FAIL lazy chunk    ${kb.toFixed(0).padStart(5)} KB gz  ${f} (cap ${LAZY_CAP_KB})`);
  }
}

if (failed) {
  console.error(
    "\nBundle budget exceeded. Before raising it: is something heavy in the entry\n" +
      "graph that should be lazy? mermaid, cytoscape and recharts all belong behind\n" +
      "a dynamic import. See ADR 0003.",
  );
  process.exit(1);
}
console.log(`\n${lazy.length} lazy chunks, all under cap`);
