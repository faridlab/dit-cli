import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";

async function boot(): Promise<void> {
  // Development-only fixture server (?mock=1 in the dev URL) so the UI can
  // be hand-checked without the Rust backend. The guard is replaced with
  // `false` in a production build and the whole branch — including the mock
  // module — is dropped from the bundle.
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("mock")) {
    const { installMockApi } = await import("./lib/mock");
    installMockApi();
  }

  const root = document.getElementById("root");
  if (!root) throw new Error("#root missing");
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void boot();
