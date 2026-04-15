import { desktopPlatformContract } from "@/platform/desktop";
import type {
  AppCommandRequest,
  AppCommandResponse,
  EventSubscriptionOptions,
  PlatformContract,
  TerminalExitEvent,
  TerminalOutputEvent,
  Unsubscribe,
} from "@/platform/contract";
import type { AppServerEvent, AuthLoginRequestDto, AuthLoginResponseDto, AuthSessionDto } from "@/types";

function noopUnsubscribe(): Unsubscribe {
  return () => {
    // No-op in web scaffold mode.
  };
}

const webPlatformContract: PlatformContract = {
  commands: {
    async request<T = unknown>(_input: AppCommandRequest): Promise<AppCommandResponse<T>> {
      return {
        ok: false,
        error: "Web command transport adapter is not registered yet.",
      };
    },
  },
  events: {
    subscribeAppServer(
      _onEvent: (event: AppServerEvent) => void,
      _options?: EventSubscriptionOptions,
    ): Unsubscribe {
      return noopUnsubscribe();
    },
  },
  terminal: {
    subscribeOutput(
      _onEvent: (event: TerminalOutputEvent) => void,
      _options?: EventSubscriptionOptions,
    ): Unsubscribe {
      return noopUnsubscribe();
    },
    subscribeExit(
      _onEvent: (event: TerminalExitEvent) => void,
      _options?: EventSubscriptionOptions,
    ): Unsubscribe {
      return noopUnsubscribe();
    },
  },
  auth: {
    async login(_input: AuthLoginRequestDto): Promise<AuthLoginResponseDto> {
      throw new Error("Web auth transport adapter is not registered yet.");
    },
    async getSession(): Promise<AuthSessionDto | null> {
      return null;
    },
    async logout(): Promise<void> {
      // No-op in web scaffold mode.
    },
  },
};

export function resolvePlatformContract(mode: string = import.meta.env.MODE): PlatformContract {
  return mode === "web" ? webPlatformContract : desktopPlatformContract;
}

export const currentPlatformContract = resolvePlatformContract();
