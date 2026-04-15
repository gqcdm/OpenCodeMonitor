import { describe, expect, it, vi } from "vitest";
import { ask, open } from "@tauri-apps/plugin-dialog";
import type { AppSettings, WorkspaceGroup, WorkspaceInfo } from "@/types";
import { useSettingsProjectsSection } from "./useSettingsProjectsSection";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: vi.fn(),
  open: vi.fn(),
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

const group: WorkspaceGroup = {
  id: "group-1",
  name: "Group 1",
  sortOrder: 0,
  copiesFolder: "/tmp/copies",
};

const groupedWorkspaces = [{ id: group.id, name: group.name, workspaces: [] as WorkspaceInfo[] }];

describe("useSettingsProjectsSection", () => {
  it("treats unavailable desktop dialogs as a no-op", async () => {
    vi.mocked(open).mockRejectedValueOnce(new Error("no tauri dialog"));
    vi.mocked(ask).mockRejectedValueOnce(new Error("no tauri dialog"));

    const onUpdateAppSettings = vi.fn();
    const onDeleteWorkspaceGroup = vi.fn();

    let section:
      | ReturnType<typeof useSettingsProjectsSection>
      | null = null;

    function Harness() {
      section = useSettingsProjectsSection({
        appSettings: baseSettings,
        workspaceGroups: [group],
        groupedWorkspaces,
        ungroupedLabel: "Ungrouped",
        projects: [],
        onUpdateAppSettings,
        onMoveWorkspace: vi.fn(),
        onDeleteWorkspace: vi.fn(),
        onCreateWorkspaceGroup: vi.fn(),
        onRenameWorkspaceGroup: vi.fn(),
        onMoveWorkspaceGroup: vi.fn(),
        onDeleteWorkspaceGroup,
        onAssignWorkspaceGroup: vi.fn(),
      });
      return null;
    }

    renderToStaticMarkup(React.createElement(Harness));

    expect(section).not.toBeNull();

    await expect(section!.onChooseGroupCopiesFolder(group)).resolves.toBeUndefined();
    await expect(section!.onDeleteGroup(group)).resolves.toBeUndefined();

    expect(onUpdateAppSettings).not.toHaveBeenCalled();
    expect(onDeleteWorkspaceGroup).not.toHaveBeenCalled();
  });
});
