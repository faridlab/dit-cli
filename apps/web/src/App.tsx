// Application root. Two jobs only: own the query client, and decide whether
// a session token exists (URL fragment -> sessionStorage -> gate).
//
// Rules this UI must never break, restated where they bite:
//  - The auth token rides the Authorization header (never a cookie, so a
//    malicious page cannot make the browser send it cross-origin).
//  - Markdown is rendered by the server; the client only injects HTML the
//    server already sanitized.
//  - Heavy editors load lazily so the first paint stays small.

import { useMemo, useState } from "react";
import {
  MutationCache,
  QueryCache,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { Toaster } from "sonner";
import { AppShell } from "./components/AppShell";
import { TokenGate } from "./components/TokenGate";
import { ApiError } from "./lib/api";
import { captureTokenFromLocation, clearToken, getToken, setToken } from "./lib/auth";

function isAuthExhausted(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}

export function App() {
  // The token may arrive as `#token=...` on the very first load. Capture and
  // scrub it before anything renders with a half-parsed fragment URL.
  const [unlocked, setUnlocked] = useState(() => {
    captureTokenFromLocation();
    return getToken() !== null;
  });

  // A rejected token means the session ended server-side (server restarted).
  // Drop the stale token instead of hammering 401s; both caches share the
  // reaction so queries and mutations behave the same way.
  const onCacheError = (error: unknown) => {
    if (isAuthExhausted(error)) {
      clearToken();
      setUnlocked(false);
    }
  };

  const queryClient = useMemo(
    () =>
      new QueryClient({
        queryCache: new QueryCache({ onError: onCacheError }),
        mutationCache: new MutationCache({ onError: onCacheError }),
        defaultOptions: {
          queries: {
            // One retry absorbs a server restart blip; more just delays the
            // error state people need to see.
            retry: 1,
            refetchOnWindowFocus: false,
          },
        },
      }),
    [],
  );

  return (
    <QueryClientProvider client={queryClient}>
      {unlocked ? (
        <AppShell />
      ) : (
        <TokenGate
          onUnlocked={(token) => {
            setToken(token);
            setUnlocked(true);
          }}
        />
      )}
      <Toaster theme="dark" position="bottom-right" gap={6} />
    </QueryClientProvider>
  );
}
