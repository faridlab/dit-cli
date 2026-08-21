#!/usr/bin/env node
/**
 * Fails loudly when the editor's WASM bridge has not been built.
 *
 * `apps/web/src/editor/wasm/` is a gitignored build artifact (like `dist/`):
 * a fresh clone does not have it, and without this check the first failure
 * is a cryptic "Cannot find module './wasm/dit_wasm'" from tsc. Run by the
 * web `build` script and by vite.config.ts (for `npm run dev`).
 */
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const GLUE = fileURLToPath(
  new URL("../apps/web/src/editor/wasm/dit_wasm.js", import.meta.url),
);

if (!existsSync(GLUE)) {
  console.error(
    "The editor's WASM bridge is not built. Run:\n" +
      "\n" +
      "    just wasm-build\n" +
      "\n" +
      "(wasm-pack is required: cargo install wasm-pack --locked)",
  );
  process.exit(1);
}
