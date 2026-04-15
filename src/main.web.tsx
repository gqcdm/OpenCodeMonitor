import React from "react";
import ReactDOM from "react-dom/client";
import { readWebPlatformEnv } from "./platform/webEnv";

type TauriInternalsMetadata = {
  currentWindow: {
    label: string;
  };
  currentWebview: {
    label: string;
  };
};

type TauriInternalsShim = {
  transformCallback?: (callback: unknown, once?: boolean) => number;
  unregisterCallback?: (callbackId: number) => void;
  invoke?: (cmd: string, args?: Record<string, unknown>, options?: unknown) => Promise<unknown>;
  convertFileSrc?: (filePath: string, protocol?: string) => string;
  plugins?: {
    metadata?: TauriInternalsMetadata;
  };
  metadata: TauriInternalsMetadata;
};

type TauriEventPluginInternalsShim = {
  unregisterListener: (event: string, eventId: number) => void;
};

function buildWebInvokeShim() {
  return async (cmd: string) => {
    if (
      cmd.includes("get_all_windows") ||
      cmd.includes("get_all_webviews") ||
      cmd === "plugin:event|listen"
    ) {
      return [];
    }
    if (
      cmd.includes("register_listener") ||
      cmd.includes("registerListener") ||
      cmd === "plugin:event|unlisten"
    ) {
      return 0;
    }
    return null;
  };
}

function ensureWebTauriMetadataShim() {
  if (typeof window === "undefined") {
    return;
  }

  const existing = window as Window & {
    __TAURI_INTERNALS__?: Partial<TauriInternalsShim>;
    __TAURI_EVENT_PLUGIN_INTERNALS__?: TauriEventPluginInternalsShim;
  };

  if (!existing.__TAURI_EVENT_PLUGIN_INTERNALS__) {
    existing.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {
        // No-op in browser scaffold mode.
      },
    };
  }

  if (existing.__TAURI_INTERNALS__?.metadata?.currentWindow?.label) {
    if (!existing.__TAURI_INTERNALS__.metadata.currentWebview?.label) {
      existing.__TAURI_INTERNALS__.metadata.currentWebview = {
        label: "web-main",
      };
    }
    if (!existing.__TAURI_INTERNALS__.plugins?.metadata?.currentWebview?.label) {
      existing.__TAURI_INTERNALS__.plugins = {
        ...(existing.__TAURI_INTERNALS__.plugins ?? {}),
        metadata: {
          currentWindow: {
            label: existing.__TAURI_INTERNALS__.metadata.currentWindow.label,
          },
          currentWebview: {
            label: existing.__TAURI_INTERNALS__.metadata.currentWebview.label,
          },
        },
      };
    }
    if (!existing.__TAURI_INTERNALS__.transformCallback) {
      let callbackId = 1;
      existing.__TAURI_INTERNALS__.transformCallback = () => callbackId++;
    }
    if (!existing.__TAURI_INTERNALS__.unregisterCallback) {
      existing.__TAURI_INTERNALS__.unregisterCallback = () => {
        // No-op in browser scaffold mode.
      };
    }
    if (!existing.__TAURI_INTERNALS__.invoke) {
      existing.__TAURI_INTERNALS__.invoke = buildWebInvokeShim();
    }
    if (!existing.__TAURI_INTERNALS__.convertFileSrc) {
      existing.__TAURI_INTERNALS__.convertFileSrc = (filePath) => filePath;
    }
    return;
  }

  existing.__TAURI_INTERNALS__ = {
    transformCallback: (() => {
      let callbackId = 1;
      return () => callbackId++;
    })(),
    unregisterCallback: () => {
      // No-op in browser scaffold mode.
    },
    invoke: buildWebInvokeShim(),
    convertFileSrc: (filePath) => filePath,
    ...(existing.__TAURI_INTERNALS__ ?? {}),
    plugins: {
      ...(existing.__TAURI_INTERNALS__?.plugins ?? {}),
      metadata: {
        currentWindow: {
          label: "web-main",
        },
        currentWebview: {
          label: "web-main",
        },
      },
    },
    metadata: {
      currentWindow: {
        label: "web-main",
      },
      currentWebview: {
        label: "web-main",
      },
    },
  };
}

readWebPlatformEnv();
ensureWebTauriMetadataShim();

async function bootstrapWebApp() {
  const { default: App } = await import("./App");
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void bootstrapWebApp();
