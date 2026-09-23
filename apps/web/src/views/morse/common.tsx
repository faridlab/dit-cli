// Small pieces every Morse tab shares: the method verb, the health pill, the
// copy-a-command button that stands in for anything the browser may not do
// itself (trusting a host, moving a pin), and the tab model.

import type { ReactNode } from "react";
import { AlertTriangle, CircleCheck, Clock, Copy } from "lucide-react";
import { toast } from "sonner";
import { cn } from "../../lib/cn";
import type { MorseScenarioDto } from "../../lib/types";

export type TabRef =
  | { kind: "overview" }
  | { kind: "op"; spec: string; op: string }
  | { kind: "step"; scenario: string; step: string }
  | { kind: "scn"; scenario: string }
  | { kind: "spec"; spec: string }
  | { kind: "env"; env: string }
  | { kind: "allow" };

export interface Tab {
  key: string;
  ref: TabRef;
  /** A preview tab is replaced by the next single click; pinned stays. */
  pinned: boolean;
}

export function tabKey(ref: TabRef): string {
  switch (ref.kind) {
    case "overview":
      return "overview";
    case "op":
      return `op:${ref.spec}/${ref.op}`;
    case "step":
      return `step:${ref.scenario}/${ref.step}`;
    case "scn":
      return `scn:${ref.scenario}`;
    case "spec":
      return `spec:${ref.spec}`;
    case "env":
      return `env:${ref.env}`;
    case "allow":
      return "allow";
  }
}

export function Verb({ method, wide }: { method: string; wide?: boolean }) {
  const m = method.toUpperCase();
  return <span className={cn("mw-verb", `mw-v-${m}`, wide && "wide")}>{m === "DELETE" && !wide ? "DEL" : m}</span>;
}

export function HealthPill({ s }: { s: Pick<MorseScenarioDto, "health" | "stale_by"> }) {
  const Icon = s.health === "fresh" ? CircleCheck : s.health === "stale" ? Clock : AlertTriangle;
  return (
    <span className={cn("mw-pill", s.health)}>
      <Icon className="i" aria-hidden />
      {s.health}
      {s.health === "stale" && s.stale_by ? ` · ${s.stale_by}` : ""}
    </span>
  );
}

export function copyText(text: string, message = "Copied — run it in your terminal") {
  const done = () => toast.success(message);
  try {
    void navigator.clipboard.writeText(text).then(done, () => toast.message(text));
  } catch {
    toast.message(text);
  }
}

/** A command the page hands over instead of doing: trusting a host, moving
 *  a pin, reading a body. Clicking copies it. */
export function CopyCmd({ command, label }: { command: string; label?: string }) {
  return (
    <button type="button" className="mw-cmd" onClick={() => copyText(command)} title="Copy">
      <Copy className="i" aria-hidden />
      <span>{label ?? command}</span>
    </button>
  );
}

export function Banner({
  tone,
  icon,
  children,
}: {
  tone: "warn" | "crit" | "info";
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={cn("mw-banner", tone)}>
      {icon}
      <div>{children}</div>
    </div>
  );
}

/** `dit morse allow <host>` out of a refusal message, which always ends in
 *  that line — the page names it and never runs it. */
export function allowCommand(refused: string): string | null {
  return refused.match(/dit morse allow \S+/)?.[0] ?? null;
}

/** Inline `code` spans in a server message, so a reason reads as it would
 *  in the terminal. */
export function Coded({ text }: { text: string }) {
  const parts = text.split(/(`[^`]+`)/g);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("`") && p.endsWith("`") ? <code key={i}>{p.slice(1, -1)}</code> : <span key={i}>{p}</span>,
      )}
    </>
  );
}

export function ago(seconds: number): string {
  const m = Math.round((Date.now() / 1000 - seconds) / 60);
  if (m < 1) return "just now";
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  return h < 48 ? `${h}h ago` : `${Math.round(h / 24)}d ago`;
}
