import type {
  AppServerEvent,
  AuthLoginRequestDto,
  AuthLoginResponseDto,
  AuthSessionDto,
} from "@/types";

export type AppServerEventEnvelopeV1 = {
  version: "v1";
  event: AppServerEvent;
};

export type AppCommandRequest = {
  command: string;
  payload?: Record<string, unknown>;
};

export type AppCommandResponse<T = unknown> = {
  ok: boolean;
  data?: T;
  error?: string;
};

export type Unsubscribe = () => void;

export type TerminalOutputEvent = {
  workspaceId: string;
  terminalId: string;
  data: string;
};

export type TerminalExitEvent = {
  workspaceId: string;
  terminalId: string;
};

export type EventSubscriptionOptions = {
  onError?: (error: unknown) => void;
};

export interface PlatformCommandClient {
  request<T = unknown>(input: AppCommandRequest): Promise<AppCommandResponse<T>>;
}

export interface PlatformEventStream {
  subscribeAppServer(
    onEvent: (event: AppServerEvent) => void,
    options?: EventSubscriptionOptions,
  ): Unsubscribe;
}

export interface PlatformTerminalStream {
  subscribeOutput(
    onEvent: (event: TerminalOutputEvent) => void,
    options?: EventSubscriptionOptions,
  ): Unsubscribe;
  subscribeExit(
    onEvent: (event: TerminalExitEvent) => void,
    options?: EventSubscriptionOptions,
  ): Unsubscribe;
}

export interface PlatformAuthClient {
  login(input: AuthLoginRequestDto): Promise<AuthLoginResponseDto>;
  getSession(): Promise<AuthSessionDto | null>;
  logout(): Promise<void>;
}

export interface PlatformContract {
  commands: PlatformCommandClient;
  events: PlatformEventStream;
  terminal: PlatformTerminalStream;
  auth: PlatformAuthClient;
}

export const WEB_PLATFORM_TEST_IDS = {
  login: {
    username: "login-username",
    password: "login-password",
    submit: "login-submit",
  },
  dashboard: {
    root: "dashboard-root",
    workspaceCard: "workspace-card",
    workspaceSwitcher: "workspace-switcher",
  },
  threadStream: {
    list: "thread-stream",
    message: "thread-stream-message",
  },
  terminal: {
    open: "terminal-open",
    panel: "terminal-panel",
    stream: "terminal-stream",
  },
} as const;
