// Development-only fixture server (activated by ?mock=1 in the dev URL).
// It intercepts fetch for /api/* and answers with in-memory data so the UI
// can be hand-checked before the Rust backend exists. This module is only
// reachable through a dead-code-eliminated dev branch — it never ships.
//
// The body_html strings below are authored constants, not sanitized input:
// in production every body_html arrives sanitized from the server's
// markdown renderer — the mock is not and must never become a
// sanitization boundary.
//
// The query evaluator is a port of the server's DQL grammar (dit-query:
// lexer.rs, parser.rs, compile.rs) onto in-memory objects, so ?mock=1
// accepts and rejects the same queries production does. When the two
// disagree, this copy is the one that is wrong.

import { setToken, getToken } from "./auth";
import type {
  BoardDto,
  CommentDto,
  DocBodyDto,
  DocEntryDto,
  FieldEventDto,
  IssueDto,
  IssueType,
  Layout,
  NumberingPolicy,
  Priority,
  SchemaDto,
  SettingsDto,
  StatusInfo,
} from "./types";

// ---------------------------------------------------------------------------
// DQL — lexer
// ---------------------------------------------------------------------------

type Tok =
  | { t: "ident"; v: string }
  | { t: "str"; v: string }
  | { t: "num"; v: number }
  | { t: "rel"; v: number }
  | { t: "me" }
  | { t: "op"; v: "=" | "!=" | ">" | ">=" | "<" | "<=" | "~" }
  | { t: "punct"; v: "(" | ")" | "," };

const REL_UNITS: Record<string, number> = { d: 1, w: 7, m: 30, y: 365 };
const KEYWORDS = new Set(["and", "or", "not", "in", "order", "by", "limit", "asc", "desc"]);

function isWordChar(ch: string): boolean {
  return /[A-Za-z0-9_:@.-]/.test(ch);
}

function lexDql(input: string): Tok[] {
  const toks: Tok[] = [];
  let i = 0;
  while (i < input.length) {
    const ch = input[i]!;
    if (/\s/.test(ch)) {
      i += 1;
      continue;
    }
    // Relative date: `-7d`, `+2w`, `-1m` — a signed count then a unit.
    const rel = /^[-+]\d+[dwmy](?![\w:@.-])/.exec(input.slice(i));
    if (rel) {
      const days = Number(rel[0]!.slice(0, -1)) * REL_UNITS[rel[0]!.slice(-1)]!;
      toks.push({ t: "rel", v: days });
      i += rel[0].length;
      continue;
    }
    if (ch === '"') {
      const end = input.indexOf('"', i + 1);
      if (end < 0) throw new Error("DQL could not be read: unterminated string");
      toks.push({ t: "str", v: input.slice(i + 1, end) });
      i = end + 1;
      continue;
    }
    if (/[0-9]/.test(ch)) {
      const num = /^\d+(?![\w:@.-])/.exec(input.slice(i));
      if (!num) throw new Error(`DQL could not be read: unexpected character ${ch}`);
      toks.push({ t: "num", v: Number(num[0]) });
      i += num[0].length;
      continue;
    }
    // `@me` only when it stands alone; otherwise `@` is a word character
    // (labels like `context:@computer` keep their @ in the value).
    if (ch === "@" && input.startsWith("@me", i) && !isWordChar(input[i + 3] ?? "")) {
      toks.push({ t: "me" });
      i += 3;
      continue;
    }
    const two = input.slice(i, i + 2);
    if (two === "!=" || two === ">=" || two === "<=") {
      toks.push({ t: "op", v: two });
      i += 2;
      continue;
    }
    if (ch === "=" || ch === ">" || ch === "<" || ch === "~") {
      toks.push({ t: "op", v: ch });
      i += 1;
      continue;
    }
    if (ch === "(" || ch === ")" || ch === ",") {
      toks.push({ t: "punct", v: ch });
      i += 1;
      continue;
    }
    if (isWordChar(ch)) {
      let j = i;
      while (j < input.length && isWordChar(input[j]!)) j += 1;
      toks.push({ t: "ident", v: input.slice(i, j) });
      i = j;
      continue;
    }
    throw new Error(`DQL could not be read: unexpected character ${ch}`);
  }
  return toks;
}

// ---------------------------------------------------------------------------
// DQL — parser (precedence: AND over OR, parentheses group, ORDER BY and
// LIMIT are suffix clauses in that order)
// ---------------------------------------------------------------------------

type Val =
  | { k: "str"; v: string }
  | { k: "num"; v: number }
  | { k: "rel"; v: number }
  | { k: "me" }
  | { k: "list"; v: Val[] };

type Expr =
  | { k: "and"; l: Expr; r: Expr }
  | { k: "or"; l: Expr; r: Expr }
  | { k: "cmp"; field: string; op: string; value: Val };

type Query = {
  filter: Expr | null;
  order: Array<{ field: string; dir: "asc" | "desc" }>;
  limit: number | null;
};

const FIELDS =
  "id, short_ref, title, type, status, priority, reporter, assignee, label, epic, estimate, sprint, created, updated, due, body";
const KNOWN_FIELDS = new Set([
  "id",
  "short_ref",
  "title",
  "type",
  "status",
  "priority",
  "reporter",
  "assignee",
  "label",
  "epic",
  "estimate",
  "number",
  "sprint",
  "created",
  "updated",
  "due",
  "body",
]);

class Parser {
  constructor(private readonly toks: Tok[]) {}
  private pos = 0;
  private limitSeen = false;

  private peek(): Tok | undefined {
    return this.toks[this.pos];
  }
  private next(): Tok | undefined {
    return this.toks[this.pos++];
  }
  private atKeyword(kw: string): boolean {
    const tok = this.peek();
    return tok?.t === "ident" && tok.v.toLowerCase() === kw.toLowerCase();
  }
  private atPunct(ch: string): boolean {
    const tok = this.peek();
    return tok?.t === "punct" && tok.v === ch;
  }

  query(): Query {
    const filter = this.peek() === undefined ? null : this.orExpr();
    const order: Query["order"] = [];
    let limit: number | null = null;
    while (this.peek() !== undefined) {
      if (this.atKeyword("ORDER")) {
        if (this.limitSeen) throw new Error("trailing input after LIMIT");
        this.next();
        if (!this.atKeyword("BY")) throw new Error("ORDER needs BY");
        this.next();
        order.push(...this.orderList());
      } else if (this.atKeyword("LIMIT")) {
        this.next();
        this.limitSeen = true;
        const value = this.next();
        if (value?.t !== "num" || !Number.isInteger(value.v)) {
          throw new Error("LIMIT needs a whole number — e.g. `LIMIT 50`");
        }
        limit = value.v;
      } else {
        throw new Error("trailing input — put ORDER BY and LIMIT last, in that order");
      }
    }
    return { filter, order, limit };
  }

  private orderList(): Query["order"] {
    const out: Query["order"] = [];
    for (;;) {
      const name = this.next();
      if (name?.t !== "ident") throw new Error("ORDER BY needs at least one field");
      if (!KNOWN_FIELDS.has(name.v)) throw new Error(`unknown field \`${name.v}\` — the available fields are ${FIELDS}`);
      let dir: "asc" | "desc" = "asc";
      if (this.atKeyword("ASC")) {
        this.next();
      } else if (this.atKeyword("DESC")) {
        this.next();
        dir = "desc";
      }
      out.push({ field: name.v, dir });
      if (this.atPunct(",")) {
        this.next();
        continue;
      }
      return out;
    }
  }

  private orExpr(): Expr {
    let left = this.andExpr();
    while (this.atKeyword("OR")) {
      this.next();
      left = { k: "or", l: left, r: this.andExpr() };
    }
    return left;
  }

  private andExpr(): Expr {
    let left = this.primary();
    while (this.atKeyword("AND")) {
      this.next();
      left = { k: "and", l: left, r: this.primary() };
    }
    return left;
  }

  private primary(): Expr {
    if (this.atPunct("(")) {
      this.next();
      const inner = this.orExpr();
      if (!this.atPunct(")")) throw new Error("missing `)`");
      this.next();
      return inner;
    }
    const name = this.next();
    if (name?.t !== "ident") throw new Error("expected a comparison like `status = todo`");
    if (!KNOWN_FIELDS.has(name.v)) throw new Error(`unknown field \`${name.v}\` — the available fields are ${FIELDS}`);
    // NOT only introduces NOT IN, as in the server grammar.
    let negated = false;
    if (this.atKeyword("NOT")) {
      this.next();
      negated = true;
      if (!this.atKeyword("IN")) throw new Error("NOT must introduce NOT IN");
    }
    if (this.atKeyword("IN")) {
      this.next();
      const list = this.valueList();
      return { k: "cmp", field: name.v, op: negated ? "NOT IN" : "IN", value: list };
    }
    if (negated) throw new Error("NOT must introduce NOT IN");
    const op = this.next();
    if (op?.t !== "op") throw new Error(`\`${name.v}\` needs an operator — e.g. \`${name.v} = todo\``);
    return { k: "cmp", field: name.v, op: op.v, value: this.value() };
  }

  private value(): Val {
    const tok = this.next();
    if (tok?.t === "str") return { k: "str", v: tok.v };
    if (tok?.t === "num") return { k: "num", v: tok.v };
    if (tok?.t === "rel") return { k: "rel", v: tok.v };
    if (tok?.t === "me") return { k: "me" };
    if (tok?.t === "ident") {
      // Keywords are never values — `status = AND` is a parse error in the
      // server lexer, not a status called "AND".
      if (KEYWORDS.has(tok.v.toLowerCase())) {
        throw new Error(`\`${tok.v}\` cannot be used as a value`);
      }
      return { k: "str", v: tok.v };
    }
    throw new Error("expected a value on the right of the operator");
  }

  private valueList(): Val {
    if (!this.atPunct("(")) {
      throw new Error("IN needs a parenthesized list — e.g. `status IN (todo, in_progress)`");
    }
    this.next();
    const items: Val[] = [];
    for (;;) {
      items.push(this.value());
      if (this.atPunct(",")) {
        this.next();
        continue;
      }
      break;
    }
    if (!this.atPunct(")")) throw new Error("missing `)`");
    this.next();
    return { k: "list", v: items };
  }
}

// ---------------------------------------------------------------------------
// DQL — evaluation over IssueDto
// ---------------------------------------------------------------------------

const ME = "farid";

/** Scalar or set projection of a field for filtering and ordering. Set
 *  fields (assignee, label) come back as arrays — the only two. */
function project(issue: IssueDto, field: string): string | number | string[] | null {
  switch (field) {
    case "id":
      return issue.id;
    case "short_ref":
      return issue.short_ref;
    case "title":
      return issue.title;
    case "type":
      return issue.type;
    case "status":
      return issue.status;
    case "priority":
      return issue.priority;
    case "reporter":
      return issue.reporter;
    case "assignee":
      return issue.assignees;
    case "label":
      return issue.labels;
    case "epic":
      return issue.epic;
    case "estimate":
      return issue.estimate;
    case "number":
      return issue.number;
    case "sprint":
      return issue.sprint;
    case "created":
      return issue.created;
    case "updated":
      return issue.updated;
    case "due":
      return issue.due;
    case "body":
      return issue.body;
    default:
      return null;
  }
}

function valueString(value: Val): string {
  if (value.k === "str") return value.v;
  if (value.k === "num") return String(value.v);
  if (value.k === "me") return ME;
  throw new Error("this position needs a plain value, not a list");
}

function evalCmp(issue: IssueDto, cmp: Extract<Expr, { k: "cmp" }>): boolean {
  const raw = project(issue, cmp.field);

  // Set membership: `label = auth` means "carries the label", IN means
  // "carries any of", and the negations invert exactly that.
  if (Array.isArray(raw)) {
    if (cmp.op === "IN" || cmp.op === "NOT IN") {
      if (cmp.value.k !== "list") throw new Error("IN needs a parenthesized list");
      const hit = cmp.value.v.some((item) => raw.includes(valueString(item)));
      return cmp.op === "IN" ? hit : !hit;
    }
    if (cmp.op === "=" || cmp.op === "!=") {
      const hit = raw.includes(valueString(cmp.value));
      return cmp.op === "=" ? hit : !hit;
    }
    throw new Error(`\`${cmp.field} ${cmp.op}\` does not compare a list — use =, != or IN`);
  }

  // ~ is text match over the field's text; only title and body have text.
  if (cmp.op === "~") {
    if (cmp.field !== "title" && cmp.field !== "body") {
      throw new Error("`~` matches text — e.g. `title ~ \"login\"` or `body ~ timeout`");
    }
    const needle = valueString(cmp.value).toLowerCase();
    return String(raw ?? "").toLowerCase().includes(needle);
  }

  if (cmp.op === "IN" || cmp.op === "NOT IN") {
    if (cmp.value.k !== "list") throw new Error("IN needs a parenthesized list");
    const hit = cmp.value.v.some((item) => String(raw ?? "") === valueString(item));
    return cmp.op === "IN" ? hit : !hit;
  }

  // Dates accept relative values (`updated > -7d`); everything else
  // compares as written. A missing value matches nothing — like SQL NULL.
  let right: string | number;
  if (cmp.value.k === "rel") {
    const at = new Date(Date.now() + cmp.value.v * 86_400_000);
    right = (cmp.field === "created" || cmp.field === "updated") ? at.toISOString() : at.toISOString().slice(0, 10);
  } else if (cmp.value.k === "num") {
    right = cmp.value.v;
  } else {
    right = valueString(cmp.value);
  }
  if (raw === null || raw === undefined) return false;
  if (typeof right === "number" || typeof raw === "number") {
    const leftNum = Number(raw);
    const rightNum = Number(right);
    switch (cmp.op) {
      case "=":
        return leftNum === rightNum;
      case "!=":
        return leftNum !== rightNum;
      case ">":
        return leftNum > rightNum;
      case ">=":
        return leftNum >= rightNum;
      case "<":
        return leftNum < rightNum;
      case "<=":
        return leftNum <= rightNum;
    }
  }
  const left = String(raw);
  const rightStr = String(right);
  switch (cmp.op) {
    case "=":
      return left === rightStr;
    case "!=":
      return left !== rightStr;
    case ">":
      return left > rightStr;
    case ">=":
      return left >= rightStr;
    case "<":
      return left < rightStr;
    case "<=":
      return left <= rightStr;
  }
  return false;
}

function evalExpr(issue: IssueDto, expr: Expr): boolean {
  if (expr.k === "and") return evalExpr(issue, expr.l) && evalExpr(issue, expr.r);
  if (expr.k === "or") return evalExpr(issue, expr.l) || evalExpr(issue, expr.r);
  return evalCmp(issue, expr);
}

function sortValue(issue: IssueDto, field: string): string | number | null {
  const raw = project(issue, field);
  if (Array.isArray(raw)) return raw.join(", ");
  return raw;
}

function runDql(q: string, issues: IssueDto[]): IssueDto[] {
  const query = new Parser(lexDql(q)).query();
  const filtered = query.filter ? issues.filter((issue) => evalExpr(issue, query.filter!)) : [...issues];
  for (const { field, dir } of query.order) {
    filtered.sort((a, b) => {
      const left = sortValue(a, field);
      const right = sortValue(b, field);
      // NULL-ish values sort last whichever way the arrow points.
      if (left === null || left === undefined) return right === null || right === undefined ? 0 : 1;
      if (right === null || right === undefined) return -1;
      const cmp =
        typeof left === "number" && typeof right === "number"
          ? left - right
          : String(left).localeCompare(String(right));
      return dir === "asc" ? cmp : -cmp;
    });
  }
  return query.limit === null ? filtered : filtered.slice(0, query.limit);
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const STATUSES: SchemaDto["workflow"]["statuses"] = [
  { id: "todo", label: "Todo", category: "todo", terminal: false, wip_limit: null },
  { id: "in_progress", label: "In progress", category: "doing", terminal: false, wip_limit: 5 },
  { id: "review", label: "In review", category: "doing", terminal: false, wip_limit: 3 },
  { id: "blocked", label: "Blocked", category: "todo", terminal: false, wip_limit: null },
  { id: "done", label: "Done", category: "done", terminal: true, wip_limit: null },
];

const PEOPLE = ["farid", "jane", "akira", "sam"];
const TYPES: IssueType[] = ["task", "bug", "story", "spike", "chore"];
const PRIORITIES: Priority[] = ["p0", "p1", "p2", "p3", "p4"];
const LABEL_POOL = ["auth", "api", "team:core", "ux", "infra"];

// Deterministic pseudo-random data: same fixture set on every reload, so
// screenshots and hand-checks are reproducible.
function seeded(n: number): number {
  const x = Math.sin(n * 999 + 7) * 10000;
  return x - Math.floor(x);
}

function isoDate(offsetDays: number): string {
  return new Date(Date.now() + offsetDays * 86_400_000).toISOString().slice(0, 10);
}

// Three epics for the Home rollup: real issues others point at through the
// `epic` field, exactly how the product models them.
const EPICS: Array<{ id: string; title: string; status: string; priority: Priority }> = [
  { id: "01JMEP1C000000000000000A", title: "Epic — merge driver hardening", status: "in_progress", priority: "p1" },
  { id: "01JMEP2C000000000000000B", title: "Epic — index rebuild pipeline", status: "review", priority: "p2" },
  { id: "01JMEP3C000000000000000C", title: "Epic — docs and onboarding", status: "in_progress", priority: "p3" },
];

const ISSUES: IssueDto[] = Array.from({ length: 36 }, (_, index) => {
  const n = index + 1;
  const created = new Date(Date.now() - Math.floor(seeded(n) * 21 * 24 * 3600_000)).toISOString();
  const updated = new Date(Date.now() - Math.floor(seeded(n + 50) * 72 * 3600_000)).toISOString();
  const body =
    n % 3 === 0 ? `## Context\n\nIssue **#${n}** body with some inline code.\n\n- one\n- two\n` : "";
  return {
    id: `01JMOCK${String(n).padStart(4, "0")}ULID`,
    short_ref: `MOCK${String(n).padStart(3, "0")}`,
    number: n,
    title: [
      "Merge driver drops %A on rename",
      "Index rebuild after force push",
      "Board columns ignore wip_limit",
      "Token gate flashes on reload",
      "DQL: IN with spaces fails to parse",
      "Comment author rendered twice",
      "Virtualized list jumpy on sort",
      "Add per-status colors",
      "Compress SQLite index on vacuum",
      "Login flow drops the session cookie",
    ][n % 10]!.concat(` (${n})`),
    type: TYPES[Math.floor(seeded(n + 11) * TYPES.length)] ?? "task",
    status: STATUSES[Math.floor(seeded(n + 21) * STATUSES.length)]?.id ?? "todo",
    priority: seeded(n + 31) > 0.2 ? PRIORITIES[Math.floor(seeded(n + 31) * PRIORITIES.length)] ?? "p2" : null,
    reporter: PEOPLE[Math.floor(seeded(n + 41) * PEOPLE.length)] ?? "farid",
    assignees: seeded(n + 3) > 0.4 ? [PEOPLE[Math.floor(seeded(n + 5) * PEOPLE.length)] ?? "jane"] : [],
    labels: seeded(n + 7) > 0.5 ? [LABEL_POOL[Math.floor(seeded(n + 9) * LABEL_POOL.length)] ?? "ux"] : [],
    epic: seeded(n + 17) > 0.35 ? (EPICS[n % EPICS.length]?.id ?? null) : null,
    estimate: seeded(n + 13) > 0.5 ? Math.ceil(seeded(n + 15) * 8) : null,
    sprint: null,
    due: n % 7 === 0 ? isoDate(0) : n % 11 === 0 ? isoDate(3) : n === 4 ? isoDate(-2) : null,
    created,
    updated,
    body,
    body_html: body
      ? `<h2>Context</h2><p>Issue <strong>#${n}</strong> body with some <code>markdown</code>.</p><ul><li>one</li><li>two</li></ul>`
      : "",
  };
});

for (const [i, epic] of EPICS.entries()) {
  ISSUES.push({
    id: epic.id,
    short_ref: `MOCK${String(37 + i).padStart(3, "0")}`,
    number: 37 + i,
    title: epic.title,
    type: "story",
    status: epic.status,
    priority: epic.priority,
    reporter: "farid",
    assignees: ["farid"],
    labels: ["epic"],
    epic: null,
    estimate: null,
    sprint: null,
    due: null,
    created: new Date(Date.now() - 24 * 24 * 3600_000).toISOString(),
    updated: new Date(Date.now() - 6 * 3600_000).toISOString(),
    body: "",
    body_html: "",
  });
}

// Overlay the GTD-ish content the Home view lives on: next actions by
// context, an inbox to triage, someday parking, and blocked work waiting
// on someone else. Done after the seeded pass so the deterministic noise
// stays comparable reload to reload.
const CONTEXTS = ["context:@computer", "context:@home", "context:@errands"];
const ENERGIES = ["energy:deep", "energy:quick"];
for (const n of [2, 5, 8, 13, 21]) {
  const issue = ISSUES[n - 1];
  if (!issue) continue;
  issue.labels = [...new Set([...issue.labels, "next", CONTEXTS[n % CONTEXTS.length]!, ENERGIES[n % ENERGIES.length]!])];
  issue.assignees = ["farid"];
  issue.status = issue.status === "done" ? "todo" : issue.status;
}
for (const n of [3, 6, 11, 15]) {
  const issue = ISSUES[n - 1];
  if (!issue) continue;
  issue.labels = [...new Set([...issue.labels, "inbox"])];
  issue.assignees = [];
  issue.status = "todo";
}
for (const n of [9, 18]) {
  const issue = ISSUES[n - 1];
  if (!issue) continue;
  issue.labels = [...new Set([...issue.labels, "someday"])];
}
for (const n of [7, 12, 26]) {
  const issue = ISSUES[n - 1];
  if (!issue) continue;
  issue.status = "blocked";
  // Waiting-on is "blocked and not mine" — leave these to other people.
  issue.assignees = issue.assignees.filter((person) => person !== ME).slice(0, 1);
  if (issue.assignees.length === 0) issue.assignees = ["jane"];
}

const COMMENTS = new Map<string, CommentDto[]>(
  ISSUES.slice(0, 6).map((issue) => [
    issue.id,
    [
      {
        id: `${issue.id}-c1`,
        issue_id: issue.id,
        author: "jane",
        created: new Date(Date.now() - 3600_000).toISOString(),
        body: "Reproduced on `main` — the merge driver keeps the marker.",
        body_html: "<p>Reproduced on <code>main</code> — the merge driver keeps the marker.</p>",
      },
    ] satisfies CommentDto[],
  ]),
);

// ---------------------------------------------------------------------------
// Field history — grown lazily, extended by PATCH so blame lines and the
// Home activity feed move when the mock is used
// ---------------------------------------------------------------------------

const EVENTS = new Map<string, FieldEventDto[]>();

function initialEvents(issue: IssueDto): FieldEventDto[] {
  const events: FieldEventDto[] = [];
  let seq = 1;
  const atCreation = (field: string, newValue: string | null) => {
    events.push({
      seq: seq++,
      field,
      old_value: null,
      new_value: newValue,
      ts: issue.created,
      author: issue.reporter ?? "unknown",
      commit_sha: "ab12cd3",
    });
  };
  atCreation("status", "todo");
  atCreation("title", issue.title);
  atCreation("type", issue.type);
  if (issue.priority !== null) atCreation("priority", issue.priority);
  if (issue.labels.length > 0) atCreation("labels", issue.labels.join(", "));
  if (issue.assignees.length > 0) atCreation("assignees", issue.assignees.join(", "));
  if (issue.epic !== null) atCreation("epic", issue.epic);
  if (issue.estimate !== null) atCreation("estimate", String(issue.estimate));
  if (issue.due !== null) atCreation("due", issue.due);
  if (issue.status !== "todo") {
    events.push({
      seq: seq++,
      field: "status",
      old_value: "todo",
      new_value: issue.status,
      ts: issue.updated,
      author: issue.assignees[0] ?? issue.reporter ?? "unknown",
      commit_sha: "cd3ef45",
    });
  }
  return events;
}

function eventsFor(issue: IssueDto): FieldEventDto[] {
  let events = EVENTS.get(issue.id);
  if (!events) {
    events = initialEvents(issue);
    EVENTS.set(issue.id, events);
  }
  return events;
}

function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function naiveRender(text: string): string {
  return text
    .split(/\n{2,}/)
    .map((block) => `<p>${escapeHtml(block).replaceAll("\n", "<br />")}</p>`)
    .join("");
}

// Mutable so the settings panel can be hand-checked against the mock.
const settings: SettingsDto = {
  layout: "root",
  numbering: "local",
  templates: ["default", "bug", "story", "spike"],
};

// §13 pages — a few per root so the docs rail, the editor and delete can
// all be hand-checked against the mock. `updated_ms` values are fixed so
// the relative timestamps in the rail are stable between reloads.
const DAY_MS = 24 * 60 * 60 * 1000;
const MOCK_NOW = Date.parse("2026-08-19T12:00:00Z");
function daysAgo(days: number): number {
  return MOCK_NOW - days * DAY_MS;
}

const DOC_BODIES = new Map<string, string>([
  [
    "docs/architecture.md",
    "# Architecture\n\nThe repo is the database: issues are Markdown files, SQLite is a disposable index, and every write is one commit.\n\n- Local-first, single writer\n- `dit fmt` owns the canonical formatting\n- The merge driver never silently drops a side\n",
  ],
  [
    "docs/adr-0010-doc-editor-api.md",
    "# ADR 0010 — the doc editor\n\nReads walk the file tree; writes go through `Transaction` like every other change. Paths are validated by `DocPath` in the pure core.\n",
  ],
  [
    "notes/2026-08-19-standup.md",
    "# Standup — Aug 19\n\n- board route fixed (parseHash was missing the branch)\n- docs editor API merged\n- next: rename support needs `git mv` semantics\n",
  ],
  [
    "epics/editor-loop.md",
    "# Epic — the editing loop\n\nCapture fast, triage honestly, never leave the keyboard.\n\n## Done\n\n- command palette\n- body editor\n\n## Open\n\n- docs editor\n",
  ],
]);

const DOC_ENTRIES: DocEntryDto[] = [
  { path: "docs/architecture.md", updated_ms: daysAgo(2), bytes: 320 },
  { path: "docs/adr-0010-doc-editor-api.md", updated_ms: daysAgo(1), bytes: 214 },
  { path: "epics/editor-loop.md", updated_ms: daysAgo(6), bytes: 268 },
  { path: "notes/2026-08-19-standup.md", updated_ms: daysAgo(0), bytes: 196 },
].sort((a, b) => (a.path < b.path ? -1 : 1));

/** The DocPath rules the server enforces, mirrored so a bad path fails in
 *  dev exactly the way production fails it. */
function mockDocPathError(path: string): string | null {
  const root = path.split("/")[0] ?? "";
  if (!["docs", "notes", "epics", "changelogs"].includes(root)) {
    return `\`${path}\` does not start with one of docs/, notes/, epics/, changelogs/`;
  }
  if (!path.endsWith(".md")) return `\`${path}\` is not a .md page`;
  if (path.includes("..") || path.startsWith("/")) return `\`${path}\` is not a path the editor may write`;
  return null;
}

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function notFound(): Response {
  return jsonResponse({ error: "no such issue" }, 404);
}

function findIssue(idOrRef: string): IssueDto | undefined {
  return ISSUES.find((issue) => issue.id === idOrRef || issue.short_ref === idOrRef);
}

export function installMockApi(): void {
  // The mock has no server to hand out a real token; invent one so the gate
  // opens and every request carries the header shape the real API expects.
  if (getToken() === null) setToken("dev-mock-token");

  const realFetch = window.fetch.bind(window);

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = new URL(input instanceof Request ? input.url : String(input), window.location.origin);
    const path = url.pathname;
    const method = (input instanceof Request ? input.method : init?.method ?? "GET").toUpperCase();
    const rawBody = typeof init?.body === "string" ? init.body : null;
    const body = rawBody ? (JSON.parse(rawBody) as Record<string, unknown>) : {};
    const set = (body.set ?? {}) as Record<string, unknown>;

    if (!path.startsWith("/api/")) return realFetch(input, init);

    if (path === "/api/status" && method === "GET") {
      const status: StatusInfo = {
        ok: true,
        version: "0.1.0-mock",
        repo: "/home/dev/example",
        branch: "main",
        head: "9f8e7d6c5b4a3f2e1d0c",
        dirty: false,
        me: ME,
      };
      return jsonResponse(status);
    }

    if (path === "/api/schema" && method === "GET") {
      return jsonResponse({
        workflow: {
          statuses: STATUSES,
          transitions: [{ from: ["todo"], to: "in_progress" }],
          derived: [],
        },
      } satisfies SchemaDto);
    }

    if (path === "/api/board" && method === "GET") {
      const board: BoardDto = {
        columns: STATUSES.map((status) => ({
          id: status.id,
          label: status.label,
          wip_limit: status.wip_limit ?? null,
          issues: ISSUES.filter((issue) => issue.status === status.id).map((issue) => ({
            id: issue.id,
            short_ref: issue.short_ref,
            number: issue.number,
            title: issue.title,
            priority: issue.priority,
            type: issue.type,
            assignees: issue.assignees,
            labels: issue.labels,
            estimate: issue.estimate,
            updated: issue.updated,
          })),
        })),
      };
      return jsonResponse(board);
    }

    if (path === "/api/issues" && method === "GET") {
      const q = url.searchParams.get("q") ?? "";
      const limit = Number(url.searchParams.get("limit") ?? ISSUES.length);
      const offset = Number(url.searchParams.get("offset") ?? 0);
      // Parse errors surface as 400 with the message, same shape as the
      // server — the search view's warn banner depends on it.
      let items: IssueDto[];
      try {
        items = runDql(q, ISSUES);
      } catch (error) {
        return jsonResponse({ error: error instanceof Error ? error.message : String(error) }, 400);
      }
      return jsonResponse({ total: items.length, items: items.slice(offset, offset + limit) });
    }

    if (path === "/api/issues" && method === "POST") {
      const title = String(body.title ?? "");
      if (title.trim().length === 0) return jsonResponse({ error: "title must not be empty" }, 400);
      const n = ISSUES.length + 1;
      const now = new Date().toISOString();
      const created: IssueDto = {
        id: `01JMOCK${String(n).padStart(4, "0")}ULID`,
        short_ref: `DIT-${n}`,
        number: n,
        title: title.trim(),
        type: (body.type as IssueType) ?? "task",
        status: (body.status as string) ?? "todo",
        priority: (body.priority as Priority | null) ?? null,
        reporter: ME,
        assignees: (body.assignees as string[]) ?? [],
        labels: (body.labels as string[]) ?? [],
        epic: null,
        estimate: null,
        sprint: null,
        due: null,
        created: now,
        updated: now,
        body: (body.body as string) ?? "",
        body_html: naiveRender((body.body as string) ?? ""),
      };
      ISSUES.push(created);
      eventsFor(created);
      return jsonResponse(created, 201);
    }

    const issueMatch = path.match(/^\/api\/issues\/([^/]+)(\/body|\/comments|\/history)?$/);
    if (issueMatch) {
      const issue = findIssue(decodeURIComponent(issueMatch[1] ?? ""));
      if (!issue) return notFound();
      const sub = issueMatch[2];

      if (sub === undefined && method === "GET") return jsonResponse(issue);

      if (sub === undefined && method === "PATCH") {
        const now = new Date().toISOString();
        const events = eventsFor(issue);
        for (const [key, value] of Object.entries(set)) {
          const record = issue as unknown as Record<string, unknown>;
          const before = record[key];
          record[key] = value;
          if (before !== value) {
            events.push({
              seq: events.length + 1,
              field: key,
              old_value: before === null || before === undefined ? null : String(before),
              new_value: value === null || value === undefined ? null : String(value),
              ts: now,
              author: ME,
              commit_sha: "ff00ee11",
            });
          }
        }
        issue.updated = now;
        return jsonResponse(issue);
      }

      if (sub === "/body" && method === "PUT") {
        const text = String(body.body ?? "");
        issue.body = text;
        issue.body_html = naiveRender(text);
        issue.updated = new Date().toISOString();
        return jsonResponse(issue);
      }

      if (sub === "/comments" && method === "GET") {
        return jsonResponse(COMMENTS.get(issue.id) ?? []);
      }

      if (sub === "/comments" && method === "POST") {
        const text = String(body.body ?? "");
        if (text.trim().length === 0) return jsonResponse({ error: "comment body required" }, 400);
        const comment: CommentDto = {
          id: `${issue.id}-c${(COMMENTS.get(issue.id)?.length ?? 0) + 1}`,
          issue_id: issue.id,
          author: ME,
          created: new Date().toISOString(),
          body: text,
          body_html: naiveRender(text),
        };
        const existing = COMMENTS.get(issue.id) ?? [];
        COMMENTS.set(issue.id, [...existing, comment]);
        return jsonResponse(comment, 201);
      }

      if (sub === "/history" && method === "GET") {
        // Without `field`, every field's events arrive merged, ordered by
        // seq — the order they happened, which is what the timeline shows.
        const field = url.searchParams.get("field");
        const events = field ? eventsFor(issue).filter((event) => event.field === field) : eventsFor(issue);
        return jsonResponse(events);
      }
    }

    if (path === "/api/settings" && method === "GET") {
      return jsonResponse(settings);
    }

    if (path === "/api/settings" && method === "PUT") {
      // The mock only pretends: a layout flip updates the state it reports,
      // without moving anything.
      if (typeof body.layout === "string") settings.layout = body.layout as Layout;
      if (typeof body.numbering === "string") settings.numbering = body.numbering as NumberingPolicy;
      return jsonResponse(settings);
    }

    if (path === "/api/markdown/render" && method === "POST") {
      return jsonResponse({ html: naiveRender(String(body.text ?? "")) });
    }

    if (path === "/api/docs" && method === "GET") {
      return jsonResponse(DOC_ENTRIES);
    }

    const docMatch = path.match(/^\/api\/docs\/(.+)$/);
    if (docMatch) {
      const docPath = decodeURIComponent(docMatch[1] ?? "");
      if (method === "GET") {
        const body = DOC_BODIES.get(docPath);
        if (body === undefined) {
          return jsonResponse({ error: `no page matches \`${docPath}\`` }, 404);
        }
        const page: DocBodyDto = { path: docPath, body };
        return jsonResponse(page);
      }
      if (method === "PUT") {
        const problem = mockDocPathError(docPath);
        if (problem) return jsonResponse({ error: problem }, 400);
        const text = String(body.body ?? "");
        DOC_BODIES.set(docPath, text);
        const existing = DOC_ENTRIES.find((entry) => entry.path === docPath);
        if (existing) {
          existing.updated_ms = Date.now();
          existing.bytes = text.length;
        } else {
          DOC_ENTRIES.push({ path: docPath, updated_ms: Date.now(), bytes: text.length });
          DOC_ENTRIES.sort((a, b) => (a.path < b.path ? -1 : 1));
        }
        const page: DocBodyDto = { path: docPath, body: text };
        return jsonResponse(page);
      }
      if (method === "DELETE") {
        if (!DOC_BODIES.has(docPath)) {
          return jsonResponse({ error: `no page matches \`${docPath}\`` }, 404);
        }
        DOC_BODIES.delete(docPath);
        const index = DOC_ENTRIES.findIndex((entry) => entry.path === docPath);
        if (index >= 0) DOC_ENTRIES.splice(index, 1);
        return new Response(null, { status: 204 });
      }
    }

    return jsonResponse({ error: `mock does not implement ${method} ${path}` }, 404);
  };
}
