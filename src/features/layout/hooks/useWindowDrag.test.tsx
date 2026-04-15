/* @vitest-environment jsdom */
import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => {
    throw new Error("no tauri window");
  }),
}));

import { useWindowDrag } from "./useWindowDrag";

function Harness() {
  useWindowDrag("drag-target");
  return <div id="drag-target">drag</div>;
}

describe("useWindowDrag", () => {
  it("ignores drag requests when the Tauri window is unavailable", () => {
    const { getByText } = render(<Harness />);

    expect(() => {
      fireEvent.mouseDown(getByText("drag"), { buttons: 1 });
    }).not.toThrow();
  });
});
