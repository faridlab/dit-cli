import { describe, expect, it } from "vitest";
import { groupByStage, slugForTitle } from "./DocTemplateDialog";
import type { DocTemplateDto } from "../lib/types";

const t = (id: string, folder: string): DocTemplateDto => ({
  id,
  name: id,
  summary: "",
  folder,
  built_in: true,
  overridden: false,
});

describe("a page from a template", () => {
  it("takes its file name from the title the way the server does", () => {
    expect(slugForTitle("Checkout: the shopper's way")).toBe("checkout-the-shopper-s-way");
    expect(slugForTitle("  Q3 — Plan  ")).toBe("q3-plan");
    expect(slugForTitle("!!!")).toBe("");
  });

  it("lists the kinds by stage, in lifecycle order, with a workspace's own last", () => {
    const groups = groupByStage([
      t("release-notes", "changelogs"),
      t("prd", "docs/business"),
      t("runbook", "docs/ops"),
      t("fsd", "docs/requirements"),
    ]);
    expect(groups.map((g) => g.label)).toEqual(["Business", "Requirements", "Changes", "This workspace's own"]);
  });
});
