import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The UI is served by dit-server from the same origin in production (embedded
// via rust-embed), so there is no CORS anywhere. In dev we proxy to the local
// server instead of enabling CORS — keeping dev and prod on the same
// same-origin assumption. See DESIGN.md §6.5.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: { outDir: "dist", emptyOutDir: true },
  server: {
    port: 5173,
    proxy: {
      // ws: true because the live-updates WebSocket also lives under /api
      // (browsers cannot set headers on a WS handshake, so the token rides
      // in the query string and the proxy must upgrade the connection).
      "/api": { target: "http://127.0.0.1:7433", changeOrigin: false, ws: true },
      "/events": { target: "ws://127.0.0.1:7433", ws: true },
    },
  },
});
