import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AgentOption, DebugEntry, WorkspaceInfo } from "../../../types";
import { getAgentList } from "../../../services/tauri";

type UseAgentsOptions = {
  activeWorkspace: WorkspaceInfo | null;
  onDebug?: (entry: DebugEntry) => void;
};

export function useAgents({ activeWorkspace, onDebug }: UseAgentsOptions) {
  const [agents, setAgents] = useState<AgentOption[]>([]);
  const lastFetchedWorkspaceId = useRef<string | null>(null);
  const inFlight = useRef(false);

  const workspaceId = activeWorkspace?.id ?? null;
  const isConnected = Boolean(activeWorkspace?.connected);

  const refreshAgents = useCallback(async () => {
    if (!workspaceId || !isConnected) {
      return;
    }
    if (inFlight.current) {
      return;
    }
    inFlight.current = true;
    onDebug?.({
      id: `${Date.now()}-client-agent-list`,
      timestamp: Date.now(),
      source: "client",
      label: "agent/list",
      payload: { workspaceId },
    });
    try {
      const response = await getAgentList(workspaceId);
      onDebug?.({
        id: `${Date.now()}-server-agent-list`,
        timestamp: Date.now(),
        source: "server",
        label: "agent/list response",
        payload: response,
      });
      const rawAgents = response.result?.data ?? response.data ?? [];
      const data: AgentOption[] = rawAgents.map((item: any) => ({
        name: String(item.name ?? ""),
        mode: String(item.mode ?? "subagent"),
        description: item.description ? String(item.description) : undefined,
      }));
      setAgents(data);
      lastFetchedWorkspaceId.current = workspaceId;
    } catch (error) {
      onDebug?.({
        id: `${Date.now()}-client-agent-list-error`,
        timestamp: Date.now(),
        source: "error",
        label: "agent/list error",
        payload: error instanceof Error ? error.message : String(error),
      });
    } finally {
      inFlight.current = false;
    }
  }, [isConnected, onDebug, workspaceId]);

  useEffect(() => {
    if (!workspaceId || !isConnected) {
      return;
    }
    if (lastFetchedWorkspaceId.current === workspaceId && agents.length > 0) {
      return;
    }
    refreshAgents();
  }, [isConnected, refreshAgents, agents.length, workspaceId]);

  const agentOptions = useMemo(
    () => agents.filter((agent) => agent.name),
    [agents],
  );

  return {
    agents: agentOptions,
    refreshAgents,
  };
}
