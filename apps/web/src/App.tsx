/**
 * Scaffold. See DESIGN.md §6.5 (server + browser) and §12 (block editor).
 *
 * Three rules this UI must never break:
 *  - Auth token goes in the `Authorization` header, never a cookie. Cookies are
 *    sent cross-origin automatically; a custom header requires a CORS preflight
 *    that will be refused. (§17.2)
 *  - Never render untrusted markdown as raw HTML. (Invariant I10)
 *  - Heavy modules — mermaid above all — are `import()`ed lazily and never
 *    reachable from the entry graph. (ADR 0003; the budget gate enforces it)
 */
import { cn } from "./lib/cn";

export function App() {
  return (
    <main className={cn("min-h-dvh p-8 font-sans text-sm")}>
      <h1 className="text-lg font-semibold">DIT</h1>
      <p className="text-neutral-500">Scaffold — see DESIGN.md §10 for the roadmap.</p>
    </main>
  );
}
