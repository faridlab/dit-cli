// "Save to scenario": the moment a draft stops living in this browser and
// becomes a fence in a document — appended as a step to a scenario that
// exists, or as the first step of a new one. Either way it is one commit,
// through the same transaction a document save uses.

import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle, Save } from "lucide-react";
import type { MorseReportDto } from "../../lib/types";
import { Coded } from "./common";

export type SaveTarget =
  | { mode: "add"; scenario: string; stepId: string }
  | { mode: "new"; name: string; doc: string; stepId: string };

const STEP_ID = /^[A-Za-z][\w-]*$/;
const SCENARIO_NAME = /^[a-z][\w-]*$/;

export function SaveDialog({
  open,
  onOpenChange,
  report,
  specId,
  suggestedId,
  pending,
  serverError,
  onSave,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  report: MorseReportDto;
  specId: string;
  suggestedId: string;
  pending: boolean;
  serverError: string | null;
  onSave: (target: SaveTarget) => void;
}) {
  const sameSpec = report.scenarios.filter((s) => s.spec_id === specId && s.health !== "unreadable");
  const others = report.scenarios.filter((s) => s.spec_id !== specId && s.health !== "unreadable");
  const choices = [...sameSpec, ...others];
  const [mode, setMode] = useState<"add" | "new">(choices.length ? "add" : "new");
  const [scenario, setScenario] = useState(choices[0]?.scenario ?? "");
  const [name, setName] = useState(`${specId}-${suggestedId}`);
  const [doc, setDoc] = useState(`docs/api/${specId}.md`);
  const [stepId, setStepId] = useState(suggestedId);
  const [error, setError] = useState<string | null>(null);

  const submit = () => {
    const sid = stepId.trim();
    if (!STEP_ID.test(sid)) return setError("A step id is one word — letters, digits, `-` and `_`.");
    if (mode === "add") {
      const target = report.scenarios.find((s) => s.scenario === scenario);
      if (!target) return setError("Choose a scenario to add the step to.");
      if (target.steps.includes(sid)) {
        return setError(`\`${scenario}\` already has a step called \`${sid}\` — saving would replace it.`);
      }
      setError(null);
      onSave({ mode: "add", scenario, stepId: sid });
    } else {
      const n = name.trim();
      const d = doc.trim();
      if (!SCENARIO_NAME.test(n)) return setError("A scenario name is lower-case, one word — letters, digits, `-`.");
      if (report.scenarios.some((s) => s.scenario === n)) {
        return setError(`A scenario called \`${n}\` already exists. Names are unique across the workspace.`);
      }
      if (!/^docs\/.+\.md$/.test(d)) return setError("The document must be a `.md` file under `docs/`.");
      setError(null);
      onSave({ mode: "new", name: n, doc: d, stepId: sid });
    }
  };

  const shown = error ?? serverError;
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="mw-scrim" />
        <Dialog.Content className="mw-dlg" aria-describedby={undefined}>
          <Dialog.Title className="mw-dlg-h">Save to scenario</Dialog.Title>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              submit();
            }}
          >
            <div className="mw-dlg-b">
              <div className="mw-radio" role="radiogroup" aria-label="Where it goes">
                <label>
                  <input type="radio" name="mw-mode" checked={mode === "add"} disabled={!choices.length} onChange={() => setMode("add")} />
                  Add as a step to an existing scenario
                </label>
                <label>
                  <input type="radio" name="mw-mode" checked={mode === "new"} onChange={() => setMode("new")} />
                  New scenario
                </label>
              </div>
              {mode === "add" ? (
                <div className="mw-fld">
                  <label htmlFor="mw-scn">Scenario</label>
                  <select id="mw-scn" value={scenario} onChange={(e) => setScenario(e.target.value)}>
                    {choices.map((s) => (
                      <option key={s.scenario} value={s.scenario}>
                        {s.scenario} — {s.path}
                        {s.spec_id === specId ? "" : ` (spec ${s.spec_id})`}
                      </option>
                    ))}
                  </select>
                </div>
              ) : (
                <>
                  <div className="mw-fld">
                    <label htmlFor="mw-name">Scenario name</label>
                    <input id="mw-name" value={name} autoComplete="off" onChange={(e) => setName(e.target.value)} />
                  </div>
                  <div className="mw-fld">
                    <label htmlFor="mw-doc">Document it lives in (created if it does not exist)</label>
                    <input id="mw-doc" value={doc} autoComplete="off" onChange={(e) => setDoc(e.target.value)} />
                  </div>
                </>
              )}
              <div className="mw-fld">
                <label htmlFor="mw-step">Step id</label>
                <input id="mw-step" value={stepId} autoComplete="off" onChange={(e) => setStepId(e.target.value)} />
              </div>
              {shown ? (
                <div className="mw-errline">
                  <AlertTriangle className="i" aria-hidden />
                  <span>
                    <Coded text={shown} />
                  </span>
                </div>
              ) : null}
              <p className="mw-hint" style={{ margin: 0 }}>
                This writes the <code>dit-morse</code> fence and makes one commit, the same way a document save does.
                {mode === "new" ? " A new scenario is pinned where the spec stands now." : ""} Names the step reads that
                nothing provides are added to <code>requires:</code>; their values stay in your local file.
              </p>
            </div>
            <div className="mw-dlg-f">
              <Dialog.Close asChild>
                <button type="button" className="mw-btn">
                  Cancel
                </button>
              </Dialog.Close>
              <button type="submit" className="mw-btn pri" disabled={pending}>
                <Save className="i" aria-hidden />
                {pending ? "Committing…" : "Save & commit"}
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** An in-page confirmation — the browser's own `confirm()` is not used. */
export function ConfirmDialog({
  open,
  title,
  body,
  action,
  onConfirm,
  onOpenChange,
}: {
  open: boolean;
  title: string;
  body: string;
  action: string;
  onConfirm: () => void;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="mw-scrim" />
        <Dialog.Content className="mw-dlg" aria-describedby={undefined}>
          <Dialog.Title className="mw-dlg-h">{title}</Dialog.Title>
          <div className="mw-dlg-b">
            <p style={{ margin: 0 }}>{body}</p>
          </div>
          <div className="mw-dlg-f">
            <Dialog.Close asChild>
              <button type="button" className="mw-btn">
                Cancel
              </button>
            </Dialog.Close>
            <button
              type="button"
              className="mw-btn pri"
              onClick={() => {
                onConfirm();
                onOpenChange(false);
              }}
            >
              {action}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
