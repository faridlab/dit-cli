// Workspace settings (ADRs 0005 + 0007): where content lives and when an
// issue gets its number. Two closed choices each — no free-form paths, no
// number entry — because every consumer of the layout branches on one bit,
// and the number is facade-owned, never typed in.

import { toast } from "sonner";
import { usePutSettings, useSettings } from "../lib/queries";
import type { Layout, NumberingPolicy } from "../lib/types";
import { cn } from "../lib/cn";
import { ErrorBox, Loading } from "../components/states";
import { SectionHeading } from "../components/chrome";

interface Option<T extends string> {
  value: T;
  label: string;
  hint: string;
}

const LAYOUTS: Option<Layout>[] = [
  {
    value: "root",
    label: "Visible at the tree root",
    hint: "issues/, epics/, docs/, notes/, changelogs/ — plain Markdown anyone browsing the repo can read. Machinery stays in .dit/.",
  },
  {
    value: "dotdir",
    label: "Tucked under .dit/",
    hint: "Everything in one hidden directory — for repos hosting DIT as a guest.",
  },
];

const NUMBERINGS: Option<NumberingPolicy>[] = [
  {
    value: "local",
    label: "Numbered when created",
    hint: "Each new issue takes the next free number at creation — #1, #2, #3…",
  },
  {
    value: "on-merge",
    label: "Numbered on merge",
    hint: "Numbers stay unset until the branch merges, so parallel branches never collide.",
  },
];

function OptionCards<T extends string>({
  options,
  value,
  onPick,
  disabled,
}: {
  options: Option<T>[];
  value: T;
  onPick: (value: T) => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex flex-col gap-2">
      {options.map((option) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            disabled={disabled}
            aria-pressed={active}
            onClick={() => onPick(option.value)}
            className={cn(
              "rounded-[10px] border p-3.5 text-left",
              active
                ? "border-accent bg-white/[0.03]"
                : "border-edge bg-card hover:border-dim",
              disabled && "opacity-50",
            )}
          >
            <span className="flex items-center gap-2 text-[13px] font-medium text-zinc-200">
              <span
                aria-hidden
                className={cn(
                  "size-2 rounded-full",
                  active ? "bg-accent" : "bg-ctl",
                )}
              />
              {option.label}
            </span>
            <span className="mt-1 block pl-4 text-xs leading-relaxed text-zinc-500">
              {option.hint}
            </span>
          </button>
        );
      })}
    </div>
  );
}

export function SettingsView() {
  const settings = useSettings();
  const put = usePutSettings();

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

  const pickLayout = (layout: Layout) => {
    if (layout === current.layout || put.isPending) return;
    // The layout change is the guided migration: git mv, one commit, index
    // rebuild. It deserves one "are you sure" more than a silent toggle.
    const confirmed = window.confirm(
      `Move all DIT content to the ${layout === "root" ? "tree root" : ".dit/"} layout?\n\n` +
        "Every issue file moves in one commit; history follows the renames. The tree must be clean.",
    );
    if (confirmed) {
      put.mutate({ layout }, { onSuccess: () => toast.success("Layout migrated") });
    }
  };

  const pickNumbering = (numbering: NumberingPolicy) => {
    if (numbering === current.numbering || put.isPending) return;
    put.mutate(
      { numbering },
      { onSuccess: () => toast.success(`Numbering: ${numbering === "local" ? "on creation" : "on merge"}`) },
    );
  };

  return (
    <div className="mx-auto w-full max-w-[700px] overflow-y-auto px-6 py-8">
      <h1 className="text-lg font-semibold text-zinc-100">Settings</h1>
      <p className="mt-1 text-[13px] text-zinc-500">
        Workspace-wide choices, recorded in <code className="font-mono text-xs">.dit/config.yaml</code>{" "}
        and committed like any other change.
      </p>

      <section id="settings-layout" className="mt-8">
        <SectionHeading>Where files live</SectionHeading>
        <p className="mt-1 text-xs text-zinc-600">
          Changing this moves every issue in one commit — history follows the files.
        </p>
        <div className="mt-3">
          <OptionCards
            options={LAYOUTS}
            value={current.layout}
            onPick={pickLayout}
            disabled={put.isPending}
          />
        </div>
      </section>

      <section id="settings-numbering" className="mt-8">
        <SectionHeading>Issue numbers</SectionHeading>
        <p className="mt-1 text-xs text-zinc-600">
          When an issue gets the <code className="font-mono text-xs">number:</code> that becomes its
          #handle. Existing numbers never change.
        </p>
        <div className="mt-3">
          <OptionCards
            options={NUMBERINGS}
            value={current.numbering}
            onPick={pickNumbering}
            disabled={put.isPending}
          />
        </div>
      </section>

      <section id="settings-templates" className="mt-8">
        <SectionHeading>Templates</SectionHeading>
        <p className="mt-1 text-xs text-zinc-600">
          Bodies seeded on creation, from <code className="font-mono text-xs">.dit/templates/</code>.
        </p>
        <ul className="mt-3 flex flex-wrap gap-1.5">
          {current.templates.map((name) => (
            <li
              key={name}
              className="rounded-[3px] border border-white/[0.06] bg-white/[0.04] px-2 py-1 font-mono text-xs text-zinc-400"
            >
              {name}
            </li>
          ))}
          {current.templates.length === 0 ? (
            <li className="text-xs text-zinc-600">none — bodies start empty</li>
          ) : null}
        </ul>
        <p className="mt-2 text-xs text-zinc-600">
          Edit them with <code className="font-mono text-xs">dit templates edit &lt;name&gt;</code>.
        </p>
      </section>
    </div>
  );
}
