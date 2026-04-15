/* @vitest-environment jsdom */
import { renderHook } from "@testing-library/react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { describe, expect, it, vi } from "vitest";

const menuNew = vi.hoisted(() =>
  vi.fn(async () => {
    throw new Error("no tauri menu");
  }),
);
const menuItemNew = vi.hoisted(() => vi.fn(async (options) => options));
const predefinedMenuItemNew = vi.hoisted(() => vi.fn(async (options) => options));

vi.mock("@tauri-apps/api/menu", () => ({
  Menu: { new: menuNew },
  MenuItem: { new: menuItemNew },
  PredefinedMenuItem: { new: predefinedMenuItemNew },
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalPosition: class LogicalPosition {
    constructor(
      public x: number,
      public y: number,
    ) {}
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => {
    throw new Error("no tauri window");
  }),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn(),
}));

vi.mock("../../../services/tauri", () => ({
  openWorkspaceIn: vi.fn(),
}));

vi.mock("../../../services/toasts", () => ({
  pushErrorToast: vi.fn(),
}));

import { pushErrorToast } from "../../../services/toasts";
import { useFileLinkOpener } from "./useFileLinkOpener";

describe("useFileLinkOpener", () => {
  it("shows a toast when desktop file menus are unavailable", async () => {
    const { result } = renderHook(() => useFileLinkOpener("/workspace", [], "vscode"));

    const event = {
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
      clientX: 10,
      clientY: 20,
    } as unknown as ReactMouseEvent;

    await expect(result.current.showFileLinkMenu(event, "foo.txt")).resolves.toBeUndefined();

    expect(pushErrorToast).toHaveBeenCalledWith({
      title: "Context menu unavailable",
      message: "Desktop file menus are unavailable in this environment.",
    });
  });
});
