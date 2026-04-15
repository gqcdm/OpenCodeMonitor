/** @vitest-environment jsdom */
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ComposerQueue } from "./ComposerQueue";

const menuNew = vi.hoisted(() => vi.fn(async () => ({ popup: vi.fn() })));
const menuItemNew = vi.hoisted(() =>
  vi.fn(async () => {
    throw new Error("no tauri menu item");
  }),
);

vi.mock("@tauri-apps/api/menu", () => ({
  Menu: { new: menuNew },
  MenuItem: { new: menuItemNew },
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => {
    throw new Error("no tauri window");
  }),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalPosition: class LogicalPosition {
    constructor(
      public x: number,
      public y: number,
    ) {}
  },
}));

vi.mock("../../../services/toasts", () => ({
  pushErrorToast: vi.fn(),
}));

describe("ComposerQueue", () => {
  it("shows a toast when desktop queue menus are unavailable", async () => {
    const { pushErrorToast } = await import("../../../services/toasts");
    const { getByRole } = render(
      <ComposerQueue
        queuedMessages={[
          { id: "q1", text: "Queued item", images: [], createdAt: Date.now() },
        ]}
      />,
    );

    fireEvent.click(getByRole("button", { name: "Queue item menu" }));

    await waitFor(() => {
      expect(pushErrorToast).toHaveBeenCalledWith({
        title: "Context menu unavailable",
        message: "Desktop queue menus are unavailable in this environment.",
      });
    });
  });
});
