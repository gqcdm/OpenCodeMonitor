import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

import { subscribeWindowDragDrop } from "./dragDrop";
import { getCurrentWindow } from "@tauri-apps/api/window";

const getCurrentWindowMock = vi.mocked(getCurrentWindow);

describe("subscribeWindowDragDrop", () => {
  beforeEach(() => {
    getCurrentWindowMock.mockReset();
  });

  it("reports an error and stays mounted when Tauri window is unavailable", () => {
    const onError = vi.fn();
    getCurrentWindowMock.mockImplementation(() => {
      throw new Error("no tauri window");
    });

    const unsubscribe = subscribeWindowDragDrop(() => {
      throw new Error("should not run");
    }, { onError });

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(Error);
    expect(() => unsubscribe()).not.toThrow();
  });
});
