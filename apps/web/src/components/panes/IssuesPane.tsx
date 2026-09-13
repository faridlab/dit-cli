// The Issues sidebar section, shared with Search: the client-side filters
// (mine, context, type) the header's Filter menu also drives, with counts
// taken from the open pool, and the saved views. A saved view is a name
// over a DQL string and lives only in this browser's localStorage — the
// repo never learns what one person likes to look at.

import { useCallback, useEffect, useMemo, useState } from "react";
import { Plus, Copy, Trash2, X, Zap } from "lucide-react";
import { toast } from "sonner";
import { openQuery } from "../../lib/dql";
import { contextOf } from "../../lib/format";
import { ISSUE_TYPES, contextsOf, filtersToDql } from "../../lib/lists";
import { useOpenPool, useSchema, useStatus } from "../../lib/queries";
import { navigate, routeToHash } from "../../lib/router";
import { useViewOptions } from "../../lib/viewopts";
import { TypeBadge } from "../badges";
import { CheckSquare, ContextMenuFor, IBtn, MenuButton, Row, SectionHeading, Sp, type MenuItem } from "../chrome";

export const SAVED_VIEWS_KEY = "dit.views";

type SavedView = [name: string, dql: string];

function loadSavedViews(): SavedView[] {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(SAVED_VIEWS_KEY) ?? "[]");
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (entry): entry is SavedView =>
        Array.isArray(entry) && entry.length === 2 && typeof entry[0] === "string" && typeof entry[1] === "string",
    );
  } catch {
    // A blocked, full or corrupt storage just means no saved views.
    return [];
  }
}

function useSavedViews(): [SavedView[], (next: SavedView[]) => void] {
  const [views, setViews] = useState<SavedView[]>(loadSavedViews);
  const save = useCallback((next: SavedView[]) => {
    setViews(next);
    try {
      window.localStorage.setItem(SAVED_VIEWS_KEY, JSON.stringify(next));
    } catch {
      // The views stay for this session; losing them is not worth an error.
    }
  }, []);
  // Another tab saving a view should show up here too.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === null || event.key === SAVED_VIEWS_KEY) setViews(loadSavedViews());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
  return [views, save];
}

async function copyText(text: string, label: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    toast(`${label} — ${text}`);
  } catch {
    toast(`${label}: ${text}`);
  }
}

export function IssuesPane({
  q: _q,
  onFilter: _onFilter,
}: {
  /** Legacy: the filters no longer travel in the URL. Kept so the shell's
   *  call site keeps compiling; the pane reads the shared view options. */
  q: string | null;
  onFilter: (q: string | null) => void;
}) {
  const status = useStatus();
  const me = status.data?.me ?? null;
  const schema = useSchema();
  const statuses = schema.data?.workflow.statuses;
  const pool = useOpenPool();
  const issues = pool.data?.items ?? [];
  const { filters, toggleMine, toggleContext, toggleType, clearFilters } = useViewOptions();
  const [views, saveViews] = useSavedViews();

  const contexts = useMemo(() => contextsOf(issues), [issues]);
  const contextCount = (context: string) => issues.filter((issue) => contextOf(issue.labels) === context).length;
  const typeCount = (type: string) => issues.filter((issue) => issue.type === type).length;

  // The view to save: the open pool narrowed by the current filters, as the
  // DQL a person would type. Lowercase "and" because that is how the chips
  // read; the parser does not care.
  const composed = [openQuery(statuses) ?? "status != done", ...filtersToDql(filters)].join(" and ");
  const saveItems: MenuItem[] = [
    { kind: "head", label: "Save as view" },
    { kind: "text", node: <span className="mono">{composed}</span> },
    {
      kind: "input",
      placeholder: "View name",
      button: "Save",
      run: (name) => {
        if (name.length === 0) return;
        saveViews([...views, [name, composed]]);
        toast(`Saved view “${name}” · kept in this browser`);
      },
    },
  ];

  const viewItems = (index: number, [name, dql]: SavedView): MenuItem[] => [
    { kind: "head", label: name },
    { label: "Run", icon: <Zap className="i" aria-hidden />, run: () => navigate({ name: "search", q: dql }) },
    { label: "Copy DQL", icon: <Copy className="i" aria-hidden />, run: () => void copyText(dql, "DQL copied") },
    {
      label: "Delete view",
      icon: <Trash2 className="i" aria-hidden />,
      danger: true,
      run: () => {
        saveViews(views.filter((_, k) => k !== index));
        toast("View deleted");
      },
    },
  ];

  return (
    <>
      <SectionHeading size="sm">
        Filters
        <Sp />
        <IBtn title="Clear all filters" aria-label="Clear all filters" onClick={clearFilters}>
          <X className="i" aria-hidden />
        </IBtn>
      </SectionHeading>
      <div className="sb-body">
        <Row
          on={filters.mine}
          onClick={toggleMine}
          disabled={me === null}
          className={me === null ? "opacity-50" : undefined}
          title={me === null ? "No alias yet — set one in Settings so @me means someone" : "assignee = @me"}
        >
          <CheckSquare on={filters.mine} />
          <span className="lbl">Assigned to me</span>
          <span className="cnt mono">@me</span>
        </Row>

        <SectionHeading size="sm" className="mt-2">
          Context
        </SectionHeading>
        {contexts.map((context) => (
          <Row
            key={context}
            on={filters.contexts.has(context)}
            onClick={() => toggleContext(context)}
            title={`label = context:${context}`}
          >
            <CheckSquare on={filters.contexts.has(context)} />
            <span className="lbl">@{context}</span>
            <span className="cnt">{contextCount(context)}</span>
          </Row>
        ))}
        {pool.data && contexts.length === 0 ? (
          <p className="empty" style={{ padding: "4px 8px" }}>
            No @context labels on open issues yet.
          </p>
        ) : null}

        <SectionHeading size="sm" className="mt-2">
          Type
        </SectionHeading>
        {ISSUE_TYPES.map((type) => (
          <Row key={type} on={filters.types.has(type)} onClick={() => toggleType(type)} title={`type = ${type}`}>
            <CheckSquare on={filters.types.has(type)} />
            <TypeBadge type={type} />
            <span className="lbl">{type}</span>
            <span className="cnt">{typeCount(type)}</span>
          </Row>
        ))}

        <SectionHeading size="sm" className="mt-2">
          Saved views
          <Sp />
          <MenuButton items={saveItems}>
            <IBtn
              title="Save the current filters as a view — kept in this browser"
              aria-label="Save the current filters as a view"
            >
              <Plus className="i" aria-hidden />
            </IBtn>
          </MenuButton>
        </SectionHeading>
        {views.map((view, index) => (
          <ContextMenuFor key={`${view[0]}:${index}`} items={viewItems(index, view)}>
            <a
              className="row sview"
              href={routeToHash({ name: "search", q: view[1] })}
              title={`${view[1]} — right-click to run, copy or delete`}
            >
              <Zap className="i" aria-hidden />
              <span className="lbl">{view[0]}</span>
            </a>
          </ContextMenuFor>
        ))}
      </div>
    </>
  );
}
