// Register an OpenAPI document as a Morse spec (ADR 0027) — the form's twin
// of a `specs:` entry in .dit/config.yaml. It offers the committed files
// that read as OpenAPI, so nobody types a path or edits YAML.

import { useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle } from "lucide-react";

import { getSpecCandidates } from "../../lib/api";
import { useMorseSpecs } from "../../lib/queries";

/** An id from a file name: `services/billing/openapi.yaml` → `billing`. */
export function specIdFor(path: string): string {
  const parts = path.toLowerCase().split("/");
  const file = (parts.pop() ?? "").replace(/\.(ya?ml|json)$/, "");
  const word = /^(openapi|swagger|api|spec)$/.test(file) ? (parts.pop() ?? file) : file;
  const id = word.replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return /^[a-z]/.test(id) ? id : `api-${id || "spec"}`;
}

export function RegisterSpecDialog({
  open,
  onOpenChange,
  onRegistered,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onRegistered: (id: string) => void;
}) {
  const [candidates, setCandidates] = useState<string[] | null>(null);
  const [path, setPath] = useState("");
  const [id, setId] = useState("");
  const [idTouched, setIdTouched] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const specs = useMorseSpecs();

  useEffect(() => {
    if (!open) return;
    getSpecCandidates()
      .then((found) => {
        setCandidates(found);
        if (found[0]) {
          setPath((p) => p || found[0] || "");
        }
      })
      .catch(() => setCandidates([]));
  }, [open]);
  useEffect(() => {
    if (!idTouched && path) setId(specIdFor(path));
  }, [path, idTouched]);

  const submit = () => {
    setProblem(null);
    specs.mutate(
      { kind: "register", input: { id: id.trim(), path: path.trim() } },
      {
        onSuccess: () => {
          onRegistered(id.trim());
          onOpenChange(false);
        },
        onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
      },
    );
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="mw-scrim" />
        <Dialog.Content className="mw-dlg" aria-describedby={undefined}>
          <Dialog.Title className="mw-dlg-h">Register an OpenAPI spec</Dialog.Title>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              submit();
            }}
          >
            <div className="mw-dlg-b">
              <div className="mw-fld">
                <label htmlFor="mw-spec-path">OpenAPI document in this repository</label>
                <input
                  id="mw-spec-path"
                  list="mw-spec-candidates"
                  value={path}
                  spellCheck={false}
                  placeholder="api/openapi.yaml"
                  onChange={(e) => setPath(e.target.value)}
                />
                <datalist id="mw-spec-candidates">
                  {(candidates ?? []).map((c) => (
                    <option key={c} value={c} />
                  ))}
                </datalist>
                <p className="mw-hint" style={{ margin: 0 }}>
                  {candidates === null
                    ? "Looking for OpenAPI documents in the last commit…"
                    : candidates.length
                      ? `${candidates.length} committed file${candidates.length === 1 ? "" : "s"} read as OpenAPI — pick one, or type a path.`
                      : "No committed file reads as OpenAPI yet — commit the document first, then register it."}{" "}
                  A spec is read from git, never fetched from an address.
                </p>
              </div>
              <div className="mw-fld">
                <label htmlFor="mw-spec-id">Spec id</label>
                <input
                  id="mw-spec-id"
                  value={id}
                  spellCheck={false}
                  onChange={(e) => {
                    setId(e.target.value);
                    setIdTouched(true);
                  }}
                />
                <p className="mw-hint" style={{ margin: 0 }}>
                  A step calls <code>{id || "<id>"}/&lt;operationId&gt;</code>.
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
              <button type="submit" className="mw-btn pri" disabled={!path.trim() || !id.trim() || specs.isPending}>
                Register
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
