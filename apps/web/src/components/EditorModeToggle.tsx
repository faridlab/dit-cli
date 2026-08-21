// The rich/source(/preview) segmented control shared by the issue body and
// the doc pages. Rich is the default because writing is the common case;
// source mode is the escape hatch for what the rich editor refuses to load
// (conflict markers) or shows only as bytes (raw HTML, image alt text).

import { Code2, Eye, Pencil } from "lucide-react";
import { cn } from "../lib/cn";

export type EditorMode = "rich" | "source" | "preview";

const MODES = [
  { id: "rich" as const, label: "Rich", Icon: Pencil },
  { id: "source" as const, label: "Source", Icon: Code2 },
  { id: "preview" as const, label: "Preview", Icon: Eye },
];

export function EditorModeToggle({
  mode,
  onChange,
  showPreview,
}: {
  mode: EditorMode;
  onChange: (mode: EditorMode) => void;
  showPreview: boolean;
}) {
  return (
    <div className="flex overflow-hidden rounded-md border border-zinc-700" role="group" aria-label="Editor mode">
      {MODES.filter((m) => m.id !== "preview" || showPreview).map(({ id, label, Icon }, index) => (
        <button
          key={id}
          type="button"
          onClick={() => onChange(id)}
          aria-pressed={mode === id}
          className={cn(
            "flex items-center gap-1 px-2 py-1 text-[11px]",
            index > 0 && "border-l border-zinc-700",
            mode === id ? "bg-zinc-800 text-zinc-100" : "text-zinc-500 hover:text-zinc-300",
          )}
        >
          <Icon className="size-3" aria-hidden />
          {label}
        </button>
      ))}
    </div>
  );
}
