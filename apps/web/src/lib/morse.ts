// The Morse workbench's pure half (ADR 0023): turning a spec operation into
// a draft, reading what a draft references, and saying what is wrong with it
// before anything is sent or saved. No fetch, no React — so every rule here
// is a unit test away from being pinned.
//
// A draft is the wire's own step shape (`MorseStepDto`), because that is
// what Send posts and what Save writes into the fence. There is no second
// model of a request to drift from the first.

import type { MorseBodyDto, MorseOperationDto, MorsePairDto, MorseSpecDto, MorseStepDto } from "./types";

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
  const body: MorseBodyDto | null =
    op.body.length === 0
      ? null
      : {
          kind: "json",
          text: JSON.stringify(
            Object.fromEntries((required.length ? required : op.body).map((f) => [f.name, placeholder(f.kind)])),
            null,
            2,
          ),
        };
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

// ---- New requests: a method and a path (ADR 0027) ---------------------------

/** A request the page typed: the spec whose server it goes to, and the
 *  method and path. There is no field for a host. */
export type InlineDef = { spec: string; method: string; path: string; summary: string | null };

export const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE"];

/** Why a method and a path cannot be a request, or null — the server's rule. */
export function requestProblem(method: string, path: string): string | null {
  if (!METHODS.includes(method.trim().toUpperCase())) return `\`${method}\` is not an HTTP method`;
  const p = path.trim();
  if (!p.startsWith("/") || p.startsWith("//") || p.includes("://")) {
    return "a path beginning with / — the host comes from the spec or the environment, never from here";
  }
  return null;
}

/** An inline request dressed as a catalogue operation, so the request tab
 *  shows and edits it like any other. Nothing about it is in a spec. */
export function inlineOp(id: string, def: InlineDef): MorseOperationDto {
  return {
    operation_id: id,
    method: def.method.toUpperCase(),
    path: def.path,
    summary: def.summary,
    tag: null,
    params: [],
    body: [],
    responses: [],
  };
}

/** A fresh draft for a request no spec describes. */
export function draftForRequest(id: string): MorseStepDto {
  return {
    id,
    operation: null,
    request: id,
    params: [],
    query: [],
    headers: [],
    body: null,
    status: null,
    checks: [],
    capture: [],
  };
}

/** Keep a draft's `params` in step with the `{name}` segments of a path the
 *  page is typing: new names appear, gone ones leave, values are kept. */
export function syncPathParams(draft: MorseStepDto, path: string): MorseStepDto {
  const names = pathParams(path);
  const kept = new Map(draft.params.map((p) => [p.key, p.value]));
  return { ...draft, params: names.map((key) => ({ key, value: kept.get(key) ?? "" })) };
}

/** A request id from what was typed: words of the path, unique among `taken`. */
export function requestIdFor(method: string, path: string, taken: readonly string[]): string {
  const words = path
    .replace(/\{[^}]*\}/g, "")
    .split(/[^A-Za-z0-9]+/)
    .filter(Boolean)
    .slice(-2)
    .join("-")
    .toLowerCase();
  const stem = `${method.toLowerCase()}${words ? `-${words}` : ""}`;
  if (!taken.includes(stem)) return stem;
  let n = 2;
  while (taken.includes(`${stem}-${n}`)) n += 1;
  return `${stem}-${n}`;
}

/** The verb a tab shows, or null when the operation is not known — a tab
 *  must not claim GET for an operation it has not looked up. */
export function tabVerb(op: { method: string } | undefined | null): string | null {
  return op ? op.method.toUpperCase() : null;
}

/** Every name a draft reads, in order, without repeats. */
export function usedVars(d: MorseStepDto): string[] {
  const texts = [
    ...d.params.map((p) => p.value),
    ...d.query.map((p) => p.value),
    ...d.headers.map((p) => p.value),
    ...bodyTexts(d.body),
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

// ---- Body shapes (ADR 0027) -------------------------------------------------

export type BodyKind = "none" | MorseBodyDto["kind"];

/** The raw types Postman offers, and the media type each one sends. */
export const RAW_TYPES: Array<{ label: string; type: string }> = [
  { label: "Text", type: "text/plain" },
  { label: "JavaScript", type: "application/javascript" },
  { label: "HTML", type: "text/html" },
  { label: "XML", type: "application/xml" },
  { label: "Binary file", type: "application/octet-stream" },
];

export function bodyKind(body: MorseBodyDto | null): BodyKind {
  return body ? body.kind : "none";
}

/** A new, empty body of a shape — what switching the Body tab's shape
 *  starts from. JSON keeps the text it had when switching from JSON. */
export function emptyBody(kind: BodyKind, from: MorseBodyDto | null = null): MorseBodyDto | null {
  switch (kind) {
    case "none":
      return null;
    case "json":
      return { kind: "json", text: from?.kind === "raw" ? (from.text ?? "") : "{\n  \n}" };
    case "form":
      return { kind: "form", fields: [{ key: "", value: "" }] };
    case "raw":
      return { kind: "raw", media_type: "text/plain", text: from?.kind === "json" ? from.text : "", file: null };
    case "multipart":
      return { kind: "multipart", parts: [{ name: "", value: "", file: null, media_type: null }] };
  }
}

/** Every text a body holds, for finding the variables it reads. */
export function bodyTexts(body: MorseBodyDto | null): string[] {
  if (!body) return [];
  switch (body.kind) {
    case "json":
      return [body.text];
    case "form":
      return body.fields.map((f) => f.value);
    case "raw":
      return [body.text ?? ""];
    case "multipart":
      return body.parts.flatMap((p) => [p.name, p.value ?? ""]);
  }
}

/** `type/subtype` with optional `; key=value` — the rule the server applies. */
export function isMediaType(raw: string): boolean {
  const token = "[A-Za-z0-9!#$%&'*+.^_`|~-]+";
  return new RegExp(`^${token}/${token}(\\s*;\\s*${token}=("?)${token}\\2)*$`).test(raw.trim());
}

/** Why a file path will be refused, or null — the server's rule (ADR 0027). */
export function filePathProblem(path: string): string | null {
  const p = path.trim();
  if (!p) return "name a file in this repository";
  if (p.startsWith("/") || p.includes("\\") || p.includes(":")) return "a path in this repository, not an absolute one";
  const segments = p.split("/");
  if (segments.some((s) => s === "" || s === "." || s === "..")) return "the path may not climb out of the repository";
  if (segments[0] === ".dit" || segments[0] === ".git") return "DIT's and git's own files cannot be sent";
  return null;
}

/** Why the body will not be accepted, or null. A `{{name}}` stands in for
 *  any value, quoted or not. */
export function bodyError(body: MorseBodyDto | null): string | null {
  if (!body) return null;
  switch (body.kind) {
    case "json": {
      if (!body.text.trim()) return null;
      try {
        JSON.parse(body.text.replace(/\{\{[^}]*\}\}/g, "0"));
        return null;
      } catch (e) {
        return e instanceof Error ? e.message : "not JSON";
      }
    }
    case "form":
      return null;
    case "raw":
      if (!isMediaType(body.media_type)) return `\`${body.media_type}\` is not a media type — like application/xml`;
      if (body.file !== null) {
        const problem = filePathProblem(body.file);
        return problem ? `file: ${problem}` : null;
      }
      return null;
    case "multipart":
      for (const part of body.parts) {
        if (!part.name.trim()) continue;
        if (/["\r\n]/.test(part.name)) return `part \`${part.name}\`: no quotes or line breaks in a name`;
        if (part.file !== null) {
          const problem = filePathProblem(part.file);
          if (problem) return `part \`${part.name}\`: ${problem}`;
        }
        if (part.media_type && !isMediaType(part.media_type)) {
          return `part \`${part.name}\`: \`${part.media_type}\` is not a media type`;
        }
      }
      return null;
  }
}

/** A body as the server will store it, so drafts that differ only in an
 *  empty row compare equal. */
function normalBody(body: MorseBodyDto | null): MorseBodyDto | null {
  if (!body) return null;
  switch (body.kind) {
    case "json":
      return body.text.trim() ? body : null;
    case "form":
      return { ...body, fields: body.fields.filter((f) => f.key.trim()) };
    case "multipart":
      return { ...body, parts: body.parts.filter((p) => p.name.trim()) };
    case "raw":
      return body;
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
    body: step.body ? structuredClone(step.body) : null,
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
      body: normalBody(s.body),
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

function bodyPreview(body: MorseBodyDto | null): string[] {
  if (!body) return [];
  switch (body.kind) {
    case "json": {
      let parsed: unknown = null;
      try {
        parsed = JSON.parse(body.text.replace(/"?\{\{\s*([^}]+?)\s*\}\}"?/g, (_m: string, n: string) => JSON.stringify(`{{${n}}}`)));
      } catch {
        parsed = undefined;
      }
      return [parsed === undefined ? "    body: # not valid JSON yet" : `    body: ${flowValue(parsed)}`];
    }
    case "form":
      return body.fields.length ? [`    form: ${flowPairs(body.fields)}`] : [];
    case "raw":
      if (body.file !== null) return [`    raw: { type: ${scalar(body.media_type)}, file: ${scalar(body.file)} }`];
      if (!(body.text ?? "").includes("\n")) return [`    raw: { type: ${scalar(body.media_type)}, text: ${scalar(body.text ?? "")} }`];
      return [
        "    raw:",
        `      type: ${scalar(body.media_type)}`,
        "      text: |",
        ...(body.text ?? "").split("\n").map((line) => (line ? `        ${line}` : "")),
      ];
    case "multipart":
      return [
        "    multipart:",
        ...body.parts.map((p) => {
          const fields = [`name: ${scalar(p.name)}`];
          fields.push(p.file !== null ? `file: ${scalar(p.file)}` : `value: ${scalar(p.value ?? "")}`);
          if (p.media_type) fields.push(`type: ${scalar(p.media_type)}`);
          return `      - { ${fields.join(", ")} }`;
        }),
      ];
  }
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
  lines.push(...bodyPreview(normalBody(d.body)));
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
