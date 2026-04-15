import type { PlatformContract } from "@/platform/contract";
import {
  subscribeAppServerEvents,
  subscribeTerminalExit,
  subscribeTerminalOutput,
} from "@/services/events";

export const desktopPlatformContract: PlatformContract = {
  commands: {
    async request() {
      return {
        ok: false,
        error: "Desktop command transport adapter is not registered yet.",
      };
    },
  },
  events: {
    subscribeAppServer: subscribeAppServerEvents,
  },
  terminal: {
    subscribeOutput: subscribeTerminalOutput,
    subscribeExit: subscribeTerminalExit,
  },
  auth: {
    async login() {
      throw new Error("Desktop auth transport adapter is not registered yet.");
    },
    async getSession() {
      return null;
    },
    async logout() {
      // No-op for desktop scaffold phase.
    },
  },
};
