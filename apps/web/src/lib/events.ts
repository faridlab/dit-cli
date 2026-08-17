// Live updates. The server pushes a tiny message whenever the index changes
// (its file watcher noticed a commit); the UI answers by refetching whatever
// TanStack Query has cached. Reconnects with exponential backoff so a busy
// or restarting server does not hammer it or give up.

import { useEffect, useRef, useState } from "react";
import { eventsUrl } from "./api";

export type ConnectionState = "off" | "connecting" | "live" | "retrying";

const MAX_BACKOFF_MS = 15_000;

export function useLiveEvents(onIndexUpdated: () => void): ConnectionState {
  const [state, setState] = useState<ConnectionState>("off");
  // Keep the latest callback without reconnecting on every render of the
  // parent (the query client reference is stable, but stay defensive).
  const callbackRef = useRef(onIndexUpdated);
  callbackRef.current = onIndexUpdated;

  useEffect(() => {
    const url = eventsUrl();
    if (!url) {
      setState("off");
      return;
    }

    let disposed = false;
    let socket: WebSocket | null = null;
    let timer: number | undefined;
    let attempt = 0;

    const scheduleReconnect = () => {
      if (disposed) return;
      setState("retrying");
      const delay = Math.min(MAX_BACKOFF_MS, 1000 * 2 ** attempt);
      attempt += 1;
      timer = window.setTimeout(connect, delay);
    };

    const connect = () => {
      if (disposed) return;
      setState(attempt === 0 ? "connecting" : "retrying");
      try {
        socket = new WebSocket(url);
      } catch {
        // Constructor throws only on malformed URLs — treat like a drop.
        scheduleReconnect();
        return;
      }
      socket.onopen = () => {
        attempt = 0;
        setState("live");
      };
      socket.onmessage = (message) => {
        try {
          const parsed = JSON.parse(String(message.data)) as { type?: unknown };
          if (parsed?.type === "index_updated") callbackRef.current();
        } catch {
          // Anything that is not JSON is ignored — the server may add
          // heartbeat or diagnostic frames later.
        }
      };
      socket.onclose = () => scheduleReconnect();
      socket.onerror = () => {
        // onerror is always followed by onclose, which schedules the retry.
        socket?.close();
      };
    };

    connect();
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
      socket?.close();
    };
  }, []);

  return state;
}
