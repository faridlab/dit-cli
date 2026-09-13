// Bottom strip: where you are (repo / branch / head), whether the tree is
// clean, how big the workspace is, who you are, the version, and whether
// live updates are connected. Everything a person glances at before
// trusting what they see.

import { Check, CircleAlert, Clock, GitBranch, Info } from "lucide-react";
import { toast } from "sonner";
import type { ConnectionState } from "../lib/events";
import { useDocs, useIssues, useStatus } from "../lib/queries";
import { shortSha } from "../lib/format";
import { cn } from "../lib/cn";
import { MenuButton, type MenuItem } from "./chrome";

function ConnBadge({ state }: { state: ConnectionState }) {
  const label =
    state === "live" ? "live" : state === "connecting" ? "connecting" : state === "retrying" ? "reconnecting" : "offline";
  return (
    <span
      className={cn("it", state === "live" && "live")}
      style={state === "live" ? undefined : { color: state === "off" ? "var(--faint)" : "var(--warn)" }}
      title={
        state === "live"
          ? "Receiving live updates from the workspace watcher"
          : "Live updates disconnected — data refreshes on reconnect"
      }
    >
      {label}
    </span>
  );
}

async function copy(text: string, label: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    toast(`${label} — ${text}`);
  } catch {
    toast(`${label}: ${text}`);
  }
}

export function StatusBar({
  conn,
  workspaceMenu,
  onOpenSettings,
  onNotes,
}: {
  conn: ConnectionState;
  workspaceMenu: MenuItem[];
  onOpenSettings: () => void;
  onNotes: () => void;
}) {
  const status = useStatus();
  // Size of the workspace, not of any filtered list.
  const all = useIssues({ limit: 1 });
  const docs = useDocs();

  if (status.isError) {
    return (
      <footer className="status">
        <span className="it" style={{ color: "var(--crit)" }}>
          <CircleAlert aria-hidden />
          server unreachable
        </span>
        <button type="button" className="it underline decoration-dotted" onClick={() => void status.refetch()}>
          retry
        </button>
      </footer>
    );
  }

  const data = status.data;
  const repo = data ? data.repo.split("/").filter(Boolean).pop() ?? data.repo : "…";
  const head = data ? shortSha(data.head) : "…";

  return (
    <footer className="status">
      <MenuButton items={workspaceMenu}>
        <button type="button" className="it" title={data?.repo ?? "Workspace menu"}>
          <b style={{ color: "var(--ink-2)", fontWeight: 500 }}>{repo}</b>
        </button>
      </MenuButton>
      {data ? (
        <button type="button" className="it" title="Copy HEAD" onClick={() => void copy(data.head ?? "", "HEAD copied")}>
          <GitBranch aria-hidden />
          {data.branch}
          <span style={{ color: "var(--faint)" }}>@</span>
          {head}
        </button>
      ) : null}
      {data ? (
        <span
          className="it"
          style={{ color: data.dirty ? "var(--warn)" : "var(--done)" }}
          title={data.dirty ? "The working tree has uncommitted changes" : "No uncommitted changes"}
        >
          {data.dirty ? <Clock aria-hidden /> : <Check aria-hidden />}
          {data.dirty ? "dirty" : "clean"}
        </span>
      ) : null}
      {all.data && docs.data ? (
        <span className="it" style={{ color: "var(--faint)" }}>
          {all.data.total} {all.data.total === 1 ? "issue" : "issues"} · {docs.data.length}{" "}
          {docs.data.length === 1 ? "page" : "pages"}
        </span>
      ) : null}
      <span className="sp" />
      {data?.me ? (
        <button type="button" className="it" title="The alias your writes are attributed to — change it in Settings" onClick={onOpenSettings}>
          {data.me}
        </button>
      ) : null}
      {data ? <span className="it">v{data.version}</span> : null}
      <ConnBadge state={conn} />
      <button type="button" className="it" onClick={onNotes} title="Notes: how this workbench works, and the keys">
        <Info aria-hidden />
        notes
      </button>
    </footer>
  );
}
