// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: vi.fn(() => {
    throw new Error("no tauri webview");
  }),
}));

import { useUiScaleShortcuts } from "./useUiScaleShortcuts";
import type { AppSettings } from "../../../types";

function createSettings(): AppSettings {
  return {
    codexBin: null,
    codexArgs: null,
    backendMode: "local",
    remoteBackendProvider: "tcp",
    remoteBackendHost: "",
    remoteBackendToken: null,
    orbitWsUrl: null,
    orbitAuthUrl: null,
    orbitRunnerName: null,
    orbitAutoStartRunner: false,
    keepDaemonRunningAfterAppClose: false,
    orbitUseAccess: false,
    orbitAccessClientId: null,
    orbitAccessClientSecretRef: null,
    defaultAccessMode: "current",
    reviewDeliveryMode: "inline",
    composerModelShortcut: null,
    composerAccessShortcut: null,
    composerReasoningShortcut: null,
    composerCollaborationShortcut: null,
    interruptShortcut: null,
    newAgentShortcut: null,
    newWorktreeAgentShortcut: null,
    newCloneAgentShortcut: null,
    archiveThreadShortcut: null,
    toggleProjectsSidebarShortcut: null,
    toggleGitSidebarShortcut: null,
    branchSwitcherShortcut: null,
    toggleDebugPanelShortcut: null,
    toggleTerminalShortcut: null,
    cycleAgentNextShortcut: null,
    cycleAgentPrevShortcut: null,
    cycleWorkspaceNextShortcut: null,
    cycleWorkspacePrevShortcut: null,
    lastComposerModelId: null,
    lastComposerReasoningEffort: null,
    uiScale: 1,
    theme: "system",
    usageShowRemaining: false,
    showMessageFilePath: true,
    threadTitleAutogenerationEnabled: false,
    uiFontFamily:
      'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif',
    codeFontFamily:
      'ui-monospace, "Cascadia Mono", "Segoe UI Mono", Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    codeFontSize: 11,
    notificationSoundsEnabled: true,
    systemNotificationsEnabled: true,
    splitChatDiffView: false,
    preloadGitDiffs: true,
    gitDiffIgnoreWhitespaceChanges: false,
    commitMessagePrompt: "",
    commitMessageModelId: null,
    experimentalCollabEnabled: false,
    collaborationModesEnabled: true,
    steerEnabled: true,
    unifiedExecEnabled: true,
    experimentalAppsEnabled: false,
    personality: "friendly",
    dictationEnabled: false,
    dictationModelId: "base",
    dictationPreferredLanguage: null,
    dictationHoldKey: null,
    composerEditorPreset: "default",
    composerFenceExpandOnSpace: false,
    composerFenceExpandOnEnter: false,
    composerFenceLanguageTags: false,
    composerFenceWrapSelection: false,
    composerFenceAutoWrapPasteMultiline: false,
    composerFenceAutoWrapPasteCodeLike: false,
    composerListContinuation: false,
    composerCodeBlockCopyUseModifier: false,
    workspaceGroups: [],
    openAppTargets: [
      {
        id: "vscode",
        label: "VS Code",
        kind: "app",
        appName: "Visual Studio Code",
        command: null,
        args: [],
      },
    ],
    selectedOpenAppId: "vscode",
  };
}

function Harness() {
  useUiScaleShortcuts({
    settings: createSettings(),
    setSettings: () => undefined,
    saveSettings: async (next) => next,
  });
  return null;
}

describe("useUiScaleShortcuts", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("does not crash when Tauri webview is unavailable", async () => {
    const container = document.createElement("div");
    const root = createRoot(container);

    await act(async () => {
      root.render(<Harness />);
    });

    expect(container.innerHTML).toBe("");

    await act(async () => {
      root.unmount();
    });
  });
});
