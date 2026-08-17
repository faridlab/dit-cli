// A styled Radix Select for the small enums (status, type, priority).
// Values are plain strings from the schema; the label shown is whatever the
// workspace configured, so the UI never hardcodes a workflow.

import * as SelectPrimitive from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import { cn } from "../lib/cn";

export interface SelectOption {
  value: string;
  label: string;
}

export function SelectField({
  value,
  options,
  onChange,
  ariaLabel,
  disabled,
  className,
}: {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  ariaLabel: string;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <SelectPrimitive.Root value={value} onValueChange={onChange} disabled={disabled}>
      <SelectPrimitive.Trigger
        aria-label={ariaLabel}
        className={cn(
          "inline-flex h-7 w-full items-center justify-between gap-1 rounded border border-zinc-700 bg-zinc-900 px-2 text-left text-xs text-zinc-200 hover:border-zinc-500 disabled:opacity-50",
          className,
        )}
      >
        <SelectPrimitive.Value />
        <SelectPrimitive.Icon>
          <ChevronDown className="size-3.5 text-zinc-500" aria-hidden />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content
          position="popper"
          sideOffset={4}
          className="z-50 max-h-72 overflow-hidden rounded-md border border-zinc-700 bg-zinc-900 shadow-xl"
        >
          <SelectPrimitive.Viewport className="p-1">
            {options.map((option) => (
              <SelectPrimitive.Item
                key={option.value}
                value={option.value}
                className="flex cursor-default select-none items-center justify-between rounded px-2 py-1 text-xs text-zinc-300 outline-none data-highlighted:bg-zinc-800 data-highlighted:text-zinc-100 data-state-checked:text-zinc-100"
              >
                <SelectPrimitive.ItemText>{option.label}</SelectPrimitive.ItemText>
                <SelectPrimitive.ItemIndicator>
                  <Check className="size-3.5 text-sky-400" aria-hidden />
                </SelectPrimitive.ItemIndicator>
              </SelectPrimitive.Item>
            ))}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
