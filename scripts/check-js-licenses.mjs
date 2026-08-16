#!/usr/bin/env node
// DESIGN.md §12.4 — the editor decision depends on staying inside MIT-compatible
// packages. `@blocknote/xl-*` is `GPL-3.0 OR PROPRIETARY`; pulling one in would
// silently relicense the project.
import { readFileSync, existsSync } from "node:fs";

const FORBIDDEN = [/^@blocknote\/xl-/];
const pkgPath = "apps/web/package.json";

if (!existsSync(pkgPath)) {
  console.log("no apps/web/package.json yet — skipping");
  process.exit(0);
}

const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
const deps = Object.keys({ ...pkg.dependencies, ...pkg.devDependencies });
const bad = deps.filter((d) => FORBIDDEN.some((re) => re.test(d)));

if (bad.length) {
  console.error(`Forbidden dependency (GPL/proprietary): ${bad.join(", ")}`);
  console.error("See DESIGN.md §12.4.");
  process.exit(1);
}
console.log(`${deps.length} JS dependencies checked, all clear`);
