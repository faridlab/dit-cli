// Reproduces the crash the issue panel exposed: markdown → document is a
// round trip through WASM, so the editor can be destroyed before the parse
// lands (close the panel, or press J for the next issue). Touching a
// destroyed TipTap editor throws from inside its own `commands` getter —
// asynchronously, so it surfaced as an unhandled rejection in the console
// rather than as a visible failure.
//
// The bridge is mocked because what is under test is the lifetime, not the
// serializer: the test needs a parse it can resolve *after* the unmount.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { BridgeResult, PmDoc } from "./bridge";

// The throw lands as an unhandled rejection, which only the test runner's
// host sees. Declared here rather than pulling node's whole type surface
// into a browser app.
declare const process: {
  on(event: "unhandledRejection", listener: (reason: unknown) => void): void;
  off(event: "unhandledRejection", listener: (reason: unknown) => void): void;
};

const EMPTY_DOC: PmDoc = { type: "doc", content: [{ type: "paragraph" }] };

let resolveParse: ((result: BridgeResult<PmDoc>) => void) | null = null;

vi.mock("./bridge", () => ({
  markdownToDoc: vi.fn(
    () =>
      new Promise<BridgeResult<PmDoc>>((resolve) => {
        resolveParse = resolve;
      }),
  ),
  docToMarkdown: vi.fn(async () => ({ ok: true, value: "" }) as BridgeResult<string>),
}));

const { default: RichEditor } = await import("./RichEditor");

/** Long enough for TipTap's deferred destroy and for Node to report an
 *  unhandled rejection. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 120));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  // React 19 checks this flag to allow act() outside a test renderer.
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  resolveParse = null;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  container.remove();
});

describe("RichEditor", () => {
  it("ignores a parse that lands after the editor is gone", async () => {
    const failures: unknown[] = [];
    const onRejection = (reason: unknown) => failures.push(reason);
    process.on("unhandledRejection", onRejection);
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation((...args: unknown[]) => failures.push(args));

    await act(async () => {
      root.render(
        <RichEditor value="# first" onChange={() => undefined} onSave={() => undefined} />,
      );
    });
    // The initial parse resolves: the editor mounts with a document.
    await act(async () => {
      resolveParse?.({ ok: true, value: EMPTY_DOC });
    });

    // A new value starts a second parse…
    await act(async () => {
      root.render(
        <RichEditor value="# second" onChange={() => undefined} onSave={() => undefined} />,
      );
    });
    const pending = resolveParse;
    expect(pending).not.toBeNull();

    // …and the panel closes before it finishes. The destroy is deferred
    // inside TipTap, so the parse must land after it, not merely after
    // React's unmount.
    await act(async () => {
      root.unmount();
    });
    await act(settle);
    pending?.({ ok: true, value: EMPTY_DOC });
    await act(settle);

    process.off("unhandledRejection", onRejection);
    consoleError.mockRestore();
    expect(failures).toEqual([]);
  });

});
