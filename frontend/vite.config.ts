import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The Rust backend listens on 127.0.0.1:8097 by default. Requests to these
// API prefixes are proxied to it in development, so the frontend can use
// relative URLs and the backend never needs CORS enabled. Override with
// VITE_API_TARGET if the backend runs elsewhere.
const backend = process.env.VITE_API_TARGET || "http://127.0.0.1:8097";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/health": backend,
      "/System": backend,
      "/Users": backend,
      "/Sessions": backend,
      "/Library": backend,
      "/ScheduledTasks": backend,
      "/Items": backend,
      "/Recommendations": backend,
      "/Videos": backend,
      "/Audio": backend,
      "/Playlists": backend,
      "/RemoteServers": backend,
      "/RemoteItems": backend,
    },
  },
});
