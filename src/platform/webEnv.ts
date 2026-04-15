export type WebPlatformEnv = {
  mode: "web";
  apiBaseUrl: string;
  wsBaseUrl: string;
};

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}

function normalizeBaseUrl(value: string, fallback: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return fallback;
  }
  return trimTrailingSlash(trimmed);
}

export function readWebPlatformEnv(env: ImportMetaEnv = import.meta.env): WebPlatformEnv {
  return {
    mode: "web",
    apiBaseUrl: normalizeBaseUrl(env.VITE_WEB_API_BASE_URL ?? "", "/api"),
    wsBaseUrl: normalizeBaseUrl(env.VITE_WEB_WS_BASE_URL ?? "", "/ws"),
  };
}
