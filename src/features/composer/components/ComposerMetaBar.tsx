import { type CSSProperties, useMemo } from "react";
import { Bot, BrainCog } from "lucide-react";
import type { ThreadTokenUsage } from "../../../types";
import { groupModelsByProvider } from "../../models/utils/groupModelsByProvider";

type ComposerMetaBarProps = {
  disabled: boolean;
  isConnected?: boolean;
  collaborationModes: { id: string; label: string }[];
  selectedCollaborationModeId: string | null;
  onSelectCollaborationMode: (id: string | null) => void;
  models: { id: string; displayName: string; model: string; provider: string }[];
  selectedModelId: string | null;
  onSelectModel: (id: string) => void;
  reasoningOptions: string[];
  selectedEffort: string | null;
  onSelectEffort: (effort: string) => void;
  reasoningSupported: boolean;
  contextUsage?: ThreadTokenUsage | null;
};

export function ComposerMetaBar({
  disabled,
  isConnected = false,
  collaborationModes,
  selectedCollaborationModeId,
  onSelectCollaborationMode,
  models,
  selectedModelId,
  onSelectModel,
  reasoningOptions,
  selectedEffort,
  onSelectEffort,
  reasoningSupported,
  contextUsage = null,
}: ComposerMetaBarProps) {
  const groupedModels = useMemo(() => groupModelsByProvider(models), [models]);
  const contextWindow = contextUsage?.modelContextWindow ?? null;
  const lastTokens = contextUsage?.last.totalTokens ?? 0;
  const totalTokens = contextUsage?.total.totalTokens ?? 0;
  const usedTokens = lastTokens > 0 ? lastTokens : totalTokens;
  const contextFreePercent =
    contextWindow && contextWindow > 0 && usedTokens > 0
      ? Math.max(
          0,
          100 -
            Math.min(Math.max((usedTokens / contextWindow) * 100, 0), 100),
        )
      : null;
  const contextLabel =
    contextFreePercent === null
      ? "Context free --"
      : `Context free ${Math.round(contextFreePercent)}%`;
  const contextTooltip =
    usedTokens > 0 && contextWindow
      ? `${usedTokens.toLocaleString()} of ${contextWindow.toLocaleString()} tokens`
      : contextLabel;
  return (
    <div className="composer-bar">
      <div className="composer-meta">
        <div className="composer-select-wrap composer-select-wrap--model">
          <span className="composer-icon composer-icon--model" aria-hidden>
            <svg viewBox="0 0 24 24" fill="none">
              <path
                d="M12 4v2"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
              <path
                d="M8 7.5h8a2.5 2.5 0 0 1 2.5 2.5v5a2.5 2.5 0 0 1-2.5 2.5H8A2.5 2.5 0 0 1 5.5 15v-5A2.5 2.5 0 0 1 8 7.5Z"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinejoin="round"
              />
              <circle cx="9.5" cy="12.5" r="1" fill="currentColor" />
              <circle cx="14.5" cy="12.5" r="1" fill="currentColor" />
              <path
                d="M9.5 15.5h5"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
              <path
                d="M5.5 11H4M20 11h-1.5"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
            </svg>
          </span>
          <select
            className="composer-select composer-select--model"
            aria-label="Model"
            value={selectedModelId ?? ""}
            onChange={(event) => onSelectModel(event.target.value)}
            disabled={disabled}
          >
            {models.length === 0 && (
              <option value="">{isConnected ? "Loading models..." : "No models"}</option>
            )}
            {groupedModels.map((group) =>
              group.label ? (
                <optgroup key={group.provider} label={group.label}>
                  {group.models.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.displayName || model.model}
                    </option>
                  ))}
                </optgroup>
              ) : (
                group.models.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.displayName || model.model}
                  </option>
                ))
              ),
            )}
          </select>
        </div>
        <div className="composer-select-wrap composer-select-wrap--effort">
          <span className="composer-icon composer-icon--effort" aria-hidden>
            <BrainCog size={14} strokeWidth={1.8} />
          </span>
          <select
            className="composer-select composer-select--effort"
            aria-label="Thinking mode"
            value={selectedEffort ?? ""}
            onChange={(event) => onSelectEffort(event.target.value)}
            disabled={disabled || !reasoningSupported}
          >
            {reasoningOptions.length === 0 && <option value="">Default</option>}
            {reasoningOptions.map((effort) => (
              <option key={effort} value={effort}>
                {effort}
              </option>
            ))}
          </select>
        </div>
        {collaborationModes.length > 0 && (
          <div className="composer-select-wrap">
            <span className="composer-icon" aria-hidden>
              <Bot size={14} strokeWidth={1.8} />
            </span>
            <select
              className="composer-select composer-select--agent"
              aria-label="Agent"
              value={selectedCollaborationModeId ?? ""}
              onChange={(event) =>
                onSelectCollaborationMode(event.target.value || null)
              }
              disabled={disabled}
            >
              {collaborationModes.map((mode) => (
                <option key={mode.id} value={mode.id}>
                  {mode.label || mode.id}
                </option>
              ))}
            </select>
          </div>
        )}
      </div>
      <div className="composer-context">
        <div
          className="composer-context-ring"
          data-tooltip={contextTooltip}
          aria-label={contextTooltip}
          style={
            {
              "--context-free": contextFreePercent ?? 0,
            } as CSSProperties
          }
        >
          <span className="composer-context-value">●</span>
        </div>
        <span className="composer-context-label">{contextLabel}</span>
      </div>
    </div>
  );
}
