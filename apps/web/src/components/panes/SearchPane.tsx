// The Search side pane: the query box and the example queries. Typing here
// drives the same route (`#/search?q=…`) the palette and the dashboard
// links use, so every entry point agrees on what a search is.

import { type FormEvent, useEffect, useState } from "react";
import { Search } from "lucide-react";
import { Kbd, SectionHeading } from "../chrome";

// The examples double as documentation: they are the fastest way to learn a
// query language — steal a working example, edit it. Every one of these is
// a query the real parser accepts as-is.
const EXAMPLES: Array<{ label: string; dql: string }> = [
  { label: "My open work", dql: "status != done AND assignee = @me" },
  { label: "Recent in auth/api", dql: "label IN (auth, api) AND updated > -7d" },
  { label: "Hot bugs", dql: "type = bug AND priority IN (p0, p1)" },
  { label: "Title contains “login”", dql: "title ~ login ORDER BY updated DESC LIMIT 20" },
];

export function SearchPane({
  q,
  onSearch,
}: {
  q: string;
  onSearch: (q: string) => void;
}) {
  const [input, setInput] = useState(q);

  // Following a palette jump or example click the route changes; keep the
  // text box in sync without fighting the typist (only when not focused).
  useEffect(() => {
    setInput(q);
  }, [q]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSearch(input);
  };

  return (
    <div className="flex flex-col gap-4 p-3">
      <form onSubmit={submit}>
        <div className="flex items-center gap-2 rounded-md border border-ctl bg-app px-2.5 transition-colors focus-within:border-accent">
          <Search className="size-4 shrink-0 text-zinc-500" aria-hidden />
          <input
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder="status != done AND …"
            aria-label="DQL query"
            spellCheck={false}
            className="h-[34px] w-full flex-1 bg-transparent font-mono text-[12.5px] text-zinc-200 placeholder:text-zinc-600 focus:outline-none"
          />
          <Kbd>⏎</Kbd>
        </div>
      </form>

      <section>
        <SectionHeading size="sm" className="mb-2 px-1">
          Examples
        </SectionHeading>
        <div className="flex flex-col">
          {EXAMPLES.map((example) => (
            <button
              key={example.dql}
              type="button"
              onClick={() => onSearch(example.dql)}
              title={example.dql}
              className="flex flex-col gap-0.5 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-card"
            >
              <span className="text-[12.5px] text-zinc-300">{example.label}</span>
              <span className="truncate font-mono text-[10.5px] text-zinc-600">
                {example.dql}
              </span>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
