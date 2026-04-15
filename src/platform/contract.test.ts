import { describe, expect, it } from "vitest";
import { WEB_PLATFORM_TEST_IDS } from "./contract";

describe("WEB_PLATFORM_TEST_IDS", () => {
  it("defines canonical ids for login, dashboard, thread stream, and terminal", () => {
    expect(WEB_PLATFORM_TEST_IDS.login.username).toBe("login-username");
    expect(WEB_PLATFORM_TEST_IDS.login.password).toBe("login-password");
    expect(WEB_PLATFORM_TEST_IDS.dashboard.root).toBe("dashboard-root");
    expect(WEB_PLATFORM_TEST_IDS.dashboard.workspaceCard).toBe("workspace-card");
    expect(WEB_PLATFORM_TEST_IDS.threadStream.list).toBe("thread-stream");
    expect(WEB_PLATFORM_TEST_IDS.terminal.panel).toBe("terminal-panel");
  });
});
