// The tabs that read rather than edit: the workspace overview, one spec, one
// environment, and the allowlist. None of them can change anything on this
// machine — a host becomes trusted, and a value gets set, in a terminal.

import { AlertTriangle, CircleCheck, Clock, Lock, ShieldAlert, ShieldCheck } from "lucide-react";
import { cn } from "../../lib/cn";
import { hostOf, relativeOnly } from "../../lib/morse";
import type { MorseEnvsDto, MorseReportDto } from "../../lib/types";
import { Banner, Coded, CopyCmd, HealthPill, type TabRef } from "./common";
import type { Seg } from "./Explorer";

export function Overview({
  report,
  envs,
  onSeg,
  onOpen,
}: {
  report: MorseReportDto;
  envs: MorseEnvsDto | undefined;
  onSeg: (seg: Seg) => void;
  onOpen: (ref: TabRef) => void;
}) {
  const ops = report.specs.reduce((n, s) => n + s.operations.length, 0);
  const count = (h: string) => report.scenarios.filter((s) => s.health === h).length;
  const relative = report.specs.filter(relativeOnly).length;
  const attention = report.scenarios.filter((s) => s.health !== "fresh");
  const problems = report.specs.filter((s) => s.problem);
  return (
    <div className="mw-page">
      <div>
        <h1>API</h1>
        <p style={{ marginTop: 4 }}>
          {report.specs.length} OpenAPI {report.specs.length === 1 ? "document" : "documents"}, read where they live
          and never copied into the repo. Scenarios are <code>dit-morse</code> fences in documents, so they appear in
          pull requests like any other change.
        </p>
      </div>
      <div className="mw-grid3">
        <button type="button" className="mw-tile" onClick={() => onSeg("specs")}>
          <span className="num">{report.specs.length}</span>
          <span className="lab">specs · {ops.toLocaleString()} operations</span>
        </button>
        <button type="button" className="mw-tile" onClick={() => onSeg("scenarios")}>
          <span className="num">{report.scenarios.length}</span>
          <span className="lab">
            <span style={{ color: "var(--done)" }}>{count("fresh")} fresh</span> ·{" "}
            <span style={{ color: "var(--warn)" }}>{count("stale")} stale</span> ·{" "}
            <span style={{ color: "var(--crit)" }}>{count("broken") + count("unreadable")} broken</span>
          </span>
        </button>
        <button type="button" className="mw-tile" onClick={() => onSeg("envs")}>
          <span className="num">{envs?.envs.length ?? 0}</span>
          <span className="lab">environments on this machine · {envs?.allow_hosts.length ?? 0} allowed hosts</span>
        </button>
      </div>
      {relative ? (
        <Banner tone="warn" icon={<AlertTriangle className="i" aria-hidden />}>
          <b>
            {relative} of {report.specs.length} specs declare <code>servers: - url: /</code>.
          </b>{" "}
          A relative server names no host, so those documents say what the API looks like but not where it runs.
          Choose an environment that sets <code>server:</code>, or add one to <code>.dit/morse.local.yaml</code>.
        </Banner>
      ) : null}
      {problems.map((s) => (
        <Banner key={s.id} tone="crit" icon={<AlertTriangle className="i" aria-hidden />}>
          <b>{s.id}</b> — <Coded text={s.problem ?? ""} />
        </Banner>
      ))}
      <h2>Needs attention</h2>
      {attention.length ? (
        <div className="mw-list">
          {attention.map((s) => (
            <button key={s.scenario} type="button" className="mw-li" onClick={() => onOpen({ kind: "scn", scenario: s.scenario })}>
              <HealthPill s={s} />
              <b>{s.scenario}</b>
              <span className="sub mw-mono">
                {s.path}:{s.line}
              </span>
              <span className="mw-sp" />
              <span className="sub">
                {s.health === "stale" ? `spec moved ${s.stale_by} commit(s) since the pin` : <Coded text={s.reasons[0] ?? ""} />}
              </span>
            </button>
          ))}
        </div>
      ) : (
        <p>{report.scenarios.length ? "Nothing broken or stale." : "No scenarios yet — open an operation and choose Save to scenario."}</p>
      )}
      <h2>How this differs from Postman</h2>
      <div className="mw-list">
        <div className="mw-li">
          <b>Endpoints come from the spec</b>
          <span className="sub">Nothing to import and nothing lost on re-import. When the spec changes, the catalogue is recomputed.</span>
        </div>
        <div className="mw-li">
          <b>Tests are Expect and Capture</b>
          <span className="sub">Status, JSONPath, header — and no scripts, because a pulled file that runs code is code execution by pull request.</span>
        </div>
        <div className="mw-li">
          <b>Nothing fires on its own</b>
          <span className="sub">Only Send, Run and the terminal commands fire, and only to a host this machine has allowed.</span>
        </div>
        <div className="mw-li">
          <b>Bodies stay out of the browser</b>
          <span className="sub">The response panel shows status, time, size and each check. Bodies and captured values go to the terminal.</span>
        </div>
      </div>
    </div>
  );
}

export function SpecTab({
  report,
  id,
  envServer,
  onOpen,
  onOpenTag,
}: {
  report: MorseReportDto;
  id: string;
  envServer: string | null;
  onOpen: (ref: TabRef) => void;
  onOpenTag: (tag: string) => void;
}) {
  const s = report.specs.find((x) => x.id === id);
  if (!s) return <div className="mw-page"><p>This spec is no longer registered.</p></div>;
  const tags = new Map<string, number>();
  for (const o of s.operations) tags.set(o.tag ?? "untagged", (tags.get(o.tag ?? "untagged") ?? 0) + 1);
  const scenarios = report.scenarios.filter((x) => x.spec_id === id);
  const relative = relativeOnly(s);
  return (
    <div className="mw-page">
      <div className="mw-hrow">
        <h1>
          {s.title ?? s.id} <span style={{ color: "var(--muted)", fontWeight: 400 }}>· {s.id}</span>
        </h1>
        {s.version ? <span className="mw-chip">v{s.version}</span> : null}
        <span className="mw-chip">{s.operations.length} operations</span>
      </div>
      <div className="mw-facts">
        {s.repo ? (
          <span>
            repo <b className="mw-mono">{s.repo}</b>
          </span>
        ) : null}
        <span>
          path <b className="mw-mono">{s.path}</b>
        </span>
        {s.head ? (
          <span>
            HEAD <b className="mw-mono">{s.head.slice(0, 7)}</b>
          </span>
        ) : null}
      </div>
      {s.problem ? (
        <Banner tone="crit" icon={<AlertTriangle className="i" aria-hidden />}>
          <Coded text={s.problem} />
        </Banner>
      ) : null}
      <h2>Servers</h2>
      <div className="mw-list">
        {s.servers.length ? (
          s.servers.map((v) => (
            <div key={v.url} className="mw-li">
              <span className="mw-mono">{v.url}</span>
              <span className="sub">{v.description ?? ""}</span>
              {!v.url.includes("://") ? (
                <>
                  <span className="mw-sp" />
                  <span className="sub" style={{ color: "var(--warn)" }}>
                    relative — names no host
                  </span>
                </>
              ) : null}
            </div>
          ))
        ) : (
          <div className="mw-li">
            <span className="sub">The document declares no servers.</span>
          </div>
        )}
      </div>
      {relative ? (
        <p className="mw-hint">
          Requests use the <code>server:</code> of the environment you choose — right now{" "}
          <b>{envServer ?? "none, so nothing can be sent"}</b>.
        </p>
      ) : null}
      <h2>Scenarios on this spec</h2>
      {scenarios.length ? (
        <div className="mw-list">
          {scenarios.map((x) => (
            <button key={x.scenario} type="button" className="mw-li" onClick={() => onOpen({ kind: "scn", scenario: x.scenario })}>
              <span className={cn("mw-dot", x.health)} />
              <b>{x.scenario}</b>
              <span className="sub">
                {x.steps.length} steps · {x.path}
              </span>
            </button>
          ))}
        </div>
      ) : (
        <p>None yet. Open any operation, fill it in, and choose Save to scenario.</p>
      )}
      <h2>Tags</h2>
      <div className="mw-list">
        {[...tags.entries()].map(([tag, n]) => (
          <button key={tag} type="button" className="mw-li" onClick={() => onOpenTag(tag)}>
            <b>{tag}</b>
            <span className="mw-sp" />
            <span className="sub">{n} ops</span>
          </button>
        ))}
      </div>
    </div>
  );
}

function allows(allowHosts: string[], server: string | null): boolean {
  if (!server) return false;
  const host = hostOf(server);
  const port = server.match(/^[a-z]+:\/\/[^/:]+:(\d+)/i)?.[1];
  return !!host && allowHosts.some((a) => a === host || (port !== undefined && a === `${host}:${port}`));
}

export function EnvTab({
  envs,
  report,
  name,
  active,
  onUse,
}: {
  envs: MorseEnvsDto | undefined;
  report: MorseReportDto;
  name: string;
  active: boolean;
  onUse: () => void;
}) {
  const e = envs?.envs.find((x) => x.name === name);
  if (!e) return <div className="mw-page"><p>This environment is no longer in <code>.dit/morse.local.yaml</code>.</p></div>;
  const host = e.server ? hostOf(e.server) : null;
  const allowed = allows(envs?.allow_hosts ?? [], e.server);
  const needed = [...new Set(report.scenarios.flatMap((s) => s.requires))];
  const names = [...new Set([...e.vars, ...needed])];
  return (
    <div className="mw-page">
      <div className="mw-hrow">
        <h1>{e.name}</h1>
        {active ? (
          <span className="mw-pill fresh">
            <CircleCheck className="i" aria-hidden />
            in use
          </span>
        ) : (
          <button type="button" className="mw-btn sm" onClick={onUse}>
            Use this environment
          </button>
        )}
      </div>
      <p>
        Read from <code>.dit/morse.local.yaml</code> on this machine. The file is gitignored, and <code>dit doctor</code>{" "}
        reports an error if it is ever tracked.
      </p>
      <h2>Server</h2>
      <div className="mw-list">
        <div className="mw-li">
          <span className="mw-mono">{e.server ?? "— the spec's own servers:"}</span>
          <span className="mw-sp" />
          {e.server ? (
            allowed ? (
              <span className="mw-pill fresh">
                <ShieldCheck className="i" aria-hidden />
                allowed
              </span>
            ) : (
              <span className="mw-pill broken">
                <ShieldAlert className="i" aria-hidden />
                not allowed
              </span>
            )
          ) : null}
        </div>
      </div>
      {e.server && !allowed && host ? (
        <Banner tone="crit" icon={<ShieldAlert className="i" aria-hidden />}>
          <b>{host}</b> is not on this machine's allowlist, so Send and Run are refused for this environment. The page
          can't add a host — run this in your terminal if you trust it:
          <br />
          <CopyCmd command={`dit morse allow ${host}`} />
        </Banner>
      ) : null}
      <h2>Variables</h2>
      <table className="mw-kv">
        <thead>
          <tr>
            <th>Name</th>
            <th>State</th>
            <th>Required by</th>
          </tr>
        </thead>
        <tbody>
          {names.map((v) => {
            const set = e.vars.includes(v);
            const by = report.scenarios.filter((s) => s.requires.includes(v)).map((s) => s.scenario);
            return (
              <tr key={v} className={cn(!set && "err")}>
                <td className="ro">{v}</td>
                <td className="note">
                  {set ? (
                    <>
                      <Lock className="i" aria-hidden style={{ width: 12, height: 12, verticalAlign: -2 }} /> set · value
                      stays on this machine
                    </>
                  ) : (
                    <span style={{ color: "var(--crit)" }}>missing</span>
                  )}
                </td>
                <td className="note">{by.join(", ") || "—"}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="mw-hint">
        Values never reach this page. To set one, edit <code>.dit/morse.local.yaml</code> under{" "}
        <code>
          envs: {e.name}: vars:
        </code>
        , or pass <code>DIT_MORSE_VARS</code> in CI.
      </p>
    </div>
  );
}

export function AllowTab({ envs }: { envs: MorseEnvsDto | undefined }) {
  const hosts = envs?.allow_hosts ?? [];
  return (
    <div className="mw-page">
      <h1>Allowed hosts</h1>
      <p>
        Send and Run reach only the hosts listed here. A scenario arriving in a pull request cannot bring its own
        permission: the list lives in <code>.dit/morse.local.yaml</code>, and only a terminal changes it.
      </p>
      <div className="mw-list">
        {hosts.length ? (
          hosts.map((h) => (
            <div key={h} className="mw-li">
              <ShieldCheck className="i" aria-hidden />
              <span className="mw-mono">{h}</span>
            </div>
          ))
        ) : (
          <div className="mw-li">
            <Clock className="i" aria-hidden />
            <span className="sub">Nothing is allowed yet, so nothing can be sent from this machine.</span>
          </div>
        )}
      </div>
      <Banner tone="info" icon={<Lock className="i" aria-hidden />}>
        This page can't add a host. The server is local and can reach the whole filesystem, so one injected script
        that could grant a host could send requests anywhere from your machine. To add one, run:
        <br />
        <CopyCmd command="dit morse allow <host>" />
      </Banner>
    </div>
  );
}
