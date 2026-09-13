// The issues table. The route's `q` (My issues, a saved view, a palette
// query) is the server query; the sidebar filters are a second, client-side
// layer over what came back, so toggling a context never refetches and the
// filters bar can always say, in DQL, exactly what the table shows. Sorting
// and bulk selection are view state shared with the header's Sort menu
// through the view options; nothing here is stored.

import { useMemo, type KeyboardEvent } from "react";
import { Check, ChevronDown, ChevronsUpDown, Tag, User, X } from "lucide-react";
import { toast } from "sonner";
import { AssigneeCircles, Chip, Due, IssueHandle, PriorityDot, StatusPill, TypeBadge } from "../components/badges";
import { Btn, Kbd, MenuButton, Sp, type MenuItem } from "../components/chrome";
import { ErrorBox, Loading } from "../components/states";
import { openQuery } from "../lib/dql";
import { relativeTime } from "../lib/format";
import { INBOX_DEFINITION, doneIds, filtersToDql, isInbox, matchesFilters } from "../lib/lists";
import { useRegisterPeekList } from "../lib/peeklist";
import { useBulkPatchIssue, useIssues, useSchema, useStatus } from "../lib/queries";
import { routeToHash, useRoute } from "../lib/router";
import { useStarred } from "../lib/starred";
import type { FieldPatch, IssueDto, StatusDto } from "../lib/types";
import { useViewOptions, type SortKey } from "../lib/viewopts";
import { cn } from "../lib/cn";

// One page is plenty for a v0.1 workspace; paging comes with bigger ones.
const PAGE_SIZE = 500;

type Comparator = (a: IssueDto, b: IssueDto) => number;

/** Ascending comparators per sort key. "Ascending" for priority is p0 first
 *  and for updated is newest first — the reading order each column has. */
function sorters(statuses: readonly StatusDto[]): Record<SortKey, Comparator> {
  const order = statuses.map((status) => status.id);
  const rank = (id: string) => {
    const index = order.indexOf(id);
    return index < 0 ? order.length : index;
  };
  return {
    number: (a, b) => (a.number ?? 1e9) - (b.number ?? 1e9),
    title: (a, b) => a.title.localeCompare(b.title),
    priority: (a, b) => (a.priority ?? "p9").localeCompare(b.priority ?? "p9"),
    status: (a, b) => rank(a.status) - rank(b.status),
    due: (a, b) => (a.due ?? "9").localeCompare(b.due ?? "9"),
    owner: (a, b) => (a.assignees[0] ?? "~").localeCompare(b.assignees[0] ?? "~"),
    updated: (a, b) => Date.parse(b.updated) - Date.parse(a.updated),
  };
}

export function IssuesView({
  q,
  starred,
  inbox = false,
  onOpen,
}: {
  q: string | null;
  /** The untriaged list: no owner or no @context, computed over the open pool. */
  inbox?: boolean;
  /** Show only this browser's shortlist. Not a server query: stars are
   *  private to the browser, so the filter happens here, over what the
   *  index returned. */
  starred: boolean;
  onOpen: (id: string) => void;
}) {
  const route = useRoute();
  const schema = useSchema();
  const status = useStatus();
  const me = status.data?.me ?? null;
  const statuses = schema.data?.workflow.statuses ?? [];
  const { filters, toggleMine, toggleContext, toggleType, sort, setSort, selected, toggleSelected, setSelected } =
    useViewOptions();
  const bulk = useBulkPatchIssue();
  const stars = useStarred();

  // Without a route query the table shows the open pool; with no schema yet
  // the query is unfiltered rather than naming a status that may not exist.
  const serverQuery = q ?? openQuery(statuses) ?? undefined;
  const issues = useIssues({ q: serverQuery, limit: PAGE_SIZE });

  const rows = useMemo(() => {
    const all = issues.data?.items ?? [];
    const done = doneIds(statuses);
    let base = all;
    if (inbox) base = base.filter((issue) => isInbox(issue, done));
    if (starred) base = base.filter((issue) => stars.has(issue.short_ref));
    const sorted = base.filter((issue) => matchesFilters(issue, filters, me)).sort(sorters(statuses)[sort.key]);
    if (sort.dir === "desc") sorted.reverse();
    return sorted;
  }, [issues.data, inbox, starred, stars, filters, me, sort, statuses]);

  useRegisterPeekList(useMemo(() => rows.map((issue) => issue.short_ref), [rows]));

  const byId = useMemo(() => new Map((issues.data?.items ?? []).map((issue) => [issue.id, issue])), [issues.data]);
  const selectedIssues = useMemo(
    () => [...selected].map((id) => byId.get(id)).filter((issue): issue is IssueDto => issue !== undefined),
    [selected, byId],
  );
  // Every label on the loaded page, for the "Add label" menu's shortlist.
  const allLabels = useMemo(
    () => [...new Set((issues.data?.items ?? []).flatMap((issue) => issue.labels))].sort(),
    [issues.data],
  );

  // -- bulk edits: one PATCH per issue (one commit each), then clear the selection
  const applyBulk = (
    pick: (issue: IssueDto) => FieldPatch | null,
    done: (count: number) => string,
  ) => {
    const edits = selectedIssues.flatMap((issue) => {
      const set = pick(issue);
      return set === null ? [] : [{ id: issue.id, set }];
    });
    const count = selectedIssues.length;
    if (edits.length === 0) {
      setSelected(new Set());
      return;
    }
    bulk.mutate(edits, {
      onSuccess: () => {
        toast(done(count));
        setSelected(new Set());
      },
    });
  };

  const statusItems: MenuItem[] = [
    { kind: "head", label: `Set status on ${selectedIssues.length}` },
    ...statuses.map((target) => ({
      label: target.label,
      icon: <StatusPill category={target.category} label={target.label.slice(0, 1)} className="h-4 text-[10.5px]" />,
      run: () =>
        applyBulk(
          (issue) => (issue.status === target.id ? null : { status: target.id }),
          (count) => `${count} issues → ${target.label} · one commit each`,
        ),
    })),
  ];

  const addLabel = (label: string) => {
    if (label.length === 0) return;
    applyBulk(
      (issue) => (issue.labels.includes(label) ? null : { labels: [...issue.labels, label] }),
      (count) => `Label ${label} added to ${count} issues`,
    );
  };
  const labelItems: MenuItem[] = [
    { kind: "head", label: "Add label" },
    { kind: "input", placeholder: "label", button: "Add", run: addLabel },
    ...allLabels.slice(0, 8).map((label) => ({
      label,
      icon: <Tag className="i" aria-hidden />,
      run: () => addLabel(label),
    })),
  ];

  const assignToMe = () => {
    if (me === null) return;
    applyBulk(
      (issue) => (issue.assignees.includes(me) ? null : { assignees: [...issue.assignees, me] }),
      (count) => `${count} issues assigned to ${me}`,
    );
  };

  // -- the filters bar: the sidebar filters as DQL chips, and the whole query as a link
  const chips: Array<{ dql: string; remove: () => void }> = [];
  if (filters.mine) chips.push({ dql: "assignee = @me", remove: toggleMine });
  for (const context of [...filters.contexts].sort()) {
    chips.push({ dql: `label = context:${context}`, remove: () => toggleContext(context) });
  }
  for (const type of [...filters.types].sort()) {
    chips.push({ dql: `type = ${type}`, remove: () => toggleType(type) });
  }
  const baseDql = q ?? openQuery(statuses) ?? "";
  const chipDql = filtersToDql(filters).join(" and ");
  const fullDql = [baseDql, chipDql].filter((part) => part.length > 0).join(" and ");

  const allSelected = selected.size > 0 && selected.size === rows.length;
  const toggleAll = () => {
    if (allSelected) setSelected(new Set());
    else setSelected(new Set([...selected, ...rows.map((issue) => issue.id)]));
  };

  const onRowKeyDown = (event: KeyboardEvent<HTMLDivElement>, id: string) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onOpen(id);
    }
  };

  const th = (key: SortKey, label: string, className?: string) => {
    const sorted = sort.key === key;
    return (
      <span
        role="button"
        tabIndex={0}
        className={cn("sortable", sorted && "sorted", className)}
        onClick={() => setSort(key)}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            setSort(key);
          }
        }}
        title={`Sort by ${label.toLowerCase()}`}
      >
        {label}{" "}
        {sorted ? (
          sort.dir === "asc" ? (
            <ChevronDown aria-hidden />
          ) : (
            <ChevronsUpDown aria-hidden />
          )
        ) : null}
      </span>
    );
  };

  return (
    <>
      <div className="filters">
        {chips.length > 0 ? (
          chips.map((chip) => (
            <span key={chip.dql} className="fchip">
              {chip.dql}
              <button type="button" className="rmf" title="Remove this filter" onClick={chip.remove}>
                <X className="i" aria-hidden />
              </button>
            </span>
          ))
        ) : (
          <span style={{ fontSize: 12, color: "var(--muted)" }}>
            No filters — toggle them in the sidebar or the Filter button, or type DQL in <Kbd>⌘K</Kbd>.
          </span>
        )}
        {starred ? (
          // Stars live in this browser; no DQL can name them, so the query is
          // shown for what it is rather than offered as something to run.
          <span className="dq" title="Stars are private to this browser — there is no query behind them">
            {["starred", chipDql].filter((part) => part.length > 0).join(" and ")}
          </span>
        ) : inbox ? (
          <span className="dq" title={`Inbox: ${INBOX_DEFINITION}. DQL cannot test for an empty field, so this part runs here.`}>
            {[fullDql, "inbox"].filter((part) => part.length > 0).join(" and ")}
          </span>
        ) : (
          <a className="dq" href={routeToHash({ name: "search", q: fullDql })} title="Run this exact query on the Search page">
            {fullDql}
          </a>
        )}
      </div>

      {selected.size > 0 ? (
        <div className="bulk">
          <b>{selected.size} selected</b>
          <MenuButton items={statusItems}>
            <Btn disabled={bulk.isPending}>
              <ChevronDown className="i" aria-hidden />
              Set status
            </Btn>
          </MenuButton>
          <Btn
            disabled={bulk.isPending || me === null}
            title={me === null ? "No alias yet — set one in Settings" : undefined}
            onClick={assignToMe}
          >
            <User className="i" aria-hidden />
            Assign to me
          </Btn>
          <MenuButton items={labelItems}>
            <Btn disabled={bulk.isPending}>
              <Tag className="i" aria-hidden />
              Add label
            </Btn>
          </MenuButton>
          <Sp />
          <Btn onClick={() => setSelected(new Set())}>Clear</Btn>
        </div>
      ) : null}

      {issues.isError ? (
        <ErrorBox error={issues.error} title="Could not load issues" onRetry={() => void issues.refetch()} />
      ) : null}
      {issues.isPending ? <Loading label="Loading issues…" /> : null}

      {issues.data ? (
        <div style={{ overflow: "auto", flex: 1 }}>
          <div className="tbl">
            <div className="tr th">
              <button
                type="button"
                className={cn("chk", allSelected && "on")}
                title="Select all"
                aria-label="Select all"
                aria-pressed={allSelected}
                style={{ opacity: 1 }}
                onClick={toggleAll}
              >
                <Check aria-hidden />
              </button>
              {th("number", "#")}
              <span />
              <span />
              {th("title", "Title")}
              {th("status", "Status")}
              <span>Labels</span>
              {th("due", "Due")}
              {th("owner", "Owner")}
              {th("updated", "Updated", "justify-end")}
            </div>
            {rows.map((issue) => {
              const isSelected = selected.has(issue.id);
              const workflowStatus = statuses.find((s) => s.id === issue.status);
              return (
                <div
                  key={issue.id}
                  role="button"
                  tabIndex={0}
                  className={cn("tr open", route.name === "issues" && route.issue === issue.short_ref && "sel")}
                  onClick={() => onOpen(issue.short_ref)}
                  onKeyDown={(event) => onRowKeyDown(event, issue.short_ref)}
                >
                  <button
                    type="button"
                    className={cn("chk selrow", isSelected && "on")}
                    title="Select"
                    aria-label={isSelected ? "Deselect issue" : "Select issue"}
                    aria-pressed={isSelected}
                    onClick={(event) => {
                      // The row underneath opens the issue; the box only selects.
                      event.stopPropagation();
                      toggleSelected(issue.id);
                    }}
                    onKeyDown={(event) => event.stopPropagation()}
                  >
                    <Check aria-hidden />
                  </button>
                  <IssueHandle shortRef={issue.short_ref} number={issue.number} />
                  <TypeBadge type={issue.type} />
                  <PriorityDot priority={issue.priority} />
                  <span className="t">{issue.title}</span>
                  <span className="st">
                    {workflowStatus ? (
                      <StatusPill status={workflowStatus} />
                    ) : (
                      // A status the workflow no longer names: shown as-is
                      // rather than guessed into a category.
                      <span className="mono" style={{ fontSize: 11, color: "var(--muted)" }}>
                        {issue.status}
                      </span>
                    )}
                  </span>
                  <span className="labels">
                    {issue.labels.slice(0, 2).map((label) => (
                      <Chip key={label} label={label} />
                    ))}
                    {issue.labels.length > 2 ? <span className="chip">+{issue.labels.length - 2}</span> : null}
                  </span>
                  <Due iso={issue.due} placeholder="—" />
                  <AssigneeCircles assignees={issue.assignees} />
                  <span className="upd" title={issue.updated}>
                    {relativeTime(issue.updated)}
                  </span>
                </div>
              );
            })}
            {rows.length === 0 ? (
              <p className="empty my-[1em]" style={{ padding: 20 }}>
                Nothing matches. Clear a filter in the sidebar.
              </p>
            ) : null}
          </div>
        </div>
      ) : null}
    </>
  );
}
