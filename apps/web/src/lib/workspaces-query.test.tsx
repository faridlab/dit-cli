// A server started for one workspace answers `/api/workspaces` with 404. The
// `/` gate then serves the workspace, whose menu asks for the same list; if
// that second consumer refetched, the gate saw "loading" again, unmounted
// the menu, saw the error, mounted it — forever, one request per frame.

import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

const listWorkspaces = vi.fn();
vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  listWorkspaces: () => listWorkspaces(),
}));

const { ApiError } = await import("./api");
const { useWorkspaces } = await import("./queries");

let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  listWorkspaces.mockReset();
});

function Consumer({ label }: { label: string }) {
  const list = useWorkspaces();
  return <span data-label={label}>{list.isError ? "error" : list.isPending ? "pending" : "ok"}</span>;
}

describe("the workspace list on a server for one workspace", () => {
  it("is asked once; a consumer mounted after the 404 does not ask again", async () => {
    listWorkspaces.mockRejectedValue(new ApiError("no such API endpoint on this server", 404));
    const client = new QueryClient();
    const host = document.createElement("div");
    root = createRoot(host);
    const render = (both: boolean) =>
      act(() =>
        root?.render(
          <QueryClientProvider client={client}>
            <Consumer label="gate" />
            {both ? <Consumer label="menu" /> : null}
          </QueryClientProvider>,
        ),
      );

    render(false);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(host.textContent).toBe("error");

    render(true);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(host.textContent).toBe("errorerror");
    expect(listWorkspaces).toHaveBeenCalledTimes(1);
  });
});
