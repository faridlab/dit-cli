// The only module allowed to call fetch. Every endpoint of the server lives
// here as a typed function, so components never touch URLs or headers and a
// wire-format change is a one-file change.

import { getToken } from "./auth";
import type {
  BoardDto,
  CommentDto,
  DocBodyDto,
  DocEntryDto,
  ActivityPageDto,
  ActivitySummaryDto,
  FieldEventDto,
  FieldPatch,
  IssueDto,
  IssueListDto,
  MorseCreateDto,
  MorseEnvsDto,
  MorseReportDto,
  MorseRunDto,
  MorseScenarioDetailDto,
  MorseSendDto,
  MorseStepDto,
  NewIssueInput,
  SchemaDto,
  SetSettingsInput,
  SettingsDto,
  StatusInfo,
  ReleaseDto,
  ReleasePatchInput,
  FlowBoardDto,
  FlowSummaryDto,
  WorkspaceCommentDto,
} from "./types";

export class ApiError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  const hasBody = init?.body !== undefined && init.body !== null;
  if (hasBody) headers["Content-Type"] = "application/json";

  let res: Response;
  try {
    res = await fetch(path, { ...init, headers });
  } catch (cause) {
    // Network refusal: server not running, or the tab lost the connection.
    // Throwing a plain message keeps every error path a string for the UI.
    throw new ApiError(
      cause instanceof Error ? `Network error: ${cause.message}` : "Network error",
      0,
    );
  }

  if (!res.ok) {
    // The server's error contract is a JSON body { error: "human-readable" }.
    let message = `Request failed (${res.status})`;
    try {
      const body = (await res.json()) as { error?: unknown };
      if (typeof body.error === "string" && body.error.length > 0) message = body.error;
    } catch {
      // Non-JSON error body — keep the generic message.
    }
    throw new ApiError(message, res.status);
  }

  // A 204 says "done, nothing to say" — DELETE is the only caller today.
  if (res.status === 204) return undefined as T;

  return (await res.json()) as T;
}

// ---------------------------------------------------------------- endpoints

export function getStatus(): Promise<StatusInfo> {
  return request<StatusInfo>("/api/status");
}

export function getSchema(): Promise<SchemaDto> {
  return request<SchemaDto>("/api/schema");
}

export interface IssueListParams {
  q?: string;
  limit?: number;
  offset?: number;
}

export function listIssues(params: IssueListParams = {}): Promise<IssueListDto> {
  const search = new URLSearchParams();
  if (params.q) search.set("q", params.q);
  if (params.limit !== undefined) search.set("limit", String(params.limit));
  if (params.offset !== undefined) search.set("offset", String(params.offset));
  const qs = search.toString();
  return request<IssueListDto>(`/api/issues${qs ? `?${qs}` : ""}`);
}

export function getIssue(id: string): Promise<IssueDto> {
  return request<IssueDto>(`/api/issues/${encodeURIComponent(id)}`);
}

export function patchIssue(id: string, set: FieldPatch): Promise<IssueDto> {
  return request<IssueDto>(`/api/issues/${encodeURIComponent(id)}`, {
    method: "PATCH",
    body: JSON.stringify({ set }),
  });
}

export function putIssueBody(id: string, body: string): Promise<IssueDto> {
  return request<IssueDto>(`/api/issues/${encodeURIComponent(id)}/body`, {
    method: "PUT",
    body: JSON.stringify({ body }),
  });
}

export function getComments(id: string): Promise<CommentDto[]> {
  return request<CommentDto[]>(`/api/issues/${encodeURIComponent(id)}/comments`);
}

export function addComment(
  id: string,
  body: string,
  replyTo?: string | null,
): Promise<CommentDto> {
  return request<CommentDto>(`/api/issues/${encodeURIComponent(id)}/comments`, {
    method: "POST",
    body: JSON.stringify({ body, ...(replyTo ? { reply_to: replyTo } : {}) }),
  });
}

export function createIssue(input: NewIssueInput): Promise<IssueDto> {
  return request<IssueDto>("/api/issues", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function getBoard(): Promise<BoardDto> {
  return request<BoardDto>("/api/board");
}

/** Morse's read model (§20): the derived catalogue and each scenario's
 *  verdict. A read — the server answers from the index and sends nothing. */
export function getMorse(): Promise<MorseReportDto> {
  return request<MorseReportDto>("/api/morse");
}

/** Fire one scenario. The one call in the UI that reaches the network, and
 *  it only ever reaches a host this machine already allows — the allowlist
 *  is not editable from the browser (§20.5). */
export function runMorse(scenario: string, env: string | null): Promise<MorseRunDto> {
  const query = env ? `?env=${encodeURIComponent(env)}` : "";
  return request<MorseRunDto>(`/api/morse/run/${encodeURIComponent(scenario)}${query}`, {
    method: "POST",
  });
}

/** Fire one operation as a tab drafted it (ADR 0023). The draft has no field
 *  that could name a host — the server takes method and path from the spec
 *  and the base URL from the spec or this machine. */
export function sendMorse(input: MorseSendDto): Promise<MorseRunDto> {
  return request<MorseRunDto>("/api/morse/send", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

/** This machine's environments by name and the hosts it allows. Variable
 *  values never reach the page. */
export function getMorseEnvs(): Promise<MorseEnvsDto> {
  return request<MorseEnvsDto>("/api/morse/envs");
}

/** Runs and sends kept in the index since its last rebuild, newest first. */
export function getMorseRuns(): Promise<MorseRunDto[]> {
  return request<MorseRunDto[]>("/api/morse/runs");
}

export function getMorseScenario(name: string): Promise<MorseScenarioDetailDto> {
  return request<MorseScenarioDetailDto>(`/api/morse/scenarios/${encodeURIComponent(name)}`);
}

/** Save one step into its fence — replaced by id, or appended. One commit. */
export function saveMorseStep(scenario: string, step: MorseStepDto): Promise<MorseScenarioDetailDto> {
  return request<MorseScenarioDetailDto>(
    `/api/morse/scenarios/${encodeURIComponent(scenario)}/steps`,
    { method: "PUT", body: JSON.stringify(step) },
  );
}

/** Start a scenario with one step, as a new fence at the end of a document. */
export function createMorseScenario(input: MorseCreateDto): Promise<MorseScenarioDetailDto> {
  return request<MorseScenarioDetailDto>("/api/morse/scenarios", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

/** Every flow with its member count (ADR 0019). */
export function getFlows(): Promise<FlowSummaryDto[]> {
  return request<FlowSummaryDto[]>("/api/flow");
}

/** One flow as a diagram; `__all__` is the union of every flow. */
export function getFlowBoard(name: string): Promise<FlowBoardDto> {
  return request<FlowBoardDto>(`/api/flow/${encodeURIComponent(name)}`);
}

// -- docs (ADR 0010) ----------------------------------------------------------

/** The path is `docs/…`-shaped with slug-safe segments, so encoding each
 *  segment separately keeps the slashes the wildcard route needs. */
function docUrl(path: string): string {
  return `/api/docs/${path.split("/").map(encodeURIComponent).join("/")}`;
}

export function listDocs(): Promise<DocEntryDto[]> {
  return request<DocEntryDto[]>("/api/docs");
}

export function getDoc(path: string): Promise<DocBodyDto> {
  return request<DocBodyDto>(docUrl(path));
}

/** One save is one commit; the response carries the formatted body that
 *  actually landed, so the editor can show the canonical form. */
export function putDoc(path: string, body: string): Promise<DocBodyDto> {
  return request<DocBodyDto>(docUrl(path), {
    method: "PUT",
    body: JSON.stringify({ body }),
  });
}

export function deleteDoc(path: string): Promise<void> {
  return request<void>(docUrl(path), { method: "DELETE" });
}

/** Move or rename a page. One commit, recorded by git as a rename so the
 *  history follows; a 409 means the target already exists and nothing
 *  moved. */
export function moveDoc(from: string, to: string): Promise<void> {
  return request<void>("/api/docs/move", {
    method: "POST",
    body: JSON.stringify({ from, to }),
  });
}

export function getSettings(): Promise<SettingsDto> {
  return request<SettingsDto>("/api/settings");
}

/** One field at a time in practice: a layout change is the guided migration
 * (git mv + reindex, one commit), a numbering change is a config flip. */
export function putSettings(input: SetSettingsInput): Promise<SettingsDto> {
  return request<SettingsDto>("/api/settings", {
    method: "PUT",
    body: JSON.stringify(input),
  });
}

/** Field history in `seq` order. `field` omitted = every field, which powers
 *  the detail history timeline and the Home activity feed in one request. */
/** One page of the workspace's field history, newest first. `beforeSeq` is
 *  the cursor from the previous page. */
export function getActivity(params: { beforeSeq?: number | null; limit?: number } = {}): Promise<ActivityPageDto> {
  const qs = new URLSearchParams();
  if (params.beforeSeq !== undefined && params.beforeSeq !== null) {
    qs.set("before_seq", String(params.beforeSeq));
  }
  if (params.limit !== undefined) qs.set("limit", String(params.limit));
  const suffix = qs.size > 0 ? `?${qs.toString()}` : "";
  return request<ActivityPageDto>(`/api/activity${suffix}`);
}

/** The board then, the board now, and the difference. `seq` is a position in
 *  the commit graph; absent means now. */
export function getActivitySummary(
  params: { seq?: number | null; days?: number } = {},
): Promise<ActivitySummaryDto> {
  const qs = new URLSearchParams();
  if (params.seq !== undefined && params.seq !== null) qs.set("seq", String(params.seq));
  if (params.days !== undefined) qs.set("days", String(params.days));
  const suffix = qs.size > 0 ? `?${qs.toString()}` : "";
  return request<ActivitySummaryDto>(`/api/activity/summary${suffix}`);
}

/** The most recent comments across the workspace, newest first. */
export function listWorkspaceComments(limit = 200): Promise<WorkspaceCommentDto[]> {
  return request<WorkspaceCommentDto[]>(`/api/comments?limit=${limit}`);
}

export function listReleases(): Promise<ReleaseDto[]> {
  return request<ReleaseDto[]>("/api/releases");
}

/** Move a release's target date or status — one commit to its release.md. */
export function patchRelease(version: string, patch: ReleasePatchInput): Promise<ReleaseDto> {
  return request<ReleaseDto>(`/api/releases/${encodeURIComponent(version)}`, {
    method: "PATCH",
    body: JSON.stringify(patch),
  });
}

export function getFieldEvents(id: string, field?: string): Promise<FieldEventDto[]> {
  const qs = new URLSearchParams();
  if (field) qs.set("field", field);
  const suffix = qs.size > 0 ? `?${qs.toString()}` : "";
  return request<FieldEventDto[]>(`/api/issues/${encodeURIComponent(id)}/history${suffix}`);
}

export function renderMarkdown(text: string): Promise<{ html: string }> {
  return request<{ html: string }>("/api/markdown/render", {
    method: "POST",
    body: JSON.stringify({ text }),
  });
}

// Build the WebSocket URL for live updates. Browsers cannot set headers on a
// WebSocket handshake, so the token rides in the query string for this one
// endpoint only. Same-origin in production; Vite proxies in development.
export function eventsUrl(): string | null {
  const token = getToken();
  if (!token) return null;
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${window.location.host}/api/events?token=${encodeURIComponent(token)}`;
}
