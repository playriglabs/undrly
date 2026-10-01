import tailwindcss from "@tailwindcss/vite";
import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import viteReact from "@vitejs/plugin-react";
import { nitro } from "nitro/vite";
import { defineConfig } from "vite";

export default defineConfig({
  // Pre-bundle client-only libraries at startup: discovering one mid-session
  // re-optimizes deps and leaves the open tab with two copies of React.
  optimizeDeps: { include: ["lightweight-charts", "better-auth/react", "clsx"] },
  // tanstackStart must come before react().
  // nitro builds a Node server into .output/ (static assets included).
  plugins: [tailwindcss(), tanstackStart(), nitro(), viteReact()],
});
