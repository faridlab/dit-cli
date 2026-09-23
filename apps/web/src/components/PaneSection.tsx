// One view in the side panel's lower split, the way VS Code stacks Explorer,
// Outline and Timeline: a heading that folds the view away, and a line above
// it that trades height with the open view before it.
//
// A view hugs its content until someone drags it; from then on it keeps the
// height they gave it. Folding and height are remembered per view id, per
// browser. The heading's own buttons sit beside the fold control, not inside
// it, so pressing one never folds the view by accident.

import { useCallback, useRef, useState, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "../lib/cn";
import { readSections, shareDrag, writeSection } from "../lib/workbench";

export function PaneSection({
  id,
  title,
  count,
  actions,
  fill = false,
  defaultCollapsed = false,
  className,
  children,
}: {
  /** Stable across releases: it keys what the reader folded or sized. */
  id: string;
  title: ReactNode;
  count?: ReactNode;
  actions?: ReactNode;
  /** Take the room nobody else uses — the one view a panel is mostly for. */
  fill?: boolean;
  defaultCollapsed?: boolean;
  className?: string;
  children: ReactNode;
}) {
  const [state, setState] = useState(() => {
    const saved = readSections()[id];
    return { collapsed: saved?.collapsed ?? defaultCollapsed, height: saved?.height };
  });
  const self = useRef<HTMLElement>(null);
  const drag = useRef<{ y: number; above: HTMLElement; a: number; b: number } | null>(null);

  const toggle = () => {
    const collapsed = !state.collapsed;
    setState((s) => ({ ...s, collapsed }));
    writeSection(id, { collapsed });
  };

  /** The open view above this one, whose height a drag trades against. */
  const previousOpen = useCallback((): HTMLElement | null => {
    let el = self.current?.previousElementSibling ?? null;
    while (el && !(el instanceof HTMLElement && el.classList.contains("ps") && el.classList.contains("open"))) {
      el = el.previousElementSibling;
    }
    return el instanceof HTMLElement ? el : null;
  }, []);

  const apply = (above: HTMLElement, a: number, b: number) => {
    above.style.flex = `0 0 ${a}px`;
    if (self.current) self.current.style.flex = `0 0 ${b}px`;
  };

  const style = state.collapsed ? undefined : state.height !== undefined ? { flex: `0 0 ${state.height}px` } : undefined;

  return (
    <section
      ref={self}
      data-ps={id}
      aria-label={typeof title === "string" ? title : undefined}
      className={cn("ps", !state.collapsed && "open", fill && "fill", className)}
      style={style}
    >
      <div
        className="ps-sash"
        role="separator"
        aria-orientation="horizontal"
        aria-label="Drag to resize this view and the one above it"
        onPointerDown={(event) => {
          if (state.collapsed) return;
          const above = previousOpen();
          if (!above || !self.current) return;
          event.preventDefault();
          (event.currentTarget as Element).setPointerCapture(event.pointerId);
          drag.current = {
            y: event.clientY,
            above,
            a: above.getBoundingClientRect().height,
            b: self.current.getBoundingClientRect().height,
          };
          event.currentTarget.classList.add("drag");
        }}
        onPointerMove={(event) => {
          const d = drag.current;
          if (!d) return;
          const [a, b] = shareDrag(d.a, d.b, event.clientY - d.y);
          apply(d.above, a, b);
        }}
        onPointerUp={(event) => {
          const d = drag.current;
          drag.current = null;
          event.currentTarget.classList.remove("drag");
          if (!d || !self.current) return;
          const [a, b] = shareDrag(d.a, d.b, event.clientY - d.y);
          const aboveId = d.above.dataset.ps;
          if (aboveId) writeSection(aboveId, { height: a });
          writeSection(id, { height: b });
          setState((s) => ({ ...s, height: b }));
        }}
        onDoubleClick={() => {
          // Back to hugging its content, for this view and its neighbour.
          const above = previousOpen();
          if (above) {
            above.style.flex = "";
            const aboveId = above.dataset.ps;
            if (aboveId) writeSection(aboveId, { height: undefined });
          }
          writeSection(id, { height: undefined });
          setState((s) => ({ ...s, height: undefined }));
        }}
      />
      <div className="ps-h">
        <button type="button" className="ps-t" aria-expanded={!state.collapsed} onClick={toggle}>
          <ChevronRight className="i chev" aria-hidden />
          <span className="lbl">{title}</span>
        </button>
        {count !== undefined && count !== null ? <span className="ps-cnt">{count}</span> : null}
        {actions ? <span className="ps-acts">{actions}</span> : null}
      </div>
      {state.collapsed ? null : <div className="ps-b">{children}</div>}
    </section>
  );
}
