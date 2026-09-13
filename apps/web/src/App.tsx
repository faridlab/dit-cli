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
import { useTheme } from "./lib/theme";
import { useStarredSync } from "./lib/starred";

function isAuthExhausted(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}

export function App() {
  // Toasts follow the resolved theme; the rest of the UI themes itself via
  // the stylesheet and the data-theme stamp on <html>.
  const theme = useTheme();
  // Two tabs on the same workspace share one shortlist.
  useStarredSync();

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
      {/* Clear of the status bar: the bottom strip is the one place a
          person looks to confirm what a toast just claimed. */}
      <Toaster theme={theme.resolved} position="bottom-right" gap={6} offset={40} />
    </QueryClientProvider>
  );
}
