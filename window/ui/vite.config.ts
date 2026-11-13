import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // The shell serves these files itself, from `tauri://localhost/`.
  base: "./",
  // One local file; its size is not a download.
  build: { outDir: "dist", emptyOutDir: true, modulePreload: { polyfill: false }, chunkSizeWarningLimit: 2000 },
});
