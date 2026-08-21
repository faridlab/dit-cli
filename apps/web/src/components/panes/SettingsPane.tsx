// The Settings side pane: section shortcuts for the settings page, which
// is one scroll. Settings has three closed choices, so the pane stays small
// and static — no tree to manage.

import { SectionHeading } from "../chrome";

const SECTIONS = [
  {
    id: "settings-layout",
    label: "Where files live",
    hint: "Tree root or .dit/ — moves content in one commit",
  },
  {
    id: "settings-numbering",
    label: "Issue numbers",
    hint: "On creation or on merge",
  },
  {
    id: "settings-templates",
    label: "Templates",
    hint: "Bodies seeded on issue creation",
  },
];

export function SettingsPane() {
  const jump = (id: string) => {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div className="flex flex-col gap-1 p-3">
      <SectionHeading size="sm" className="px-1 pb-2">
        Sections
      </SectionHeading>
      {SECTIONS.map((section) => (
        <button
          key={section.id}
          type="button"
          onClick={() => jump(section.id)}
          className="flex flex-col gap-0.5 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-card"
        >
          <span className="text-[12.5px] text-zinc-300">{section.label}</span>
          <span className="text-[10.5px] leading-relaxed text-zinc-600">{section.hint}</span>
        </button>
      ))}
    </div>
  );
}
