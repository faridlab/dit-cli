// The Settings sidebar section: one jump link per section of the settings
// page, which is a single scroll. Settings is a handful of closed choices, so
// the pane stays small and static — no tree to manage.

import { ChevronRight } from "lucide-react";
import { Row } from "../chrome";
import { PaneSection } from "../PaneSection";

const SECTIONS: { id: string; label: string }[] = [
  { id: "s-layout", label: "Where files live" },
  { id: "s-numbering", label: "Issue numbers" },
  { id: "s-appearance", label: "Appearance" },
  { id: "s-people", label: "People & attribution" },
];

/** Scroll the section into view and flash its background once, so the eye
 *  lands on the right block even when the page barely moved. */
function jump(id: string) {
  const target = document.getElementById(id);
  if (!target) return;
  target.scrollIntoView({ behavior: "smooth", block: "start" });
  target.animate?.([{ background: "var(--active)" }, { background: "transparent" }], { duration: 900 });
}

export function SettingsPane() {
  return (
    <PaneSection id="settings.sections" title="On this page" fill>
      <div className="sb-body">
        {SECTIONS.map((section) => (
          <Row key={section.id} className="jump" onClick={() => jump(section.id)}>
            <ChevronRight className="i" aria-hidden />
            <span className="lbl">{section.label}</span>
          </Row>
        ))}
      </div>
    </PaneSection>
  );
}
