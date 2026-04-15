/** @vitest-environment jsdom */
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FileTreePanel } from "./FileTreePanel";

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

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: vi.fn(({ count }: { count: number }) => ({
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        index,
        key: `row-${index}`,
        size: 28,
        start: index * 28,
        end: index * 28 + 28,
      })),
    getTotalSize: () => count * 28,
  })),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => {
    throw new Error("no tauri window");
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn((value: string) => value),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalPosition: class LogicalPosition {
    constructor(
      public x: number,
      public y: number,
    ) {}
  },
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn(),
}));

vi.mock("../../../services/tauri", () => ({
  readWorkspaceFile: vi.fn(),
}));

vi.mock("../../../services/toasts", () => ({
  pushErrorToast: vi.fn(),
}));

describe("FileTreePanel", () => {
  it("shows a toast when desktop file menus are unavailable", async () => {
    const { pushErrorToast } = await import("../../../services/toasts");
    const { container } = render(
      <FileTreePanel
        workspaceId="ws-1"
        workspacePath="/tmp/repo"
        files={["sample.ts"]}
        modifiedFiles={[]}
        isLoading={false}
        filePanelMode="files"
        onFilePanelModeChange={vi.fn()}
        onInsertText={vi.fn()}
        canInsertText={true}
        openTargets={[]}
        openAppIconById={{}}
        selectedOpenAppId="vscode"
        onSelectOpenAppId={vi.fn()}
      />,
    );

    const row = container.querySelector(".file-tree-row");
    expect(row).not.toBeNull();
    fireEvent.contextMenu(row as Element);

    await waitFor(() => {
      expect(pushErrorToast).toHaveBeenCalledWith({
        title: "Context menu unavailable",
        message: "Desktop file menus are unavailable in this environment.",
      });
    });
  });
});
