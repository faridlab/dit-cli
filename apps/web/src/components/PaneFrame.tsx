// The secondary pane frame — the slot left of the main area whose content
// belongs to the active view (the VS Code "side bar"). Three modes:
//
//   expanded  full width, resizable by dragging the right edge
//   slim      a 44px strip: present, labeled, cheap. Hovering, focusing or
//             clicking it expands it back.
//   hidden    fully out of the way (⌘B); ⌘B again brings it back.
//
// The width animates; the content inside keeps a fixed inner width so text
// never reflows while the pane is in motion.

import {
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { PanelLeftClose } from "lucide-react";
import { cn } from "../lib/cn";

export type PaneMode = "expanded" | "slim" | "hidden";

export const PANE_SLIM_WIDTH = 44;
export const PANE_MIN_WIDTH = 220;
export const PANE_MAX_WIDTH = 460;

export function PaneFrame({
  mode,
  width,
  title,
  icon: Icon,
  headerExtra,
  onInteract,
  onLeave,
  onCollapse,
  onResizeStart,
  onResize,
  onResizeEnd,
  children,
}: {
  mode: PaneMode;
  width: number;
  title: string;
  icon: typeof PanelLeftClose;
  headerExtra?: ReactNode;
  /** Hover or focus arrived inside the pane — expand it (a pane hidden via
   *  ⌘B stays hidden; only ⌘B restores it). */
  onInteract: () => void;
  onLeave: () => void;
  onCollapse: () => void;
  onResizeStart: () => void;
  /** Live width while the handle is dragged; already clamped. */
  onResize: (width: number) => void;
  onResizeEnd: (width: number) => void;
  children: ReactNode;
}) {
  const [resizing, setResizing] = useState(false);
  const drag = useRef({ startX: 0, startWidth: 0, lastWidth: 0 });

  const beginResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    drag.current = { startX: event.clientX, startWidth: width, lastWidth: width };
    onResizeStart();
    setResizing(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!resizing) return;
    const next = Math.min(
      PANE_MAX_WIDTH,
      Math.max(
        PANE_MIN_WIDTH,
        drag.current.startWidth + event.clientX - drag.current.startX,
      ),
    );
    drag.current.lastWidth = next;
    onResize(next);
  };

  const endResize = () => {
    if (!resizing) return;
    setResizing(false);
    onResizeEnd(drag.current.lastWidth);
  };

  return (
    <aside
      style={{ width: mode === "expanded" ? width : mode === "slim" ? PANE_SLIM_WIDTH : 0 }}
      onPointerEnter={mode === "hidden" ? undefined : onInteract}
      onFocusCapture={mode === "hidden" ? undefined : onInteract}
      onPointerLeave={onLeave}
      aria-label={`${title} pane`}
      className={cn(
        "relative flex shrink-0 flex-col overflow-hidden bg-panel",
        mode === "hidden" ? "pointer-events-none border-transparent" : "border-r border-edge",
        resizing
          ? "transition-none"
          : "transition-[width] duration-[220ms] ease-[cubic-bezier(0.2,0,0,1)]",
      )}
    >
      {/* Full content, held at the expanded width so the animation slides a
          stable surface instead of re-wrapping text at every pixel. */}
      <div
        style={{ width }}
        className={cn(
          "flex h-full min-h-0 flex-col transition-opacity duration-150",
          mode === "expanded" ? "opacity-100" : "pointer-events-none opacity-0",
        )}
      >
        <div className="flex h-[42px] shrink-0 items-center gap-2 border-b border-edge px-3">
          <Icon className="size-4 shrink-0 text-zinc-500" aria-hidden />
          <span className="text-[13px] font-medium text-zinc-200">{title}</span>
          {headerExtra ? <span className="ml-auto">{headerExtra}</span> : null}
          <button
            type="button"
            onClick={onCollapse}
            title="Hide the pane (⌘B)"
            className={cn(
              "rounded p-1 text-zinc-500 transition-colors hover:bg-card hover:text-zinc-300",
              !headerExtra && "ml-auto",
            )}
          >
            <PanelLeftClose className="size-4" aria-hidden />
          </button>
        </div>
        <div className="flex min-h-0 flex-1 flex-col">{children}</div>
      </div>

      {mode === "slim" ? (
        <button
          type="button"
          onClick={onInteract}
          title={`Expand the ${title.toLowerCase()} pane`}
          className="absolute inset-y-0 left-0 flex w-[44px] flex-col items-center gap-2.5 border-r border-edge pt-3.5"
        >
          <Icon className="size-4 shrink-0 text-zinc-400" aria-hidden />
          <span className="text-[10px] font-medium uppercase tracking-[0.09em] text-zinc-500 [writing-mode:vertical-rl]">
            {title}
          </span>
        </button>
      ) : null}

      {mode === "expanded" ? (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize the pane"
          onPointerDown={beginResize}
          onPointerMove={moveResize}
          onPointerUp={endResize}
          onPointerCancel={endResize}
          className={cn(
            "absolute inset-y-0 right-0 z-10 w-[5px] cursor-col-resize touch-none",
            resizing ? "bg-accent/60" : "hover:bg-accent/30",
          )}
        />
      ) : null}
    </aside>
  );
}
