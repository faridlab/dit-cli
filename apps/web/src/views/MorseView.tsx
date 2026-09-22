// Morse (§20, ADR 0022): the API scenarios this repository states, and
// whether they still match the specs they were written against.
//
// Everything on this screen is derived. The endpoint catalogue comes from
// each registered OpenAPI document at the commit it is read at — never
// copied into a DIT file — and each scenario's verdict was worked out at
// reindex, so this view is a plain read of the index.
//
// One control here reaches the network, and only one: Run. Opening this
// screen, selecting a spec or reading a step never sends anything (§20.5).
//
// The allowlist cannot be edited from this page. A host that has not been
// trusted on this machine comes back named, with the command that would
// trust it — because §17.2 makes XSS the primary threat, and a browser that
// could grant trust would turn one injection into "send requests anywhere
// from the maintainer's machine". The decision stays in the terminal.

import { useMemo, useState } from "react";
import {
  AlertTriangle,
  CircleCheck,
  CircleSlash,
  Clock,
  Copy,
  FileJson,
  Play,
  ShieldAlert,
} from "lucide-react";
import { toast } from "sonner";
import { useMorse, useRunMorse } from "../lib/queries";
import { ErrorBox, Loading } from "../components/states";
import { cn } from "../lib/cn";
import type { MorseRunDto, MorseScenarioDto, MorseSpecDto } from "../lib/types";

/** Health maps onto the three status tones the workbench already uses, so a
 *  reader does not learn a second colour language for one screen. */
const TONE: Record<string, "todo" | "doing" | "done"> = {
  fresh: "done",
  stale: "doing",
  broken: "todo",
  unreadable: "todo",
};

function HealthPill({ s }: { s: MorseScenarioDto }) {
  const Icon =
    s.health === "fresh"
      ? CircleCheck
      : s.health === "stale"
        ? Clock
        : s.health === "unreadable"
          ? CircleSlash
          : AlertTriangle;
  const text =
    s.health === "stale" && s.stale_by !== null && s.stale_by !== undefined
      ? `stale · ${s.stale_by}`
      : s.health;
  return (
    <span className={cn("pill", TONE[s.health] ?? "todo")} title={titleFor(s)}>
      <Icon className="i" aria-hidden />
      {text}
    </span>
  );
}

function titleFor(s: MorseScenarioDto): string {
  switch (s.health) {
    case "fresh":
      return "The spec has not moved since this scenario was pinned to it.";
    case "stale":
      return `The spec has moved ${s.stale_by ?? 0} commit(s) since this was last checked. Running it is what settles whether it still works.`;
    case "unreadable":
      return "The fence did not parse — nothing could be judged.";
    default:
      return "This cannot be run as written.";
  }
}

export function MorseView() {
  const morse = useMorse();
  const [openSpec, setOpenSpec] = useState<string | null>(null);

  const scenariosBySpec = useMemo(() => {
    const out = new Map<string, MorseScenarioDto[]>();
    for (const s of morse.data?.scenarios ?? []) {
      const list = out.get(s.spec_id) ?? [];
      list.push(s);
      out.set(s.spec_id, list);
    }
    return out;
  }, [morse.data]);

  if (morse.isPending) return <Loading label="Loading Morse…" className="flex-1" />;
  if (morse.isError) return <ErrorBox error={morse.error} />;
  const report = morse.data;
  if (!report) return null;

  const noSpecs = report.specs.length === 0;

  return (
    <div className="morse">
      <section className="morse-sec">
        <div className="sec-h">
          Specs
          <span className="sp" />
          <span className="dql">derived from each document at HEAD — never copied into the repo</span>
        </div>
        {noSpecs ? (
          <p className="empty">
            No specs registered yet. Add one under <code className="mono">specs:</code> in{" "}
            <code className="mono">.dit/config.yaml</code>, for example{" "}
            <code className="mono">{"- { id: auth, path: api/openapi.yaml }"}</code>. The path is a
            path in a repository, never a URL.
          </p>
        ) : (
          <div className="morse-specs">
            {report.specs.map((spec) => (
              <SpecCard
                key={spec.id}
                spec={spec}
                scenarios={scenariosBySpec.get(spec.id)?.length ?? 0}
                open={openSpec === spec.id}
                onToggle={() => setOpenSpec(openSpec === spec.id ? null : spec.id)}
              />
            ))}
          </div>
        )}
      </section>

      <section className="morse-sec">
        <div className="sec-h">
          Scenarios
          <span className="sp" />
          <span className="dql">
            {report.clean ? "nothing broken" : "something needs attention"} · `dit morse check`
          </span>
        </div>
        {report.scenarios.length === 0 ? (
          <p className="empty">
            No scenarios yet. Write a <code className="mono">dit-morse</code> fence in any document
            under <code className="mono">docs/</code> — it names its own scenario, so the document
            may live anywhere.
          </p>
        ) : (
          <div className="morse-list">
            {report.scenarios.map((s) => (
              <ScenarioRow key={s.scenario} s={s} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function SpecCard({
  spec,
  scenarios,
  open,
  onToggle,
}: {
  spec: MorseSpecDto;
  scenarios: number;
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <div className={cn("card", open && "sel")}>
      <button type="button" className="morse-spec-top" onClick={onToggle} aria-expanded={open}>
        <FileJson className="i" aria-hidden />
        <span className="title">{spec.title ?? spec.id}</span>
        <span className="sp" />
        <span className="chip">{spec.id}</span>
      </button>
      <div className="morse-meta">
        <span className="mono">
          {spec.repo ? `${spec.repo}:` : ""}
          {spec.path}
        </span>
        {spec.version ? <span className="chip">v{spec.version}</span> : null}
        <span className="chip">{spec.operations.length} ops</span>
        <span className="chip">{scenarios} scenarios</span>
      </div>
      {spec.problem ? (
        <p className="morse-problem">
          <AlertTriangle className="i" aria-hidden />
          {spec.problem}
        </p>
      ) : null}
      {open && spec.operations.length > 0 ? (
        <ul className="morse-ops">
          {spec.operations.map((op) => (
            <li key={`${op.method} ${op.path}`}>
              <span className={cn("chip", "morse-verb")}>{op.method}</span>
              <span className="mono">{op.path}</span>
              <span className="morse-opid mono">{op.operation_id}</span>
              <span className="morse-summary">{op.summary ?? ""}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function ScenarioRow({ s }: { s: MorseScenarioDto }) {
  const run = useRunMorse();
  const running = run.isPending && run.variables === s.scenario;
  // A scenario that does not parse, or that calls an operation the spec has
  // dropped, has nothing to send — offering Run would be offering a failure.
  const runnable = s.health === "fresh" || s.health === "stale";
  return (
    <div className="morse-row">
      <div className="morse-row-top">
        <HealthPill s={s} />
        <span className="morse-name">{s.scenario}</span>
        <span className="chip">{s.spec_id}</span>
        {s.env ? <span className="chip">{s.env}</span> : null}
        <span className="sp" />
        <span className="mono morse-where">
          {s.path}:{s.line}
        </span>
        {runnable ? (
          <button
            type="button"
            className="morse-run"
            disabled={running}
            onClick={() => run.mutate(s.scenario)}
            title={`Send ${s.steps.length} request(s) to the server this environment names`}
          >
            <Play className="i" aria-hidden />
            {running ? "running…" : "Run"}
          </button>
        ) : null}
      </div>
      {s.steps.length > 0 ? (
        <ol className="morse-steps">
          {s.steps.map((step) => (
            <li key={step} className="mono">
              {step}
            </li>
          ))}
        </ol>
      ) : null}
      {s.requires.length > 0 ? (
        <p className="morse-requires">
          needs from the environment: <span className="mono">{s.requires.join(", ")}</span>
          <span className="morse-note"> — names only; values never live in the repo</span>
        </p>
      ) : null}
      {s.reasons.map((reason) => (
        <p key={reason} className="morse-problem">
          <AlertTriangle className="i" aria-hidden />
          {reason}
        </p>
      ))}
      {s.last_run ? <RunResult run={s.last_run} /> : null}
    </div>
  );
}

function RunResult({ run }: { run: MorseRunDto }) {
  if (run.refused) return <Refused message={run.refused} />;
  return (
    <div className="morse-run-out">
      {run.steps.map((step) => (
        <div key={step.id} className={cn("morse-run-step", !step.passed && "bad")}>
          <span className="chip morse-verb">{step.method}</span>
          <span className="mono">{step.status ?? "—"}</span>
          <span className="morse-step-id">{step.id}</span>
          <span className="mono morse-note">{step.duration_ms}ms</span>
          <span className="morse-summary">{step.detail}</span>
        </div>
      ))}
      <p className="morse-note">
        {run.passed ? "all steps green" : "stopped at the first step that did not hold"} — this
        result lives only in the index and is gone at the next reindex
      </p>
    </div>
  );
}

/** A host this machine has not been told to trust. The page names it and
 *  hands over the command; it cannot grant the trust itself. */
function Refused({ message }: { message: string }) {
  const command = message.slice(message.indexOf("dit morse allow"));
  return (
    <div className="morse-refused">
      <p className="morse-problem">
        <ShieldAlert className="i" aria-hidden />
        {message.slice(0, message.indexOf("If you trust it")).trim()}
      </p>
      <button
        type="button"
        className="morse-copy"
        onClick={() => {
          void navigator.clipboard?.writeText(command);
          toast.success("command copied — run it in your terminal");
        }}
      >
        <Copy className="i" aria-hidden />
        <span className="mono">{command}</span>
      </button>
    </div>
  );
}
