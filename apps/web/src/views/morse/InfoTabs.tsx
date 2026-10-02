// The tabs that read rather than edit: the workspace overview, one spec, one
// environment, and the allowlist. None of them can change anything on this
// machine — a host becomes trusted, and a value gets set, in a terminal.

import { useEffect, useState } from "react";
import { AlertTriangle, CircleCheck, Clock, Lock, MoreHorizontal, Plus, ShieldAlert, ShieldCheck, X } from "lucide-react";
import { MenuButton, type MenuItem } from "../../components/chrome";
import { cn } from "../../lib/cn";
import { hostOf, relativeOnly } from "../../lib/morse";
import type { MorseEnvSetDto, MorseEnvsDto, MorseReportDto } from "../../lib/types";
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
  onSet,
  onRename,
  onDelete,
}: {
  envs: MorseEnvsDto | undefined;
  report: MorseReportDto;
  name: string;
  active: boolean;
  onUse: () => void;
  /** Values go one way (ADR 0027): set or clear, never read back. */
  onSet: (input: MorseEnvSetDto) => void;
  onRename: (to: string) => void;
  onDelete: () => void;
}) {
  const e = envs?.envs.find((x) => x.name === name);
  const [server, setServer] = useState(e?.server ?? "");
  const [values, setValues] = useState<Record<string, string>>({});
  const [newName, setNewName] = useState("");
  const [newValue, setNewValue] = useState("");
  // Follow the file when it changes underneath (another tab, the terminal).
  useEffect(() => setServer(e?.server ?? ""), [e?.server]);
  if (!e) return <div className="mw-page"><p>This environment is no longer in <code>.dit/morse.local.yaml</code>.</p></div>;
  const host = e.server ? hostOf(e.server) : null;
  const allowed = allows(envs?.allow_hosts ?? [], e.server);
  const needed = [...new Set(report.scenarios.flatMap((s) => s.requires))];
  const names = [...new Set([...e.vars, ...needed])];
  const serverChanged = server.trim() !== (e.server ?? "");
  const setValue = (variable: string) => {
    const value = values[variable];
    if (!value) return;
    onSet({ vars: [{ name: variable, value }] });
    setValues(({ [variable]: _sent, ...rest }) => rest);
  };
  const menu: MenuItem[] = [
    { kind: "input", placeholder: "New name", value: e.name, button: "Rename", run: (to) => to && to !== e.name && onRename(to) },
    { kind: "sep" },
    { label: "Delete environment", danger: true, confirm: "Click again — its values are removed from this machine", run: onDelete },
  ];
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
        <span className="mw-sp" />
        <MenuButton items={menu} align="end">
          <button type="button" className="mw-btn" aria-label="Rename or delete this environment">
            <MoreHorizontal className="i" aria-hidden />
          </button>
        </MenuButton>
      </div>
      <p>
        Kept in <code>.dit/morse.local.yaml</code> on this machine — gitignored, and <code>dit doctor</code> reports an
        error if it is ever tracked. Values you set here are written there and never shown again, not even to this page.
      </p>
      <h2>Server</h2>
      <form
        className="mw-envserver"
        onSubmit={(event) => {
          event.preventDefault();
          onSet({ server: server.trim() ? server.trim() : null, vars: [] });
        }}
      >
        <input
          aria-label="Server address"
          value={server}
          spellCheck={false}
          placeholder="Empty: the spec's own servers: — or https://api.staging.example.com"
          onChange={(event) => setServer(event.target.value)}
        />
        <button type="submit" className="mw-btn pri" disabled={!serverChanged}>
          Save
        </button>
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
      </form>
      {e.server && !allowed && host ? (
        <Banner tone="crit" icon={<ShieldAlert className="i" aria-hidden />}>
          <b>{host}</b> is not on this machine's allowlist, so Send and Run are refused for this environment. The page
          can point an environment anywhere, but only a person at this machine can trust a host — run this in your
          terminal if you do:
          <br />
          <CopyCmd command={`dit morse allow ${host}`} />
        </Banner>
      ) : null}
      <h2>Variables</h2>
      <table className="mw-kv">
        <thead>
          <tr>
            <th style={{ width: "22%" }}>Name</th>
            <th style={{ width: "20%" }}>State</th>
            <th>New value</th>
            <th style={{ width: "18%" }}>Required by</th>
            <th className="rm" />
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
                      <Lock className="i" aria-hidden style={{ width: 12, height: 12, verticalAlign: -2 }} /> set
                    </>
                  ) : (
                    <span style={{ color: "var(--crit)" }}>missing</span>
                  )}
                </td>
                <td>
                  <form
                    className="mw-envvalue"
                    onSubmit={(event) => {
                      event.preventDefault();
                      setValue(v);
                    }}
                  >
                    <input
                      type="password"
                      autoComplete="off"
                      aria-label={`New value for ${v}`}
                      placeholder={set ? "replace the value" : "set a value"}
                      value={values[v] ?? ""}
                      onChange={(event) => setValues((all) => ({ ...all, [v]: event.target.value }))}
                    />
                    {values[v] ? (
                      <button type="submit" className="mw-btn sm pri">
                        Set
                      </button>
                    ) : null}
                  </form>
                </td>
                <td className="note">{by.join(", ") || "—"}</td>
                <td className="rm">
                  {set ? (
                    <button
                      type="button"
                      className="mw-ib"
                      title="Remove the value"
                      aria-label={`Remove the value of ${v}`}
                      onClick={() => onSet({ vars: [{ name: v, value: null }] })}
                    >
                      <X className="i" aria-hidden />
                    </button>
                  ) : null}
                </td>
              </tr>
            );
          })}
          <tr>
            <td colSpan={5}>
              <form
                className="mw-envadd"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (!newName.trim() || !newValue) return;
                  onSet({ vars: [{ name: newName.trim(), value: newValue }] });
                  setNewName("");
                  setNewValue("");
                }}
              >
                <input aria-label="New variable name" placeholder="name, like api_key" value={newName} spellCheck={false} onChange={(event) => setNewName(event.target.value)} />
                <input type="password" autoComplete="off" aria-label="Its value" placeholder="value" value={newValue} onChange={(event) => setNewValue(event.target.value)} />
                <button type="submit" className="mw-btn sm" disabled={!newName.trim() || !newValue}>
                  <Plus className="i" aria-hidden />
                  Add variable
                </button>
              </form>
            </td>
          </tr>
        </tbody>
      </table>
      <p className="mw-hint">
        A scenario names variables as <code>{"{{name}}"}</code>; the value is filled in here, on this machine, when it
        runs. In CI, pass <code>DIT_MORSE_VARS</code> instead.
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
