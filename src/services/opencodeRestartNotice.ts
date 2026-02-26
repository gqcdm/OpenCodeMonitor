const STORAGE_KEY = "opencode.restartNotice";
const EVENT_NAME = "opencode-restart-notice-change";
const SERVER_RESTARTED_EVENT_NAME = "opencode-server-restarted";

export type OpenCodeRestartNotice = {
  required: boolean;
  updatedAt: number | null;
  reason: string | null;
  source: "manual" | "detector" | null;
};

const DEFAULT_NOTICE: OpenCodeRestartNotice = {
  required: false,
  updatedAt: null,
  reason: null,
  source: null,
};

function canUseBrowserStorage() {
  return typeof window !== "undefined" && typeof localStorage !== "undefined";
}

function emitNoticeChange(notice: OpenCodeRestartNotice) {
  if (typeof window === "undefined") {
    return;
  }
  window.dispatchEvent(new CustomEvent<OpenCodeRestartNotice>(EVENT_NAME, { detail: notice }));
}

export function readOpenCodeRestartNotice(): OpenCodeRestartNotice {
  if (!canUseBrowserStorage()) {
    return DEFAULT_NOTICE;
  }
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return DEFAULT_NOTICE;
    }
    const parsed = JSON.parse(raw) as Partial<OpenCodeRestartNotice>;
    return {
      required: parsed.required === true,
      updatedAt: typeof parsed.updatedAt === "number" ? parsed.updatedAt : null,
      reason: typeof parsed.reason === "string" ? parsed.reason : null,
      source:
        parsed.source === "manual" || parsed.source === "detector" ? parsed.source : null,
    };
  } catch {
    return DEFAULT_NOTICE;
  }
}

export function markOpenCodeRestartRequired(
  reason?: string | null,
  source: "manual" | "detector" = "manual",
) {
  if (!canUseBrowserStorage()) {
    return;
  }
  const next: OpenCodeRestartNotice = {
    required: true,
    updatedAt: Date.now(),
    reason: reason?.trim() ? reason.trim() : null,
    source,
  };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // No-op: storage can fail in restricted contexts.
  }
  emitNoticeChange(next);
}

export function clearOpenCodeRestartRequired() {
  if (!canUseBrowserStorage()) {
    return;
  }
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // No-op: storage can fail in restricted contexts.
  }
  emitNoticeChange(DEFAULT_NOTICE);
}

export function notifyOpenCodeServerRestarted() {
  if (typeof window === "undefined") {
    return;
  }
  window.dispatchEvent(
    new CustomEvent<{ restartedAt: number }>(SERVER_RESTARTED_EVENT_NAME, {
      detail: { restartedAt: Date.now() },
    }),
  );
}

export function subscribeOpenCodeRestartNotice(
  listener: (notice: OpenCodeRestartNotice) => void,
) {
  if (typeof window === "undefined") {
    return () => {};
  }

  const onCustom = (event: Event) => {
    const detail = (event as CustomEvent<OpenCodeRestartNotice>).detail;
    listener(detail ?? readOpenCodeRestartNotice());
  };
  const onStorage = (event: StorageEvent) => {
    if (event.key !== STORAGE_KEY) {
      return;
    }
    listener(readOpenCodeRestartNotice());
  };

  window.addEventListener(EVENT_NAME, onCustom as EventListener);
  window.addEventListener("storage", onStorage);

  return () => {
    window.removeEventListener(EVENT_NAME, onCustom as EventListener);
    window.removeEventListener("storage", onStorage);
  };
}

export function subscribeOpenCodeServerRestarted(listener: () => void) {
  if (typeof window === "undefined") {
    return () => {};
  }
  const onRestarted = () => {
    listener();
  };
  window.addEventListener(SERVER_RESTARTED_EVENT_NAME, onRestarted as EventListener);
  return () => {
    window.removeEventListener(SERVER_RESTARTED_EVENT_NAME, onRestarted as EventListener);
  };
}
