// The only module allowed to call fetch. Every endpoint of the server lives
// here as a typed function, so components never touch URLs or headers and a
// wire-format change is a one-file change.

import { getToken } from "./auth";
import type {
  BoardDto,
  CommentDto,
  FieldEventDto,
  FieldPatch,
  IssueDto,
  IssueListDto,
  NewIssueInput,
  SchemaDto,
  StatusInfo,
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

export function addComment(id: string, body: string): Promise<CommentDto> {
  return request<CommentDto>(`/api/issues/${encodeURIComponent(id)}/comments`, {
    method: "POST",
    body: JSON.stringify({ body }),
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

export function getFieldEvents(id: string, field: string): Promise<FieldEventDto[]> {
  const qs = new URLSearchParams({ field });
  return request<FieldEventDto[]>(
    `/api/issues/${encodeURIComponent(id)}/history?${qs.toString()}`,
  );
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
