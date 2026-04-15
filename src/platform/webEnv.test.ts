import { describe, expect, it } from "vitest";
import { readWebPlatformEnv } from "./webEnv";

describe("readWebPlatformEnv", () => {
  it("uses defaults when env vars are missing", () => {
    const env = readWebPlatformEnv({} as ImportMetaEnv);
    expect(env.mode).toBe("web");
    expect(env.apiBaseUrl).toBe("/api");
    expect(env.wsBaseUrl).toBe("/ws");
  });

  it("normalizes configured base urls", () => {
    const env = readWebPlatformEnv({
      VITE_WEB_API_BASE_URL: "https://example.com/api/",
      VITE_WEB_WS_BASE_URL: "wss://example.com/ws///",
    } as ImportMetaEnv);
    expect(env.apiBaseUrl).toBe("https://example.com/api");
    expect(env.wsBaseUrl).toBe("wss://example.com/ws");
  });
});
