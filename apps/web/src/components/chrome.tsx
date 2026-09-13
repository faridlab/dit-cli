// Shared atoms and class recipes for the app chrome: section headings, keyboard
// hints, the two button shapes, check rows, and the one popover menu every
// property, column and toolbar uses. They emit the class names the approved
// design is written in (styles.css, "Workbench recipes") so a menu on the
// board and a menu in the issue panel are the same menu.

import { useState, type ReactNode } from "react";
import * as Popover from "@radix-ui/react-popover";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { Check } from "lucide-react";
import { cn } from "../lib/cn";

/** Uppercase label that opens every section. `size="sm"` is the sidebar's
 *  `.sb-h`; the default is the content's `.sec-h`. */
export function SectionHeading({
  children,
  size = "md",
  className,
}: {
  children: ReactNode;
  size?: "md" | "sm";
  className?: string;
}) {
  return <h2 className={cn(size === "sm" ? "sb-h" : "sec-h", className)}>{children}</h2>;
}

/** A section heading's trailing mono note: the DQL it runs, a count, a hint. */
export function HeadingNote({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={cn("dql", className)}>{children}</span>;
}

/** Flexible spacer inside headings, headers and rows. */
export function Sp() {
  return <span className="sp" />;
}

/** Keyboard hint badge. */
export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return <kbd className={className}>{children}</kbd>;
}

/** The bordered 28px button; `primary` is the one accent action on a screen. */
export function Btn({
  children,
  primary = false,
  className,
  type = "button",
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { primary?: boolean }) {
  return (
    <button type={type} className={cn("btn", primary && "primary", className)} {...rest}>
      {children}
    </button>
  );
}

/** The 28px icon-only button. `on` paints it in the accent. */
export function IBtn({
  children,
  on = false,
  className,
  type = "button",
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { on?: boolean }) {
  return (
    <button type={type} className={cn("ibtn", on && "on", className)} {...rest}>
      {children}
    </button>
  );
}

/** Pill-shaped filter chip. `on` draws the accent ring; mono because most
 *  chips carry DQL fragments. */
export function ContextChip({
  children,
  on = false,
  onClick,
  title,
}: {
  children: ReactNode;
  on?: boolean;
  onClick?: () => void;
  title?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={cn(
        "rounded-full border px-2.5 py-0.5 font-mono text-[11px] transition-colors",
        on ? "border-accent bg-hover text-ink" : "border-edge text-ink-2 hover:border-dim hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}

/** Checkbox square (or radio circle) for sidebar rows and menu items. */
export function CheckSquare({
  on,
  radio = false,
  className,
}: {
  on: boolean;
  radio?: boolean;
  className?: string;
}) {
  return (
    <span aria-hidden className={cn("chk", on && "on", radio && "rounded-full", className)}>
      <Check />
    </span>
  );
}

/** One 28px sidebar row: check + label + count, or icon + label + count. */
export function Row({
  children,
  on = false,
  className,
  type = "button",
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { on?: boolean }) {
  return (
    <button type={type} className={cn("row", on && "on", className)} {...rest}>
      {children}
    </button>
  );
}

/** Standard 30px control: fields in the detail rail, selects in dialogs. */
export const INPUT_CLASS =
  "h-[30px] rounded-md border border-ctl bg-card px-2 text-[12.5px] text-ink outline-none transition-colors focus:border-accent placeholder:text-faint";

export const BUTTON_PRIMARY = "btn primary";

export const BUTTON_OUTLINED = "btn";

// ---------------------------------------------------------------------------
// The popover menu. One item vocabulary for every menu in the app: the
// property pickers in the issue panel, the column menu on the board, the
// filter/display/sort menus in the header, the docs tree context menu.
// ---------------------------------------------------------------------------

export type MenuItem =
  | { kind: "head"; label: ReactNode }
  | { kind: "sep" }
  | { kind: "text"; node: ReactNode }
  | {
      kind: "input";
      placeholder: string;
      type?: "text" | "number" | "date";
      value?: string;
      button?: string;
      run: (value: string) => void;
    }
  | {
      kind?: "item";
      label: ReactNode;
      icon?: ReactNode;
      /** Renders a checkbox; the menu stays open so several can be flipped. */
      check?: boolean;
      /** The current choice in a pick-one list. */
      on?: boolean;
      danger?: boolean;
      kbd?: string;
      meta?: ReactNode;
      /** Keep the menu open after running (checkboxes default to this). */
      keepOpen?: boolean;
      /** Two-step confirmation: the first click swaps the label for this
       *  text and arms the item, the second click runs it. */
      confirm?: string;
      disabled?: boolean;
      run: () => void;
    };

export function MenuList({ items, onClose }: { items: MenuItem[]; onClose: () => void }) {
  const [armed, setArmed] = useState<number | null>(null);
  return (
    <>
      {items.map((item, index) => {
        if (item.kind === "sep") return <div key={index} className="sep" />;
        if (item.kind === "head") {
          return (
            <div key={index} className="mh">
              {item.label}
            </div>
          );
        }
        if (item.kind === "text") {
          return (
            <div key={index} className="txt">
              {item.node}
            </div>
          );
        }
        if (item.kind === "input") {
          return <MenuInput key={index} item={item} onClose={onClose} />;
        }
        const isArmed = armed === index;
        return (
          <button
            key={index}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            className={cn("mi", item.on && "on", item.danger && "danger", item.disabled && "opacity-50")}
            onClick={() => {
              if (item.confirm && !isArmed) {
                setArmed(index);
                return;
              }
              item.run();
              const keep = item.keepOpen ?? item.check !== undefined;
              if (!keep) onClose();
            }}
          >
            {item.check !== undefined ? <CheckSquare on={item.check} /> : item.icon ?? null}
            <span className="lbl">{isArmed ? item.confirm : item.label}</span>
            {item.kbd ? <kbd>{item.kbd}</kbd> : null}
            {item.meta ? <span className="meta mono text-[11px] text-faint">{item.meta}</span> : null}
          </button>
        );
      })}
    </>
  );
}

function MenuInput({
  item,
  onClose,
}: {
  item: Extract<MenuItem, { kind: "input" }>;
  onClose: () => void;
}) {
  const [value, setValue] = useState(item.value ?? "");
  const fire = () => {
    item.run(value.trim());
    onClose();
  };
  return (
    <div className="in">
      <input
        autoFocus
        type={item.type ?? "text"}
        placeholder={item.placeholder}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            fire();
          }
        }}
      />
      <button type="button" className="btn primary" onClick={fire}>
        {item.button ?? "Set"}
      </button>
    </div>
  );
}

/** A button that opens a menu. `children` is the trigger; the menu is
 *  positioned under it, right-aligned when `align="end"`. Controlled or
 *  uncontrolled. */
export function MenuButton({
  items,
  children,
  align = "start",
  open,
  onOpenChange,
  className,
}: {
  items: MenuItem[];
  children: ReactNode;
  align?: "start" | "end";
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  className?: string;
}) {
  const [inner, setInner] = useState(false);
  const isOpen = open ?? inner;
  const setOpen = (next: boolean) => {
    setInner(next);
    onOpenChange?.(next);
  };
  return (
    <Popover.Root open={isOpen} onOpenChange={setOpen}>
      <Popover.Trigger asChild>{children}</Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align={align}
          sideOffset={4}
          collisionPadding={8}
          className={cn("menu", className)}
          onOpenAutoFocus={(event) => {
            // Inputs take focus themselves; otherwise let the first item
            // have it so ↑↓ work at once.
            if (items.some((item) => item.kind === "input")) event.preventDefault();
          }}
        >
          <MenuList items={items} onClose={() => setOpen(false)} />
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** Right-click menu around `children`, same item vocabulary. */
export function ContextMenuFor({
  items,
  children,
  disabled = false,
}: {
  items: MenuItem[];
  children: ReactNode;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <ContextMenu.Root onOpenChange={setOpen}>
      <ContextMenu.Trigger asChild disabled={disabled}>
        {children}
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu" collisionPadding={8}>
          {open ? (
            <MenuList
              items={items}
              onClose={() => {
                // Radix closes on Escape / outside click; an item that ran
                // dismisses by dispatching the same Escape.
                document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
              }}
            />
          ) : null}
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
