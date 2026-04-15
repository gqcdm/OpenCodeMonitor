import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { AppSettings } from "@/types";
import { useSettingsFeaturesSection } from "./useSettingsFeaturesSection";

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn(async () => {
    throw new Error("no tauri opener");
  }),
}));

vi.mock("@services/tauri", () => ({
  getCodexConfigPath: vi.fn(async () => "/tmp/config.toml"),
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

describe("useSettingsFeaturesSection", () => {
  it("treats unavailable desktop openers as a no-op", async () => {
    let section: ReturnType<typeof useSettingsFeaturesSection> | null = null;

    function Harness() {
      section = useSettingsFeaturesSection({
        appSettings: baseSettings,
        hasCodexHomeOverrides: false,
        onUpdateAppSettings: vi.fn(async () => undefined),
      });
      return null;
    }

    renderToStaticMarkup(React.createElement(Harness));

    expect(section).not.toBeNull();

    await section!.onOpenConfig();

    expect(section!.openConfigError).toBeNull();
  });
});
