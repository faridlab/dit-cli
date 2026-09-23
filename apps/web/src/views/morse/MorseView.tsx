// Morse (§20, ADR 0023): the API workbench.
//
// An explorer of specs, scenarios, environments and runs; tabs for what is
// open; and in each tab either a request — an operation drafted here, or a
// step of a scenario — or a page about a scenario, a spec or an environment.
//
// Where things live is the whole design, so it is worth saying once:
//   - an operation's method, path and fields come from the spec at HEAD;
//   - an operation's draft lives in this component until Save writes it into
//     a fence — and is deliberately not persisted in the browser, because a
//     draft may hold a token someone pasted in to try a request;
//   - a step's edit is written back to its fence after a pause, one commit
//     per pause;
//   - the environment picker holds a *name*; values stay in the local file;
//   - the page can fire Send and Run, and can never trust a host.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { ChevronDown, Globe, Layers, Link2, PanelLeft, Radio, ShieldCheck, X } from "lucide-react";
import { toast } from "sonner";
import { cn } from "../../lib/cn";
import {
  bodyError,
  cloneStep,
  draftForOperation,
  isSelector,
  sameStep,
  secretHeaders,
  suggestStepId,
} from "../../lib/morse";
import {
  useCreateMorseScenario,
  useMorse,
  useMorseEnvs,
  useMorseRuns,
  useMorseScenario,
  useRunMorse,
  useSaveMorseStep,
  useSendMorse,
} from "../../lib/queries";
import type { MorseRunDto, MorseStepDto } from "../../lib/types";
import { ErrorBox, Loading } from "../../components/states";
import { type Tab, type TabRef, tabKey, Verb } from "./common";
import { Explorer, type Seg } from "./Explorer";
import { AllowTab, EnvTab, Overview, SpecTab } from "./InfoTabs";
import { RequestTab, type RunState, type SaveState } from "./RequestTab";
import { ConfirmDialog, SaveDialog, type SaveTarget } from "./SaveDialog";
import { ScenarioTab } from "./ScenarioTab";

type Sub = Parameters<typeof RequestTab>[0]["sub"];

const AUTOSAVE_MS = 1500;
const OVERVIEW: Tab = { key: "overview", ref: { kind: "overview" }, pinned: true };

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

function keep(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* a private window keeps nothing, and nothing here needs keeping */
  }
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function MorseView() {
  const morse = useMorse();
  const envsQ = useMorseEnvs();
  const runsQ = useMorseRuns();
  const send = useSendMorse();
  const run = useRunMorse();
  const saveStep = useSaveMorseStep();
  const createScenario = useCreateMorseScenario();

  const [seg, setSeg] = useState<Seg>(() => load("dit.morse.seg", "specs"));
  const [open, setOpen] = useState<Set<string>>(() => new Set(load<string[]>("dit.morse.open", [])));
  const [tabs, setTabs] = useState<Tab[]>(() => {
    const saved = load<Tab[]>("dit.morse.tabs", []);
    return saved.length ? saved : [OVERVIEW];
  });
  const [active, setActive] = useState<string>(() => load("dit.morse.active", "overview"));
  const [envName, setEnvName] = useState<string | null>(() => load<string | null>("dit.morse.env", null));
  const [showFence, setShowFence] = useState<boolean>(() => load("dit.morse.fence", false));
  const [paneOpen, setPaneOpen] = useState(false);

  const [drafts, setDrafts] = useState<Record<string, MorseStepDto>>({});
  const [baseline, setBaseline] = useState<Record<string, MorseStepDto>>({});
  const [dirty, setDirty] = useState<Set<string>>(new Set());
  const [subs, setSubs] = useState<Record<string, Sub>>({});
  const [results, setResults] = useState<Record<string, RunState>>({});
  const [saves, setSaves] = useState<Record<string, SaveState>>({});
  const [saveFor, setSaveFor] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [confirmClose, setConfirmClose] = useState<string | null>(null);
  const timers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});

  useEffect(() => keep("dit.morse.seg", seg), [seg]);
  useEffect(() => keep("dit.morse.open", [...open]), [open]);
  useEffect(() => keep("dit.morse.tabs", tabs), [tabs]);
  useEffect(() => keep("dit.morse.active", active), [active]);
  useEffect(() => keep("dit.morse.env", envName), [envName]);
  useEffect(() => keep("dit.morse.fence", showFence), [showFence]);
  useEffect(() => () => Object.values(timers.current).forEach(clearTimeout), []);

  const report = morse.data;
  const envs = envsQ.data;
  // A remembered environment that is no longer on this machine is no choice.
  const env = envs?.envs.find((e) => e.name === envName) ?? null;
  const effectiveEnv = env ? env.name : null;

  const activeTab = tabs.find((t) => t.key === active) ?? tabs[0] ?? OVERVIEW;
  const scenarioName =
    activeTab.ref.kind === "step" || activeTab.ref.kind === "scn" ? activeTab.ref.scenario : null;
  const detailQ = useMorseScenario(scenarioName);

  const findOp = useCallback(
    (ref: string | null) => {
      if (!ref || !report) return { spec: undefined, op: undefined };
      const [specId, ...rest] = ref.split("/");
      const spec = report.specs.find((s) => s.id === specId);
      return { spec, op: spec?.operations.find((o) => o.operation_id === rest.join("/")) };
    },
    [report],
  );

  // A step's draft starts as the fence says, the first time its tab is seen.
  useEffect(() => {
    if (activeTab.ref.kind !== "step" || !detailQ.data) return;
    const key = activeTab.key;
    const stepId = activeTab.ref.step;
    const step = detailQ.data.steps.find((s) => s.id === stepId);
    if (!step) return;
    setBaseline((b) => (b[key] ? b : { ...b, [key]: cloneStep(step) }));
    setDrafts((d) => (d[key] ? d : { ...d, [key]: cloneStep(step) }));
  }, [activeTab, detailQ.data]);

  // A tab restored from the last visit comes back without its draft — drafts
  // are never kept in the browser — so it starts again from the spec.
  useEffect(() => {
    if (activeTab.ref.kind !== "op" || drafts[activeTab.key]) return;
    const { spec, op } = activeTab.ref;
    const found = report?.specs.find((s) => s.id === spec)?.operations.find((o) => o.operation_id === op);
    if (found) setDrafts((d) => (d[activeTab.key] ? d : { ...d, [activeTab.key]: draftForOperation(spec, found) }));
  }, [activeTab, drafts, report]);

  const openTab = useCallback(
    (ref: TabRef, pin = false) => {
      const key = tabKey(ref);
      if (ref.kind === "op" && report) {
        const spec = report.specs.find((s) => s.id === ref.spec);
        const op = spec?.operations.find((o) => o.operation_id === ref.op);
        if (op) {
          setDrafts((d) => (d[key] ? d : { ...d, [key]: draftForOperation(ref.spec, op) }));
          setSubs((s) => (s[key] ? s : { ...s, [key]: op.body.length ? "body" : op.params.some((p) => p.location === "path") ? "params" : "docs" }));
        }
      }
      if (ref.kind === "step") setSubs((s) => (s[key] ? s : { ...s, [key]: "params" }));
      setTabs((list) => {
        const found = list.find((t) => t.key === key);
        if (found) return pin && !found.pinned ? list.map((t) => (t.key === key ? { ...t, pinned: true } : t)) : list;
        const next: Tab = { key, ref, pinned: pin };
        const preview = pin ? -1 : list.findIndex((t) => !t.pinned && !dirty.has(t.key));
        if (preview >= 0) return list.map((t, i) => (i === preview ? next : t));
        return [...list, next];
      });
      setActive(key);
      setPaneOpen(false);
    },
    [report, dirty],
  );

  const closeTab = useCallback(
    (key: string, force = false) => {
      if (!force && dirty.has(key)) {
        setConfirmClose(key);
        return;
      }
      setTabs((list) => {
        const i = list.findIndex((t) => t.key === key);
        const next = list.filter((t) => t.key !== key);
        const kept = next.length ? next : [OVERVIEW];
        if (active === key) setActive(kept[Math.max(0, i - 1)]?.key ?? "overview");
        return kept;
      });
      setDrafts(({ [key]: _gone, ...rest }) => rest);
      setBaseline(({ [key]: _gone, ...rest }) => rest);
      setResults(({ [key]: _gone, ...rest }) => rest);
      setDirty((d) => {
        const n = new Set(d);
        n.delete(key);
        return n;
      });
    },
    [dirty, active],
  );

  const pin = (key: string) => setTabs((list) => list.map((t) => (t.key === key ? { ...t, pinned: true } : t)));

  // ---- editing ------------------------------------------------------------

  const scheduleSave = (key: string, scenario: string, draft: MorseStepDto) => {
    clearTimeout(timers.current[key]);
    setSaves((s) => ({ ...s, [key]: { state: "editing" } }));
    timers.current[key] = setTimeout(() => {
      const refuse = (why: string) => setSaves((s) => ({ ...s, [key]: { state: "error", message: why } }));
      if (secretHeaders(draft).length) return refuse("a header holds a credential literal — use {{token}}");
      if (bodyError(draft.body)) return refuse("the body is not valid JSON yet");
      if (draft.capture.some((c) => c.name && !isSelector(c.from))) return refuse("a capture is not a selector");
      setSaves((s) => ({ ...s, [key]: { state: "saving" } }));
      saveStep.mutate(
        { scenario, step: draft },
        {
          onSuccess: (detail) => {
            const saved = detail.steps.find((s) => s.id === draft.id);
            if (saved) setBaseline((b) => ({ ...b, [key]: cloneStep(saved) }));
            setSaves((s) => ({ ...s, [key]: { state: "saved" } }));
          },
          onError: (e) => refuse(message(e)),
        },
      );
    }, AUTOSAVE_MS);
  };

  const onDraftChange = (tab: Tab, next: MorseStepDto) => {
    setDrafts((d) => ({ ...d, [tab.key]: next }));
    if (!tab.pinned) pin(tab.key);
    if (tab.ref.kind === "op") {
      setDirty((d) => new Set(d).add(tab.key));
    } else if (tab.ref.kind === "step" && detailQ.data?.editable !== false) {
      const base = baseline[tab.key];
      if (base && sameStep(base, next)) {
        clearTimeout(timers.current[tab.key]);
        setSaves((s) => ({ ...s, [tab.key]: { state: "saved" } }));
      } else {
        scheduleSave(tab.key, tab.ref.scenario, next);
      }
    }
  };

  // ---- firing -------------------------------------------------------------

  const doSend = (tab: Tab) => {
    const draft = drafts[tab.key];
    if (!draft) return;
    if (!draft.operation) {
      setResults((r) => ({
        ...r,
        [tab.key]: { state: "error", message: "An inline request is sent from its scenario — use Run on the scenario." },
      }));
      return;
    }
    setResults((r) => ({ ...r, [tab.key]: { state: "pending" } }));
    send.mutate(
      { env: effectiveEnv, step: draft },
      {
        onSuccess: (out) => setResults((r) => ({ ...r, [tab.key]: { state: "done", run: out } })),
        onError: (e) => setResults((r) => ({ ...r, [tab.key]: { state: "error", message: message(e) } })),
      },
    );
  };

  const doRun = (scenario: string) => {
    const key = `scn:${scenario}`;
    setResults((r) => ({ ...r, [key]: { state: "pending" } }));
    run.mutate(
      { scenario, env: effectiveEnv },
      {
        onSuccess: (out) => {
          setResults((r) => ({ ...r, [key]: { state: "done", run: out } }));
          if (out.refused) toast.error(`${scenario}: the host is not allowed on this machine`);
          else toast.success(out.passed ? `${scenario}: all ${out.steps.length} steps passed` : `${scenario}: stopped at a failing step`);
        },
        onError: (e) => setResults((r) => ({ ...r, [key]: { state: "error", message: message(e) } })),
      },
    );
  };

  const doSave = (target: SaveTarget) => {
    const key = saveFor;
    if (!key) return;
    const draft = drafts[key];
    if (!draft) return;
    const step = { ...draft, id: target.stepId };
    const land = (scenario: string, detailSteps: MorseStepDto[], doc: string) => {
      const stepKey = tabKey({ kind: "step", scenario, step: step.id });
      const saved = detailSteps.find((s) => s.id === step.id) ?? step;
      // The draft becomes that step: the same tab, now backed by the fence.
      setTabs((list) =>
        list.map((t) => (t.key === key ? { key: stepKey, ref: { kind: "step", scenario, step: step.id }, pinned: true } : t)),
      );
      setDrafts(({ [key]: _gone, ...rest }) => ({ ...rest, [stepKey]: cloneStep(saved) }));
      setBaseline((b) => ({ ...b, [stepKey]: cloneStep(saved) }));
      setSubs((s) => ({ ...s, [stepKey]: s[key] ?? "params" }));
      setResults(({ [key]: moved, ...rest }) => (moved ? { ...rest, [stepKey]: moved } : rest));
      setSaves((s) => ({ ...s, [stepKey]: { state: "saved" } }));
      setDirty((d) => {
        const n = new Set(d);
        n.delete(key);
        return n;
      });
      setActive(stepKey);
      setSaveFor(null);
      setSaveError(null);
      toast.success(`Committed ${doc} — step ${step.id} is in ${scenario}`);
    };
    if (target.mode === "add") {
      saveStep.mutate(
        { scenario: target.scenario, step },
        {
          onSuccess: (detail) => land(detail.scenario, detail.steps, detail.path),
          onError: (e) => setSaveError(message(e)),
        },
      );
    } else {
      const specId = step.operation?.split("/")[0] ?? "";
      createScenario.mutate(
        { doc: target.doc, name: target.name, spec_id: specId, env: effectiveEnv, step },
        {
          onSuccess: (detail) => land(detail.scenario, detail.steps, detail.path),
          onError: (e) => setSaveError(message(e)),
        },
      );
    }
  };

  const openFromHistory = (ref: TabRef, kept: MorseRunDto) => {
    openTab(ref, true);
    const key = tabKey(ref);
    setResults((r) => ({ ...r, [key]: { state: "done", run: kept } }));
  };

  const toggle = (key: string) =>
    setOpen((o) => {
      const n = new Set(o);
      if (n.has(key)) n.delete(key);
      else n.add(key);
      return n;
    });

  const openTag = (spec: string, tag: string) => {
    setSeg("specs");
    setOpen((o) => new Set(o).add(`s:${spec}`).add(`s:${spec}/${tag}`));
    setPaneOpen(true);
  };

  const capturedEarlier = useMemo(() => {
    if (activeTab.ref.kind !== "step" || !detailQ.data) return [];
    const stepId = activeTab.ref.step;
    const at = detailQ.data.steps.findIndex((s) => s.id === stepId);
    return detailQ.data.steps.slice(0, Math.max(0, at)).flatMap((s) => s.capture.map((c) => c.name));
  }, [activeTab, detailQ.data]);

  if (morse.isPending) return <Loading label="Loading Morse…" className="flex-1" />;
  if (morse.isError) return <ErrorBox error={morse.error} />;
  if (!report) return null;

  const title = (t: Tab): { label: string; lead: React.ReactNode } => {
    const r = t.ref;
    switch (r.kind) {
      case "overview":
        return { label: "Overview", lead: <Radio className="i" aria-hidden /> };
      case "op": {
        const { op } = findOp(`${r.spec}/${r.op}`);
        return { label: op?.summary ?? r.op, lead: <Verb method={op?.method ?? "GET"} wide /> };
      }
      case "step": {
        const { op } = findOp(drafts[t.key]?.operation ?? null);
        return { label: `${r.scenario} › ${r.step}`, lead: <Verb method={op?.method ?? "GET"} wide /> };
      }
      case "scn":
        return { label: r.scenario, lead: <Link2 className="i" aria-hidden /> };
      case "spec":
        return { label: r.spec, lead: <Layers className="i" aria-hidden /> };
      case "env":
        return { label: r.env, lead: <Globe className="i" aria-hidden /> };
      case "allow":
        return { label: "Allowed hosts", lead: <ShieldCheck className="i" aria-hidden /> };
    }
  };

  const content = (() => {
    const r = activeTab.ref;
    const key = activeTab.key;
    switch (r.kind) {
      case "overview":
        return (
          <Overview
            report={report}
            envs={envs}
            onSeg={(s) => {
              setSeg(s);
              setPaneOpen(true);
            }}
            onOpen={(ref) => openTab(ref, true)}
          />
        );
      case "spec":
        return (
          <SpecTab
            report={report}
            id={r.spec}
            envServer={env?.server ?? null}
            onOpen={(ref) => openTab(ref, true)}
            onOpenTag={(tag) => openTag(r.spec, tag)}
          />
        );
      case "env":
        return (
          <EnvTab
            envs={envs}
            report={report}
            name={r.env}
            active={effectiveEnv === r.env}
            onUse={() => {
              setEnvName(r.env);
              toast.success(`Requests now use the ${r.env} environment`);
            }}
          />
        );
      case "allow":
        return <AllowTab envs={envs} />;
      case "scn":
        return (
          <ScenarioTab
            view={report.scenarios.find((s) => s.scenario === r.scenario)}
            detail={detailQ.data}
            detailError={detailQ.isError ? message(detailQ.error) : null}
            report={report}
            env={env}
            envName={effectiveEnv}
            result={results[key]}
            onRun={() => doRun(r.scenario)}
            onOpenStep={(step) => openTab({ kind: "step", scenario: r.scenario, step }, true)}
          />
        );
      case "op":
      case "step": {
        const draft = drafts[key];
        if (!draft) {
          if (r.kind === "step" && detailQ.isError) return <ErrorBox error={detailQ.error} />;
          if (r.kind === "step" && detailQ.data && !detailQ.data.steps.some((s) => s.id === r.step)) {
            return (
              <div className="mw-page">
                <p>
                  Step <code>{r.step}</code> is no longer in <code>{r.scenario}</code>.
                </p>
              </div>
            );
          }
          if (r.kind === "op" && !findOp(`${r.spec}/${r.op}`).op) {
            return (
              <div className="mw-page">
                <p>
                  <code>
                    {r.spec}/{r.op}
                  </code>{" "}
                  is not in the catalogue any more — the spec no longer describes it.
                </p>
              </div>
            );
          }
          return <Loading label="Reading the fence…" />;
        }
        const { spec, op } = findOp(draft.operation);
        const view = r.kind === "step" ? report.scenarios.find((s) => s.scenario === r.scenario) : undefined;
        return (
          <RequestTab
            key={key}
            kind={r.kind}
            draft={draft}
            onChange={(next) => onDraftChange(activeTab, next)}
            sub={subs[key] ?? "docs"}
            onSub={(s) => setSubs((m) => ({ ...m, [key]: s }))}
            spec={spec}
            op={op}
            env={env}
            envName={effectiveEnv}
            result={results[key]}
            onSend={() => doSend(activeTab)}
            onSave={
              r.kind === "op"
                ? () => {
                    if (secretHeaders(draft).length) {
                      toast.error("Save is blocked: a header holds a credential literal");
                      return;
                    }
                    setSaveError(null);
                    setSaveFor(key);
                  }
                : undefined
            }
            save={saves[key]}
            scenario={r.kind === "step" ? { name: r.scenario, doc: view?.path ?? detailQ.data?.path ?? "" } : undefined}
            capturedEarlier={capturedEarlier}
            editable={r.kind === "op" || detailQ.data?.editable !== false}
            fence={detailQ.data?.fence ?? ""}
            showFence={showFence}
            onToggleFence={() => setShowFence((v) => !v)}
          />
        );
      }
    }
  })();

  const saveDraft = saveFor ? drafts[saveFor] : undefined;

  return (
    <div className={cn("mw", paneOpen && "pane-open")}>
      <Explorer
        report={report}
        envs={envs}
        runs={runsQ.data}
        seg={seg}
        onSeg={setSeg}
        open={open}
        onToggle={toggle}
        selected={active}
        onOpen={(ref) => openTab(ref)}
        onPin={(ref) => openTab(ref, true)}
        onHistory={openFromHistory}
      />
      <section className="mw-main">
        <div className="mw-tabs">
          <button
            type="button"
            className="mw-ib mw-only-narrow"
            style={{ alignSelf: "center", marginLeft: 6 }}
            title="Show the explorer"
            onClick={() => setPaneOpen((v) => !v)}
          >
            <PanelLeft className="i" aria-hidden />
          </button>
          <div className="mw-tabstrip" role="tablist">
            {tabs.map((t) => {
              const { label, lead } = title(t);
              const isDirty = dirty.has(t.key);
              return (
                <div
                  key={t.key}
                  role="tab"
                  aria-selected={t.key === activeTab.key}
                  tabIndex={0}
                  className={cn("mw-tab", t.key === activeTab.key && "on", !t.pinned && "preview")}
                  title={t.pinned ? label : `${label} — preview; double-click to keep`}
                  onClick={() => setActive(t.key)}
                  onDoubleClick={() => pin(t.key)}
                  onAuxClick={(e) => {
                    if (e.button === 1) closeTab(t.key);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") setActive(t.key);
                  }}
                >
                  {lead}
                  <span className="tl">{label}</span>
                  {isDirty ? <span className="dirty" title="Not in any scenario yet" /> : null}
                  {t.key === "overview" && tabs.length === 1 ? null : (
                    <button
                      type="button"
                      className={cn("x", isDirty && "hasdirty")}
                      title="Close"
                      onClick={(e) => {
                        e.stopPropagation();
                        closeTab(t.key);
                      }}
                    >
                      <X className="i" aria-hidden />
                    </button>
                  )}
                </div>
              );
            })}
          </div>
          <div className="mw-tabs-r">
            <DropdownMenu.Root>
              <DropdownMenu.Trigger asChild>
                <button type="button" className="mw-envbtn" aria-label="Environment">
                  <Globe className="i" aria-hidden />
                  <span className="lbl">{env ? `${env.name}${env.server ? ` · ${env.server.replace(/^[a-z]+:\/\//, "")}` : ""}` : "Default environment"}</span>
                  <ChevronDown className="i chev" aria-hidden />
                </button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content className="mw-menu" align="end" sideOffset={4}>
                  <DropdownMenu.Item className="mw-mi" onSelect={() => setEnvName(null)}>
                    <span className="ck">{env ? "" : "✓"}</span>
                    Default
                    <span className="sub">the fence's env: · the spec's server</span>
                  </DropdownMenu.Item>
                  {(envs?.envs ?? []).map((e) => (
                    <DropdownMenu.Item key={e.name} className="mw-mi" onSelect={() => setEnvName(e.name)}>
                      <span className="ck">{env?.name === e.name ? "✓" : ""}</span>
                      {e.name}
                      <span className="sub mw-mono">{e.server ?? "spec server"}</span>
                    </DropdownMenu.Item>
                  ))}
                  <DropdownMenu.Separator />
                  <DropdownMenu.Item
                    className="mw-mi"
                    onSelect={() => {
                      setSeg("envs");
                      setPaneOpen(true);
                    }}
                  >
                    <Globe className="i" aria-hidden />
                    Manage environments…
                  </DropdownMenu.Item>
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          </div>
        </div>
        <div className="mw-body">{content}</div>
      </section>

      {saveFor && saveDraft ? (
        <SaveDialog
          open
          onOpenChange={(o) => {
            if (!o) setSaveFor(null);
          }}
          report={report}
          specId={saveDraft.operation?.split("/")[0] ?? ""}
          suggestedId={suggestStepId(saveDraft.operation?.split("/")[1] ?? saveDraft.id)}
          pending={saveStep.isPending || createScenario.isPending}
          serverError={saveError}
          onSave={doSave}
        />
      ) : null}
      <ConfirmDialog
        open={confirmClose !== null}
        onOpenChange={(o) => {
          if (!o) setConfirmClose(null);
        }}
        title="Discard this draft?"
        body="This request has changes that are not in any scenario. Closing the tab drops them — nothing was committed."
        action="Discard"
        onConfirm={() => {
          if (confirmClose) closeTab(confirmClose, true);
        }}
      />
    </div>
  );
}
