// Workspace settings (ADRs 0005 + 0007) plus the two choices that live in this
// browser only. Where files live and when an issue gets its number are each a
// pair of closed cards — no free-form paths, no number entry — because every
// consumer of the layout branches on one bit and the number is facade-owned.
// Appearance and "open an issue as" never reach .dit/config.yaml; the alias
// does, because commits are attributed to it.

import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Maximize2, Moon, PanelRight, Sun } from "lucide-react";
import { toast } from "sonner";
import { useIssues, usePutSettings, useSettings, useStatus } from "../lib/queries";
import type { Layout, NumberingPolicy } from "../lib/types";
import { cn } from "../lib/cn";
import { ErrorBox, Loading } from "../components/states";
import { Btn, SectionHeading } from "../components/chrome";
import { useTheme, type ThemePreference } from "../lib/theme";
import { useViewOptions, type OpenAs } from "../lib/viewopts";

interface Option<T extends string> {
  value: T;
  title: ReactNode;
  desc: string;
}

const LAYOUTS: Option<Layout>[] = [
  {
    value: "root",
    title: (
      <>
        Repo root — <span className="mono">issues/</span>, <span className="mono">docs/</span>
      </>
    ),
    desc: "Visible next to the code. Default for a single repo.",
  },
  {
    value: "dotdir",
    title: (
      <>
        Dot directory — <span className="mono">.dit/</span>
      </>
    ),
    desc: "Out of the way of the code tree. Pick this for a monorepo.",
  },
];

const NUMBERINGS: Option<NumberingPolicy>[] = [
  {
    value: "local",
    title: "Local",
    desc: "Numbered on creation. Two offline machines can collide.",
  },
  {
    value: "on-merge",
    title: "On merge",
    desc: "Numbered when the issue lands on the data branch. Shows the short ref until then.",
  },
];

const THEMES: { value: ThemePreference; label: string; icon: ReactNode }[] = [
  { value: "system", label: "System", icon: null },
  { value: "light", label: "Light", icon: <Sun className="i" aria-hidden /> },
  { value: "dark", label: "Dark", icon: <Moon className="i" aria-hidden /> },
];

const OPEN_AS: { value: OpenAs; label: string; icon: ReactNode; toast: string }[] = [
  {
    value: "panel",
    label: "Side panel",
    icon: <PanelRight className="i" aria-hidden />,
    toast: "Issues open in the side panel",
  },
  {
    value: "page",
    label: "Full page",
    icon: <Maximize2 className="i" aria-hidden />,
    toast: "Issues open as a full page",
  },
];

/** One radio card: a hollow circle that fills when the card is the current
 *  value, a title and a one-line hint. */
function OptionCard<T extends string>({
  option,
  on,
  disabled,
  onPick,
}: {
  option: Option<T>;
  on: boolean;
  disabled?: boolean;
  onPick: (value: T) => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={on}
      disabled={disabled}
      onClick={() => onPick(option.value)}
      className={cn("opt", on && "on")}
    >
      <span className="rd" aria-hidden />
      <div>
        <b>{option.title}</b>
        <span>{option.desc}</span>
      </div>
    </button>
  );
}

/** The alias field. The input keeps its own draft so typing does not fight
 *  the server value; a commit happens on change (Enter or blur) and only when
 *  the trimmed draft is non-empty and differs from what is already stored. */
function AliasField({
  current,
  suggestions,
  onCommit,
}: {
  current: string;
  suggestions: string[];
  onCommit: (alias: string) => void;
}) {
  const [draft, setDraft] = useState(current);
  // A change made elsewhere (another tab, the CLI) should show up here
  // without the user having to reload the page.
  useEffect(() => setDraft(current), [current]);

  const commit = () => {
    const next = draft.trim();
    if (!next || next === current) return;
    onCommit(next);
  };

  return (
    <div className="field">
      <label htmlFor="aliasIn">Your alias</label>
      <input
        id="aliasIn"
        list="aliases"
        value={draft}
        style={{ maxWidth: 280 }}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            commit();
            event.currentTarget.blur();
          }
        }}
      />
      <datalist id="aliases">
        {suggestions.map((alias) => (
          <option key={alias} value={alias} />
        ))}
      </datalist>
    </div>
  );
}

export function SettingsView() {
  const settings = useSettings();
  const status = useStatus();
  const put = usePutSettings();
  const theme = useTheme();
  const { openAs, setOpenAs } = useViewOptions();
  const client = useQueryClient();
  // Aliases already in the repo, so a typo does not create a second person.
  // 500 is the pool cap the other lists use; the datalist is a hint, not a
  // directory, so a truncated set is acceptable.
  const issues = useIssues({ limit: 500 });
  const aliases = useMemo(() => {
    const seen = new Set<string>();
    for (const issue of issues.data?.items ?? []) {
      if (issue.reporter) seen.add(issue.reporter);
      for (const assignee of issue.assignees) seen.add(assignee);
    }
    return [...seen].sort((a, b) => a.localeCompare(b));
  }, [issues.data]);

  if (settings.isPending) return <Loading label="Loading settings…" />;
  if (settings.isError) {
    return (
      <ErrorBox
        error={settings.error}
        onRetry={() => void settings.refetch()}
        title="Could not read settings"
      />
    );
  }

  const current = settings.data;
  const me = current.me ?? status.data?.me ?? "";

  const pickLayout = (layout: Layout) => {
    if (layout === current.layout || put.isPending) return;
    // The layout change is the guided migration: git mv, one commit, index
    // rebuild. It deserves one "are you sure" more than a silent toggle.
    const confirmed = window.confirm(
      `Move all DIT content to the ${layout === "root" ? "repo root" : ".dit/"} layout?\n\n` +
        "Every file moves in one commit; history follows the renames. The tree must be clean.",
    );
    if (!confirmed) return;
    put.mutate(
      { layout },
      { onSuccess: () => toast.success(`Committed .dit/config.yaml · layout: ${layout}`) },
    );
  };

  const pickNumbering = (numbering: NumberingPolicy) => {
    if (numbering === current.numbering || put.isPending) return;
    put.mutate(
      { numbering },
      { onSuccess: () => toast.success(`Committed .dit/config.yaml · numbering: ${numbering}`) },
    );
  };

  const pickOpenAs = (next: OpenAs) => {
    setOpenAs(next);
    toast.success(OPEN_AS.find((option) => option.value === next)?.toast ?? "");
  };

  const commitAlias = (alias: string) => {
    if (put.isPending) return;
    // usePutSettings already toasts the server's refusal verbatim; only the
    // success message is ours. The status bar reads /api/status, which is
    // not part of the workspace sweep, so it is invalidated by hand.
    put.mutate(
      { me: alias },
      {
        onSuccess: () => {
          toast.success(`Writes now attributed to ${alias}`);
          void client.invalidateQueries({ queryKey: ["status"] });
        },
      },
    );
  };

  return (
    <div className="settings w-full">
      <h1>Settings</h1>

      <section className="sset" id="s-layout" role="radiogroup" aria-label="Where files live">
        <SectionHeading>Where files live</SectionHeading>
        <p>Both are plain Markdown in git; this only decides the folder.</p>
        {LAYOUTS.map((option) => (
          <OptionCard
            key={option.value}
            option={option}
            on={option.value === current.layout}
            disabled={put.isPending}
            onPick={pickLayout}
          />
        ))}
      </section>

      <section className="sset" id="s-numbering" role="radiogroup" aria-label="Issue numbers">
        <SectionHeading>Issue numbers</SectionHeading>
        <p>
          The short ref is permanent. The number is the handle you say out loud. New issues follow
          the choice.
        </p>
        {NUMBERINGS.map((option) => (
          <OptionCard
            key={option.value}
            option={option}
            on={option.value === current.numbering}
            disabled={put.isPending}
            onPick={pickNumbering}
          />
        ))}
      </section>

      <section className="sset" id="s-appearance">
        <SectionHeading>Appearance</SectionHeading>
        <p>Follows your system by default. The choice lives in this browser only.</p>
        <div style={{ display: "flex", gap: 8 }}>
          {THEMES.map((option) => (
            <Btn
              key={option.value}
              primary={theme.preference === option.value}
              aria-pressed={theme.preference === option.value}
              onClick={() => theme.setPreference(option.value)}
            >
              {option.icon}
              {option.label}
            </Btn>
          ))}
        </div>
        <div className="field">
          <label>Open an issue as</label>
          <div style={{ display: "flex", gap: 8 }}>
            {OPEN_AS.map((option) => (
              <Btn
                key={option.value}
                primary={openAs === option.value}
                aria-pressed={openAs === option.value}
                onClick={() => pickOpenAs(option.value)}
              >
                {option.icon}
                {option.label}
              </Btn>
            ))}
          </div>
        </div>
      </section>

      <section className="sset" id="s-people">
        <SectionHeading>People &amp; attribution</SectionHeading>
        <p>
          Writes from this workspace are committed as this alias. Change it and the status bar,
          @me filters and new commits follow.
        </p>
        <AliasField current={me} suggestions={aliases} onCommit={commitAlias} />
      </section>
    </div>
  );
}
