// One scenario: its chain, what each step reads and captures, its verdict
// against the spec it is pinned to, and Run. Moving the pin stays in the
// terminal — `dit morse sync` fires the chain first and moves the pin only
// on green, and a browser moving pins would be a claim nobody verified.

import { AlertTriangle, CircleCheck, CircleX, Clock, Code2, Copy, Lock, Play, ShieldAlert } from "lucide-react";
import { cn } from "../../lib/cn";
import { usedVars } from "../../lib/morse";
import type { MorseEnvDto, MorseReportDto, MorseRunDto, MorseScenarioDetailDto, MorseScenarioDto } from "../../lib/types";
import { Loading } from "../../components/states";
import { ago, allowCommand, Banner, Coded, copyText, CopyCmd, HealthPill, Verb } from "./common";
import type { RunState } from "./RequestTab";

export function ScenarioTab({
  view,
  detail,
  detailError,
  report,
  env,
  envName,
  result,
  onRun,
  onOpenStep,
}: {
  view: MorseScenarioDto | undefined;
  detail: MorseScenarioDetailDto | undefined;
  detailError: string | null;
  report: MorseReportDto;
  env: MorseEnvDto | null;
  envName: string | null;
  result: RunState | undefined;
  onRun: () => void;
  onOpenStep: (step: string) => void;
}) {
  if (!view) {
    return (
      <div className="mw-page">
        <p>This scenario is no longer in the index — its fence was removed or renamed.</p>
      </div>
    );
  }
  const runnable = view.health === "fresh" || view.health === "stale";
  const run: MorseRunDto | null = result?.state === "done" ? result.run : view.last_run;
  const effectiveEnv = envName ?? view.env;
  const missing = env ? view.requires.filter((v) => !env.vars.includes(v)) : [];
  const produced = new Map<string, string>();
  for (const st of detail?.steps ?? []) for (const c of st.capture) if (c.name) produced.set(c.name, st.id);
  const opOf = (ref: string | null) => {
    if (!ref) return undefined;
    const [spec, ...rest] = ref.split("/");
    return report.specs.find((s) => s.id === spec)?.operations.find((o) => o.operation_id === rest.join("/"));
  };

  return (
    <div className="mw-page">
      <div className="mw-hrow">
        <h1>{view.scenario}</h1>
        <HealthPill s={view} />
        <span className="mw-sp" />
        {runnable ? (
          <button type="button" className="mw-btn pri" onClick={onRun} disabled={result?.state === "pending"}>
            {result?.state === "pending" ? <span className="mw-spin" /> : <Play className="i" aria-hidden />}
            {result?.state === "pending" ? "running…" : "Run"}
          </button>
        ) : null}
        <button
          type="button"
          className="mw-btn"
          title="Runs the chain and moves the pin only if every step passes — from a terminal"
          onClick={() => copyText(`dit morse sync ${view.scenario}${effectiveEnv ? ` --env ${effectiveEnv}` : ""}`)}
        >
          <Copy className="i" aria-hidden />
          Copy sync command
        </button>
      </div>
      <div className="mw-facts">
        <span>
          in <b className="mw-mono">{view.path}:{view.line}</b>
        </span>
        <span>
          spec <b className="mw-mono">{view.spec_id}</b>
        </span>
        <span>
          pinned at <b className="mw-mono">{view.pin.slice(0, 7)}</b>
        </span>
        {view.env ? (
          <span>
            env <b className="mw-mono">{view.env}</b>
          </span>
        ) : null}
        {view.requires.length ? (
          <span>
            requires <b className="mw-mono">{view.requires.join(", ")}</b>
          </span>
        ) : null}
      </div>

      {view.health === "stale" ? (
        <Banner tone="warn" icon={<Clock className="i" aria-hidden />}>
          The <code>{view.spec_id}</code> spec has moved <b>{view.stale_by} commit(s)</b> since this scenario was
          pinned. It may still work — running it tells you, and <code>dit morse sync</code> moves the pin only after a
          green run.
        </Banner>
      ) : null}
      {view.health === "broken" || view.health === "unreadable" ? (
        <Banner tone="crit" icon={<AlertTriangle className="i" aria-hidden />}>
          <b>{view.health === "unreadable" ? "The fence does not parse." : "This can't be run as written."}</b>
          {view.reasons.map((r) => (
            <div key={r}>
              <Coded text={r} />
            </div>
          ))}
        </Banner>
      ) : null}
      {runnable && missing.length ? (
        <Banner tone="warn" icon={<Lock className="i" aria-hidden />}>
          Environment <b>{envName}</b> has no value for <code>{missing.join("`, `")}</code>. Run will stop before
          sending anything that needs it.
        </Banner>
      ) : null}

      <div className="mw-hrow">
        <h2>Steps</h2>
        <span className="mw-sp" />
        <span className="mw-hint" style={{ margin: 0 }}>
          {detail?.editable === false
            ? "The fence has a comment, so steps are read-only here — edit it in its document."
            : "Open a step to edit it. Each pause in typing is written back to the fence as one commit."}
        </span>
      </div>
      {detailError ? (
        <Banner tone="crit" icon={<AlertTriangle className="i" aria-hidden />}>
          <Coded text={detailError} />
        </Banner>
      ) : !detail ? (
        <Loading label="Reading the fence…" />
      ) : (
        <div className="mw-chain">
          {detail.steps.map((st, i) => {
            const op = opOf(st.operation);
            const line = run?.steps.find((r) => r.id === st.id);
            return (
              <div key={st.id}>
                {i ? <div className="mw-link" /> : null}
                <button type="button" className={cn("mw-step", !op && st.operation && "bad")} onClick={() => onOpenStep(st.id)}>
                  <span className="n">{i + 1}</span>
                  <span>
                    <span className="t">
                      <b>{st.id}</b>
                      {op ? (
                        <>
                          <Verb method={op.method} wide />
                          <span className="mw-mono" style={{ color: "var(--ink-2)" }}>
                            {op.path}
                          </span>
                        </>
                      ) : (
                        <span className="mw-mono" style={{ color: st.request ? "var(--ink-2)" : "var(--crit)" }}>
                          {st.request ? `request ${st.request}` : `${st.operation} — not in the spec`}
                        </span>
                      )}
                    </span>
                    <span className="io">
                      {usedVars(st).map((v) => {
                        const from = produced.get(v);
                        const fromEarlier = from && from !== st.id;
                        return (
                          <span
                            key={v}
                            className={cn("mw-chip mono", fromEarlier ? "in" : view.requires.includes(v) ? "" : "miss")}
                            title={fromEarlier ? `captured by ${from}` : view.requires.includes(v) ? "from the environment" : "nothing provides it"}
                          >
                            ↳ {v}
                            {fromEarlier ? ` ← ${from}` : ""}
                          </span>
                        );
                      })}
                      {st.capture.map((c) => (
                        <span key={c.name} className="mw-chip mono out">
                          {c.name} ← {c.from}
                        </span>
                      ))}
                      {st.status ? <span className="mw-chip mono">expect {st.status}</span> : null}
                    </span>
                  </span>
                  <span className={cn("res", line && (line.passed ? "ok" : "no"))}>
                    {line ? `${line.status ?? "—"} · ${line.duration_ms}ms` : ""}
                  </span>
                </button>
              </div>
            );
          })}
        </div>
      )}

      {result?.state === "error" ? (
        <Banner tone="warn" icon={<AlertTriangle className="i" aria-hidden />}>
          <Coded text={result.message} />
        </Banner>
      ) : run ? (
        <RunSummary run={run} scenario={view.scenario} env={effectiveEnv} />
      ) : null}

      {detail ? (
        <>
          <div className="mw-hrow">
            <h2>Fence</h2>
            <span className="mw-sp" />
            <button type="button" className="mw-btn sm" onClick={() => copyText("```dit-morse\n" + detail.fence + "\n```", "Fence copied")}>
              <Copy className="i" aria-hidden />
              Copy
            </button>
          </div>
          <div className="mw-fence boxed">
            <div className="mw-fence-h">
              <Code2 className="i" aria-hidden />
              {detail.path}:{detail.line}
            </div>
            <pre>{detail.fence}</pre>
          </div>
        </>
      ) : null}
    </div>
  );
}

function RunSummary({ run, scenario, env }: { run: MorseRunDto; scenario: string; env: string | null }) {
  if (run.refused) {
    const command = allowCommand(run.refused);
    return (
      <Banner tone="crit" icon={<ShieldAlert className="i" aria-hidden />}>
        {(run.refused.split("If you trust it")[0] ?? "").trim()} Run this in your terminal if you trust it:
        <br />
        {command ? <CopyCmd command={command} /> : null}
      </Banner>
    );
  }
  const cli = `dit morse run ${scenario}${env ? ` --env ${env}` : ""}`;
  const failed = run.steps.find((s) => !s.passed);
  return (
    <Banner
      tone={run.passed ? "info" : "crit"}
      icon={run.passed ? <CircleCheck className="i" aria-hidden /> : <CircleX className="i" aria-hidden />}
    >
      <b>{run.passed ? `All ${run.steps.length} steps passed` : `Stopped at ${failed?.id ?? "a step"}`}</b>{" "}
      {ago(Number(run.ran_at))}.
      {failed?.detail ? (
        <div>
          <Coded text={failed.detail} />
        </div>
      ) : null}
      <div>
        This result is kept in the index only and disappears at the next reindex. For bodies and captured values, run
        it in your terminal:
      </div>
      <CopyCmd command={cli} />
    </Banner>
  );
}
