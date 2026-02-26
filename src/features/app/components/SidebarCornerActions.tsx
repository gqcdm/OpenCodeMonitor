import ScrollText from "lucide-react/dist/esm/icons/scroll-text";
import Settings from "lucide-react/dist/esm/icons/settings";
import RotateCcw from "lucide-react/dist/esm/icons/rotate-ccw";
import User from "lucide-react/dist/esm/icons/user";
import X from "lucide-react/dist/esm/icons/x";
import { useEffect, useRef, useState } from "react";
import { PopoverSurface } from "../../design-system/components/popover/PopoverPrimitives";
import { getOpenCodeRestartRequiredStatus, restartOpenCodeServer } from "@services/tauri";
import {
  clearOpenCodeRestartRequired,
  markOpenCodeRestartRequired,
  notifyOpenCodeServerRestarted,
  readOpenCodeRestartNotice,
  subscribeOpenCodeServerRestarted,
  subscribeOpenCodeRestartNotice,
} from "@services/opencodeRestartNotice";
import { useDismissibleMenu } from "../hooks/useDismissibleMenu";

const RESTART_CHECK_FOCUS_THROTTLE_MS = 3_000;
const RESTART_CHECK_POST_RESTART_COOLDOWN_MS = 10_000;

type SidebarCornerActionsProps = {
  onOpenSettings: () => void;
  onOpenDebug: () => void;
  showDebugButton: boolean;
  showAccountSwitcher: boolean;
  accountLabel: string;
  accountActionLabel: string;
  accountDisabled: boolean;
  accountSwitching: boolean;
  accountCancelDisabled: boolean;
  onSwitchAccount: () => void;
  onCancelSwitchAccount: () => void;
};

export function SidebarCornerActions({
  onOpenSettings,
  onOpenDebug,
  showDebugButton,
  showAccountSwitcher,
  accountLabel,
  accountActionLabel,
  accountDisabled,
  accountSwitching,
  accountCancelDisabled,
  onSwitchAccount,
  onCancelSwitchAccount,
}: SidebarCornerActionsProps) {
  const [accountMenuOpen, setAccountMenuOpen] = useState(false);
  const [restartNotice, setRestartNotice] = useState(readOpenCodeRestartNotice);
  const [restartingServer, setRestartingServer] = useState(false);
  const accountMenuRef = useRef<HTMLDivElement | null>(null);
  const restartCheckInFlightRef = useRef(false);
  const restartCheckLastAtRef = useRef(0);
  const restartCheckCooldownUntilRef = useRef(0);

  useDismissibleMenu({
    isOpen: accountMenuOpen,
    containerRef: accountMenuRef,
    onClose: () => setAccountMenuOpen(false),
  });

  useEffect(() => {
    if (!showAccountSwitcher) {
      setAccountMenuOpen(false);
    }
  }, [showAccountSwitcher]);

  useEffect(() => subscribeOpenCodeRestartNotice(setRestartNotice), []);

  useEffect(() => {
    let cancelled = false;

    const checkRestartRequirement = async (force = false) => {
      const now = Date.now();
      if (!force) {
        if (restartCheckInFlightRef.current) {
          return;
        }
        if (now < restartCheckCooldownUntilRef.current) {
          return;
        }
        if (now - restartCheckLastAtRef.current < RESTART_CHECK_FOCUS_THROTTLE_MS) {
          return;
        }
      }

      restartCheckInFlightRef.current = true;
      try {
        const status = await getOpenCodeRestartRequiredStatus();
        if (cancelled || !status.detected) {
          return;
        }
        if (status.required) {
          markOpenCodeRestartRequired(status.reason ?? "OpenCode config changed.", "detector");
        }
      } catch {
        // No-op: banner is best-effort and should not break sidebar interactions.
      } finally {
        restartCheckInFlightRef.current = false;
        restartCheckLastAtRef.current = Date.now();
      }
    };

    void checkRestartRequirement(true);
    const onFocus = () => {
      void checkRestartRequirement();
    };
    const unsubscribeRestarted = subscribeOpenCodeServerRestarted(() => {
      restartCheckCooldownUntilRef.current =
        Date.now() + RESTART_CHECK_POST_RESTART_COOLDOWN_MS;
      restartCheckLastAtRef.current = Date.now();
    });
    window.addEventListener("focus", onFocus);

    return () => {
      cancelled = true;
      unsubscribeRestarted();
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  const handleRestartServer = async () => {
    if (restartingServer) {
      return;
    }
    setRestartingServer(true);
    try {
      await restartOpenCodeServer();
      clearOpenCodeRestartRequired();
      notifyOpenCodeServerRestarted();
    } finally {
      setRestartingServer(false);
    }
  };

  const reasonText = restartNotice.reason?.trim().replace(/[.\s]+$/, "") ?? "";
  const restartTooltip = reasonText
    ? `${reasonText}. Restart OpenCode to refresh agents and models.`
    : "OpenCode config changed. Restart OpenCode to refresh agents and models.";

  return (
    <div className="sidebar-corner-actions">
      <div className="sidebar-corner-actions-row">
        <button
          className="ghost sidebar-corner-button"
          type="button"
          onClick={onOpenSettings}
          aria-label="Open settings"
          title="Settings"
        >
          <Settings size={14} aria-hidden />
        </button>
        {showDebugButton && (
          <button
            className="ghost sidebar-corner-button"
            type="button"
            onClick={onOpenDebug}
            aria-label="Open debug log"
            title="Debug log"
          >
            <ScrollText size={14} aria-hidden />
          </button>
        )}
        {restartNotice.required && (
          <div className="sidebar-restart-banner-wrap">
            <button
              type="button"
              className={`sidebar-restart-banner${restartingServer ? " is-restarting" : ""}`}
              onClick={() => void handleRestartServer()}
              disabled={restartingServer}
              aria-label="Restart OpenCode server to apply config changes"
              aria-describedby="sidebar-restart-tooltip"
            >
              <RotateCcw size={12} aria-hidden />
              <span>{restartingServer ? "Restarting..." : "Restart"}</span>
            </button>
            <div
              id="sidebar-restart-tooltip"
              role="tooltip"
              className="sidebar-restart-tooltip"
            >
              <strong>Restart required</strong>
              <span>{restartTooltip}</span>
            </div>
          </div>
        )}
        {showAccountSwitcher && (
          <div className="sidebar-account-menu" ref={accountMenuRef}>
            <button
              className="ghost sidebar-corner-button"
              type="button"
              onClick={() => setAccountMenuOpen((open) => !open)}
              aria-label="Account"
              title="Account"
            >
              <User size={14} aria-hidden />
            </button>
            {accountMenuOpen && (
              <PopoverSurface className="sidebar-account-popover" role="dialog">
                <div className="sidebar-account-title">Account</div>
                <div className="sidebar-account-value">{accountLabel}</div>
                <div className="sidebar-account-actions-row">
                  <button
                    type="button"
                    className="primary sidebar-account-action"
                    onClick={onSwitchAccount}
                    disabled={accountDisabled}
                    aria-busy={accountSwitching}
                  >
                    <span className="sidebar-account-action-content">
                      {accountSwitching && (
                        <span className="sidebar-account-spinner" aria-hidden />
                      )}
                      <span>{accountActionLabel}</span>
                    </span>
                  </button>
                  {accountSwitching && (
                    <button
                      type="button"
                      className="secondary sidebar-account-cancel"
                      onClick={onCancelSwitchAccount}
                      disabled={accountCancelDisabled}
                      aria-label="Cancel account switch"
                      title="Cancel"
                    >
                      <X size={12} aria-hidden />
                    </button>
                  )}
                </div>
              </PopoverSurface>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
