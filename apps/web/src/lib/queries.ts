// TanStack Query wiring. Query keys are declared once so cache reads and
// invalidations cannot disagree about the shape of a key. The WebSocket
// invalidates everything on "index_updated"; per-mutation invalidations
// below are the fine-grained fallback between events.

import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { toast } from "sonner";
import * as api from "./api";
import { openQuery } from "./dql";
import { POOL_LIMIT } from "./lists";
import type { BoardDto, FieldPatch, NewIssueInput, ReleasePatchInput, SetSettingsInput } from "./types";

export const queryKeys = {
  status: ["status"] as const,
  schema: ["schema"] as const,
  board: ["board"] as const,
  settings: ["settings"] as const,
  docs: ["docs"] as const,
  doc: (path: string) => ["doc", path] as const,
  issues: (params: { q?: string; limit?: number; offset?: number }) =>
    ["issues", params.q ?? "", params.limit ?? 0, params.offset ?? 0] as const,
  issue: (id: string) => ["issue", id] as const,
  comments: (id: string) => ["comments", id] as const,
  history: (id: string, field?: string) => ["history", id, field ?? ""] as const,
  activity: (params: { beforeSeq?: number | null; limit?: number }) =>
    ["activity", params.beforeSeq ?? null, params.limit ?? null] as const,
  activitySummary: (params: { seq?: number | null; days?: number }) =>
    ["activity-summary", params.seq ?? null, params.days ?? null] as const,
  markdownPreview: (text: string) => ["markdown-preview", text] as const,
  workspaceComments: (limit: number) => ["workspace-comments", limit] as const,
  releases: ["releases"] as const,
  flows: ["flows"] as const,
  flowBoard: (name: string) => ["flow-board", name] as const,
  morse: ["morse"] as const,
};

/** Mark everything a commit can change as stale. The schema is deliberately
 *  absent: workflow.yaml changes only through a hand edit or a server
 *  restart, neither of which a commit notification says anything about. */
export function invalidateWorkspaceData(client: QueryClient) {
  for (const prefix of [
    ["workspace-comments"] as const,
    queryKeys.releases,
    queryKeys.status,
    ["issues"],
    queryKeys.board,
    queryKeys.flows,
    ["flow-board"],
    // A commit can change a spec, a fence, or the config that registers
    // either — all three change what Morse reports.
    queryKeys.morse,
    ["issue"],
    ["comments"],
    ["history"],
    ["activity"],
    ["activity-summary"],
    ["docs"],
    ["doc"],
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

/** Morse (§20): the derived catalogue and every scenario's verdict. */
export function useMorse() {
  return useQuery({
    queryKey: queryKeys.morse,
    queryFn: api.getMorse,
    staleTime: STALE_TIME_MS,
  });
}

/** Fire one scenario, then refresh the report so its verdict and its last
 *  run come from the same place the CLI reads. */
export function useRunMorse() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (scenario: string) => api.runMorse(scenario),
    onSettled: () => client.invalidateQueries({ queryKey: queryKeys.morse }),
  });
}

/** Every flow with its member count (ADR 0019). */
export function useFlows() {
  return useQuery({
    queryKey: queryKeys.flows,
    queryFn: api.getFlows,
    staleTime: STALE_TIME_MS,
  });
}

/** One flow as a diagram (ADR 0019); `__all__` is the union. Derived on the
 *  server per call, so a short stale time plus the live event keeps it
 *  moving without polling. */
export function useFlowBoard(name: string) {
  return useQuery({
    queryKey: queryKeys.flowBoard(name),
    queryFn: () => api.getFlowBoard(name),
    staleTime: STALE_TIME_MS,
  });
}

export function useSettings() {
  return useQuery({
    queryKey: queryKeys.settings,
    queryFn: api.getSettings,
    staleTime: STALE_TIME_MS,
  });
}

// -- docs (ADR 0010) ------------------------------------------------------------

/** `enabled` false keeps occasional surfaces (the palette) from fetching
 *  until they are actually open. */
export function useDocs(enabled = true) {
  return useQuery({
    queryKey: queryKeys.docs,
    queryFn: api.listDocs,
    enabled,
    staleTime: STALE_TIME_MS,
  });
}

/** `path` null = the docs landing state (no page selected), query disabled. */
export function useDoc(path: string | null) {
  return useQuery({
    queryKey: queryKeys.doc(path ?? ""),
    queryFn: () => api.getDoc(path ?? ""),
    enabled: path !== null,
    staleTime: STALE_TIME_MS,
  });
}

/** Save (create or overwrite) a page. The response is the formatted body
 *  that landed, cached directly so the editor snaps to the canonical form. */
export function usePutDoc() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ path, body }: { path: string; body: string }) => api.putDoc(path, body),
    onSuccess: (saved) => {
      void client.setQueryData(queryKeys.doc(saved.path), saved);
      void client.invalidateQueries({ queryKey: queryKeys.docs });
    },
    // A bad path is a 400 with a message that says exactly which segment is
    // wrong — surface it verbatim so the inline form can show it.
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });
}

export function useDeleteDoc() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => api.deleteDoc(path),
    onSuccess: (_data, path) => {
      client.removeQueries({ queryKey: queryKeys.doc(path) });
      void client.invalidateQueries({ queryKey: queryKeys.docs });
    },
    onError: reportError("Could not delete page"),
  });
}

/** Move or rename a page. The old path's cache dies with the path; the new
 *  one is invalidated so it refetches the relocated bytes. A refusal (409 —
 *  target occupied) surfaces verbatim: it names the path in the way. */
export function useMoveDoc() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ from, to }: { from: string; to: string }) => api.moveDoc(from, to),
    onSuccess: (_data, { from, to }) => {
      client.removeQueries({ queryKey: queryKeys.doc(from) });
      void client.invalidateQueries({ queryKey: queryKeys.doc(to) });
      void client.invalidateQueries({ queryKey: queryKeys.docs });
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
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

/** The open pool every derived list reads (Inbox, Next actions, filters,
 *  the sidebar counts): one bounded query, one cache entry, so the sidebar
 *  and the view it describes can never disagree. */
export function useOpenPool() {
  const schema = useSchema();
  const open = openQuery(schema.data?.workflow.statuses);
  return useIssues(open ? { q: open, limit: POOL_LIMIT } : { limit: POOL_LIMIT });
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

/** One page of the workspace's field history. The cursor is part of the key,
 *  so paging never overwrites the page behind it. */
export function useActivity(params: { beforeSeq?: number | null; limit?: number } = {}) {
  return useQuery({
    queryKey: queryKeys.activity(params),
    queryFn: () => api.getActivity(params),
    staleTime: STALE_TIME_MS,
    placeholderData: (previous) => previous,
  });
}

/** The board at a point in history, next to now. Recomputed server-side on
 *  every call — there is nothing cached in the workspace to go stale. */
export function useActivitySummary(params: { seq?: number | null; days?: number } = {}) {
  return useQuery({
    queryKey: queryKeys.activitySummary(params),
    queryFn: () => api.getActivitySummary(params),
    staleTime: STALE_TIME_MS,
    placeholderData: (previous) => previous,
  });
}

/** Recent comments across the workspace, for the Timeline. */
export function useWorkspaceComments(limit = 200) {
  return useQuery({
    queryKey: queryKeys.workspaceComments(limit),
    queryFn: () => api.listWorkspaceComments(limit),
    staleTime: STALE_TIME_MS,
    placeholderData: (previous) => previous,
  });
}

export function useReleases() {
  return useQuery({
    queryKey: queryKeys.releases,
    queryFn: api.listReleases,
    staleTime: STALE_TIME_MS,
  });
}

export function usePatchRelease() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ version, patch }: { version: string; patch: ReleasePatchInput }) =>
      api.patchRelease(version, patch),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: queryKeys.releases });
    },
    onError: reportError("Could not update the release"),
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
function useIssueInvalidator() {
  const client = useQueryClient();
  return () => {
    void client.invalidateQueries({ queryKey: ["issues"] });
    void client.invalidateQueries({ queryKey: queryKeys.board });
    // An issue is reachable by its id and by its short ref, and the views
    // use whichever the route carries — so refresh the whole prefix rather
    // than the one spelling this mutation happened to know.
    void client.invalidateQueries({ queryKey: ["issue"] });
    void client.invalidateQueries({ queryKey: ["comments"] });
    void client.invalidateQueries({ queryKey: ["history"] });
  };
}

export function usePatchIssue(id: string) {
  const invalidate = useIssueInvalidator();
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
  const invalidate = useIssueInvalidator();
  return useMutation({
    mutationFn: (body: string) => api.putIssueBody(id, body),
    onSuccess: invalidate,
    onError: reportError("Could not save body"),
  });
}

export function useAddComment(id: string) {
  const invalidate = useIssueInvalidator();
  return useMutation({
    mutationFn: (input: { body: string; replyTo?: string | null }) =>
      api.addComment(id, input.body, input.replyTo ?? null),
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
