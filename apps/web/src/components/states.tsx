// The three states every data view must handle, as components so no view
// forgets one. Dots instead of spinners: this tool is calm, not busy.

import { AlertTriangle, Loader2, RotateCw } from "lucide-react";
import { cn } from "../lib/cn";

export function Loading({ label = "Loading…", className }: { label?: string; className?: string }) {
  return (
    <div className={cn("flex items-center gap-2 p-6 text-sm text-zinc-500", className)}>
      <Loader2 className="size-4 animate-spin" aria-hidden />
      <span>{label}</span>
    </div>
  );
}

export function ErrorBox({
  error,
  onRetry,
  title = "Something went wrong",
}: {
  error: unknown;
  onRetry?: () => void;
  title?: string;
}) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div role="alert" className="m-4 rounded-md border border-red-900/60 bg-red-950/30 p-4 text-sm">
      <div className="flex items-center gap-2 font-medium text-red-300">
        <AlertTriangle className="size-4" aria-hidden />
        {title}
      </div>
      <p className="mt-1 font-mono text-xs text-red-200/80">{message}</p>
      {onRetry ? (
        <button
          type="button"
          onClick={onRetry}
          className="mt-3 flex items-center gap-1.5 rounded border border-zinc-700 px-2 py-1 text-xs text-zinc-300 hover:border-zinc-500 hover:text-zinc-100"
        >
          <RotateCw className="size-3" aria-hidden />
          Retry
        </button>
      ) : null}
    </div>
  );
}

export function Empty({
  title,
  hint,
  className,
}: {
  title: string;
  hint?: string;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col items-center gap-1 p-10 text-center", className)}>
      <p className="text-sm text-zinc-400">{title}</p>
      {hint ? <p className="text-xs text-zinc-600">{hint}</p> : null}
    </div>
  );
}
