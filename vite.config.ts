import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

const packageJson = JSON.parse(
  readFileSync(new URL("./package.json", import.meta.url), "utf-8"),
) as {
  version: string;
};

function getGitInfo() {
  try {
    const commitHash = execSync("git rev-parse --short HEAD", { encoding: "utf-8" }).trim();
    const gitBranch = execSync("git rev-parse --abbrev-ref HEAD", { encoding: "utf-8" }).trim();
    const commitDate = execSync("git log -1 --format=%ci", { encoding: "utf-8" }).trim();
    const buildDate = new Date().toISOString();
    return { commitHash, gitBranch, commitDate, buildDate };
  } catch {
    return {
      commitHash: "unknown",
      gitBranch: "unknown",
      commitDate: "unknown",
      buildDate: new Date().toISOString(),
    };
  }
}

const gitInfo = getGitInfo();

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      "@app": fileURLToPath(new URL("./src/features/app", import.meta.url)),
      "@settings": fileURLToPath(new URL("./src/features/settings", import.meta.url)),
      "@threads": fileURLToPath(new URL("./src/features/threads", import.meta.url)),
      "@services": fileURLToPath(new URL("./src/services", import.meta.url)),
      "@utils": fileURLToPath(new URL("./src/utils", import.meta.url)),
    },
  },
  worker: {
    format: "es",
  },
  define: {
    __APP_VERSION__: JSON.stringify(packageJson.version),
    __APP_COMMIT_HASH__: JSON.stringify(gitInfo.commitHash),
    __APP_BUILD_DATE__: JSON.stringify(gitInfo.buildDate),
    __APP_GIT_BRANCH__: JSON.stringify(gitInfo.gitBranch),
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    setupFiles: ["src/test/vitest.setup.ts"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**", "**/.codex-worktrees/**"],
    },
  },
}));
