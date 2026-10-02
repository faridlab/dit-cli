import { describe, expect, it } from "vitest";
import { altFrom, dirOf, resolveAttachmentSrc, targetQuery, withAttachmentSrcs } from "./attachments";

describe("resolveAttachmentSrc", () => {
  it("turns a page's relative link into the attachments route, token in the query", () => {
    expect(resolveAttachmentSrc("attachments/guide-shot-0a1b2c3d.png", "docs", "t0k")).toBe(
      "/api/attachments/docs/attachments/guide-shot-0a1b2c3d.png?token=t0k",
    );
  });

  it("follows a comment's ../ up to its issue's attachments", () => {
    expect(
      resolveAttachmentSrc("../attachments/err-0a1b2c3d.png", "issues/2026/10/01M3-FKZK-x/comments", "t"),
    ).toBe("/api/attachments/issues/2026/10/01M3-FKZK-x/attachments/err-0a1b2c3d.png?token=t");
  });

  it.each([
    "https://example.com/a.png",
    "data:image/png;base64,AAAA",
    "/absolute.png",
    "javascript:alert(1)",
    "",
  ])("leaves %j as written — the CSP decides what loads", (src) => {
    expect(resolveAttachmentSrc(src, "docs", "t")).toBe(src);
  });

  it("leaves a relative link that is not an attachment, or climbs out of the workspace, alone", () => {
    expect(resolveAttachmentSrc("pictures/a.png", "docs", "t")).toBe("pictures/a.png");
    expect(resolveAttachmentSrc("../../../attachments/a.png", "docs", "t")).toBe("../../../attachments/a.png");
  });

  it("works without a token (the header-less fallback is the server's 401)", () => {
    expect(resolveAttachmentSrc("attachments/a-0a1b2c3d.png", "notes", null)).toBe(
      "/api/attachments/notes/attachments/a-0a1b2c3d.png",
    );
  });
});

describe("withAttachmentSrcs", () => {
  it("points a comment's relative picture at the attachments route and leaves the rest alone", () => {
    const html = '<p>see <img src="../attachments/e-0a1b2c3d.png" alt="error"> and <img src="https://x.test/a.png"></p>';
    const out = withAttachmentSrcs(html, "issues/2026/10/F/comments", "t");
    expect(out).toContain('src="/api/attachments/issues/2026/10/F/attachments/e-0a1b2c3d.png?token=t"');
    expect(out).toContain('src="https://x.test/a.png"');
    expect(out).toContain('alt="error"');
  });

  it("returns HTML without pictures untouched", () => {
    expect(withAttachmentSrcs("<p>plain</p>", "docs", "t")).toBe("<p>plain</p>");
  });
});

describe("helpers", () => {
  it("names the target in the query", () => {
    expect(targetQuery({ doc: "docs/a b.md" })).toBe("doc=docs%2Fa%20b.md");
    expect(targetQuery({ issue: "FKZKJCW" })).toBe("issue=FKZKJCW");
    expect(targetQuery({ comment: "FKZKJCW" })).toBe("comment=FKZKJCW");
  });

  it("takes alt text from a file's name, but not from a clipboard's anonymous one", () => {
    expect(altFrom("Screen Shot 2026-10-03.png")).toBe("Screen Shot 2026 10 03");
    expect(altFrom("login_error-dialog.jpg")).toBe("login error dialog");
    expect(altFrom("image.png")).toBe("");
    expect(altFrom("image 2.png")).toBe("");
  });

  it("finds the folder of a path", () => {
    expect(dirOf("docs/flows/auth.md")).toBe("docs/flows");
    expect(dirOf("readme.md")).toBe("");
  });
});
