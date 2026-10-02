// Import into Morse (ADR 0027): a curl line, a Postman collection, or a
// Postman environment. Preview first — what would be written, and every
// address, credential and script that does not come along — then Import,
// which converts the same text again on the server and writes one commit.

import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle, FileUp, Info } from "lucide-react";

import { previewMorseImport } from "../../lib/api";
import { useMorseImport, useMorseImportEnv } from "../../lib/queries";
import type { MorseImportPreviewDto, MorseReportDto } from "../../lib/types";
import { cn } from "../../lib/cn";
import { Verb } from "./common";

type Kind = "curl" | "postman" | "env";

export function ImportDialog({
  open,
  onOpenChange,
  report,
  onImported,
  onEnvImported,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  report: MorseReportDto;
  /** The scenarios written, to open the first. */
  onImported: (scenarios: string[]) => void;
  onEnvImported: (name: string) => void;
}) {
  const [kind, setKind] = useState<Kind>("curl");
  const [text, setText] = useState("");
  const [scenario, setScenario] = useState("imported");
  const [spec, setSpec] = useState<string>(report.specs.length === 1 ? (report.specs[0]?.id ?? "") : "");
  const [doc, setDoc] = useState("docs/api/imported.md");
  const [preview, setPreview] = useState<MorseImportPreviewDto | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const doImport = useMorseImport();
  const doImportEnv = useMorseImportEnv();

  const input = () => ({
    kind: kind === "env" ? "postman" : kind,
    text,
    scenario: kind === "curl" ? scenario : undefined,
    spec: spec || undefined,
    doc,
  });
  const reset = (next: Kind) => {
    setKind(next);
    setPreview(null);
    setProblem(null);
  };
  const runPreview = () => {
    setBusy(true);
    setProblem(null);
    previewMorseImport(input())
      .then(setPreview)
      .catch((e: unknown) => {
        setPreview(null);
        setProblem(e instanceof Error ? e.message : String(e));
      })
      .finally(() => setBusy(false));
  };
  const runImport = () => {
    setProblem(null);
    if (kind === "env") {
      doImportEnv.mutate(text, {
        onSuccess: (done) => {
          onEnvImported(done.name);
          onOpenChange(false);
        },
        onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
      });
      return;
    }
    doImport.mutate(input(), {
      onSuccess: (done) => {
        onImported(done.scenarios);
        onOpenChange(false);
      },
      onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
    });
  };
  const readFile = (file: File | undefined) => {
    if (!file) return;
    void file.text().then((t) => {
      setText(t);
      setPreview(null);
    });
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="mw-scrim" />
        <Dialog.Content className="mw-dlg mw-import" aria-describedby={undefined}>
          <Dialog.Title className="mw-dlg-h">Import into Morse</Dialog.Title>
          <div className="mw-dlg-b">
            <div className="mw-radio" role="radiogroup" aria-label="What to import">
              {(
                [
                  ["curl", "curl command"],
                  ["postman", "Postman collection"],
                  ["env", "Postman environment"],
                ] as Array<[Kind, string]>
              ).map(([value, label]) => (
                <label key={value}>
                  <input type="radio" name="mw-import-kind" checked={kind === value} onChange={() => reset(value)} />
                  {label}
                </label>
              ))}
            </div>
            <div className="mw-fld">
              <label htmlFor="mw-import-text">
                {kind === "curl" ? "Paste the command" : "Paste the JSON, or choose the exported file"}
              </label>
              <textarea
                id="mw-import-text"
                className="mw-code"
                spellCheck={false}
                value={text}
                placeholder={kind === "curl" ? "curl -X POST https://api.example.com/v1/sessions -H 'Content-Type: application/json' -d '{…}'" : "{ \"info\": … }"}
                onChange={(e) => {
                  setText(e.target.value);
                  setPreview(null);
                }}
              />
              {kind !== "curl" ? (
                <label className="mw-filepick">
                  <FileUp className="i" aria-hidden />
                  Choose a file…
                  <input type="file" accept=".json,application/json" onChange={(e) => readFile(e.target.files?.[0])} />
                </label>
              ) : null}
            </div>
            {kind !== "env" ? (
              <div className="mw-import-opts">
                {kind === "curl" ? (
                  <div className="mw-fld">
                    <label htmlFor="mw-import-name">Scenario name</label>
                    <input id="mw-import-name" value={scenario} onChange={(e) => setScenario(e.target.value)} />
                  </div>
                ) : null}
                <div className="mw-fld">
                  <label htmlFor="mw-import-spec">Spec for a host it can't place</label>
                  <select id="mw-import-spec" value={spec} onChange={(e) => setSpec(e.target.value)}>
                    <option value="">— match by host only</option>
                    {report.specs.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.id}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="mw-fld">
                  <label htmlFor="mw-import-doc">Document it goes in</label>
                  <input id="mw-import-doc" value={doc} onChange={(e) => setDoc(e.target.value)} />
                </div>
              </div>
            ) : (
              <p className="mw-hint" style={{ margin: 0 }}>
                The environment's values go into <code>.dit/morse.local.yaml</code> on this machine — never into the
                repository — and this page never shows them. A URL variable such as <code>baseUrl</code> becomes its
                server; trusting that host is still <code>dit morse allow</code>.
              </p>
            )}

            {preview ? (
              <div className="mw-import-preview">
                {preview.scenarios.map((s) => (
                  <div key={s.name} className="mw-import-scn">
                    <b>{s.name}</b> <span className="mw-mono">on {s.spec}</span>
                    {s.requires.length ? <span className="mw-hint"> · requires {s.requires.join(", ")}</span> : null}
                    <ul>
                      {s.steps.map((st) => (
                        <li key={st.id}>
                          <Verb method={st.method || "GET"} wide /> <span className="mw-mono">{st.id}</span> →{" "}
                          <span className={cn("mw-mono", st.target.startsWith("request") && "mw-inline")}>{st.target}</span>
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
                {preview.notes.length ? (
                  <div className="mw-import-notes">
                    <Info className="i" aria-hidden />
                    <ul>
                      {preview.notes.map((n, i) => (
                        <li key={i}>{n}</li>
                      ))}
                    </ul>
                  </div>
                ) : null}
              </div>
            ) : null}
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
            {kind !== "env" ? (
              <button type="button" className="mw-btn" disabled={!text.trim() || busy} onClick={runPreview}>
                Preview
              </button>
            ) : null}
            <button
              type="button"
              className="mw-btn pri"
              disabled={!text.trim() || (kind !== "env" && !preview) || doImport.isPending || doImportEnv.isPending}
              onClick={runImport}
            >
              {kind === "env" ? "Import environment" : "Import"}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
