/** @vitest-environment jsdom */
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PromptPanel } from "./PromptPanel";

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

vi.mock("../../../services/toasts", () => ({
  pushErrorToast: vi.fn(),
}));

describe("PromptPanel", () => {
  it("shows a toast when desktop prompt menus are unavailable", async () => {
    const { pushErrorToast } = await import("../../../services/toasts");
    const { getByRole } = render(
      <PromptPanel
        prompts={[
          {
            name: "prompt-one",
            path: "/tmp/prompt.md",
            scope: "workspace",
            description: "Example prompt",
            argumentHint: undefined,
            content: "Prompt body",
          },
        ]}
        workspacePath="/tmp/repo"
        filePanelMode="prompts"
        onFilePanelModeChange={vi.fn()}
        onSendPrompt={vi.fn()}
        onSendPromptToNewAgent={vi.fn()}
        onCreatePrompt={vi.fn()}
        onUpdatePrompt={vi.fn()}
        onDeletePrompt={vi.fn()}
        onMovePrompt={vi.fn()}
        onRevealWorkspacePrompts={vi.fn()}
        onRevealGeneralPrompts={vi.fn()}
        canRevealGeneralPrompts={true}
      />,
    );

    fireEvent.click(getByRole("button", { name: "Prompt actions" }));

    await waitFor(() => {
      expect(pushErrorToast).toHaveBeenCalledWith({
        title: "Context menu unavailable",
        message: "Desktop prompt menus are unavailable in this environment.",
      });
    });
  });
});
