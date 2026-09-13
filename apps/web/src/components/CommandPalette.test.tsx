// The palette exists to be typed into, so where the caret lands when it
// opens is the whole feature. Radix focuses the dialog container by default;
// the palette prevents that to claim focus for the search box, and this pins
// that it actually does — without it the first keystroke goes nowhere and
// the box stays empty.
//
// The data hooks are stubbed: what is under test is the palette's own
// behavior, not the index behind it.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("../lib/queries", () => ({
  useIssues: () => ({ data: undefined, isFetching: false, isError: false, error: null }),
  useDocs: () => ({ data: [] }),
  useStatus: () => ({ data: undefined }),
  useSchema: () => ({ data: undefined }),
}));

vi.mock("../lib/theme", () => ({
  useTheme: () => ({ preference: "system", resolved: "light", setPreference: () => undefined }),
}));

// jsdom has no ResizeObserver; the dialog's positioning asks for one.
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= NoopResizeObserver as unknown as typeof ResizeObserver;
// …nor scrollIntoView, which cmdk calls to keep the selected row in view.
Element.prototype.scrollIntoView ??= function scrollIntoView() {};

const { CommandPalette } = await import("./CommandPalette");
const { ViewOptionsProvider } = await import("../lib/viewopts");

let container: HTMLDivElement;
let root: Root;

function render(open: boolean) {
  return act(async () => {
    root.render(
      <ViewOptionsProvider>
        <CommandPalette
          open={open}
          onOpenChange={() => undefined}
          onNavigate={() => undefined}
          onOpenIssue={() => undefined}
          onNewIssue={() => undefined}
          onOpenDoc={() => undefined}
          onToggleSidebar={() => undefined}
          sidebarHidden={false}
          onNotes={() => undefined}
          cli="dit ui"
        />
      </ViewOptionsProvider>,
    );
  });
}

const searchBox = () => document.querySelector<HTMLInputElement>("input[cmdk-input]");

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});

describe("CommandPalette", () => {
  it("puts the caret in the search box when it opens", async () => {
    await render(true);

    const input = searchBox();
    expect(input).not.toBeNull();
    expect(document.activeElement).toBe(input);
  });

  it("is not in the page at all while closed", async () => {
    await render(false);
    expect(searchBox()).toBeNull();
  });
});
