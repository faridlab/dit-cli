// The left column: specs grouped by OpenAPI tag, scenarios grouped by the
// document that holds their fence, this machine's environments, and what
// has run since the last reindex. Selecting anything opens a tab; nothing
// here sends a request.

import { useMemo, useState } from "react";
import { ChevronRight, Clock, Folder, Globe, Layers, Link2, Search, ShieldCheck } from "lucide-react";
import { cn } from "../../lib/cn";
import { runKey } from "../../lib/morse";
import type { MorseEnvsDto, MorseReportDto, MorseRunDto } from "../../lib/types";
import { ago, type TabRef, Verb } from "./common";

export type Seg = "specs" | "scenarios" | "envs" | "history";

const FILTER_LIMIT = 250;

export function Explorer({
  report,
  envs,
  runs,
  seg,
  onSeg,
  open,
  onToggle,
  selected,
  onOpen,
  onPin,
  onHistory,
}: {
  report: MorseReportDto;
  envs: MorseEnvsDto | undefined;
  runs: MorseRunDto[] | undefined;
  seg: Seg;
  onSeg: (seg: Seg) => void;
  open: Set<string>;
  onToggle: (key: string) => void;
  selected: string | null;
  onOpen: (ref: TabRef) => void;
  onPin: (ref: TabRef) => void;
  /** Open what a kept run was of, with that run shown in the tab. */
  onHistory: (ref: TabRef, run: MorseRunDto) => void;
}) {
  const [filter, setFilter] = useState("");
  const q = filter.trim().toLowerCase();

  const byDoc = useMemo(() => {
    const out = new Map<string, typeof report.scenarios>();
    for (const s of report.scenarios) {
      if (q && !s.scenario.toLowerCase().includes(q) && !s.path.toLowerCase().includes(q)) continue;
      out.set(s.path, [...(out.get(s.path) ?? []), s]);
    }
    return out;
  }, [report.scenarios, q]);

  const hits = useMemo(() => {
    if (seg !== "specs" || q.length < 2) return null;
    let total = 0;
    const groups: { spec: string; ops: MorseReportDto["specs"][number]["operations"] }[] = [];
    for (const s of report.specs) {
      const matched = s.operations.filter(
        (o) =>
          s.id.includes(q) ||
          o.operation_id.toLowerCase().includes(q) ||
          o.path.toLowerCase().includes(q) ||
          (o.summary ?? "").toLowerCase().includes(q),
      );
      if (matched.length) {
        total += matched.length;
        groups.push({ spec: s.id, ops: matched });
      }
    }
    return { total, groups };
  }, [report.specs, seg, q]);

  const scenarioCount = (spec: string) => report.scenarios.filter((s) => s.spec_id === spec).length;

  const opRow = (spec: string, o: MorseReportDto["specs"][number]["operations"][number], depth: string) => {
    const ref: TabRef = { kind: "op", spec, op: o.operation_id };
    const key = `op:${spec}/${o.operation_id}`;
    return (
      <button
        key={key}
        type="button"
        className={cn("mw-tr", depth, selected === key && "sel")}
        title={`${o.method} ${o.path}`}
        onClick={() => onOpen(ref)}
        onDoubleClick={() => onPin(ref)}
      >
        <Verb method={o.method} />
        <span className="lbl">{o.summary ?? o.operation_id}</span>
      </button>
    );
  };

  let body: React.ReactNode;
  if (seg === "specs" && hits) {
    let shown = 0;
    body = hits.total ? (
      <>
        {hits.groups.map((g) => {
          if (shown >= FILTER_LIMIT) return null;
          const ops = g.ops.slice(0, FILTER_LIMIT - shown);
          shown += ops.length;
          return (
            <div key={g.spec}>
              <div className="mw-tr" style={{ cursor: "default" }}>
                <Layers className="i" aria-hidden />
                <span className="lbl">
                  <b>{g.spec}</b>
                </span>
                <span className="cnt">{g.ops.length}</span>
              </div>
              {ops.map((o) => opRow(g.spec, o, "d1"))}
            </div>
          );
        })}
        {hits.total > shown ? (
          <div className="mw-more">and {(hits.total - shown).toLocaleString()} more — narrow the filter</div>
        ) : null}
      </>
    ) : (
      <div className="mw-note">
        No operation matches “{filter.trim()}” in {report.specs.length} specs.
      </div>
    );
  } else if (seg === "specs") {
    body = report.specs.length ? (
      report.specs.map((s) => {
        const key = `s:${s.id}`;
        const isOpen = open.has(key);
        const tags = new Map<string, typeof s.operations>();
        if (isOpen) for (const o of s.operations) tags.set(o.tag ?? "untagged", [...(tags.get(o.tag ?? "untagged") ?? []), o]);
        const n = scenarioCount(s.id);
        return (
          <div key={s.id}>
            <button
              type="button"
              className={cn("mw-tr", selected === `spec:${s.id}` && "sel")}
              aria-expanded={isOpen}
              onClick={() => {
                onToggle(key);
                if (!isOpen) onOpen({ kind: "spec", spec: s.id });
              }}
            >
              <ChevronRight className="i chev" aria-hidden />
              <Layers className="i" aria-hidden />
              <span className="lbl">{s.id}</span>
              <span className="cnt">
                {n ? `${n} ◆ ` : ""}
                {s.problem ? "!" : s.operations.length}
              </span>
            </button>
            {[...tags.entries()].map(([tag, ops]) => {
              const tk = `${key}/${tag}`;
              const tagOpen = open.has(tk);
              return (
                <div key={tk}>
                  <button type="button" className="mw-tr d1" aria-expanded={tagOpen} onClick={() => onToggle(tk)}>
                    <ChevronRight className="i chev" aria-hidden />
                    <Folder className="i" aria-hidden />
                    <span className="lbl">{tag}</span>
                    <span className="cnt">{ops.length}</span>
                  </button>
                  {tagOpen ? ops.map((o) => opRow(s.id, o, "d2")) : null}
                </div>
              );
            })}
          </div>
        );
      })
    ) : (
      <div className="mw-note">
        No specs registered yet. Add one under <code>specs:</code> in <code>.dit/config.yaml</code>, for example{" "}
        <code>{"- { id: auth, path: api/openapi.yaml }"}</code>. The path is a path in a repository, never a URL.
      </div>
    );
  } else if (seg === "scenarios") {
    body = (
      <>
        {[...byDoc.entries()].map(([doc, list]) => (
          <div key={doc}>
            <div className="mw-sec path">{doc}</div>
            {list.map((s) => {
              const key = `scn:${s.scenario}`;
              return (
                <button
                  key={s.scenario}
                  type="button"
                  className={cn("mw-tr", selected === key && "sel")}
                  onClick={() => onOpen({ kind: "scn", scenario: s.scenario })}
                  onDoubleClick={() => onPin({ kind: "scn", scenario: s.scenario })}
                >
                  <span className={cn("mw-dot", s.health)} title={s.health} />
                  <span className="lbl">{s.scenario}</span>
                  <span className="cnt">{s.steps.length} steps</span>
                </button>
              );
            })}
          </div>
        ))}
        {byDoc.size === 0 ? (
          <div className="mw-note">
            {report.scenarios.length
              ? "No scenario matches."
              : "No scenarios yet. Open an operation, fill it in, and choose Save to scenario."}
          </div>
        ) : null}
        <div className="mw-note">
          A scenario is a <code>dit-morse</code> fence inside a document. The fence is the source of truth; this list
          is read from the index.
        </div>
      </>
    );
  } else if (seg === "envs") {
    const list = (envs?.envs ?? []).filter((e) => !q || e.name.toLowerCase().includes(q));
    body = (
      <>
        <div className="mw-sec">On this machine</div>
        {list.map((e) => (
          <button
            key={e.name}
            type="button"
            className={cn("mw-tr", selected === `env:${e.name}` && "sel")}
            onClick={() => onOpen({ kind: "env", env: e.name })}
          >
            <Globe className="i" aria-hidden />
            <span className="lbl">{e.name}</span>
            <span className="cnt mw-mono">{e.server ?? "spec server"}</span>
          </button>
        ))}
        <button
          type="button"
          className={cn("mw-tr", selected === "allow" && "sel")}
          onClick={() => onOpen({ kind: "allow" })}
        >
          <ShieldCheck className="i" aria-hidden />
          <span className="lbl">Allowed hosts</span>
          <span className="cnt">{envs?.allow_hosts.length ?? 0}</span>
        </button>
        {list.length === 0 && !q ? (
          <div className="mw-note">
            No environments yet. They live in <code>.dit/morse.local.yaml</code>, which is gitignored, under{" "}
            <code>envs:</code>.
          </div>
        ) : null}
        <div className="mw-note">
          Environments stay on this machine. The repo holds variable <em>names</em> only, and this page never sees a
          value.
        </div>
      </>
    );
  } else {
    const list = (runs ?? []).filter((r) => !q || r.scenario.toLowerCase().includes(q));
    body = list.length ? (
      <>
        {list.map((r) => {
          const k = runKey(r.scenario);
          const ref: TabRef =
            k.kind === "send"
              ? { kind: "op", spec: k.operation.split("/")[0] ?? "", op: k.operation.split("/").slice(1).join("/") }
              : { kind: "scn", scenario: k.name };
          return (
            <button key={r.scenario} type="button" className="mw-tr" onClick={() => onHistory(ref, r)}>
              <span className={cn("mw-dot", r.refused ? "broken" : r.passed ? "pass" : "fail")} />
              {k.kind === "send" ? <Verb method={r.steps[0]?.method ?? "GET"} /> : <Link2 className="i" aria-hidden />}
              <span className="lbl">{k.kind === "send" ? k.operation : k.name}</span>
              <span className="cnt">{ago(Number(r.ran_at))}</span>
            </button>
          );
        })}
        <div className="mw-note">
          Timings and pass or fail only, kept in the index until the next reindex. Response bodies and captured values
          are never stored.
        </div>
      </>
    ) : (
      <div className="mw-note">
        Nothing has run on this machine since the last reindex. Send an operation or Run a scenario and it appears
        here.
      </div>
    );
  }

  const placeholder = {
    specs: "Filter specs, paths, operationIds",
    scenarios: "Filter scenarios",
    envs: "Filter environments",
    history: "Filter runs",
  }[seg];

  return (
    <aside className="mw-pane" aria-label="Morse explorer">
      <div className="mw-seg" role="tablist">
        {(
          [
            ["specs", Layers, "Specs"],
            ["scenarios", Link2, "Scenarios"],
            ["envs", Globe, "Envs"],
            ["history", Clock, "History"],
          ] as const
        ).map(([id, Icon, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={seg === id}
            className={cn(seg === id && "on")}
            onClick={() => {
              onSeg(id);
              setFilter("");
            }}
          >
            <Icon className="i" aria-hidden />
            {label}
          </button>
        ))}
      </div>
      <label className="mw-filter">
        <Search className="i" aria-hidden />
        <input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder={placeholder}
          aria-label={placeholder}
          autoComplete="off"
        />
        {hits ? <span className="n">{hits.total.toLocaleString()}</span> : null}
      </label>
      <div className="mw-tree">{body}</div>
    </aside>
  );
}
