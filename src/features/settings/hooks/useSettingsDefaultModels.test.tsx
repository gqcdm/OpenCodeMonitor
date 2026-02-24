// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceInfo } from "@/types";
import { getSettingsModelList } from "@services/tauri";
import { useSettingsDefaultModels } from "./useSettingsDefaultModels";

vi.mock("@services/tauri", () => ({
  getSettingsModelList: vi.fn(),
}));

const getSettingsModelListMock = vi.mocked(getSettingsModelList);

function workspace(id: string, connected = true): WorkspaceInfo {
  return {
    id,
    name: `Workspace ${id}`,
    path: `/tmp/${id}`,
    connected,
    settings: { sidebarCollapsed: false },
  };
}

function modelListResponse(model: string) {
  return {
    result: {
      data: [
        {
          id: model,
          model,
          displayName: model,
          description: "",
          supportedReasoningEfforts: [],
          defaultReasoningEffort: null,
          isDefault: false,
        },
      ],
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useSettingsDefaultModels", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("loads configured provider models even with no connected workspaces", async () => {
    getSettingsModelListMock.mockResolvedValueOnce(modelListResponse("gpt-5"));

    const { result } = renderHook(
      ({ projects }: { projects: WorkspaceInfo[] }) => useSettingsDefaultModels(projects),
      {
        initialProps: {
          projects: [workspace("w1", false)],
        },
      },
    );

    await waitFor(() => {
      expect(getSettingsModelListMock).toHaveBeenCalledTimes(1);
      expect(result.current.models[0]?.model).toBe("gpt-5");
      expect(result.current.connectedWorkspaceCount).toBe(0);
    });
  });

  it("ignores stale results when project connectivity metadata changes", async () => {
    const first = deferred<any>();
    const second = deferred<any>();
    getSettingsModelListMock
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const { result, rerender } = renderHook(
      ({ projects }: { projects: WorkspaceInfo[] }) => useSettingsDefaultModels(projects),
      {
        initialProps: {
          projects: [workspace("w1", true)],
        },
      },
    );

    await waitFor(() => {
      expect(getSettingsModelListMock).toHaveBeenCalledTimes(1);
    });

    rerender({ projects: [workspace("w1", false)] });

    await waitFor(() => {
      expect(getSettingsModelListMock).toHaveBeenCalledTimes(2);
      expect(result.current.connectedWorkspaceCount).toBe(0);
    });

    await act(async () => {
      second.resolve(modelListResponse("gpt-5.1"));
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(result.current.models[0]?.model).toBe("gpt-5.1");
    });

    await act(async () => {
      first.resolve(modelListResponse("gpt-4.1"));
      await Promise.resolve();
    });

    expect(result.current.models[0]?.model).toBe("gpt-5.1");
  });
});
