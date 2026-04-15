import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppSettings } from "@/types";
import { useSettingsCodexSection } from "./useSettingsCodexSection";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => {
    throw new Error("no tauri dialog");
  }),
}));

vi.mock("@services/tauri", () => ({
  getOpenCodeServerStatus: vi.fn(async () => ({
    baseUrl: "http://127.0.0.1:14096",
    healthy: true,
    managed: true,
    source: "managed",
    version: "test",
  })),
  restartOpenCodeServer: vi.fn(async () => ({ restarted: true })),
  takeoverOpenCodeServer: vi.fn(async () => ({ takenOver: true })),
}));

vi.mock("./useGlobalAgentsMd", () => ({
  useGlobalAgentsMd: vi.fn(() => ({
    content: "",
    exists: false,
    truncated: false,
    isLoading: false,
    isSaving: false,
    error: null,
    isDirty: false,
    setContent: vi.fn(),
    refresh: vi.fn(),
    save: vi.fn(),
  })),
}));

vi.mock("./useGlobalCodexConfigToml", () => ({
  useGlobalOpenCodeConfig: vi.fn(() => ({
    content: "",
    exists: false,
    truncated: false,
    isLoading: false,
    isSaving: false,
    error: null,
    isDirty: false,
    setContent: vi.fn(),
    refresh: vi.fn(),
    save: vi.fn(),
  })),
}));

vi.mock("./useSettingsDefaultModels", () => ({
  useSettingsDefaultModels: vi.fn(() => ({
    models: [],
    isLoading: false,
    error: null,
    connectedWorkspaceCount: 0,
    refresh: vi.fn(),
  })),
}));

vi.mock("@settings/components/settingsViewHelpers", () => ({
  buildEditorContentMeta: vi.fn(() => ""),
  buildWorkspaceOverrideDrafts: vi.fn((_projects, prev) => prev ?? {}),
}));

vi.mock("@services/opencodeRestartNotice", () => ({
  clearOpenCodeRestartRequired: vi.fn(),
  markOpenCodeRestartRequired: vi.fn(),
  notifyOpenCodeServerRestarted: vi.fn(),
}));

const baseSettings: AppSettings = {
  codexBin: null,
  codexArgs: null,
  backendMode: "local",
  remoteBackendProvider: "tcp",
  remoteBackendHost: "127.0.0.1:4732",
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
  uiFontFamily: "system-ui",
  codeFontFamily: "monospace",
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
  openAppTargets: [],
  selectedOpenAppId: "vscode",
};

describe("useSettingsCodexSection", () => {
  it("treats unavailable codex picker dialogs as a no-op", async () => {
    let section: ReturnType<typeof useSettingsCodexSection> | null = null;

    function Harness() {
      section = useSettingsCodexSection({
        appSettings: baseSettings,
        projects: [],
        activeWorkspaceId: null,
        onUpdateAppSettings: vi.fn(async () => undefined),
        onRunDoctor: vi.fn(async () => ({
          ok: true,
          codexBin: null,
          version: null,
          appServerOk: true,
          details: null,
          path: null,
          nodeOk: true,
          nodeVersion: null,
          nodeDetails: null,
        })),
        onUpdateWorkspaceCodexBin: vi.fn(async () => undefined),
        onUpdateWorkspaceSettings: vi.fn(async () => undefined),
      });
      return null;
    }

    renderToStaticMarkup(React.createElement(Harness));

    expect(section).not.toBeNull();

    await section!.onBrowseCodex();

    expect(open).toHaveBeenCalledWith({ multiple: false, directory: false });
    expect(section!.codexPathDraft).toBe("");
  });
});
