import { useEffect } from 'react';
import { connectStrategyPositionUpdate } from 'services/sse';
import { useToast, type ToastVariant } from 'components/ui/Toast';
import { useNotificationPrefs } from 'hooks/useNotificationPrefs';

const STATUS_VARIANT: Record<string, ToastVariant> = {
  BuySubmitted: 'info',
  Holding: 'success',
  ExitPending: 'warning',
  End: 'neutral',
  ExitFailed: 'danger',
  ExitUnconfirmed: 'danger',
};

const STATUS_LABEL: Record<string, string> = {
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  End: 'Closed',
  ExitFailed: 'Exit failed',
  ExitUnconfirmed: 'Exit unconfirmed',
};

/** Mounted once in the live App — subscribes to generic-engine position deltas
 *  app-wide and fires toasts according to the user's notification preferences. */
export function usePositionNotifications() {
  const { addToast } = useToast();
  const [prefs] = useNotificationPrefs();

  useEffect(() => {
    const handle = connectStrategyPositionUpdate((delta) => {
      const status = delta.status;
      if (!prefs.statuses.includes(status)) return;

      const tradeMode = delta.trade_mode ?? 'paper';
      const isReal = tradeMode === 'real';
      if (isReal && !prefs.realEnabled) return;
      if (!isReal && !prefs.paperEnabled) return;

      const ruleName = delta.rule_name ?? `rule ${delta.rule_id.slice(0, 8)}`;
      const symbol = delta.mint_address.slice(0, 8);
      const modeLabel = isReal ? 'real' : 'paper';

      const title = `[${modeLabel}] ${symbol} → ${STATUS_LABEL[status] ?? status}`;

      const bodyParts: string[] = [`"${ruleName}"`];
      if (status === 'End' && delta.exit_reason) {
        bodyParts.push(delta.exit_reason);
      }
      if (
        (status === 'ExitFailed' || status === 'ExitUnconfirmed') &&
        delta.exit_reason
      ) {
        bodyParts.push(delta.exit_reason);
      }

      addToast(title, bodyParts.join(' | '), STATUS_VARIANT[status] ?? 'neutral');

      if (
        prefs.desktopEnabled &&
        typeof Notification !== 'undefined' &&
        Notification.permission === 'granted'
      ) {
        new Notification(title, { body: bodyParts.join(' | '), silent: true });
      }
    });

    return () => handle.close();
  }, [addToast, prefs]);
}
