import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  AgentMention,
  AppMention,
  OpenCodeSlashCommand,
  QueuedMessage,
  WorkspaceInfo,
} from "@/types";

type UseQueuedSendOptions = {
  activeThreadId: string | null;
  activeTurnId: string | null;
  isProcessing: boolean;
  isReviewing: boolean;
  steerEnabled: boolean;
  appsEnabled: boolean;
  activeWorkspace: WorkspaceInfo | null;
  connectWorkspace: (workspace: WorkspaceInfo) => Promise<void>;
  startThreadForWorkspace: (
    workspaceId: string,
    options?: { activate?: boolean },
  ) => Promise<string | null>;
  sendUserMessage: (
    text: string,
    images?: string[],
    appMentions?: AppMention[],
    agentMentions?: AgentMention[],
  ) => Promise<void>;
  sendUserMessageToThread: (
    workspace: WorkspaceInfo,
    threadId: string,
    text: string,
    images?: string[],
  ) => Promise<void>;
  startFork: (text: string) => Promise<void>;
  startReview: (text: string) => Promise<void>;
  startResume: (text: string) => Promise<void>;
  startCompact: (text: string) => Promise<void>;
  startUndo: (text: string) => Promise<void>;
  startRedo: (text: string) => Promise<void>;
  startApps: (text: string) => Promise<void>;
  startMcp: (text: string) => Promise<void>;
  startStatus: (text: string) => Promise<void>;
  slashCommands?: OpenCodeSlashCommand[];
  executeSlashCommand?: (text: string) => Promise<void>;
  clearActiveImages: () => void;
};

type UseQueuedSendResult = {
  queuedByThread: Record<string, QueuedMessage[]>;
  activeQueue: QueuedMessage[];
  handleSend: (
    text: string,
    images?: string[],
    appMentions?: AppMention[],
    agentMentions?: AgentMention[],
  ) => Promise<void>;
  queueMessage: (
    text: string,
    images?: string[],
    appMentions?: AppMention[],
    agentMentions?: AgentMention[],
  ) => Promise<void>;
  removeQueuedMessage: (threadId: string, messageId: string) => void;
};

type SlashCommandKind =
  | "apps"
  | "compact"
  | "fork"
  | "mcp"
  | "new"
  | "redo"
  | "resume"
  | "review"
  | "status"
  | "undo";

type ParsedSlashCommand =
  | { kind: "local"; command: SlashCommandKind }
  | { kind: "opencode"; command: string };

function parseLocalSlashCommand(text: string, appsEnabled: boolean): SlashCommandKind | null {
  if (appsEnabled && /^\/apps\b/i.test(text)) {
    return "apps";
  }
  if (/^\/fork\b/i.test(text)) {
    return "fork";
  }
  if (/^\/mcp\b/i.test(text)) {
    return "mcp";
  }
  if (/^\/review\b/i.test(text)) {
    return "review";
  }
  if (/^\/compact\b/i.test(text)) {
    return "compact";
  }
  if (/^\/new\b/i.test(text)) {
    return "new";
  }
  if (/^\/undo\b/i.test(text)) {
    return "undo";
  }
  if (/^\/redo\b/i.test(text)) {
    return "redo";
  }
  if (/^\/resume\b/i.test(text)) {
    return "resume";
  }
  if (/^\/status\b/i.test(text)) {
    return "status";
  }
  return null;
}

function parseOpenCodeSlashCommand(
  text: string,
  slashCommands: OpenCodeSlashCommand[],
): string | null {
  const match = /^\/([^\s]+)/.exec(text.trim());
  const slashName = (match?.[1] ?? "").trim().toLowerCase();
  if (!slashName) {
    return null;
  }
  for (const command of slashCommands) {
    const name = command.name.trim();
    if (name && name.toLowerCase() === slashName) {
      return name;
    }
    for (const alias of command.aliases ?? []) {
      if (alias.trim().toLowerCase() === slashName) {
        return name || alias.trim();
      }
    }
  }
  return null;
}

function parseSlashCommand(
  text: string,
  appsEnabled: boolean,
  slashCommands: OpenCodeSlashCommand[],
): ParsedSlashCommand | null {
  const local = parseLocalSlashCommand(text, appsEnabled);
  if (local) {
    return { kind: "local", command: local };
  }
  const opencode = parseOpenCodeSlashCommand(text, slashCommands);
  if (opencode) {
    return { kind: "opencode", command: opencode };
  }
  return null;
}

export function useQueuedSend({
  activeThreadId,
  activeTurnId,
  isProcessing,
  isReviewing,
  steerEnabled,
  appsEnabled,
  activeWorkspace,
  connectWorkspace,
  startThreadForWorkspace,
  sendUserMessage,
  sendUserMessageToThread,
  startFork,
  startReview,
  startResume,
  startCompact,
  startUndo,
  startRedo,
  startApps,
  startMcp,
  startStatus,
  slashCommands = [],
  executeSlashCommand = async () => {},
  clearActiveImages,
}: UseQueuedSendOptions): UseQueuedSendResult {
  const [queuedByThread, setQueuedByThread] = useState<
    Record<string, QueuedMessage[]>
  >({});
  const [inFlightByThread, setInFlightByThread] = useState<
    Record<string, QueuedMessage | null>
  >({});
  const [hasStartedByThread, setHasStartedByThread] = useState<
    Record<string, boolean>
  >({});

  const activeQueue = useMemo(
    () => (activeThreadId ? queuedByThread[activeThreadId] ?? [] : []),
    [activeThreadId, queuedByThread],
  );

  const enqueueMessage = useCallback((threadId: string, item: QueuedMessage) => {
    setQueuedByThread((prev) => ({
      ...prev,
      [threadId]: [...(prev[threadId] ?? []), item],
    }));
  }, []);

  const removeQueuedMessage = useCallback(
    (threadId: string, messageId: string) => {
      setQueuedByThread((prev) => ({
        ...prev,
        [threadId]: (prev[threadId] ?? []).filter(
          (entry) => entry.id !== messageId,
        ),
      }));
    },
    [],
  );

  const prependQueuedMessage = useCallback((threadId: string, item: QueuedMessage) => {
    setQueuedByThread((prev) => ({
      ...prev,
      [threadId]: [item, ...(prev[threadId] ?? [])],
    }));
  }, []);

  const runSlashCommand = useCallback(
    async (command: ParsedSlashCommand, trimmed: string) => {
      if (command.kind === "opencode") {
        await executeSlashCommand(trimmed);
        return;
      }
      if (command.command === "fork") {
        await startFork(trimmed);
        return;
      }
      if (command.command === "review") {
        await startReview(trimmed);
        return;
      }
      if (command.command === "resume") {
        await startResume(trimmed);
        return;
      }
      if (command.command === "compact") {
        await startCompact(trimmed);
        return;
      }
      if (command.command === "undo") {
        await startUndo(trimmed);
        return;
      }
      if (command.command === "redo") {
        await startRedo(trimmed);
        return;
      }
      if (command.command === "apps") {
        await startApps(trimmed);
        return;
      }
      if (command.command === "mcp") {
        await startMcp(trimmed);
        return;
      }
      if (command.command === "status") {
        await startStatus(trimmed);
        return;
      }
      if (command.command === "new" && activeWorkspace) {
        const threadId = await startThreadForWorkspace(activeWorkspace.id);
        const rest = trimmed.replace(/^\/new\b/i, "").trim();
        if (threadId && rest) {
          await sendUserMessageToThread(activeWorkspace, threadId, rest, []);
        }
      }
    },
    [
      activeWorkspace,
      sendUserMessageToThread,
      startFork,
      startReview,
      startResume,
      startCompact,
      startUndo,
      startRedo,
      startApps,
      startMcp,
      startStatus,
      executeSlashCommand,
      startThreadForWorkspace,
    ],
  );

  const handleSend = useCallback(
    async (
      text: string,
      images: string[] = [],
      appMentions: AppMention[] = [],
      agentMentions: AgentMention[] = [],
    ) => {
      const trimmed = text.trim();
      const command = parseSlashCommand(trimmed, appsEnabled, slashCommands);
      const nextImages = command ? [] : images;
      const nextAppMentions = command ? [] : appMentions;
      const nextAgentMentions = command ? [] : agentMentions;
      if (!trimmed && nextImages.length === 0) {
        return;
      }
      if (activeThreadId && isReviewing) {
        return;
      }
      if (isProcessing && activeThreadId && (!steerEnabled || !activeTurnId)) {
        const item: QueuedMessage = {
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
          text: trimmed,
          createdAt: Date.now(),
          images: nextImages,
          ...(nextAppMentions.length > 0 ? { appMentions: nextAppMentions } : {}),
          ...(nextAgentMentions.length > 0 ? { agentMentions: nextAgentMentions } : {}),
        };
        enqueueMessage(activeThreadId, item);
        clearActiveImages();
        return;
      }
      if (activeWorkspace && !activeWorkspace.connected) {
        await connectWorkspace(activeWorkspace);
      }
      if (command) {
        await runSlashCommand(command, trimmed);
        clearActiveImages();
        return;
      }
      const hasAppMentions = nextAppMentions.length > 0;
      const hasAgentMentions = nextAgentMentions.length > 0;
      await sendUserMessage(
        trimmed,
        nextImages,
        hasAppMentions ? nextAppMentions : undefined,
        hasAgentMentions ? nextAgentMentions : undefined,
      );
      clearActiveImages();
    },
    [
      activeThreadId,
      appsEnabled,
      slashCommands,
      activeWorkspace,
      clearActiveImages,
      connectWorkspace,
      enqueueMessage,
      activeTurnId,
      isProcessing,
      isReviewing,
      steerEnabled,
      runSlashCommand,
      sendUserMessage,
    ],
  );

  const queueMessage = useCallback(
    async (
      text: string,
      images: string[] = [],
      appMentions: AppMention[] = [],
      agentMentions: AgentMention[] = [],
    ) => {
      const trimmed = text.trim();
      const command = parseSlashCommand(trimmed, appsEnabled, slashCommands);
      const nextImages = command ? [] : images;
      const nextAppMentions = command ? [] : appMentions;
      const nextAgentMentions = command ? [] : agentMentions;
      if (!trimmed && nextImages.length === 0) {
        return;
      }
      if (activeThreadId && isReviewing) {
        return;
      }
      if (!activeThreadId) {
        return;
      }
      const item: QueuedMessage = {
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        text: trimmed,
        createdAt: Date.now(),
        images: nextImages,
        ...(nextAppMentions.length > 0 ? { appMentions: nextAppMentions } : {}),
        ...(nextAgentMentions.length > 0 ? { agentMentions: nextAgentMentions } : {}),
      };
      enqueueMessage(activeThreadId, item);
      clearActiveImages();
    },
    [
      activeThreadId,
      appsEnabled,
      slashCommands,
      clearActiveImages,
      enqueueMessage,
      isReviewing,
    ],
  );

  useEffect(() => {
    if (!activeThreadId) {
      return;
    }
    const inFlight = inFlightByThread[activeThreadId];
    if (!inFlight) {
      return;
    }
    if (isProcessing || isReviewing) {
      if (!hasStartedByThread[activeThreadId]) {
        setHasStartedByThread((prev) => ({
          ...prev,
          [activeThreadId]: true,
        }));
      }
      return;
    }
    if (hasStartedByThread[activeThreadId]) {
      setInFlightByThread((prev) => ({ ...prev, [activeThreadId]: null }));
      setHasStartedByThread((prev) => ({ ...prev, [activeThreadId]: false }));
    }
  }, [
    activeThreadId,
    hasStartedByThread,
    inFlightByThread,
    isProcessing,
    isReviewing,
  ]);

  useEffect(() => {
    if (!activeThreadId || isProcessing || isReviewing) {
      return;
    }
    if (inFlightByThread[activeThreadId]) {
      return;
    }
    const queue = queuedByThread[activeThreadId] ?? [];
    if (queue.length === 0) {
      return;
    }
    const threadId = activeThreadId;
    const nextItem = queue[0];
    setInFlightByThread((prev) => ({ ...prev, [threadId]: nextItem }));
    setHasStartedByThread((prev) => ({ ...prev, [threadId]: false }));
    setQueuedByThread((prev) => ({
      ...prev,
      [threadId]: (prev[threadId] ?? []).slice(1),
    }));
    (async () => {
      try {
        const trimmed = nextItem.text.trim();
        const command = parseSlashCommand(trimmed, appsEnabled, slashCommands);
        if (command) {
          await runSlashCommand(command, trimmed);
        } else {
          const queuedAppMentions = nextItem.appMentions ?? [];
          const queuedAgentMentions = nextItem.agentMentions ?? [];
          await sendUserMessage(
            nextItem.text,
            nextItem.images ?? [],
            queuedAppMentions.length > 0 ? queuedAppMentions : undefined,
            queuedAgentMentions.length > 0 ? queuedAgentMentions : undefined,
          );
        }
      } catch {
        setInFlightByThread((prev) => ({ ...prev, [threadId]: null }));
        setHasStartedByThread((prev) => ({ ...prev, [threadId]: false }));
        prependQueuedMessage(threadId, nextItem);
      }
    })();
  }, [
    activeThreadId,
    appsEnabled,
    slashCommands,
    inFlightByThread,
    isProcessing,
    isReviewing,
    prependQueuedMessage,
    queuedByThread,
    runSlashCommand,
    sendUserMessage,
  ]);

  return {
    queuedByThread,
    activeQueue,
    handleSend,
    queueMessage,
    removeQueuedMessage,
  };
}
