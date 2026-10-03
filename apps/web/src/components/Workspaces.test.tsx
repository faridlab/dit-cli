// Choosing a folder for a workspace (ADR 0030). Found in use: picking a
// folder that already was a DIT workspace in "New workspace" offered to make
// a second one inside it (`serpa-dit/serpa-dit`). The dialog's answer says
// whether the folder is a workspace, and the forms act on it.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

const chooseFolder = vi.fn();
const addWorkspace = vi.fn();
const createWorkspace = vi.fn();
vi.mock("../lib/api", async (original) => ({
  ...(await original<typeof import("../lib/api")>()),
  chooseFolder: (purpose: string) => chooseFolder(purpose),
  addWorkspace: (path: string) => addWorkspace(path),
  createWorkspace: (name: string, at?: string) => createWorkspace(name, at),
}));

const { FirstRun } = await import("./Workspaces");

let root: Root;
let host: HTMLDivElement;
const assign = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  vi.stubGlobal("location", { ...window.location, assign, pathname: "/" });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

const button = (text: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;

async function settle() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

function type(input: HTMLInputElement, value: string) {
  act(() => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    setter?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("New workspace in a chosen folder", () => {
  it("offers to add a folder that already is a workspace, and never makes one inside it", async () => {
    chooseFolder.mockResolvedValue({ path: "/Users/me/apps/serpa-dit", is_workspace: true });
    addWorkspace.mockResolvedValue({ name: "serpa-dit" });
    act(() => root.render(<FirstRun root="/Users/me/Documents/DIT" canChoose />));
    type(host.querySelector("#ws-new-name") as HTMLInputElement, "Serpa DIT");

    act(() => button("Choose folder")?.click());
    await settle();

    expect(host.textContent).toContain("already is a DIT workspace");
    expect(host.textContent).not.toContain("serpa-dit/serpa-dit");
    expect(button("Create workspace")?.disabled).toBe(true);

    act(() => button("Add this workspace")?.click());
    await settle();
    expect(addWorkspace).toHaveBeenCalledWith("/Users/me/apps/serpa-dit");
    expect(createWorkspace).not.toHaveBeenCalled();
    expect(assign).toHaveBeenCalledWith("/w/serpa-dit/");
  });

  it("makes the workspace inside an ordinary chosen folder, and can go back to the default", async () => {
    chooseFolder.mockResolvedValue({ path: "/Users/me/Clients", is_workspace: false });
    act(() => root.render(<FirstRun root="/Users/me/Documents/DIT" canChoose />));
    type(host.querySelector("#ws-new-name") as HTMLInputElement, "Globex");
    act(() => button("Choose folder")?.click());
    await settle();
    expect(host.textContent).toContain("/Users/me/Clients/globex");
    expect(button("Create workspace")?.disabled).toBe(false);

    act(() => button("Use the default folder")?.click());
    expect(host.textContent).toContain("/Users/me/Documents/DIT/globex");
  });
});

describe("Add an existing workspace", () => {
  it("says at once when the chosen folder is not a workspace", async () => {
    chooseFolder.mockResolvedValue({ path: "/Users/me/code/plain-repo", is_workspace: false });
    act(() => root.render(<FirstRun root="/Users/me/Documents/DIT" canChoose />));
    act(() => button("Add an existing one instead")?.click());
    act(() => button("Choose folder")?.click());
    await settle();
    expect((host.querySelector("#ws-add-path") as HTMLInputElement).value).toBe("/Users/me/code/plain-repo");
    expect(host.textContent).toContain("This folder is not a DIT workspace");
    expect(button("Add workspace")?.disabled).toBe(true);
  });
});
