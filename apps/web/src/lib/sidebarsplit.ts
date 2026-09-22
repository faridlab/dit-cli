// Where the rail's navigation stops and the view's own section begins.
//
// The nav is a fixed list; the section under it is not — a lane's waiting
// list, a board's columns, a docs tree all grow without limit, and on a
// short window the section was left with a sliver. So the split is the
// reader's to set, dragged and remembered, the way VS Code lets you size the
// panes of its sidebar.
//
// It is remembered per browser and never in the repo: how tall someone's
// window is says nothing about the plan.

const KEY = "dit.sidebarSplit";

/** Enough nav to keep Home and the first group reachable. */
export const MIN_NAV = 96;
/** Enough section to show a heading and a few rows, or dragging it away
 *  would hide the thing the drag exists to reveal. */
export const MIN_PANE = 120;

/** The nav height that actually fits, given how much room the rail has.
 *  A window that shrinks below both minimums gives the nav whatever is
 *  left rather than letting either side go negative. */
export function clampSplit(wanted: number, available: number): number {
  const ceiling = available - MIN_PANE;
  if (ceiling <= MIN_NAV) return Math.max(0, Math.min(wanted, Math.max(0, ceiling)));
  return Math.max(MIN_NAV, Math.min(wanted, ceiling));
}

/** The remembered split, or null when the reader has never set one — in
 *  which case the nav keeps its natural height and nothing looks dragged. */
export function readSplit(): number | null {
  try {
    const raw = window.localStorage.getItem(KEY);
    if (raw === null) return null;
    const value = Number.parseInt(raw, 10);
    return Number.isFinite(value) && value > 0 ? value : null;
  } catch {
    // Private mode, blocked storage: the split simply is not remembered.
    return null;
  }
}

export function writeSplit(value: number | null): void {
  try {
    if (value === null) window.localStorage.removeItem(KEY);
    else window.localStorage.setItem(KEY, String(Math.round(value)));
  } catch {
    // Losing the preference is the whole cost; the rail still works.
  }
}
