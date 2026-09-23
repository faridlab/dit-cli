// One request: an operation drafted in a tab, or a step of a scenario.
//
// The same editor serves both, and they differ only in where an edit goes.
// An operation's draft lives in this browser until "Save to scenario" writes
// it into a fence; a step's edit is written back to its fence after a pause,
// one commit per pause, like a document (ADR 0023).
//
// What the response panel shows is what crosses the boundary: status, time,
// size, whether each check held, and the *names* of what was captured. The
// body and the captured values stay with the server (§20.7), and the panel
// hands over the terminal command that prints them.

import { useEffect, useRef } from "react";
import {
  AlertTriangle,
  ChevronRight,
  CircleCheck,
  CircleX,
  Code2,
  Copy,
  GitCommitHorizontal,
  Layers,
  Lock,
  Play,
  Plus,
  Save,
  ShieldAlert,
  X,
} from "lucide-react";
import { cn } from "../../lib/cn";
import {
  bodyError,
  isSelector,
  pathSegments,
  secretHeaders,
  stepPreview,
} from "../../lib/morse";
import type { MorseEnvDto, MorseOperationDto, MorsePairDto, MorseRunDto, MorseSpecDto, MorseStepDto } from "../../lib/types";
import { allowCommand, Banner, Coded, copyText, CopyCmd } from "./common";

export type RunState =
  | { state: "pending" }
  | { state: "done"; run: MorseRunDto }
  | { state: "error"; message: string };

export type SaveState = { state: "saved" | "saving" | "editing" } | { state: "error"; message: string };

type Sub = "docs" | "params" | "headers" | "body" | "expect" | "capture";

const SUBS: { id: Sub; label: string }[] = [
  { id: "docs", label: "Docs" },
  { id: "params", label: "Params" },
  { id: "headers", label: "Headers" },
  { id: "body", label: "Body" },
  { id: "expect", label: "Expect" },
  { id: "capture", label: "Capture" },
];

export function RequestTab({
  kind,
  draft,
  onChange,
  sub,
  onSub,
  spec,
  op,
  env,
  envName,
  result,
  onSend,
  onSave,
  save,
  scenario,
  capturedEarlier,
  editable,
  fence,
  showFence,
  onToggleFence,
}: {
  kind: "op" | "step";
  draft: MorseStepDto;
  onChange: (next: MorseStepDto) => void;
  sub: Sub;
  onSub: (sub: Sub) => void;
  spec: MorseSpecDto | undefined;
  op: MorseOperationDto | undefined;
  env: MorseEnvDto | null;
  envName: string | null;
  result: RunState | undefined;
  onSend: () => void;
  onSave?: () => void;
  save?: SaveState;
  scenario?: { name: string; doc: string };
  /** Names an earlier step of the scenario captures — not there when one
   *  step is sent on its own. */
  capturedEarlier: string[];
  editable: boolean;
  fence: string;
  showFence: boolean;
  onToggleFence: () => void;
}) {
  const secret = secretHeaders(draft);
  const sendRef = useRef(onSend);
  sendRef.current = onSend;
  const saveRef = useRef(onSave);
  saveRef.current = onSave;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key === "Enter") {
        e.preventDefault();
        sendRef.current();
      } else if (mod && e.key.toLowerCase() === "s" && saveRef.current) {
        e.preventDefault();
        saveRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!op || !spec) {
    return (
      <div className="mw-page">
        <Banner tone="crit" icon={<AlertTriangle className="i" aria-hidden />}>
          <code>{draft.operation ?? draft.request}</code> is not an operation the spec describes at HEAD, so there is
          nothing to send. {scenario ? <>Point the step at one that exists in its fence, in {scenario.doc}.</> : null}
        </Banner>
      </div>
    );
  }

  const base = env?.server ?? (spec.servers[0]?.url.includes("://") ? spec.servers[0].url : null);
  const counts: Partial<Record<Sub, number>> = {
    params: draft.params.length + draft.query.filter((q) => q.key).length,
    headers: draft.headers.filter((h) => h.key).length,
    expect: (draft.status ? 1 : 0) + draft.checks.filter((c) => c.path).length,
    capture: draft.capture.filter((c) => c.name).length,
  };
  const setPairs = (field: "params" | "query" | "headers", pairs: MorsePairDto[]) =>
    onChange({ ...draft, [field]: pairs });

  return (
    <div className="mw-req">
      <div className="mw-req-h">
        <div className="mw-crumb">
          <Layers className="i" aria-hidden />
          <span>{spec.id}</span>
          <ChevronRight className="i" aria-hidden />
          <span>{op.tag ?? "untagged"}</span>
          <ChevronRight className="i" aria-hidden />
          <b>{op.summary ?? op.operation_id}</b>
          {scenario ? (
            <span className="mw-chip">
              step&nbsp;<b>{draft.id}</b>&nbsp;of {scenario.name}
            </span>
          ) : null}
        </div>
        <span className="mw-sp" />
        {kind === "step" ? <SavedNote save={save} editable={editable} doc={scenario?.doc ?? ""} /> : null}
        {kind === "op" ? (
          <button
            type="button"
            className="mw-btn"
            disabled={secret.length > 0}
            title={secret.length ? "A credential literal can't be committed — use {{token}}" : "Add this request to a scenario (⌘S)"}
            onClick={onSave}
          >
            <Save className="i" aria-hidden />
            Save to scenario
          </button>
        ) : null}
        <button
          type="button"
          className={cn("mw-btn", showFence && "on")}
          aria-pressed={showFence}
          title="Show the dit-morse fence this request is written in"
          onClick={onToggleFence}
        >
          <Code2 className="i" aria-hidden />
          Fence
        </button>
      </div>

      <div className="mw-urlrow">
        <div className="mw-url">
          <span className={cn("m", `mw-v-${op.method}`)}>{op.method}</span>
          <span
            className={cn("base", !base && "bad")}
            title={
              base
                ? env?.server
                  ? `server: from environment ${envName}`
                  : "from the spec's servers:"
                : "The spec's servers: names no host — choose an environment with a server:"
            }
          >
            {base ?? "no server"}
          </span>
          <span className="path">
            {pathSegments(op.path, draft.params).map((seg, i) => (
              <span key={i} className={seg.kind === "param" ? "mw-pp" : seg.kind === "filled" ? "mw-vv" : undefined}>
                {seg.text}
              </span>
            ))}
            {draft.query.some((q) => q.key && q.value) ? (
              <>
                ?
                {draft.query
                  .filter((q) => q.key && q.value)
                  .map((q, i) => (
                    <span key={i}>
                      {i ? "&" : ""}
                      {q.key}=<span className="mw-vv">{q.value}</span>
                    </span>
                  ))}
              </>
            ) : null}
          </span>
        </div>
        <button
          type="button"
          className="mw-btn pri mw-send"
          onClick={onSend}
          disabled={result?.state === "pending"}
          title="Send (⌘Enter)"
        >
          <Play className="i" aria-hidden />
          Send
        </button>
      </div>

      <div className="mw-subtabs" role="tablist">
        {SUBS.map((s) => (
          <button
            key={s.id}
            type="button"
            role="tab"
            aria-selected={sub === s.id}
            className={cn(sub === s.id && "on")}
            onClick={() => onSub(s.id)}
          >
            {s.label}
            {counts[s.id] ? <span className="c">{counts[s.id]}</span> : null}
            {s.id === "body" && draft.body?.trim() ? <span className="g" /> : null}
          </button>
        ))}
      </div>

      <div className={cn("mw-split", showFence && "withfence")}>
        <div className="mw-editor">
          {sub === "docs" ? <DocsPane spec={spec} op={op} /> : null}
          {sub === "params" ? (
            <ParamsPane
              draft={draft}
              op={op}
              onParams={(p) => setPairs("params", p)}
              onQuery={(p) => setPairs("query", p)}
            />
          ) : null}
          {sub === "headers" ? (
            <HeadersPane draft={draft} secret={secret} onHeaders={(p) => setPairs("headers", p)} />
          ) : null}
          {sub === "body" ? <BodyPane draft={draft} op={op} onBody={(body) => onChange({ ...draft, body })} /> : null}
          {sub === "expect" ? <ExpectPane draft={draft} op={op} onChange={onChange} /> : null}
          {sub === "capture" ? <CapturePane draft={draft} onChange={onChange} /> : null}
        </div>
        {showFence ? (
          <div className="mw-fence">
            <div className="mw-fence-h">
              <Code2 className="i" aria-hidden />
              <span>
                {kind === "step" ? `the fence in ${scenario?.doc ?? ""}` : "the step Save adds to a fence (preview)"}
              </span>
              <span className="mw-sp" />
              <button type="button" className="mw-ib" title="Copy" onClick={() => copyText(kind === "step" ? fence : stepPreview(draft), "Fence copied")}>
                <Copy className="i" aria-hidden />
              </button>
            </div>
            <pre>{kind === "step" ? fence : stepPreview(draft)}</pre>
          </div>
        ) : null}
        <ResponsePanel
          result={result}
          draft={draft}
          base={base}
          capturedEarlier={capturedEarlier}
          cli={
            kind === "step" && scenario
              ? `dit morse run ${scenario.name}${envName ? ` --env ${envName}` : ""}`
              : cliFor(draft, envName)
          }
        />
      </div>
    </div>
  );
}

function cliFor(d: MorseStepDto, envName: string | null): string {
  const quote = (s: string) => (/^[\w./:@{}-]*$/.test(s) ? s : `'${s.replace(/'/g, "'\\''")}'`);
  const parts = [`dit morse send ${d.operation ?? ""}`];
  if (envName) parts.push(`--env ${envName}`);
  for (const p of d.params.filter((p) => p.key && p.value)) parts.push(`--param ${quote(`${p.key}=${p.value}`)}`);
  for (const p of d.query.filter((p) => p.key && p.value)) parts.push(`--query ${quote(`${p.key}=${p.value}`)}`);
  for (const h of d.headers.filter((h) => h.key)) parts.push(`--header ${quote(`${h.key}: ${h.value}`)}`);
  if (d.body?.trim()) parts.push(`--body ${quote(d.body.replace(/\s*\n\s*/g, " "))}`);
  return parts.join(" ");
}

function SavedNote({ save, editable, doc }: { save: SaveState | undefined; editable: boolean; doc: string }) {
  if (!editable) {
    return (
      <span className="mw-saved bad" title="A form would drop the fence's comments">
        <AlertTriangle className="i" aria-hidden />
        read-only here — the fence has a comment; edit it in {doc}
      </span>
    );
  }
  if (!save || save.state === "saved") {
    return (
      <span className="mw-saved">
        <GitCommitHorizontal className="i" aria-hidden />
        in sync with the fence
      </span>
    );
  }
  if (save.state === "error") {
    return (
      <span className="mw-saved bad">
        <AlertTriangle className="i" aria-hidden />
        not saved — {save.message}
      </span>
    );
  }
  return (
    <span className="mw-saved">
      <GitCommitHorizontal className="i" aria-hidden />
      {save.state === "saving" ? "committing…" : "editing…"}
    </span>
  );
}

function DocsPane({ spec, op }: { spec: MorseSpecDto; op: MorseOperationDto }) {
  return (
    <div className="mw-docs">
      <h3>{op.summary ?? op.operation_id}</h3>
      <dl>
        <dt>operationId</dt>
        <dd className="mw-mono">
          {spec.id}/{op.operation_id}
        </dd>
        <dt>method · path</dt>
        <dd className="mw-mono">
          {op.method} {op.path}
        </dd>
        <dt>tag</dt>
        <dd>{op.tag ?? "—"}</dd>
        <dt>parameters</dt>
        <dd>
          {op.params.length
            ? op.params.map((p) => (
                <span key={`${p.location}:${p.name}`} className="mw-chip mono">
                  {p.name} · {p.location}
                  {p.required ? " *" : ""}
                </span>
              ))
            : "none"}
        </dd>
        <dt>request body</dt>
        <dd>
          {op.body.length
            ? op.body.map((f) => (
                <span key={f.name} className="mw-chip mono">
                  {f.name}: {f.kind}
                  {f.required ? " *" : ""}
                </span>
              ))
            : "none"}
        </dd>
        <dt>responses</dt>
        <dd>
          {op.responses.map((r) => (
            <span key={r} className="mw-chip mono">
              {r}
            </span>
          ))}
        </dd>
        <dt>from</dt>
        <dd className="mw-mono">
          {spec.repo ? `${spec.repo}:` : ""}
          {spec.path} @ {spec.head?.slice(0, 7) ?? "—"}
        </dd>
      </dl>
      <p className="mw-hint">
        Read from the spec at HEAD. Nothing about this operation is copied into a DIT file. If the spec renames it,
        every scenario that uses it is reported <b>broken</b> by name.
      </p>
    </div>
  );
}

function PairTable({
  head,
  rows,
  onRows,
  placeholderKey,
  placeholderValue,
  errRows = [],
  addLabel,
}: {
  head: string;
  rows: MorsePairDto[];
  onRows: (rows: MorsePairDto[]) => void;
  placeholderKey: string;
  placeholderValue: string;
  errRows?: number[];
  addLabel: string;
}) {
  const set = (i: number, patch: Partial<MorsePairDto>) =>
    onRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <table className="mw-kv">
      <thead>
        <tr>
          <th style={{ width: "34%" }}>{head}</th>
          <th>Value</th>
          <th className="rm" />
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i} className={cn(errRows.includes(i) && "err")}>
            <td>
              <input
                aria-label={`${head} ${i + 1} name`}
                value={r.key}
                placeholder={placeholderKey}
                onChange={(e) => set(i, { key: e.target.value })}
              />
            </td>
            <td>
              <input
                aria-label={`${head} ${i + 1} value`}
                value={r.value}
                placeholder={placeholderValue}
                onChange={(e) => set(i, { value: e.target.value })}
              />
            </td>
            <td className="rm">
              <button type="button" className="mw-ib" title="Remove" onClick={() => onRows(rows.filter((_, j) => j !== i))}>
                <X className="i" aria-hidden />
              </button>
            </td>
          </tr>
        ))}
        <tr>
          <td colSpan={3}>
            <button type="button" className="mw-btn sm" style={{ margin: 6 }} onClick={() => onRows([...rows, { key: "", value: "" }])}>
              <Plus className="i" aria-hidden />
              {addLabel}
            </button>
          </td>
        </tr>
      </tbody>
    </table>
  );
}

function ParamsPane({
  draft,
  op,
  onParams,
  onQuery,
}: {
  draft: MorseStepDto;
  op: MorseOperationDto;
  onParams: (p: MorsePairDto[]) => void;
  onQuery: (p: MorsePairDto[]) => void;
}) {
  return (
    <>
      {draft.params.length ? (
        <table className="mw-kv" style={{ marginBottom: 12 }}>
          <thead>
            <tr>
              <th style={{ width: "28%" }}>Path parameter</th>
              <th>Value</th>
              <th style={{ width: "26%" }}>Description</th>
            </tr>
          </thead>
          <tbody>
            {draft.params.map((p, i) => (
              <tr key={p.key}>
                <td className="ro">
                  {p.key}
                  <span className="reqd">*</span>
                </td>
                <td>
                  <input
                    aria-label={`path parameter ${p.key}`}
                    value={p.value}
                    placeholder={`{{${p.key}}} or a literal`}
                    onChange={(e) => onParams(draft.params.map((q, j) => (j === i ? { ...q, value: e.target.value } : q)))}
                  />
                </td>
                <td className="note">in path · required</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
      <PairTable
        head="Query"
        rows={draft.query}
        onRows={onQuery}
        placeholderKey="key"
        placeholderValue="value or {{name}}"
        addLabel="Add query parameter"
      />
      <p className="mw-hint">
        {draft.params.length
          ? "Each value goes into the path percent-encoded, so a captured ../admin stays one segment. An empty path parameter is refused before anything is sent."
          : `${op.method} ${op.path} takes no path parameters.`}
      </p>
    </>
  );
}

function HeadersPane({
  draft,
  secret,
  onHeaders,
}: {
  draft: MorseStepDto;
  secret: number[];
  onHeaders: (p: MorsePairDto[]) => void;
}) {
  const hasAuth = draft.headers.some((h) => h.key.toLowerCase() === "authorization");
  return (
    <>
      <PairTable
        head="Header"
        rows={draft.headers}
        onRows={onHeaders}
        placeholderKey="Header"
        placeholderValue="value or {{name}}"
        errRows={secret}
        addLabel="Add header"
      />
      {!hasAuth ? (
        <button
          type="button"
          className="mw-btn sm"
          style={{ marginTop: 8 }}
          onClick={() => onHeaders([...draft.headers, { key: "Authorization", value: "Bearer {{token}}" }])}
        >
          <Lock className="i" aria-hidden />
          Add Authorization: Bearer {"{{token}}"}
        </button>
      ) : null}
      {secret.length ? (
        <div className="mw-errline">
          <ShieldAlert className="i" aria-hidden />
          <span>
            This looks like a real credential written out. It would stay in git history for good, so Save is blocked.
            Put the value in <code>.dit/morse.local.yaml</code> and write <code>Bearer {"{{token}}"}</code>. You can
            still Send.
          </span>
        </div>
      ) : (
        <p className="mw-hint">
          Use <code>{"{{name}}"}</code> for anything secret. The environment or an earlier step fills it in; the value
          is never written into the repo.
        </p>
      )}
    </>
  );
}

function BodyPane({
  draft,
  op,
  onBody,
}: {
  draft: MorseStepDto;
  op: MorseOperationDto;
  onBody: (body: string | null) => void;
}) {
  if (!op.body.length && !draft.body) {
    return (
      <div className="mw-empty" style={{ height: "auto", padding: 30 }}>
        The spec describes no request body for this operation.
        <button type="button" className="mw-btn sm" onClick={() => onBody("{\n  \n}")}>
          <Plus className="i" aria-hidden />
          Add a JSON body anyway
        </button>
      </div>
    );
  }
  const err = bodyError(draft.body);
  return (
    <>
      <textarea
        className="mw-code"
        aria-label="Request body"
        spellCheck={false}
        value={draft.body ?? ""}
        onChange={(e) => onBody(e.target.value)}
      />
      {err ? (
        <div className="mw-errline">
          <AlertTriangle className="i" aria-hidden />
          <span>Not valid JSON: {err}</span>
        </div>
      ) : (
        <p className="mw-hint">
          Pre-filled with the schema's required fields. <code>{"{{name}}"}</code> can stand in for any value; a
          value written entirely as one reference takes the shape of what it holds.
        </p>
      )}
    </>
  );
}

function ExpectPane({
  draft,
  op,
  onChange,
}: {
  draft: MorseStepDto;
  op: MorseOperationDto;
  onChange: (d: MorseStepDto) => void;
}) {
  const setCheck = (i: number, patch: Partial<MorseStepDto["checks"][number]>) =>
    onChange({ ...draft, checks: draft.checks.map((c, j) => (j === i ? { ...c, ...patch } : c)) });
  return (
    <>
      <table className="mw-kv">
        <tbody>
          <tr>
            <td className="ro" style={{ width: "28%" }}>
              status
            </td>
            <td>
              <input
                aria-label="Expected status"
                inputMode="numeric"
                value={draft.status ?? ""}
                placeholder="any"
                onChange={(e) => {
                  const n = Number(e.target.value.trim());
                  onChange({ ...draft, status: e.target.value.trim() && Number.isInteger(n) ? n : null });
                }}
              />
            </td>
            <td className="note" style={{ width: "30%" }}>
              spec lists {op.responses.join(", ") || "none"}
            </td>
          </tr>
        </tbody>
      </table>
      <table className="mw-kv" style={{ marginTop: 12 }}>
        <thead>
          <tr>
            <th style={{ width: "36%" }}>JSONPath</th>
            <th style={{ width: "20%" }}>Rule</th>
            <th>Value</th>
            <th className="rm" />
          </tr>
        </thead>
        <tbody>
          {draft.checks.map((c, i) => (
            <tr key={i}>
              <td>
                <input aria-label={`check ${i + 1} path`} value={c.path} placeholder="$.data.id" onChange={(e) => setCheck(i, { path: e.target.value })} />
              </td>
              <td>
                <select aria-label={`check ${i + 1} rule`} value={c.rule} onChange={(e) => setCheck(i, { rule: e.target.value })}>
                  <option value="exists">exists</option>
                  <option value="equals">equals</option>
                </select>
              </td>
              <td>
                <input
                  aria-label={`check ${i + 1} value`}
                  value={c.value}
                  disabled={c.rule === "exists"}
                  placeholder={c.rule === "exists" ? "—" : "literal or {{name}}"}
                  onChange={(e) => setCheck(i, { value: e.target.value })}
                />
              </td>
              <td className="rm">
                <button
                  type="button"
                  className="mw-ib"
                  title="Remove"
                  onClick={() => onChange({ ...draft, checks: draft.checks.filter((_, j) => j !== i) })}
                >
                  <X className="i" aria-hidden />
                </button>
              </td>
            </tr>
          ))}
          <tr>
            <td colSpan={4}>
              <button
                type="button"
                className="mw-btn sm"
                style={{ margin: 6 }}
                onClick={() => onChange({ ...draft, checks: [...draft.checks, { path: "", rule: "exists", value: "" }] })}
              >
                <Plus className="i" aria-hidden />
                Add check
              </button>
            </td>
          </tr>
        </tbody>
      </table>
      <p className="mw-hint">
        This is where Postman has test scripts. A check compares with a literal or a bound name, and that is all —
        there is no expression language, because a pulled file that runs code is code execution by pull request.
      </p>
    </>
  );
}

function CapturePane({ draft, onChange }: { draft: MorseStepDto; onChange: (d: MorseStepDto) => void }) {
  const set = (i: number, patch: Partial<MorseStepDto["capture"][number]>) =>
    onChange({ ...draft, capture: draft.capture.map((c, j) => (j === i ? { ...c, ...patch } : c)) });
  const bad = draft.capture.some((c) => c.name && c.from && !isSelector(c.from));
  return (
    <>
      <table className="mw-kv">
        <thead>
          <tr>
            <th style={{ width: "30%" }}>Name</th>
            <th>From</th>
            <th className="rm" />
          </tr>
        </thead>
        <tbody>
          {draft.capture.map((c, i) => (
            <tr key={i} className={cn(c.from && !isSelector(c.from) && "err")}>
              <td>
                <input aria-label={`capture ${i + 1} name`} value={c.name} placeholder="party_id" onChange={(e) => set(i, { name: e.target.value })} />
              </td>
              <td>
                <input
                  aria-label={`capture ${i + 1} source`}
                  value={c.from}
                  placeholder="$.data.id · header:Location · status"
                  onChange={(e) => set(i, { from: e.target.value })}
                />
              </td>
              <td className="rm">
                <button
                  type="button"
                  className="mw-ib"
                  title="Remove"
                  onClick={() => onChange({ ...draft, capture: draft.capture.filter((_, j) => j !== i) })}
                >
                  <X className="i" aria-hidden />
                </button>
              </td>
            </tr>
          ))}
          <tr>
            <td colSpan={3}>
              <button
                type="button"
                className="mw-btn sm"
                style={{ margin: 6 }}
                onClick={() => onChange({ ...draft, capture: [...draft.capture, { name: "", from: "" }] })}
              >
                <Plus className="i" aria-hidden />
                Add capture
              </button>
            </td>
          </tr>
        </tbody>
      </table>
      {bad ? (
        <div className="mw-errline">
          <AlertTriangle className="i" aria-hidden />
          <span>
            A capture reads one of exactly three things: a JSONPath (<code>$.…</code>), a header (
            <code>header:Name</code>) or <code>status</code>.
          </span>
        </div>
      ) : (
        <p className="mw-hint">
          Later steps read a captured value as <code>{"{{name}}"}</code>. The value stays with the run and is never
          shown here or stored.
        </p>
      )}
    </>
  );
}

function ResponsePanel({
  result,
  draft,
  base,
  capturedEarlier,
  cli,
}: {
  result: RunState | undefined;
  draft: MorseStepDto;
  base: string | null;
  capturedEarlier: string[];
  cli: string;
}) {
  if (!result) {
    return (
      <div className="mw-resp">
        <div className="mw-resp-h">
          <b>Response</b>
        </div>
        <div className="mw-resp-b">
          <div className="mw-empty">
            <Play className="i big" aria-hidden />
            <div>
              Send fires this one request{base ? <> to <b>{base}</b></> : null}.
            </div>
            <div className="mw-hint" style={{ margin: 0 }}>
              Only to a host this machine allows · redirects are never followed · nothing fires when a tab opens
            </div>
          </div>
        </div>
      </div>
    );
  }
  if (result.state === "pending") {
    return (
      <div className="mw-resp">
        <div className="mw-resp-h">
          <b>Response</b>
          <span className="mw-spin" />
          <span className="mw-meta">sending…</span>
        </div>
        <div className="mw-resp-b" />
      </div>
    );
  }
  if (result.state === "error") {
    return (
      <div className="mw-resp">
        <div className="mw-resp-h">
          <b>Response</b>
          <span className="mw-stat bad">not sent</span>
        </div>
        <div className="mw-resp-b">
          <Banner tone="warn" icon={<AlertTriangle className="i" aria-hidden />}>
            <Coded text={result.message} />
          </Banner>
        </div>
      </div>
    );
  }
  const run = result.run;
  if (run.refused) {
    const command = allowCommand(run.refused);
    return (
      <div className="mw-resp">
        <div className="mw-resp-h">
          <b>Response</b>
          <span className="mw-stat bad">refused</span>
          <span className="mw-meta">nothing was sent</span>
        </div>
        <div className="mw-resp-b">
          <Banner tone="crit" icon={<ShieldAlert className="i" aria-hidden />}>
            {(run.refused.split("If you trust it")[0] ?? "").trim()} The page can fire a request but cannot decide what to
            trust — run this in your terminal if you trust the host:
            <br />
            {command ? <CopyCmd command={command} /> : null}
          </Banner>
        </div>
      </div>
    );
  }
  const step = run.steps[0];
  if (!step) return null;
  const details = step.detail ? step.detail.split("; ") : [];
  const captured = details.filter((d) => d.startsWith("captured ")).map((d) => d.slice("captured ".length));
  const failures = step.passed ? [] : details;
  const unboundEarlier = failures
    .map((f) => f.match(/`\{\{([^}]+)\}\}` is not bound/)?.[1])
    .find((name): name is string => !!name && capturedEarlier.includes(name));
  return (
    <div className="mw-resp">
      <div className="mw-resp-h">
        <b>Response</b>
        {step.status !== null ? (
          <span className={cn("mw-stat", step.passed ? "ok" : "bad")}>{step.status}</span>
        ) : (
          <span className="mw-stat bad">no response</span>
        )}
        <span className="mw-meta">{step.duration_ms} ms</span>
        {step.bytes !== null ? <span className="mw-meta">{formatBytes(Number(step.bytes))}</span> : null}
        <span className="mw-sp" />
        <span className={cn("mw-pill", step.passed ? "pass" : "fail")}>{step.passed ? "checks held" : "failed"}</span>
      </div>
      <div className="mw-resp-b">
        <div className="mw-checks">
          {step.passed ? (
            <>
              {draft.status ? (
                <div className="mw-ck ok">
                  <CircleCheck className="i" aria-hidden />
                  <span>
                    status is <code>{draft.status}</code>
                  </span>
                </div>
              ) : null}
              {draft.checks
                .filter((c) => c.path)
                .map((c, i) => (
                  <div key={i} className="mw-ck ok">
                    <CircleCheck className="i" aria-hidden />
                    <span>
                      <code>{c.path}</code> {c.rule === "exists" ? "exists" : <>equals <code>{c.value}</code></>}
                    </span>
                  </div>
                ))}
              {!draft.status && !draft.checks.some((c) => c.path) ? (
                <div className="mw-ck ok">
                  <CircleCheck className="i" aria-hidden />
                  <span>sent — nothing was expected, so any response counts</span>
                </div>
              ) : null}
            </>
          ) : (
            failures.map((f, i) => (
              <div key={i} className="mw-ck no">
                <CircleX className="i" aria-hidden />
                <span>
                  <Coded text={f} />
                </span>
              </div>
            ))
          )}
          {captured.map((name) => (
            <div key={name} className="mw-ck ok">
              <Lock className="i" aria-hidden />
              <span>
                captured <code>{name}</code> · the value stays out of the browser
              </span>
            </div>
          ))}
        </div>
        {unboundEarlier ? (
          <p className="mw-hint">
            <code>{`{{${unboundEarlier}}}`}</code> is captured by an earlier step. A step sent on its own has no
            captured values — Run the scenario to send the chain, or type a literal in its place to try this step
            alone.
          </p>
        ) : null}
        <div className="mw-vault">
          <Lock className="i" aria-hidden />
          <div>
            The response body stayed on the server. Bodies and captured values never reach this page, because a
            response is where a real token is most likely to turn up. To read the body, run the same request in your
            terminal:
            <br />
            <CopyCmd command={cli} />
          </div>
        </div>
      </div>
    </div>
  );
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  return `${(n / 1024).toFixed(1)} KB`;
}
