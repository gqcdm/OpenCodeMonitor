import { useEffect, useMemo, useRef } from "react";
import Stethoscope from "lucide-react/dist/esm/icons/stethoscope";
import type { Dispatch, SetStateAction } from "react";
import type {
  AppSettings,
  CodexDoctorResult,
  CodexUpdateResult,
  ModelOption,
  WorkspaceInfo,
} from "@/types";
import type { OpenCodeServerStatus } from "@services/tauri";
import { FileEditorCard } from "@/features/shared/components/FileEditorCard";
import { groupModelsByProvider } from "@/features/models/utils/groupModelsByProvider";

type SettingsCodexSectionProps = {
  appSettings: AppSettings;
  onUpdateAppSettings: (next: AppSettings) => Promise<void>;
  defaultModels: ModelOption[];
  defaultModelsLoading: boolean;
  defaultModelsError: string | null;
  defaultModelsConnectedWorkspaceCount: number;
  onRefreshDefaultModels: () => void;
  opencodeServerStatus: OpenCodeServerStatus | null;
  opencodeServerStatusLoading: boolean;
  opencodeServerStatusError: string | null;
  opencodeServerStatusLastCheckedAt: number | null;
  opencodeServerRestarting: boolean;
  onRefreshOpenCodeServerStatus: () => void;
  onRestartOpenCodeServer: () => void;
  codexPathDraft: string;
  codexArgsDraft: string;
  codexDirty: boolean;
  isSavingSettings: boolean;
  doctorState: {
    status: "idle" | "running" | "done";
    result: CodexDoctorResult | null;
  };
  codexUpdateState: {
    status: "idle" | "running" | "done";
    result: CodexUpdateResult | null;
  };
  globalAgentsMeta: string;
  globalAgentsError: string | null;
  globalAgentsContent: string;
  globalAgentsLoading: boolean;
  globalAgentsRefreshDisabled: boolean;
  globalAgentsSaveDisabled: boolean;
  globalAgentsSaveLabel: string;
  globalConfigMeta: string;
  globalConfigError: string | null;
  globalConfigContent: string;
  globalConfigLoading: boolean;
  globalConfigRefreshDisabled: boolean;
  globalConfigSaveDisabled: boolean;
  globalConfigSaveLabel: string;
  projects: WorkspaceInfo[];
  codexBinOverrideDrafts: Record<string, string>;
  codexHomeOverrideDrafts: Record<string, string>;
  codexArgsOverrideDrafts: Record<string, string>;
  onSetCodexPathDraft: Dispatch<SetStateAction<string>>;
  onSetCodexArgsDraft: Dispatch<SetStateAction<string>>;
  onSetGlobalAgentsContent: (value: string) => void;
  onSetGlobalConfigContent: (value: string) => void;
  onSetCodexBinOverrideDrafts: Dispatch<SetStateAction<Record<string, string>>>;
  onSetCodexHomeOverrideDrafts: Dispatch<SetStateAction<Record<string, string>>>;
  onSetCodexArgsOverrideDrafts: Dispatch<SetStateAction<Record<string, string>>>;
  onBrowseCodex: () => Promise<void>;
  onSaveCodexSettings: () => Promise<void>;
  onRunDoctor: () => Promise<void>;
  onRunCodexUpdate: () => Promise<void>;
  onRefreshGlobalAgents: () => void;
  onSaveGlobalAgents: () => void;
  onRefreshGlobalConfig: () => void;
  onSaveGlobalConfig: () => void;
  onUpdateWorkspaceCodexBin: (id: string, codexBin: string | null) => Promise<void>;
  onUpdateWorkspaceSettings: (
    id: string,
    settings: Partial<WorkspaceInfo["settings"]>,
  ) => Promise<void>;
};

const normalizeOverrideValue = (value: string): string | null => {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
};

const DEFAULT_REASONING_EFFORT = "medium";

const normalizeEffortValue = (value: unknown): string | null => {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed.toLowerCase() : null;
};

function coerceSavedModelId(value: string | null, models: ModelOption[]): string | null {
  const trimmed = (value ?? "").trim();
  if (!trimmed) {
    return null;
  }
  const byId = models.find((model) => model.id === trimmed);
  if (byId) {
    return byId.id;
  }
  const bySlug = models.filter((model) => model.model === trimmed);
  if (bySlug.length === 0) {
    return null;
  }
  return (bySlug.find((model) => model.isDefault) ?? bySlug[0])?.id ?? null;
}

const getReasoningSupport = (model: ModelOption | null): boolean => {
  if (!model) {
    return false;
  }
  return model.supportedReasoningEfforts.length > 0 || model.defaultReasoningEffort !== null;
};

const getReasoningOptions = (model: ModelOption | null): string[] => {
  if (!model) {
    return [];
  }
  const supported = model.supportedReasoningEfforts
    .map((effort) => normalizeEffortValue(effort.reasoningEffort))
    .filter((effort): effort is string => Boolean(effort));
  if (supported.length > 0) {
    return Array.from(new Set(supported));
  }
  const fallback = normalizeEffortValue(model.defaultReasoningEffort);
  return fallback ? [fallback] : [];
};

export function SettingsCodexSection({
  appSettings,
  onUpdateAppSettings,
  defaultModels,
  defaultModelsLoading,
  defaultModelsError,
  defaultModelsConnectedWorkspaceCount,
  onRefreshDefaultModels,
  opencodeServerStatus,
  opencodeServerStatusLoading,
  opencodeServerStatusError,
  opencodeServerStatusLastCheckedAt,
  opencodeServerRestarting,
  onRefreshOpenCodeServerStatus,
  onRestartOpenCodeServer,
  codexPathDraft,
  codexArgsDraft,
  codexDirty,
  isSavingSettings,
  doctorState,
  codexUpdateState,
  globalAgentsMeta,
  globalAgentsError,
  globalAgentsContent,
  globalAgentsLoading,
  globalAgentsRefreshDisabled,
  globalAgentsSaveDisabled,
  globalAgentsSaveLabel,
  globalConfigMeta,
  globalConfigError,
  globalConfigContent,
  globalConfigLoading,
  globalConfigRefreshDisabled,
  globalConfigSaveDisabled,
  globalConfigSaveLabel,
  projects,
  codexBinOverrideDrafts,
  codexHomeOverrideDrafts,
  codexArgsOverrideDrafts,
  onSetCodexPathDraft,
  onSetCodexArgsDraft,
  onSetGlobalAgentsContent,
  onSetGlobalConfigContent,
  onSetCodexBinOverrideDrafts,
  onSetCodexHomeOverrideDrafts,
  onSetCodexArgsOverrideDrafts,
  onBrowseCodex,
  onSaveCodexSettings,
  onRunDoctor,
  onRunCodexUpdate,
  onRefreshGlobalAgents,
  onSaveGlobalAgents,
  onRefreshGlobalConfig,
  onSaveGlobalConfig,
  onUpdateWorkspaceCodexBin,
  onUpdateWorkspaceSettings,
}: SettingsCodexSectionProps) {
  const groupedDefaultModels = useMemo(() => groupModelsByProvider(defaultModels), [defaultModels]);
  const latestModelId = defaultModels[0]?.id ?? null;
  const savedModelId = useMemo(
    () => coerceSavedModelId(appSettings.lastComposerModelId, defaultModels),
    [appSettings.lastComposerModelId, defaultModels],
  );
  const selectedModelId = savedModelId ?? latestModelId ?? "";
  const selectedModel = useMemo(
    () => defaultModels.find((model) => model.id === selectedModelId) ?? null,
    [defaultModels, selectedModelId],
  );
  const reasoningSupported = useMemo(
    () => getReasoningSupport(selectedModel),
    [selectedModel],
  );
  const reasoningOptions = useMemo(
    () => getReasoningOptions(selectedModel),
    [selectedModel],
  );
  const savedEffort = useMemo(
    () => normalizeEffortValue(appSettings.lastComposerReasoningEffort),
    [appSettings.lastComposerReasoningEffort],
  );
  const selectedEffort = useMemo(() => {
    if (!reasoningSupported) {
      return "";
    }
    if (savedEffort && reasoningOptions.includes(savedEffort)) {
      return savedEffort;
    }
    if (reasoningOptions.includes(DEFAULT_REASONING_EFFORT)) {
      return DEFAULT_REASONING_EFFORT;
    }
    const fallback = normalizeEffortValue(selectedModel?.defaultReasoningEffort);
    if (fallback && reasoningOptions.includes(fallback)) {
      return fallback;
    }
    return reasoningOptions[0] ?? "";
  }, [reasoningOptions, reasoningSupported, savedEffort, selectedModel]);

  const didNormalizeDefaultsRef = useRef(false);
  useEffect(() => {
    if (didNormalizeDefaultsRef.current) {
      return;
    }
    if (!defaultModels.length) {
      return;
    }
    const savedRawModel = (appSettings.lastComposerModelId ?? "").trim();
    const savedRawEffort = (appSettings.lastComposerReasoningEffort ?? "").trim();
    const shouldNormalizeModel = savedRawModel.length === 0 || savedModelId === null;
    const shouldNormalizeEffort =
      reasoningSupported &&
      (savedRawEffort.length === 0 ||
        savedEffort === null ||
        !reasoningOptions.includes(savedEffort));
    if (!shouldNormalizeModel && !shouldNormalizeEffort) {
      didNormalizeDefaultsRef.current = true;
      return;
    }

    const next: AppSettings = {
      ...appSettings,
      lastComposerModelId: shouldNormalizeModel ? selectedModelId : appSettings.lastComposerModelId,
      lastComposerReasoningEffort: shouldNormalizeEffort
        ? selectedEffort
        : appSettings.lastComposerReasoningEffort,
    };
    didNormalizeDefaultsRef.current = true;
    void onUpdateAppSettings(next);
  }, [
    appSettings,
    defaultModels.length,
    onUpdateAppSettings,
    reasoningOptions,
    reasoningSupported,
    savedEffort,
    savedModelId,
    selectedModelId,
    selectedEffort,
  ]);

  return (
    <section className="settings-section">
      <div className="settings-section-title">OpenCode</div>
      <div className="settings-section-subtitle">
        Configure the OpenCode CLI used by OpenCode Monitor and validate the
        install.
      </div>
      <div className="settings-field">
        <label className="settings-field-label" htmlFor="codex-path">
          Default OpenCode path
        </label>
        <div className="settings-field-row">
          <input
            id="codex-path"
            className="settings-input"
            value={codexPathDraft}
            placeholder="opencode"
            onChange={(event) => onSetCodexPathDraft(event.target.value)}
          />
          <button
            type="button"
            className="ghost"
            onClick={() => {
              void onBrowseCodex();
            }}
          >
            Browse
          </button>
          <button
            type="button"
            className="ghost"
            onClick={() => onSetCodexPathDraft("")}
          >
            Use PATH
          </button>
        </div>
        <div className="settings-help">Leave empty to use the system PATH resolution.</div>
        <label className="settings-field-label" htmlFor="codex-args">
          Default OpenCode args
        </label>
        <div className="settings-field-row">
          <input
            id="codex-args"
            className="settings-input"
            value={codexArgsDraft}
            placeholder="--profile personal"
            onChange={(event) => onSetCodexArgsDraft(event.target.value)}
          />
          <button
            type="button"
            className="ghost"
            onClick={() => onSetCodexArgsDraft("")}
          >
            Clear
          </button>
        </div>
        <div className="settings-help">
          Extra flags passed to <code>serve</code>. Use quotes for values with spaces.
        </div>
        <div className="settings-help">
          OpenCode server:{" "}
          <code>{opencodeServerStatus?.baseUrl ?? "http://127.0.0.1:14096"}</code>{" "}
          {opencodeServerStatusLoading
            ? "(checking...)"
            : opencodeServerStatus
              ? opencodeServerStatus.healthy
                ? `(${opencodeServerStatus.source}, healthy${
                    opencodeServerStatus.version ? `, v${opencodeServerStatus.version}` : ""
                  })`
                : `(${opencodeServerStatus.source}, unavailable)`
              : "(unknown)"}
        </div>
        {opencodeServerStatusLastCheckedAt && !opencodeServerStatusLoading && (
          <div className="settings-help">
            Last checked: {new Date(opencodeServerStatusLastCheckedAt).toLocaleTimeString()}
          </div>
        )}
        {opencodeServerStatusError && (
          <div className="settings-help" style={{ color: "var(--color-danger, #d55)" }}>
            Server status error: {opencodeServerStatusError}
          </div>
        )}
        <div className="settings-field-actions">
          {codexDirty && (
            <button
              type="button"
              className="primary"
              onClick={() => {
                void onSaveCodexSettings();
              }}
              disabled={isSavingSettings}
            >
              {isSavingSettings ? "Saving..." : "Save"}
            </button>
          )}
          <button
            type="button"
            className="ghost settings-button-compact"
            onClick={() => {
              void onRunDoctor();
            }}
            disabled={doctorState.status === "running"}
          >
            <Stethoscope aria-hidden />
            {doctorState.status === "running" ? "Running..." : "Run doctor"}
          </button>
          <button
            type="button"
            className="ghost settings-button-compact"
            onClick={() => {
              void onRunCodexUpdate();
            }}
            disabled={codexUpdateState.status === "running"}
            title="Update OpenCode"
          >
            <Stethoscope aria-hidden />
            {codexUpdateState.status === "running" ? "Updating..." : "Update"}
          </button>
          <button
            type="button"
            className="ghost settings-button-compact"
            onClick={onRefreshOpenCodeServerStatus}
            disabled={opencodeServerStatusLoading || opencodeServerRestarting}
            title="Refresh OpenCode server status"
          >
            {opencodeServerStatusLoading ? "Checking..." : "Server status"}
          </button>
          <button
            type="button"
            className="ghost settings-button-compact"
            onClick={onRestartOpenCodeServer}
            disabled={opencodeServerRestarting}
            title="Restart the OpenCode server used by OpenCode Monitor"
          >
            {opencodeServerRestarting ? "Restarting..." : "Restart server"}
          </button>
        </div>

        {doctorState.result && (
          <div className={`settings-doctor ${doctorState.result.ok ? "ok" : "error"}`}>
            <div className="settings-doctor-title">
              {doctorState.result.ok ? "OpenCode looks good" : "OpenCode issue detected"}
            </div>
            <div className="settings-doctor-body">
              <div>Version: {doctorState.result.version ?? "unknown"}</div>
              <div>Server: {doctorState.result.appServerOk ? "ok" : "failed"}</div>
              <div>
                Node:{" "}
                {doctorState.result.nodeOk
                  ? `ok (${doctorState.result.nodeVersion ?? "unknown"})`
                  : "missing"}
              </div>
              {doctorState.result.details && <div>{doctorState.result.details}</div>}
              {doctorState.result.nodeDetails && <div>{doctorState.result.nodeDetails}</div>}
              {doctorState.result.path && (
                <div className="settings-doctor-path">PATH: {doctorState.result.path}</div>
              )}
            </div>
          </div>
        )}

        {codexUpdateState.result && (
          <div
            className={`settings-doctor ${codexUpdateState.result.ok ? "ok" : "error"}`}
          >
            <div className="settings-doctor-title">
              {codexUpdateState.result.ok
                ? codexUpdateState.result.upgraded
                ? "OpenCode updated"
                   : "OpenCode already up-to-date"
                : "OpenCode update failed"}
            </div>
            <div className="settings-doctor-body">
              <div>Method: {codexUpdateState.result.method}</div>
              {codexUpdateState.result.package && (
                <div>Package: {codexUpdateState.result.package}</div>
              )}
              <div>
                Version:{" "}
                {codexUpdateState.result.afterVersion ??
                  codexUpdateState.result.beforeVersion ??
                  "unknown"}
              </div>
              {codexUpdateState.result.details && <div>{codexUpdateState.result.details}</div>}
              {codexUpdateState.result.output && (
                <details>
                  <summary>output</summary>
                  <pre>{codexUpdateState.result.output}</pre>
                </details>
              )}
            </div>
          </div>
        )}
      </div>

      <div className="settings-divider" />
      <div className="settings-field-label settings-field-label--section">
        Default parameters
      </div>

      <div className="settings-toggle-row">
        <div>
          <label className="settings-toggle-title" htmlFor="default-model">
            Model
          </label>
          <div className="settings-toggle-subtitle">
            {defaultModelsLoading
              ? "Loading models from configured OpenCode providers…"
              : defaultModelsError
                ? `Couldn’t load models from OpenCode providers: ${defaultModelsError}`
                : `Loaded from configured OpenCode providers${
                    defaultModelsConnectedWorkspaceCount > 0
                      ? ` (${defaultModelsConnectedWorkspaceCount} connected project${
                          defaultModelsConnectedWorkspaceCount === 1 ? "" : "s"
                        })`
                      : ""
                  }. Used when there is no thread-specific override.`}
          </div>
        </div>
        <div className="settings-field-row">
          <select
            id="default-model"
            className="settings-select"
            value={selectedModelId}
            disabled={!defaultModels.length || defaultModelsLoading}
            onChange={(event) =>
              void onUpdateAppSettings({
                ...appSettings,
                lastComposerModelId: event.target.value,
              })
            }
            aria-label="Model"
          >
            {groupedDefaultModels.map((group) =>
              group.label ? (
                <optgroup key={group.provider} label={group.label}>
                  {group.models.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.displayName?.trim() || model.model}
                    </option>
                  ))}
                </optgroup>
              ) : (
                group.models.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.displayName?.trim() || model.model}
                  </option>
                ))
              ),
            )}
          </select>
          <button
            type="button"
            className="ghost"
            onClick={onRefreshDefaultModels}
            disabled={defaultModelsLoading}
          >
            Refresh
          </button>
        </div>
      </div>

      <div className="settings-toggle-row">
        <div>
          <label className="settings-toggle-title" htmlFor="default-effort">
            Reasoning effort
          </label>
          <div className="settings-toggle-subtitle">
            {reasoningSupported
              ? "Available options depend on the selected model."
              : "The selected model does not expose reasoning effort options."}
          </div>
        </div>
        <select
          id="default-effort"
          className="settings-select"
          value={selectedEffort}
          onChange={(event) =>
            void onUpdateAppSettings({
              ...appSettings,
              lastComposerReasoningEffort: event.target.value,
            })
          }
          aria-label="Reasoning effort"
          disabled={!reasoningSupported}
        >
          {!reasoningSupported && <option value="">not supported</option>}
          {reasoningOptions.map((effort) => (
            <option key={effort} value={effort}>
              {effort}
            </option>
          ))}
        </select>
      </div>

      <div className="settings-field">
        <label className="settings-field-label" htmlFor="review-delivery">
          Review mode
        </label>
        <select
          id="review-delivery"
          className="settings-select"
          value={appSettings.reviewDeliveryMode}
          onChange={(event) =>
            void onUpdateAppSettings({
              ...appSettings,
              reviewDeliveryMode: event.target.value as AppSettings["reviewDeliveryMode"],
            })
          }
        >
          <option value="inline">Inline (same thread)</option>
          <option value="detached">Detached (new review thread)</option>
        </select>
        <div className="settings-help">
          Choose whether <code>/review</code> runs in the current thread or a detached review
          thread.
        </div>
      </div>

      <FileEditorCard
        title="Global AGENTS.md"
        meta={globalAgentsMeta}
        error={globalAgentsError}
        value={globalAgentsContent}
        placeholder="Add global instructions for OpenCode agents…"
        disabled={globalAgentsLoading}
        refreshDisabled={globalAgentsRefreshDisabled}
        saveDisabled={globalAgentsSaveDisabled}
        saveLabel={globalAgentsSaveLabel}
        onChange={onSetGlobalAgentsContent}
        onRefresh={onRefreshGlobalAgents}
        onSave={onSaveGlobalAgents}
        helpText={
          <>
            Stored at <code>~/.config/opencode/AGENTS.md</code>.
          </>
        }
        classNames={{
          container: "settings-field settings-agents",
          header: "settings-agents-header",
          title: "settings-field-label",
          actions: "settings-agents-actions",
          meta: "settings-help settings-help-inline",
          iconButton: "ghost settings-icon-button",
          error: "settings-agents-error",
          textarea: "settings-agents-textarea",
          help: "settings-help",
        }}
      />

      <FileEditorCard
        title="Global opencode.json"
        meta={globalConfigMeta}
        error={globalConfigError}
        value={globalConfigContent}
        placeholder="Edit the global OpenCode config…"
        disabled={globalConfigLoading}
        refreshDisabled={globalConfigRefreshDisabled}
        saveDisabled={globalConfigSaveDisabled}
        saveLabel={globalConfigSaveLabel}
        onChange={onSetGlobalConfigContent}
        onRefresh={onRefreshGlobalConfig}
        onSave={onSaveGlobalConfig}
        helpText={
          <>
            Stored at <code>~/.config/opencode/opencode.json</code> or{" "}
            <code>opencode.jsonc</code>.
          </>
        }
        classNames={{
          container: "settings-field settings-agents",
          header: "settings-agents-header",
          title: "settings-field-label",
          actions: "settings-agents-actions",
          meta: "settings-help settings-help-inline",
          iconButton: "ghost settings-icon-button",
          error: "settings-agents-error",
          textarea: "settings-agents-textarea",
          help: "settings-help",
        }}
      />

      <div className="settings-field">
        <div className="settings-field-label">Workspace overrides</div>
        <div className="settings-overrides">
          {projects.map((workspace) => (
            <div key={workspace.id} className="settings-override-row">
              <div className="settings-override-info">
                <div className="settings-project-name">{workspace.name}</div>
                <div className="settings-project-path">{workspace.path}</div>
              </div>
              <div className="settings-override-actions">
                <div className="settings-override-field">
                  <input
                    className="settings-input settings-input--compact"
                    value={codexBinOverrideDrafts[workspace.id] ?? ""}
                    placeholder="OpenCode binary override"
                    onChange={(event) =>
                      onSetCodexBinOverrideDrafts((prev) => ({
                        ...prev,
                        [workspace.id]: event.target.value,
                      }))
                    }
                    onBlur={async () => {
                      const draft = codexBinOverrideDrafts[workspace.id] ?? "";
                      const nextValue = normalizeOverrideValue(draft);
                      if (nextValue === (workspace.codex_bin ?? null)) {
                        return;
                      }
                      await onUpdateWorkspaceCodexBin(workspace.id, nextValue);
                    }}
                    aria-label={`OpenCode binary override for ${workspace.name}`}
                  />
                  <button
                    type="button"
                    className="ghost"
                    onClick={async () => {
                      onSetCodexBinOverrideDrafts((prev) => ({
                        ...prev,
                        [workspace.id]: "",
                      }));
                      await onUpdateWorkspaceCodexBin(workspace.id, null);
                    }}
                  >
                    Clear
                  </button>
                </div>
                <div className="settings-override-field">
                  <input
                    className="settings-input settings-input--compact"
                    value={codexHomeOverrideDrafts[workspace.id] ?? ""}
                    placeholder="OPENCODE_HOME override"
                    onChange={(event) =>
                      onSetCodexHomeOverrideDrafts((prev) => ({
                        ...prev,
                        [workspace.id]: event.target.value,
                      }))
                    }
                    onBlur={async () => {
                      const draft = codexHomeOverrideDrafts[workspace.id] ?? "";
                      const nextValue = normalizeOverrideValue(draft);
                      if (nextValue === (workspace.settings.codexHome ?? null)) {
                        return;
                      }
                      await onUpdateWorkspaceSettings(workspace.id, {
                        codexHome: nextValue,
                      });
                    }}
                    aria-label={`OPENCODE_HOME override for ${workspace.name}`}
                  />
                  <button
                    type="button"
                    className="ghost"
                    onClick={async () => {
                      onSetCodexHomeOverrideDrafts((prev) => ({
                        ...prev,
                        [workspace.id]: "",
                      }));
                      await onUpdateWorkspaceSettings(workspace.id, {
                        codexHome: null,
                      });
                    }}
                  >
                    Clear
                  </button>
                </div>
                <div className="settings-override-field">
                  <input
                    className="settings-input settings-input--compact"
                    value={codexArgsOverrideDrafts[workspace.id] ?? ""}
                    placeholder="OpenCode args override"
                    onChange={(event) =>
                      onSetCodexArgsOverrideDrafts((prev) => ({
                        ...prev,
                        [workspace.id]: event.target.value,
                      }))
                    }
                    onBlur={async () => {
                      const draft = codexArgsOverrideDrafts[workspace.id] ?? "";
                      const nextValue = normalizeOverrideValue(draft);
                      if (nextValue === (workspace.settings.codexArgs ?? null)) {
                        return;
                      }
                      await onUpdateWorkspaceSettings(workspace.id, {
                        codexArgs: nextValue,
                      });
                    }}
                    aria-label={`OpenCode args override for ${workspace.name}`}
                  />
                  <button
                    type="button"
                    className="ghost"
                    onClick={async () => {
                      onSetCodexArgsOverrideDrafts((prev) => ({
                        ...prev,
                        [workspace.id]: "",
                      }));
                      await onUpdateWorkspaceSettings(workspace.id, {
                        codexArgs: null,
                      });
                    }}
                  >
                    Clear
                  </button>
                </div>
              </div>
            </div>
          ))}
          {projects.length === 0 && <div className="settings-empty">No projects yet.</div>}
        </div>
      </div>
    </section>
  );
}
