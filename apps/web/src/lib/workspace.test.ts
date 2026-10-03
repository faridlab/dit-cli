import { describe, expect, it } from "vitest";
import { scopedKey, workspaceBase, workspaceHref, workspaceName } from "./workspace";
import { resolveAttachmentSrc } from "./attachments";

describe("the workspace a page belongs to", () => {
  it("is named by /w/<name>/ and absent at the root", () => {
    expect(workspaceName("/w/acme/")).toBe("acme");
    expect(workspaceName("/w/acme")).toBe("acme");
    expect(workspaceName("/w/side-project/index.html")).toBe("side-project");
    expect(workspaceName("/")).toBeNull();
    expect(workspaceName("/wx/acme/")).toBeNull();
  });

  it("prefixes requests only on a server that serves several", () => {
    expect(workspaceBase("/w/acme/")).toBe("/w/acme");
    expect(workspaceBase("/")).toBe("");
    expect(workspaceHref("home")).toBe("/w/home/");
  });

  it("keeps what this browser remembers apart per workspace", () => {
    expect(scopedKey("dit.starred", "/w/acme/")).toBe("dit.starred@acme");
    expect(scopedKey("dit.starred", "/w/home/")).toBe("dit.starred@home");
    expect(scopedKey("dit.starred", "/")).toBe("dit.starred");
  });

  it("points an attachment at its own workspace", () => {
    expect(resolveAttachmentSrc("attachments/a-0a1b2c3d.png", "docs", "t", "/w/acme")).toBe(
      "/w/acme/api/attachments/docs/attachments/a-0a1b2c3d.png?token=t",
    );
  });
});

describe("a workspace name from what someone typed", () => {
  it("becomes the lowercase word the server accepts", async () => {
    const { toWorkspaceName } = await import("../components/Workspaces");
    expect(toWorkspaceName("Acme website")).toBe("acme-website");
    expect(toWorkspaceName("  Side  Project! ")).toBe("side-project");
    expect(toWorkspaceName("2026 plan")).toBe("ws-2026-plan");
    expect(toWorkspaceName("!!!")).toBe("");
  });
});
