/** @vitest-environment jsdom */
import { createRef } from "react";
import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useComposerAutocompleteState } from "./useComposerAutocompleteState";

describe("useComposerAutocompleteState file mentions", () => {
  it("suggests a file even if it is already mentioned earlier in the message", () => {
    const files = ["src/App.tsx", "src/main.tsx"];
    const text = "Please review @src/App.tsx and also @";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: true,
        skills: [],
        apps: [],
        prompts: [],
        files,
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    expect(result.current.isAutocompleteOpen).toBe(true);
    expect(result.current.autocompleteMatches.map((item) => item.label)).toContain(
      "src/App.tsx",
    );
  });

  it("marks root-level file suggestions as Files group", () => {
    const files = ["AGENTS.md", "src/main.tsx"];
    const text = "@";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: true,
        skills: [],
        apps: [],
        prompts: [],
        files,
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    const rootItem = result.current.autocompleteMatches.find(
      (item) => item.label === "AGENTS.md",
    );
    expect(rootItem?.group).toBe("Files");
  });
});

describe("useComposerAutocompleteState slash commands", () => {
  const slashCommands = [
    { name: "review", description: "Start a review" },
    { name: "compact", description: "Compact context" },
    { name: "mcp", description: "MCP tools" },
    { name: "resume", description: "Resume session" },
    { name: "fork", description: "Fork thread" },
    { name: "status", description: "Status" },
    { name: "new", description: "New thread" },
    { name: "apps", description: "Apps" },
  ];

  it("renders OpenCode slash commands in alphabetical order", () => {
    const text = "/";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: true,
        slashCommands,
        skills: [],
        apps: [],
        prompts: [],
        files: [],
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    const labels = result.current.autocompleteMatches.map((item) => item.label);
    expect(labels.slice(0, 8)).toEqual([
      "apps",
      "compact",
      "fork",
      "mcp",
      "new",
      "resume",
      "review",
      "status",
    ]);
  });

  it("does not filter OpenCode slash commands based on apps feature flag", () => {
    const text = "/";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: false,
        slashCommands,
        skills: [],
        apps: [],
        prompts: [],
        files: [],
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    const labels = result.current.autocompleteMatches.map((item) => item.label);
    expect(labels).toEqual([
      "apps",
      "compact",
      "fork",
      "mcp",
      "new",
      "resume",
      "review",
      "status",
    ]);
  });
});

describe("useComposerAutocompleteState $ completions", () => {
  it("separates skills and apps into grouped results", () => {
    const text = "$";
    const selectionStart = text.length;
    const textareaRef = createRef<HTMLTextAreaElement>();
    textareaRef.current = {
      focus: vi.fn(),
      setSelectionRange: vi.fn(),
    } as unknown as HTMLTextAreaElement;

    const { result } = renderHook(() =>
      useComposerAutocompleteState({
        text,
        selectionStart,
        disabled: false,
        appsEnabled: true,
        skills: [
          { name: "skill-a", description: "Skill A" },
          { name: "skill-b", description: "Skill B" },
        ],
        apps: [
          {
            id: "connector_calendar",
            name: "Calendar App",
            description: "Calendar app",
            isAccessible: true,
            installUrl: null,
            distributionChannel: null,
          },
          {
            id: "not-ready",
            name: "Not Ready App",
            description: "Unreleased",
            isAccessible: false,
            installUrl: "https://example.com/install",
            distributionChannel: "beta",
          },
        ],
        prompts: [],
        files: [],
        textareaRef,
        setText: vi.fn(),
        setSelectionStart: vi.fn(),
      }),
    );

    const ids = result.current.autocompleteMatches.map((item) => item.id);
    const groups = result.current.autocompleteMatches.map((item) => item.group);
    const appSuggestion = result.current.autocompleteMatches.find(
      (item) => item.id === "app:connector_calendar",
    );
    expect(ids).toEqual(["skill:skill-a", "skill:skill-b", "app:connector_calendar"]);
    expect(groups).toEqual(["Skills", "Skills", "Apps"]);
    expect(ids).not.toContain("app:not-ready");
    expect(appSuggestion?.insertText).toBe("calendar-app");
    expect(appSuggestion?.mentionPath).toBe("app://connector_calendar");
  });
});
