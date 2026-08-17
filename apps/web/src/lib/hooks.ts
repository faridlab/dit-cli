// Small shared hooks that do not belong to any one view.

import { useEffect, useState } from "react";

/** Debounce a fast-changing value (typing) before it reaches the network. */
export function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(value), delayMs);
    return () => window.clearTimeout(timer);
  }, [value, delayMs]);
  return debounced;
}
