// Development-only fixture server (activated by ?mock=1 in the dev URL).
// It intercepts fetch for /api/* and answers with in-memory data so the UI
// can be hand-checked before the Rust backend exists. This module is only
// reachable through a dead-code-eliminated dev branch — it never ships.
//
// The body_html strings below are authored constants, not sanitized input:
// in production every body_html arrives sanitized from the server's
// markdown renderer — the mock is not and must never become a
// sanitization boundary.

import { setToken, getToken } from "./auth";
import type {
  BoardDto,
  CommentDto,
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

const STATUSES: SchemaDto["workflow"]["statuses"] = [
  { id: "todo", label: "Todo", category: "todo", terminal: false, wip_limit: null },
  { id: "in_progress", label: "In progress", category: "doing", terminal: false, wip_limit: 5 },
  { id: "review", label: "In review", category: "doing", terminal: false, wip_limit: 3 },
  { id: "done", label: "Done", category: "done", terminal: true, wip_limit: null },
];

const PEOPLE = ["farid", "jane", "akira", "sam"];
const TYPES: IssueType[] = ["task", "bug", "story", "spike", "chore"];
const PRIORITIES: Priority[] = ["p0", "p1", "p2", "p3", "p4"];
const LABEL_POOL = ["area:auth", "area:api", "team:core", "ux", "infra"];

// Deterministic pseudo-random data: same fixture set on every reload, so
// screenshots and hand-checks are reproducible.
function seeded(n: number): number {
  const x = Math.sin(n * 999 + 7) * 10000;
  return x - Math.floor(x);
}

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
      "Document deployment mode B",
    ][n % 10]!.concat(` (${n})`),
    type: TYPES[Math.floor(seeded(n + 11) * TYPES.length)] ?? "task",
    status: STATUSES[Math.floor(seeded(n + 21) * STATUSES.length)]?.id ?? "todo",
    priority: seeded(n + 31) > 0.2 ? PRIORITIES[Math.floor(seeded(n + 31) * PRIORITIES.length)] ?? "p2" : null,
    reporter: PEOPLE[Math.floor(seeded(n + 41) * PEOPLE.length)] ?? "farid",
    assignees: seeded(n + 3) > 0.4 ? [PEOPLE[Math.floor(seeded(n + 5) * PEOPLE.length)] ?? "jane"] : [],
    labels: seeded(n + 7) > 0.5 ? [LABEL_POOL[Math.floor(seeded(n + 9) * LABEL_POOL.length)] ?? "ux"] : [],
    epic: null,
    estimate: seeded(n + 13) > 0.5 ? Math.ceil(seeded(n + 15) * 8) : null,
    sprint: null,
    due: null,
    created,
    updated,
    body,
    body_html: body
      ? `<h2>Context</h2><p>Issue <strong>#${n}</strong> body with some <code>markdown</code>.</p><ul><li>one</li><li>two</li></ul>`
      : "",
  };
});

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

function fieldEvents(issue: IssueDto, field: string): FieldEventDto[] {
  if (field !== "status") {
    return [
      {
        seq: 1,
        field,
        old_value: null,
        new_value: issue[field as keyof IssueDto] === null ? null : String(issue[field as keyof IssueDto]),
        ts: issue.created,
        author: issue.reporter ?? "unknown",
        commit_sha: "ab12cd3",
      },
    ];
  }
  return [
    { seq: 1, field, old_value: null, new_value: "todo", ts: issue.created, author: issue.reporter ?? "unknown", commit_sha: "ab12cd3" },
    { seq: 2, field, old_value: "todo", new_value: issue.status, ts: issue.updated, author: issue.assignees[0] ?? issue.reporter ?? "unknown", commit_sha: "cd3ef45" },
  ];
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

function searchIssues(q: string): IssueDto[] {
  // Just enough DQL for the fixture: `title ~ X` / `body ~ X` matching and a
  // bare-word fallback. Full parsing belongs to the server.
  const titleMatch = q.match(/title\s+~\s+(\S+)/i);
  const bodyMatch = q.match(/body\s+~\s+(\S+)/i);
  const needle = titleMatch?.[1] ?? bodyMatch?.[1] ?? q.trim();
  if (needle.length === 0) return ISSUES;
  return ISSUES.filter(
    (issue) =>
      issue.title.toLowerCase().includes(needle.toLowerCase()) ||
      (bodyMatch !== null && issue.body.toLowerCase().includes(needle.toLowerCase())),
  );
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
        me: "farid",
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
      const items = searchIssues(q);
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
        reporter: "farid",
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
        for (const [key, value] of Object.entries(set)) {
          (issue as unknown as Record<string, unknown>)[key] = value;
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
          author: "farid",
          created: new Date().toISOString(),
          body: text,
          body_html: naiveRender(text),
        };
        const existing = COMMENTS.get(issue.id) ?? [];
        COMMENTS.set(issue.id, [...existing, comment]);
        return jsonResponse(comment, 201);
      }

      if (sub === "/history" && method === "GET") {
        const field = url.searchParams.get("field") ?? "status";
        return jsonResponse(fieldEvents(issue, field));
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

    return jsonResponse({ error: `mock does not implement ${method} ${path}` }, 404);
  };
}
