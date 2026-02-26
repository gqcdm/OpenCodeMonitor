// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Markdown } from "./Markdown";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

describe("Markdown code block copy", () => {
  const writeTextMock = vi.fn();
  const originalClipboardDescriptor = Object.getOwnPropertyDescriptor(
    navigator,
    "clipboard",
  );

  beforeEach(() => {
    writeTextMock.mockReset();
    writeTextMock.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: writeTextMock,
      },
    });
  });

  afterEach(() => {
    cleanup();
    if (originalClipboardDescriptor) {
      Object.defineProperty(navigator, "clipboard", originalClipboardDescriptor);
    } else {
      delete (navigator as { clipboard?: Clipboard }).clipboard;
    }
  });

  it("copies only code content by default", async () => {
    render(
      <Markdown value={'```bash\necho "hello"\necho "world"\n```'} codeBlockStyle="message" />,
    );

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy code block" }));
    });

    expect(writeTextMock).toHaveBeenCalledWith('echo "hello"\necho "world"');
  });

  it("includes fences when modifier copy is enabled and Option is held", async () => {
    render(
      <Markdown
        value={'```bash\necho "hello"\necho "world"\n```'}
        codeBlockStyle="message"
        codeBlockCopyUseModifier
      />,
    );

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy code block" }), {
        altKey: true,
      });
    });

    expect(writeTextMock).toHaveBeenCalledWith('```bash\necho "hello"\necho "world"\n```');
  });
});
