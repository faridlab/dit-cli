// The Board sidebar section: which columns are on screen, what the cards
// carry, and how the board is grouped. It reads the same column model the
// board draws from and writes the same view options the header's Display
// menu writes, so ticking a row here and picking an item there are one
// action seen from two places.

import { SlidersVertical } from "lucide-react";
import { useViewOptions, type BoardGroupBy, type BoardOptions, type CardSort } from "../../lib/viewopts";
import { useBoardModel } from "../../views/BoardView";
import { CheckSquare, IBtn, MenuButton, Row, type MenuItem } from "../chrome";
import { PaneSection } from "../PaneSection";

const GROUPS: Array<[BoardGroupBy, string, string | null]> = [
  ["status", "Status", "workflow"],
  ["assignee", "Assignee", null],
  ["epic", "Epic", null],
  ["context", "Context", "@label"],
];
const CARD_OPTIONS: Array<[keyof BoardOptions["cards"], string]> = [
  ["labels", "Show labels"],
  ["due", "Show due dates"],
  ["epic", "Show epic on cards"],
];
const CARD_SORTS: Array<[CardSort, string]> = [
  ["priority", "Priority"],
  ["updated", "Recently updated"],
  ["due", "Due date"],
];

export function BoardPane() {
  const { board, setGroupBy, toggleCardOption, setColSort, toggleColumn } = useViewOptions();
  const { columns } = useBoardModel();

  // The same Display menu the header opens, so the gear in this heading and
  // the button up there never disagree about what can be set.
  const display: MenuItem[] = [
    { kind: "head", label: "Group by" },
    ...GROUPS.map(([key, label]): MenuItem => ({ label, on: board.groupBy === key, run: () => setGroupBy(key) })),
    { kind: "head", label: "Cards" },
    ...CARD_OPTIONS.map(([key, label]): MenuItem => ({
      label,
      check: board.cards[key],
      run: () => toggleCardOption(key),
    })),
    { kind: "head", label: "Order cards by" },
    ...CARD_SORTS.map(([key, label]): MenuItem => ({ label, on: board.colSort === key, run: () => setColSort(key) })),
  ];

  return (
    <>
      <PaneSection
        id="board.columns"
        title={`Columns · by ${board.groupBy}`}
        count={`${columns.filter((c) => !board.hidden.has(c.key)).length}/${columns.length}`}
        actions={
          <MenuButton items={display} align="end">
            <IBtn title="Display options">
              <SlidersVertical className="i" aria-hidden />
            </IBtn>
          </MenuButton>
        }
      >
        {columns.map((column) => {
          const on = !board.hidden.has(column.key);
          return (
            <Row
              key={column.key}
              onClick={() => toggleColumn(column.key)}
              title={on ? `Hide ${column.label}` : `Show ${column.label}`}
            >
              <CheckSquare on={on} />
              <span className="lbl">{column.label}</span>
              <span className="cnt">{column.all.length}</span>
            </Row>
          );
        })}
      </PaneSection>

      <PaneSection id="board.cards" title="Cards">
        {CARD_OPTIONS.map(([key, label]) => (
          <Row key={key} onClick={() => toggleCardOption(key)}>
            <CheckSquare on={board.cards[key]} />
            <span className="lbl">{label}</span>
          </Row>
        ))}
      </PaneSection>

      <PaneSection id="board.group" title="Group by">
        {GROUPS.map(([key, label, hint]) => (
          <Row key={key} on={board.groupBy === key} onClick={() => setGroupBy(key)}>
            <CheckSquare on={board.groupBy === key} radio />
            <span className="lbl">
              {label}
              {hint !== null ? <span style={{ color: "var(--muted)" }}> ({hint})</span> : null}
            </span>
          </Row>
        ))}
      </PaneSection>
    </>
  );
}
