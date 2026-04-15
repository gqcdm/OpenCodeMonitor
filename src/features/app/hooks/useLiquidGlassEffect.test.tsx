/* @vitest-environment jsdom */
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("tauri-plugin-liquid-glass-api", () => ({
  isGlassSupported: vi.fn(async () => false),
  setLiquidGlassEffect: vi.fn(async () => undefined),
  GlassMaterialVariant: {
    Regular: "regular",
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  Effect: {
    HudWindow: "hud",
  },
  EffectState: {
    Active: "active",
  },
  getCurrentWindow: vi.fn(() => {
    throw new Error("no tauri window");
  }),
}));

import { useLiquidGlassEffect } from "./useLiquidGlassEffect";

describe("useLiquidGlassEffect", () => {
  it("becomes a no-op when the Tauri window is unavailable", async () => {
    const onDebug = vi.fn();

    renderHook(() =>
      useLiquidGlassEffect({
        reduceTransparency: false,
        onDebug,
      }),
    );

    await waitFor(() => {
      expect(onDebug).not.toHaveBeenCalled();
    });
  });
});
