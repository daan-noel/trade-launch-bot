import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { connectArmedChanged, connectStrategyPositionUpdate } from 'services/sse';
import { useToast, type ToastVariant } from 'components/ui/Toast';
import { useNotificationPrefs } from 'hooks/useNotificationPrefs';
import { opsNotifyHref } from 'lib/strategy/nav';
import { ensureNotificationSw, showDesktopNotify } from '@live/lib/desktopNotify';

const STATUS_VARIANT: Record<string, ToastVariant> = {
  Armed: 'neutral',
  Disarmed: 'neutral',
  BuySubmitted: 'info',
  Holding: 'success',
  ExitPending: 'warning',
  End: 'neutral',
  EntryFailed: 'warning',
  ExitStuck: 'danger',
  ExitUnconfirmed: 'danger',
};

const STATUS_LABEL: Record<string, string> = {
  Armed: 'Armed',
  Disarmed: 'Disarmed',
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  End: 'Closed',
  EntryFailed: 'Buy never filled',
  ExitStuck: 'Exit stuck',
  ExitUnconfirmed: 'Exit unconfirmed',
};

/** Map `strategy_armed_changed.state` → Settings pill key. */
function armedStatusKey(state: string): 'Armed' | 'Disarmed' | null {
  if (state === 'armed') return 'Armed';
  if (state === 'disarmed') return 'Disarmed';
  return null;
}

/**
 * Mode filter for toasts. Missing mode: allow if either real or paper is enabled
 * (do not invent paper/real — that drops real alerts).
 */
function modeAllowed(
  tradeMode: string | null | undefined,
  prefs: { realEnabled: boolean; paperEnabled: boolean },
): boolean {
  if (tradeMode === 'real') return prefs.realEnabled;
  if (tradeMode === 'paper') return prefs.paperEnabled;
  return prefs.realEnabled || prefs.paperEnabled;
}

/** Mounted once in the live App — position/arm toasts + desktop notifications.
 *  Desktop path uses a service worker (actions, status icon, lifecycle tags).
 *  Click / Ops → Ops deep-link; Trade action → trade desk. */
export function usePositionNotifications() {
  const { addToast } = useToast();
  const [prefs] = useNotificationPrefs();
  const navigate = useNavigate();

  // Warm the notification SW so the first alert can use action buttons.
  useEffect(() => {
    if (prefs.desktopEnabled) void ensureNotificationSw();
  }, [prefs.desktopEnabled]);

  // SW click → navigate (also page-Notification fallback via CustomEvent).
  useEffect(() => {
    const onSwMessage = (e: MessageEvent) => {
      if (e.data?.type === 'hunter-notification-navigate' && typeof e.data.href === 'string') {
        navigate(e.data.href);
      }
    };
    const onPageNav = (e: Event) => {
      const href = (e as CustomEvent<{ href: string }>).detail?.href;
      if (href) navigate(href);
    };
    navigator.serviceWorker?.addEventListener('message', onSwMessage);
    window.addEventListener('hunter-notification-navigate', onPageNav);
    return () => {
      navigator.serviceWorker?.removeEventListener('message', onSwMessage);
      window.removeEventListener('hunter-notification-navigate', onPageNav);
    };
  }, [navigate]);

  useEffect(() => {
    const positionHandle = connectStrategyPositionUpdate((delta) => {
      const status = delta.status;
      if (!prefs.statuses.includes(status)) return;
      if (!modeAllowed(delta.trade_mode, prefs)) return;

      const tradeMode = delta.trade_mode ?? 'real';
      const modeLabel = tradeMode === 'paper' ? 'paper' : 'real';
      const ruleName = delta.rule_name ?? `rule ${delta.rule_id.slice(0, 8)}`;
      const href = opsNotifyHref({
        status,
        mode: tradeMode,
        mint: delta.mint_address,
        ruleId: delta.rule_id,
        positionId: delta.position_id,
      });
      const tradeHref = `/console?mint=${encodeURIComponent(delta.mint_address)}`;

      const detail =
        status === 'End' ||
        status === 'EntryFailed' ||
        status === 'ExitStuck' ||
        status === 'ExitUnconfirmed'
          ? delta.exit_reason
          : null;

      addToast(
        `[${modeLabel}] ${delta.mint_address.slice(0, 8)} → ${STATUS_LABEL[status] ?? status}`,
        [`"${ruleName}"`, detail].filter(Boolean).join(' | '),
        STATUS_VARIANT[status] ?? 'neutral',
      );

      if (prefs.desktopEnabled) {
        void showDesktopNotify({
          status,
          mint: delta.mint_address,
          modeLabel,
          ruleName,
          detail,
          href,
          tradeHref,
          tag: `hunter-pos-${delta.position_id}`,
        });
      }
    });

    const armedHandle = connectArmedChanged((delta) => {
      const status = armedStatusKey(delta.state);
      if (!status || !prefs.statuses.includes(status)) return;
      // Leave-Waiting after buy — not an operator-facing disarm (dead/migrated/…).
      if (status === 'Disarmed' && delta.reason === 'entered') return;
      if (!modeAllowed(delta.trade_mode, prefs)) return;

      const tradeMode = delta.trade_mode ?? 'paper';
      const modeLabel = tradeMode === 'real' ? 'real' : 'paper';
      const ruleName = delta.rule_name ?? `rule ${delta.rule_id.slice(0, 8)}`;
      const href = opsNotifyHref({
        status,
        mode: tradeMode,
        mint: delta.mint_address,
        ruleId: delta.rule_id,
      });
      const tradeHref = `/console?mint=${encodeURIComponent(delta.mint_address)}`;
      const detail = status === 'Disarmed' ? delta.reason : null;

      addToast(
        `[${modeLabel}] ${delta.mint_address.slice(0, 8)} → ${STATUS_LABEL[status]}`,
        [`"${ruleName}"`, detail].filter(Boolean).join(' | '),
        STATUS_VARIANT[status] ?? 'neutral',
      );

      if (prefs.desktopEnabled) {
        void showDesktopNotify({
          status,
          mint: delta.mint_address,
          modeLabel,
          ruleName,
          detail,
          href,
          tradeHref,
          // Same tag for arm→disarm so the OS replaces the waiting toast.
          tag: `hunter-arm-${delta.rule_id}-${delta.mint_address}`,
        });
      }
    });

    return () => {
      positionHandle.close();
      armedHandle.close();
    };
  }, [addToast, prefs, navigate]);
}
