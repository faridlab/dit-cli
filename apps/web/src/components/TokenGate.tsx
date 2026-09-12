// Full-page gate shown when no session token exists. `dit serve` normally
// lands the user here with the token already in the URL fragment; this page
// covers the reload-after-sessionStorage-was-cleared case, and the case of
// a second browser that was handed the URL without the fragment.

import { type FormEvent, useState } from "react";

export function TokenGate({
  onUnlocked,
}: {
  // Receives the pasted token so the caller decides where it is stored.
  onUnlocked: (token: string) => void;
}) {
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const token = value.trim();
    if (token.length === 0) {
      setError("Paste the token from the URL `dit serve` printed.");
      return;
    }
    setError(null);
    onUnlocked(token);
  };

  return (
    <main className="flex min-h-dvh items-center justify-center bg-app p-6">
      <div className="w-full max-w-sm rounded-lg border border-edge bg-card p-6">
        <h1 className="text-lg font-semibold text-ink">DIT</h1>
        <p className="mt-2 text-sm text-ink-2">
          Open the URL printed by <code className="font-mono text-xs text-ink-2">dit serve</code>{" "}
          in this browser, or paste your session token below.
        </p>
        <form onSubmit={submit} className="mt-4 flex flex-col gap-2">
          <input
            type="password"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder="Session token"
            autoComplete="off"
            autoFocus
            aria-label="Session token"
            className="h-8 rounded border border-ctl bg-app px-2 font-mono text-xs text-ink placeholder:text-faint focus:border-accent focus:outline-none"
          />
          {error ? <p className="text-xs text-crit-text">{error}</p> : null}
          <button
            type="submit"
            className="h-8 rounded bg-accent text-xs font-medium text-on-accent hover:bg-accent-hi"
          >
            Unlock workspace
          </button>
        </form>
        <p className="mt-3 text-[11px] text-faint">
          The token is kept in sessionStorage for this tab only — closing the tab logs you out.
        </p>
      </div>
    </main>
  );
}
