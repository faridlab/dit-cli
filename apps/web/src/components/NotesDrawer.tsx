// The notes drawer: how this workbench works, where its data lives, and the
// keys. It is the one place the UI explains itself in prose, so a person
// who lands here from `dit ui` for the first time has somewhere to look
// before asking.

import { ChevronRight, Info, X } from "lucide-react";
import { cn } from "../lib/cn";
import { IBtn } from "./chrome";

export function NotesDrawer({
  open,
  onClose,
  onOpenPalette,
  onSwitchTheme,
  onGo,
}: {
  open: boolean;
  onClose: () => void;
  onOpenPalette: () => void;
  onSwitchTheme: () => void;
  onGo: (hash: string) => void;
}) {
  const go = (hash: string) => {
    onClose();
    onGo(hash);
  };
  return (
    <aside className={cn("notes", open && "open")} aria-label="Notes" aria-hidden={!open}>
      <div className="notes-h">
        <Info className="i" aria-hidden />
        Notes
        <span className="sp" />
        <IBtn onClick={onClose} title="Close (Esc)" aria-label="Close notes">
          <X className="i" aria-hidden />
        </IBtn>
      </div>
      <div className="notes-b">
        <p style={{ margin: 0 }}>
          Everything on this screen is read from the Markdown files in this repository and the git history
          behind them. Every change you make here is one commit. There is no database to lose.
        </p>
        <div>
          <h3>How it works</h3>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div className="note">
              <b>1 · Issues open beside the list</b>
              <p>
                From Board, Home, Issues and Search an issue slides in on the right. The list stays
                visible, <kbd>Esc</kbd> returns, <kbd>J</kbd>/<kbd>K</kbd> walk the list behind it. The
                URL carries <span className="mono">?issue=</span>, so a reload or a shared link reopens the
                same panel.
              </p>
              <button type="button" className="try" onClick={() => go("#/board")}>
                Open the board <ChevronRight className="i" style={{ width: 12, height: 12 }} aria-hidden />
              </button>
            </div>
            <div className="note">
              <b>2 · Expand only when asked</b>
              <p>
                The ⤢ button (or <kbd>⌘↵</kbd>) turns the panel into the full page: description and
                activity on the left, properties, field history and commits on the right. "Show as panel"
                collapses it back. Settings can flip the default.
              </p>
            </div>
            <div className="note">
              <b>3 · Properties first, then prose</b>
              <p>
                Status, priority, owner, labels, epic, estimate and dates sit above the description. Click
                any value to change it — one commit each. Hover for who touched it last, derived from{" "}
                <span className="mono">field_events</span> and never stored.
              </p>
            </div>
            <div className="note">
              <b>4 · One activity timeline</b>
              <p>
                Comments and field changes interleave in commit order. A comment is sent when you say so —
                it is its own commit. Titles and descriptions commit 1.5 s after you stop typing.
              </p>
            </div>
            <div className="note">
              <b>5 · ⌘K is search and commands</b>
              <p>
                Issues with a matching snippet, pages, a query runner, navigation and actions — one box.
                Words search everything; a query (<span className="mono">status = review</span>) runs as
                written.
              </p>
              <button type="button" className="try" onClick={() => { onClose(); onOpenPalette(); }}>
                Open the palette <ChevronRight className="i" style={{ width: 12, height: 12 }} aria-hidden />
              </button>
            </div>
            <div className="note">
              <b>6 · Plan: Timeline, Roadmap, Gantt</b>
              <p>
                <b>Timeline</b> is the history layer: every field change and comment, grouped by day, with a
                scrubber that recomputes the board as it stood then. <b>Roadmap</b> lays epics, releases
                or people across quarters. <b>Gantt</b> schedules issues on <span className="mono">start</span>
                /<span className="mono">due</span> with <span className="mono">blocked_by</span> arrows and
                drag-to-commit.
              </p>
              <span>
                <button type="button" className="try" onClick={() => go("#/gantt")}>
                  Gantt <ChevronRight className="i" style={{ width: 12, height: 12 }} aria-hidden />
                </button>{" "}
                ·{" "}
                <button type="button" className="try" onClick={() => go("#/roadmap")}>
                  Roadmap <ChevronRight className="i" style={{ width: 12, height: 12 }} aria-hidden />
                </button>{" "}
                ·{" "}
                <button type="button" className="try" onClick={() => go("#/timeline")}>
                  Timeline <ChevronRight className="i" style={{ width: 12, height: 12 }} aria-hidden />
                </button>
              </span>
            </div>
            <div className="note">
              <b>7 · Light and dark, system by default</b>
              <p>Both share the teal accent; state colors stay separate so they read in either theme.</p>
              <button type="button" className="try" onClick={onSwitchTheme}>
                Switch theme <ChevronRight className="i" style={{ width: 12, height: 12 }} aria-hidden />
              </button>
            </div>
          </div>
        </div>
        <div>
          <h3>Where things live</h3>
          <ol>
            <li>Issues, epics and pages are Markdown files in this repo — read them with any editor.</li>
            <li>History, blame and the Timeline are computed from commits. Nothing is written back.</li>
            <li>Stars, theme, sidebar and "open as" live in this browser only.</li>
            <li>The SQLite index is disposable: delete it and it rebuilds from git.</li>
          </ol>
        </div>
        <div>
          <h3>Keys</h3>
          <div className="keys">
            <kbd>⌘K</kbd><span>palette</span>
            <kbd>⌘B</kbd><span>sidebar</span>
            <kbd>⌘1 3 4 5</kbd><span>Home, Board, Issues, Docs</span>
            <kbd>⌘6 7 8</kbd><span>Timeline, Roadmap, Gantt</span>
            <kbd>C</kbd><span>new issue</span>
            <kbd>J / K</kbd><span>next / previous issue</span>
            <kbd>⌘↵</kbd><span>expand panel · send comment · create</span>
            <kbd>⌘S</kbd><span>commit the page now</span>
            <kbd>Esc</kbd><span>close</span>
          </div>
        </div>
      </div>
    </aside>
  );
}
