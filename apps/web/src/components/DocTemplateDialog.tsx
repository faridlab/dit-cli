// New page from a template (ADR 0031): pick the kind of document, name it,
// and DIT places it by its stage — business, requirements, technical,
// decisions, testing, changes — with a prompt under every heading.

import { useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle } from "lucide-react";

import { useDocFromTemplate, useDocTemplates } from "../lib/queries";
import type { DocTemplateDto } from "../lib/types";
import { cn } from "../lib/cn";

/** The stages the built-in kinds are placed by, in lifecycle order. */
const STAGES: [folder: string, label: string][] = [
  ["docs/business", "Business"],
  ["docs/requirements", "Requirements"],
  ["docs/technical", "Technical"],
  ["docs/adr", "Decisions"],
  ["docs/testing", "Testing"],
  ["changelogs", "Changes"],
];

/** A page's file name from its title — the rule the server applies. */
export function slugForTitle(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80)
    .replace(/-+$/g, "");
}

/** Templates grouped by stage; a workspace's own kinds under their folder. */
export function groupByStage(templates: DocTemplateDto[]): { label: string; items: DocTemplateDto[] }[] {
  const groups: { label: string; items: DocTemplateDto[] }[] = [];
  for (const [folder, label] of STAGES) {
    const items = templates.filter((t) => t.folder === folder);
    if (items.length > 0) groups.push({ label, items });
  }
  const others = templates.filter((t) => !STAGES.some(([folder]) => folder === t.folder));
  if (others.length > 0) groups.push({ label: "This workspace's own", items: others });
  return groups;
}

export function DocTemplateDialog({
  open,
  onOpenChange,
  onMade,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The page that landed: reveal and open it. */
  onMade: (path: string) => void;
}) {
  const templates = useDocTemplates(open);
  const make = useDocFromTemplate();
  const [kind, setKind] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setTitle("");
    setProblem(null);
  }, [open]);
  useEffect(() => {
    if (kind === null && templates.data?.[0]) setKind(templates.data[0].id);
  }, [kind, templates.data]);

  const chosen = templates.data?.find((t) => t.id === kind) ?? null;
  const slug = slugForTitle(title);
  const submit = () => {
    if (!chosen || !slug) return;
    setProblem(null);
    make.mutate(
      { kind: chosen.id, title: title.trim() },
      {
        onSuccess: (made) => {
          onOpenChange(false);
          onMade(made.path);
        },
        onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
      },
    );
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="mw-scrim" />
        <Dialog.Content className="mw-dlg tpl-dlg" aria-describedby={undefined}>
          <Dialog.Title className="mw-dlg-h">New page from a template</Dialog.Title>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              submit();
            }}
          >
            <div className="mw-dlg-b">
              <div className="tpl-list" role="radiogroup" aria-label="Kind of document">
                {templates.isPending ? <p className="mw-hint">Loading the templates…</p> : null}
                {groupByStage(templates.data ?? []).map((group) => (
                  <div key={group.label} className="tpl-group">
                    <div className="tpl-stage">{group.label}</div>
                    {group.items.map((t) => (
                      <button
                        key={t.id}
                        type="button"
                        role="radio"
                        aria-checked={t.id === kind}
                        className={cn("tpl-opt", t.id === kind && "on")}
                        onClick={() => setKind(t.id)}
                      >
                        <span className="tpl-name">
                          {t.name}
                          {t.overridden ? <span className="tpl-tag">this workspace's version</span> : null}
                        </span>
                        <span className="tpl-sum">{t.summary}</span>
                      </button>
                    ))}
                  </div>
                ))}
              </div>
              <div className="mw-fld">
                <label htmlFor="tpl-title">Title</label>
                <input
                  id="tpl-title"
                  value={title}
                  spellCheck={false}
                  placeholder="Checkout v2"
                  onChange={(e) => setTitle(e.target.value)}
                />
                <p className="mw-hint" style={{ margin: 0 }}>
                  {chosen && slug ? (
                    <>
                      Made as <code>{`${chosen.folder}/${slug}.md`}</code>, with a prompt under every heading to replace.
                    </>
                  ) : (
                    <>The file name comes from the title.</>
                  )}
                </p>
              </div>
              {problem ? (
                <div className="mw-errline">
                  <AlertTriangle className="i" aria-hidden />
                  <span>{problem}</span>
                </div>
              ) : null}
            </div>
            <div className="mw-dlg-f">
              <Dialog.Close asChild>
                <button type="button" className="mw-btn">
                  Cancel
                </button>
              </Dialog.Close>
              <button type="submit" className="mw-btn pri" disabled={!chosen || !slug || make.isPending}>
                Create page
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
