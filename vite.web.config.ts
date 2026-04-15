import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import packageJson from "./package.json";

const packageMetadata = packageJson as {
  version: string;
};

function fromProjectRoot(path: string) {
  return new URL(path, import.meta.url).pathname;
}

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fromProjectRoot("./src"),
      "@app": fromProjectRoot("./src/features/app"),
      "@settings": fromProjectRoot("./src/features/settings"),
      "@threads": fromProjectRoot("./src/features/threads"),
      "@services": fromProjectRoot("./src/services"),
      "@utils": fromProjectRoot("./src/utils"),
    },
  },
  worker: {
    format: "es",
  },
  define: {
    __APP_VERSION__: JSON.stringify(packageMetadata.version),
    __APP_COMMIT_HASH__: JSON.stringify("web"),
    __APP_BUILD_DATE__: JSON.stringify(new Date().toISOString()),
    __APP_GIT_BRANCH__: JSON.stringify("web"),
  },
  build: {
    outDir: "dist-web",
    rollupOptions: {
      input: fromProjectRoot("./index.web.html"),
    },
  },
});
