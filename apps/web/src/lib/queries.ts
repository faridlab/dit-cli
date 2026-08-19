// TanStack Query wiring. Query keys are declared once so cache reads and
// invalidations cannot disagree about the shape of a key. The WebSocket
// invalidates everything on "index_updated"; per-mutation invalidations
// below are the fine-grained fallback between events.

import {
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { toast } from "sonner";
import * as api from "./api";
import type { BoardDto, FieldEventDto, FieldPatch, NewIssueInput, SetSettingsInput } from "./types";

export const queryKeys = {
  status: ["status"] as const,
  schema: ["schema"] as const,
  board: ["board"] as const,
  settings: ["settings"] as const,
  issues: (params: { q?: string; limit?: number; offset?: number }) =>
    ["issues", params.q ?? "", params.limit ?? 0, params.offset ?? 0] as const,
  issue: (id: string) => ["issue", id] as const,
  comments: (id: string) => ["comments", id] as const,
  history: (id: string, field?: string) => ["history", id, field ?? ""] as const,
  markdownPreview: (text: string) => ["markdown-preview", text] as const,
};

/** Mark everything a commit can change as stale. The schema is deliberately
 *  absent: workflow.yaml changes only through a hand edit or a server
 *  restart, neither of which a commit notification says anything about. */
export function invalidateWorkspaceData(client: QueryClient) {
  for (const prefix of [
    queryKeys.status,
    ["issues"],
    queryKeys.board,
    ["issue"],
    ["comments"],
    ["history"],
  ]) {
    void client.invalidateQueries({ queryKey: prefix });
  }
}

// The server is the only writer of the repo, so data cannot be stale for
// long — the WS event handles that. Keep a short stale time so palette and
// list views feel instant without serving ancient rows.
const STALE_TIME_MS = 15_000;

export function useStatus() {
  return useQuery({ queryKey: queryKeys.status, queryFn: api.getStatus, staleTime: STALE_TIME_MS });
}

export function useSchema() {
  return useQuery({
    queryKey: queryKeys.schema,
    queryFn: api.getSchema,
    staleTime: 5 * 60_000,
  });
}

export function useBoard() {
  return useQuery({ queryKey: queryKeys.board, queryFn: api.getBoard, staleTime: STALE_TIME_MS });
}

export function useSettings() {
  return useQuery({
    queryKey: queryKeys.settings,
    queryFn: api.getSettings,
    staleTime: STALE_TIME_MS,
  });
}

export function useIssues(
  params: { q?: string; limit?: number; offset?: number } = {},
  enabled = true,
) {
  return useQuery({
    queryKey: queryKeys.issues(params),
    queryFn: () => api.listIssues(params),
    enabled,
    staleTime: STALE_TIME_MS,
    // Keep the previous rows visible while a new query runs — the list and
    // search views must not flash empty on every keystroke or refresh.
    placeholderData: (previous) => previous,
  });
}

export function useIssue(id: string) {
  return useQuery({
    queryKey: queryKeys.issue(id),
    queryFn: () => api.getIssue(id),
    staleTime: STALE_TIME_MS,
  });
}

export function useComments(id: string) {
  return useQuery({
    queryKey: queryKeys.comments(id),
    queryFn: () => api.getComments(id),
    staleTime: STALE_TIME_MS,
  });
}

export function useFieldEvents(id: string, field?: string) {
  return useQuery({
    queryKey: queryKeys.history(id, field),
    queryFn: () => api.getFieldEvents(id, field),
    staleTime: STALE_TIME_MS,
  });
}

/** A field event plus the issue it belongs to. The wire event carries no
 *  issue id (it was fetched from that issue's endpoint), so the merge tags
 *  each one with the id of the query that produced it. */
export type ActivityEvent = FieldEventDto & { issueId: string };

/** The workspace activity feed, derived (never stored): the newest field
 *  events across the given issues, merged and ordered by `seq` descending.
 *  Callers pass the handful of most-recently-updated issue ids so the feed
 *  costs a bounded number of requests. */
export function useActivity(ids: ReadonlyArray<string>, limit = 15) {
  const trimmed = ids.slice(0, 8);
  const results = useQueries({
    queries: trimmed.map((id) => ({
      queryKey: queryKeys.history(id),
      queryFn: () => api.getFieldEvents(id),
      staleTime: STALE_TIME_MS,
    })),
  });
  const events = results
    .flatMap((result, index) => {
      const issueId = trimmed[index];
      if (!issueId) return [];
      return (result.data ?? []).map((event): ActivityEvent => ({ ...event, issueId }));
    })
    .sort((a, b) => b.seq - a.seq)
    .slice(0, limit);
  const pending = results.some((result) => result.isPending);
  const error = results.find((result) => result.error)?.error ?? null;
  return { data: events, isPending: pending, error };
}

export function useMarkdownPreview(text: string, enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.markdownPreview(text),
    queryFn: () => api.renderMarkdown(text),
    enabled,
    // Previews are cheap and throwaway.
    staleTime: 0,
    gcTime: 60_000,
  });
}

function reportError(prefix: string): (error: unknown) => void {
  return (error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    toast.error(`${prefix}: ${message}`);
  };
}

/** Every issue mutation invalidates the issue plus every view that could be
 *  showing it. Cheap (they refetch only if mounted) and never stale. */
function useIssueInvalidator(id: string | null) {
  const client = useQueryClient();
  return () => {
    void client.invalidateQueries({ queryKey: ["issues"] });
    void client.invalidateQueries({ queryKey: queryKeys.board });
    if (id) {
      void client.invalidateQueries({ queryKey: queryKeys.issue(id) });
      void client.invalidateQueries({ queryKey: queryKeys.comments(id) });
      void client.invalidateQueries({ queryKey: ["history", id] });
    }
  };
}

export function usePatchIssue(id: string) {
  const invalidate = useIssueInvalidator(id);
  return useMutation({
    mutationFn: (set: FieldPatch) => api.patchIssue(id, set),
    onSuccess: invalidate,
    onError: reportError("Could not save"),
  });
}

/** Bulk edit from the issues list. Each issue is its own PATCH (one commit
 *  per file on the server), run sequentially so the repo never sees
 *  concurrent writes; one failure leaves the rest applied and reports. */
export function useBulkPatchIssue() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: async (edits: ReadonlyArray<{ id: string; set: FieldPatch }>) => {
      for (const edit of edits) {
        await api.patchIssue(edit.id, edit.set);
      }
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["issues"] });
      void client.invalidateQueries({ queryKey: queryKeys.board });
      void client.invalidateQueries({ queryKey: queryKeys.status });
    },
    onError: reportError("Could not save every issue"),
  });
}

export function usePutIssueBody(id: string) {
  const invalidate = useIssueInvalidator(id);
  return useMutation({
    mutationFn: (body: string) => api.putIssueBody(id, body),
    onSuccess: invalidate,
    onError: reportError("Could not save body"),
  });
}

export function useAddComment(id: string) {
  const invalidate = useIssueInvalidator(id);
  return useMutation({
    mutationFn: (body: string) => api.addComment(id, body),
    onSuccess: invalidate,
    onError: reportError("Could not comment"),
  });
}

export function useCreateIssue() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: NewIssueInput) => api.createIssue(input),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["issues"] });
      void client.invalidateQueries({ queryKey: queryKeys.board });
    },
    onError: reportError("Could not create issue"),
  });
}

/** A settings change can move every file in the workspace (layout) or change
 *  what the next commit contains (numbering), so everything goes stale —
 *  the same sweep the live watcher does. */
export function usePutSettings() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: SetSettingsInput) => api.putSettings(input),
    onSuccess: () => {
      invalidateWorkspaceData(client);
      void client.invalidateQueries({ queryKey: queryKeys.settings });
    },
    // Refusals (dirty tree, same layout) arrive as 409 with their own way
    // out in the message — surface that text verbatim, not a generic prefix.
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });
}

/** Board drag-and-drop: optimistic move in the board cache, PATCH behind it,
 *  roll back on failure. The index_updated event would also fix it, but a
 *  snap-back minutes later is a bug report waiting to happen. */
export function useMoveIssue() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, status }: { id: string; status: string }) =>
      api.patchIssue(id, { status }),
    onMutate: async ({ id, status }) => {
      await client.cancelQueries({ queryKey: queryKeys.board });
      const previous = client.getQueryData<BoardDto>(queryKeys.board);
      if (previous) {
        const moved = previous.columns
          .flatMap((column) => column.issues)
          .find((issue) => issue.id === id);
        if (moved) {
          client.setQueryData<BoardDto>(queryKeys.board, {
            columns: previous.columns.map((column) => {
              const withoutMoved = column.issues.filter((issue) => issue.id !== id);
              return column.id === status
                ? { ...column, issues: [...withoutMoved, moved] }
                : { ...column, issues: withoutMoved };
            }),
          });
        }
      }
      return { previous };
    },
    onError: (error, _vars, context) => {
      if (context?.previous) client.setQueryData(queryKeys.board, context.previous);
      reportError("Could not move issue")(error);
    },
    onSettled: () => {
      void client.invalidateQueries({ queryKey: queryKeys.board });
      void client.invalidateQueries({ queryKey: ["issues"] });
    },
  });
}
