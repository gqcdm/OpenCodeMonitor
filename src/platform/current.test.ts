import { describe, expect, it } from "vitest";
import { desktopPlatformContract } from "./desktop";
import { resolvePlatformContract } from "./current";

describe("resolvePlatformContract", () => {
  it("returns web no-op terminal subscriptions in web mode", async () => {
    const contract = resolvePlatformContract("web");

    expect(await contract.commands.request({ command: "noop" })).toEqual({
      ok: false,
      error: "Web command transport adapter is not registered yet.",
    });
    expect(contract.terminal.subscribeOutput(() => {})).toBeTypeOf("function");
    expect(contract.terminal.subscribeExit(() => {})).toBeTypeOf("function");
  });

  it("returns desktop contract outside web mode", () => {
    expect(resolvePlatformContract("development")).toBe(desktopPlatformContract);
  });
});
