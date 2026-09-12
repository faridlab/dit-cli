// The three states every data view must handle, as components so no view
// forgets one. Dots instead of spinners: this tool is calm, not busy.

import { AlertTriangle, Loader2, RotateCw } from "lucide-react";
import { cn } from "../lib/cn";

export function Loading({ label = "Loading…", className }: { label?: string; className?: string }) {
  return (
    <div className={cn("flex items-center gap-2 p-6 text-sm text-muted", className)}>
      <Loader2 className="size-4 animate-spin" aria-hidden />
      <span>{label}</span>
    </div>
  );
}

export function ErrorBox({
  error,
  onRetry,
  title = "Something went wrong",
  tone = "error",
}: {
  error: unknown;
  onRetry?: () => void;
  title?: string;
  /** `warn` is for correctable input feedback (a DQL parse error); errors
   *  stay red because something actually failed. */
  tone?: "error" | "warn";
}) {
  const message = error instanceof Error ? error.message : String(error);
  const box =
    tone === "warn"
      ? "border-warn-line bg-warn-bg text-warn-text"
      : "border-crit-line bg-crit-bg text-crit-text";
  const messageTone = tone === "warn" ? "text-warn-text-dim" : "text-crit-text-dim";
  return (
    <div role="alert" className={cn("m-4 rounded-md border p-4 text-sm", box)}>
      <div className="flex items-center gap-2 font-medium">
        <AlertTriangle className="size-4" aria-hidden />
        {title}
      </div>
      <p className={cn("mt-1 font-mono text-xs", messageTone)}>{message}</p>
      {onRetry ? (
        <button
          type="button"
          onClick={onRetry}
          className="mt-3 flex items-center gap-1.5 rounded border border-ctl px-2 py-1 text-xs text-ink-2 hover:border-dim hover:text-ink"
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
      <p className="text-sm text-ink-2">{title}</p>
      {hint ? <p className="text-xs text-faint">{hint}</p> : null}
    </div>
  );
}
