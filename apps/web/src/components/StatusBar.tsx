// Bottom strip: where you are (repo / branch / head), whether the repo is
// dirty, who you are, and whether live updates are connected. Everything a
// person glances at before trusting what they see.

import { CircleAlert, CircleCheck, GitBranch } from "lucide-react";
import type { ConnectionState } from "../lib/events";
import { useStatus } from "../lib/queries";
import { shortSha } from "../lib/format";
import { cn } from "../lib/cn";

function ConnBadge({ state }: { state: ConnectionState }) {
  const label =
    state === "live"
      ? "live"
      : state === "connecting"
        ? "connecting"
        : state === "retrying"
          ? "reconnecting"
          : "offline";
  return (
    <span
      className={cn(
        "flex items-center gap-1.5",
        state === "live" ? "text-emerald-400" : state === "off" ? "text-zinc-500" : "text-amber-400",
      )}
      title={
        state === "live"
          ? "Receiving live updates from the workspace watcher"
          : "Live updates disconnected — data refreshes on reconnect"
      }
    >
      <span
        className={cn(
          "inline-block size-1.5 rounded-full",
          state === "live" ? "bg-emerald-400" : state === "off" ? "bg-zinc-600" : "bg-amber-400",
        )}
      />
      {label}
    </span>
  );
}

export function StatusBar({ conn }: { conn: ConnectionState }) {
  const status = useStatus();

  if (status.isError) {
    return (
      <footer className="flex h-7 items-center gap-3 border-t border-zinc-800 bg-zinc-950 px-3 font-mono text-[11px] text-zinc-500">
        <span className="flex items-center gap-1.5 text-red-400">
          <CircleAlert className="size-3" aria-hidden />
          server unreachable
        </span>
        <button
          type="button"
          className="underline decoration-dotted hover:text-zinc-300"
          onClick={() => void status.refetch()}
        >
          retry
        </button>
      </footer>
    );
  }

  const data = status.data;
  const repo = data ? data.repo.split("/").filter(Boolean).pop() ?? data.repo : "…";

  return (
    <footer className="flex h-7 items-center gap-4 border-t border-zinc-800 bg-zinc-950 px-3 font-mono text-[11px] text-zinc-500">
      <span className="flex items-center gap-1.5" title={data ? data.repo : undefined}>
        <span className="text-zinc-400">{repo}</span>
      </span>
      {data ? (
        <span className="flex items-center gap-1.5">
          <GitBranch className="size-3" aria-hidden />
          {data.branch}
          <span className="text-zinc-600">@</span>
          {shortSha(data.head)}
        </span>
      ) : null}
      {data?.dirty ? (
        <span className="flex items-center gap-1 text-amber-400" title="The repo has uncommitted changes">
          <CircleAlert className="size-3" aria-hidden />
          dirty
        </span>
      ) : data && !data.dirty ? (
        <span className="flex items-center gap-1" title="No uncommitted changes">
          <CircleCheck className="size-3 text-emerald-600" aria-hidden />
          clean
        </span>
      ) : null}
      <span className="ml-auto flex items-center gap-4">
        {data?.me ? (
          <span title="the alias your writes are attributed to">
            <span className="text-zinc-400">{data.me}</span>
          </span>
        ) : null}
        {data ? <span>v{data.version}</span> : null}
        <ConnBadge state={conn} />
      </span>
    </footer>
  );
}
