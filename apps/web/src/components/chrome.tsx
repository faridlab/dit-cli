// Shared atoms and class recipes for the app chrome. Views were drifting
// toward private copies of the same button/input/label shapes; one home for
// them keeps the redesign tweakable in a single place.

import type { ReactNode } from "react";
import { cn } from "../lib/cn";

/** Uppercase mono-ish label that opens every section. `size` shrinks inside
 *  the right rails, where sections stack tighter. */
export function SectionHeading({
  children,
  size = "md",
  className,
}: {
  children: ReactNode;
  size?: "md" | "sm";
  className?: string;
}) {
  return (
    <h2
      className={cn(
        "font-semibold uppercase text-zinc-400",
        size === "md" ? "text-[13px] tracking-[0.06em]" : "text-[11px] tracking-[0.07em]",
        className,
      )}
    >
      {children}
    </h2>
  );
}

/** Keyboard hint badge, as seen next to "Jump to…" and inside dialogs. */
export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        "rounded-[3px] border border-ctl bg-card px-1.5 py-px font-mono text-[10px] text-zinc-500",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

/** Pill-shaped filter chip. `on` draws the accent ring; mono because most
 *  chips carry DQL fragments. */
export function ContextChip({
  children,
  on = false,
  onClick,
  title,
}: {
  children: ReactNode;
  on?: boolean;
  onClick?: () => void;
  title?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={cn(
        "rounded-full border px-2.5 py-0.5 font-mono text-[11px] transition-colors",
        on
          ? "border-accent bg-white/[0.04] text-zinc-200"
          : "border-edge text-zinc-400 hover:border-zinc-400 hover:text-zinc-200",
      )}
    >
      {children}
    </button>
  );
}

/** Standard 30px control: fields in the detail rail, selects in dialogs. */
export const INPUT_CLASS =
  "h-[30px] rounded-md border border-ctl bg-card px-2 text-[12.5px] text-zinc-200 outline-none transition-colors focus:border-accent placeholder:text-zinc-600";

export const BUTTON_PRIMARY =
  "rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-accent-hi disabled:opacity-50";

export const BUTTON_OUTLINED =
  "rounded-md border border-ctl px-3 py-1.5 text-xs text-zinc-400 transition-colors hover:border-zinc-400 hover:text-zinc-200 disabled:opacity-50";
