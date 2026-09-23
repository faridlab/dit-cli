// The Morse workbench's pure half (ADR 0023): turning a spec operation into
// a draft, reading what a draft references, and saying what is wrong with it
// before anything is sent or saved. No fetch, no React — so every rule here
// is a unit test away from being pinned.
//
// A draft is the wire's own step shape (`MorseStepDto`), because that is
// what Send posts and what Save writes into the fence. There is no second
// model of a request to drift from the first.

import type { MorseOperationDto, MorsePairDto, MorseSpecDto, MorseStepDto } from "./types";

/** Every `{{name}}` in a string, in the order written — the same reading
 *  `dit-model`'s `variables_in` makes. */
export function variablesIn(text: string): string[] {
  const out: string[] = [];
  for (const match of text.matchAll(/\{\{\s*([^}\s][^}]*?)\s*\}\}/g)) if (match[1]) out.push(match[1]);
  return out;
}

/** The `{name}` segments of a spec's path — OpenAPI's own syntax, never a
 *  `{{reference}}`. */
export function pathParams(path: string): string[] {
  const out: string[] = [];
  for (const match of path.matchAll(/(?<!\{)\{([^{}]+)\}(?!\})/g)) if (match[1]) out.push(match[1].trim());
  return out;
}

/** A step id suggested from an operationId: its leading verb, which is how
 *  people name steps by hand (`create`, `get`, `login`). */
export function suggestStepId(operationId: string): string {
  const verb = operationId.match(/^[a-z]+/)?.[0];
  return verb && verb.length > 1 ? verb : "step";
}

function placeholder(kind: string): unknown {
  switch (kind) {
    case "integer":
    case "number":
      return 0;
    case "boolean":
      return false;
    case "array":
      return [];
    case "object":
      return {};
    default:
      return "";
  }
}

/** A fresh draft for one operation, pre-filled from what the spec says it
 *  takes: its path and query parameters, its required body fields, and the
 *  first success status it documents. Nothing is guessed that the spec
 *  does not state. */
export function draftForOperation(specId: string, op: MorseOperationDto): MorseStepDto {
  const inPath = op.params.filter((p) => p.location === "path").map((p) => p.name);
  const params = [...new Set([...inPath, ...pathParams(op.path)])].map((key) => ({ key, value: "" }));
  const query = op.params
    .filter((p) => p.location === "query")
    .map((p) => ({ key: p.name, value: "" }));
  const headers = op.params
    .filter((p) => p.location === "header")
    .map((p) => ({ key: p.name, value: "" }));
  const required = op.body.filter((f) => f.required);
  const body =
    op.body.length === 0
      ? null
      : JSON.stringify(
          Object.fromEntries((required.length ? required : op.body).map((f) => [f.name, placeholder(f.kind)])),
          null,
          2,
        );
  const ok = op.responses.find((r) => /^2\d\d$/.test(r));
  return {
    id: suggestStepId(op.operation_id),
    operation: `${specId}/${op.operation_id}`,
    request: null,
    params,
    query,
    headers,
    body,
    status: ok ? Number(ok) : null,
    checks: [],
    capture: [],
  };
}

/** Every name a draft reads, in order, without repeats. */
export function usedVars(d: MorseStepDto): string[] {
  const texts = [
    ...d.params.map((p) => p.value),
    ...d.query.map((p) => p.value),
    ...d.headers.map((p) => p.value),
    d.body ?? "",
    ...d.checks.filter((c) => c.rule === "equals").map((c) => c.value),
  ];
  return [...new Set(texts.flatMap(variablesIn))];
}

// The same lists `dit-model` uses for `morse-secrets`, so the tab warns about
// exactly what the server will refuse to commit.
const SECRET_PREFIXES = ["Bearer ", "Basic ", "sk-", "ghp_", "github_pat_", "xoxb-", "AKIA", "eyJ"];
const SCHEME_WORDS = ["bearer", "basic", "token", "digest"];
const SECRET_FIELDS = [
  "authorization",
  "password",
  "passwd",
  "secret",
  "token",
  "api_key",
  "apikey",
  "x-api-key",
  "access_token",
  "refresh_token",
  "client_secret",
  "private_key",
];

/** Why a literal looks like a credential, or null. `Bearer {{token}}` and
 *  `{{password}}` are the shape the design asks for and pass. */
export function secretReason(field: string, value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const residue = trimmed.replace(/\{\{[^}]*\}\}/g, "").trim();
  if (!residue) return null;
  if (trimmed.includes("{{") && SCHEME_WORDS.includes(residue.toLowerCase())) return null;
  if (SECRET_PREFIXES.some((p) => trimmed.startsWith(p))) return "a credential written out in full";
  if (SECRET_FIELDS.includes(field.trim().toLowerCase())) {
    return "a field that holds a credential, written as a literal";
  }
  return null;
}

/** Indexes of the headers carrying a credential literal. */
export function secretHeaders(d: MorseStepDto): number[] {
  return d.headers.flatMap((h, i) => (h.key && secretReason(h.key, h.value) ? [i] : []));
}

/** Why the body will not parse, or null. A `{{name}}` stands in for any
 *  value, quoted or not. */
export function bodyError(body: string | null): string | null {
  if (!body || !body.trim()) return null;
  try {
    JSON.parse(body.replace(/\{\{[^}]*\}\}/g, "0"));
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : "not JSON";
  }
}

/** A capture reads one of exactly three things. */
export function isSelector(from: string): boolean {
  const t = from.trim();
  return t === "status" || /^header:\s*\S/.test(t) || t.startsWith("$");
}

/** The path with every filled parameter shown in place, for the URL bar.
 *  Returned as segments so the caller can colour them. */
export function pathSegments(
  path: string,
  params: MorsePairDto[],
): { text: string; kind: "plain" | "param" | "filled" }[] {
  const out: { text: string; kind: "plain" | "param" | "filled" }[] = [];
  let last = 0;
  for (const match of path.matchAll(/(?<!\{)\{([^{}]+)\}(?!\})/g)) {
    const at = match.index ?? 0;
    if (at > last) out.push({ text: path.slice(last, at), kind: "plain" });
    const value = params.find((p) => p.key === (match[1] ?? "").trim())?.value ?? "";
    out.push(value ? { text: value, kind: "filled" } : { text: match[0], kind: "param" });
    last = at + match[0].length;
  }
  if (last < path.length) out.push({ text: path.slice(last), kind: "plain" });
  return out;
}

/** True when every one of the spec's servers is a path — `servers: - url: /`
 *  — which names no host at all. serpa's 45 generated documents all do. */
export function relativeOnly(spec: MorseSpecDto): boolean {
  return spec.servers.length > 0 && spec.servers.every((s) => !s.url.includes("://"));
}

/** Where a request would go, in words, before anyone presses Send. */
export function baseFor(spec: MorseSpecDto | undefined, server: string | null | undefined): string | null {
  if (server) return server;
  const first = spec?.servers[0]?.url;
  return first && first.includes("://") ? first : null;
}

/** A host out of a base URL, for the allowlist comparison and the
 *  `dit morse allow` line. */
export function hostOf(url: string): string | null {
  const m = url.match(/^[a-z][a-z0-9+.-]*:\/\/([^/?#:@]+)/i);
  return m?.[1] ? m[1].toLowerCase() : null;
}

/** Split a `send:<spec>/<op>` history key from a scenario name. */
export function runKey(key: string): { kind: "send"; operation: string } | { kind: "scenario"; name: string } {
  return key.startsWith("send:")
    ? { kind: "send", operation: key.slice("send:".length) }
    : { kind: "scenario", name: key };
}

/** Deep-copy a draft so an edit never reaches a cached server object. */
export function cloneStep(step: MorseStepDto): MorseStepDto {
  return {
    ...step,
    params: step.params.map((p) => ({ ...p })),
    query: step.query.map((p) => ({ ...p })),
    headers: step.headers.map((p) => ({ ...p })),
    checks: step.checks.map((c) => ({ ...c })),
    capture: step.capture.map((c) => ({ ...c })),
  };
}

/** Two drafts say the same thing — what "saved" means for autosave. */
export function sameStep(a: MorseStepDto, b: MorseStepDto): boolean {
  const norm = (s: MorseStepDto) =>
    JSON.stringify({
      ...s,
      params: s.params.filter((p) => p.key.trim()),
      query: s.query.filter((p) => p.key.trim()),
      headers: s.headers.filter((p) => p.key.trim()),
      checks: s.checks.filter((c) => c.path.trim()),
      capture: s.capture.filter((c) => c.name.trim()),
      body: s.body && s.body.trim() ? s.body : null,
    });
  return norm(a) === norm(b);
}

// ---- A preview of the fence a draft becomes --------------------------------
//
// The server's fence writer is the one that saves; this mirrors its quoting
// rules so the Fence panel of an unsaved draft shows what Save would write.
// A step already in a fence shows the fence's real bytes instead.

function scalar(s: string): string {
  const needs =
    s === "" ||
    s !== s.trim() ||
    s === "null" ||
    s === "~" ||
    /^[-&*!|>%@`]/.test(s) ||
    s.includes(": ") ||
    s.endsWith(":") ||
    /[,#"'{}[\]\n\t]/.test(s);
  if (!needs) return s;
  return s.includes('"') ? `'${s}'` : `"${s}"`;
}

function flowValue(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(flowValue).join(", ")}]`;
  if (value && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>);
    return entries.length ? `{ ${entries.map(([k, v]) => `${k}: ${flowValue(v)}`).join(", ")} }` : "{}";
  }
  return scalar(value === null ? "" : String(value));
}

function flowPairs(pairs: MorsePairDto[]): string {
  return `{ ${pairs.map((p) => `${p.key}: ${scalar(p.value)}`).join(", ")} }`;
}

export function stepPreview(d: MorseStepDto): string {
  const lines = [`  - id: ${scalar(d.id)}`];
  lines.push(d.operation ? `    operation: ${scalar(d.operation)}` : `    request: ${scalar(d.request ?? "")}`);
  const params = d.params.filter((p) => p.key && p.value);
  if (params.length) lines.push(`    params: ${flowPairs(params)}`);
  const query = d.query.filter((p) => p.key);
  if (query.length) lines.push(`    query: ${flowPairs(query)}`);
  const headers = d.headers.filter((p) => p.key);
  if (headers.length) lines.push(`    headers: ${flowPairs(headers)}`);
  if (d.body && d.body.trim()) {
    let parsed: unknown = null;
    try {
      parsed = JSON.parse(d.body.replace(/"?\{\{\s*([^}]+?)\s*\}\}"?/g, (_m, n) => JSON.stringify(`{{${n}}}`)));
    } catch {
      parsed = undefined;
    }
    lines.push(parsed === undefined ? "    body: # not valid JSON yet" : `    body: ${flowValue(parsed)}`);
  }
  const checks = d.checks.filter((c) => c.path);
  if (d.status || checks.length) {
    lines.push("    expect:");
    if (d.status) lines.push(`      status: ${d.status}`);
    if (checks.length) {
      lines.push("      jsonpath:");
      for (const c of checks) {
        lines.push(`        ${c.path}: ${c.rule === "exists" ? "{ exists: true }" : scalar(c.value)}`);
      }
    }
  }
  const capture = d.capture.filter((c) => c.name);
  if (capture.length) lines.push(`    capture: { ${capture.map((c) => `${c.name}: ${scalar(c.from)}`).join(", ")} }`);
  return lines.join("\n");
}
