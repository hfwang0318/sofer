import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri needs a fixed port and must not clear the terminal it shares with cargo.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  build: {
    target: "safari15",
    sourcemap: false,
  },
});
