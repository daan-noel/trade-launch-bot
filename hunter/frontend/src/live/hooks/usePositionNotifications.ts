import { useEffect } from 'react';
import { connectArmedChanged, connectStrategyPositionUpdate } from 'services/sse';
import { useToast, type ToastVariant } from 'components/ui/Toast';
import { useNotificationPrefs } from 'hooks/useNotificationPrefs';

const STATUS_VARIANT: Record<string, ToastVariant> = {
  Armed: 'neutral',
  Disarmed: 'neutral',
  BuySubmitted: 'info',
  Holding: 'success',
  ExitPending: 'warning',
  End: 'neutral',
  ExitFailed: 'danger',
  ExitUnconfirmed: 'danger',
};

const STATUS_LABEL: Record<string, string> = {
  Armed: 'Armed',
  Disarmed: 'Disarmed',
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  End: 'Closed',
  ExitFailed: 'Exit failed',
  ExitUnconfirmed: 'Exit unconfirmed',
};

function fireToast(
  addToast: (title: string, body: string, variant: ToastVariant) => void,
  prefs: { desktopEnabled: boolean },
  status: string,
  title: string,
  body: string,
) {
  addToast(title, body, STATUS_VARIANT[status] ?? 'neutral');
  if (
    prefs.desktopEnabled &&
    typeof Notification !== 'undefined' &&
    Notification.permission === 'granted'
  ) {
    new Notification(title, { body, silent: true });
  }
}

/** Map `strategy_armed_changed.state` → Settings pill key. */
function armedStatusKey(state: string): 'Armed' | 'Disarmed' | null {
  if (state === 'armed') return 'Armed';
  if (state === 'disarmed') return 'Disarmed';
  return null;
}

/** Mounted once in the live App — subscribes to generic-engine position + arm
 *  deltas app-wide and fires toasts according to the user's notification prefs. */
export function usePositionNotifications() {
  const { addToast } = useToast();
  const [prefs] = useNotificationPrefs();

  useEffect(() => {
    const positionHandle = connectStrategyPositionUpdate((delta) => {
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

      fireToast(addToast, prefs, status, title, bodyParts.join(' | '));
    });

    const armedHandle = connectArmedChanged((delta) => {
      const status = armedStatusKey(delta.state);
      if (!status || !prefs.statuses.includes(status)) return;

      const tradeMode = delta.trade_mode ?? 'paper';
      const isReal = tradeMode === 'real';
      if (isReal && !prefs.realEnabled) return;
      if (!isReal && !prefs.paperEnabled) return;

      const ruleName = delta.rule_name ?? `rule ${delta.rule_id.slice(0, 8)}`;
      const symbol = delta.mint_address.slice(0, 8);
      const modeLabel = isReal ? 'real' : 'paper';

      const title = `[${modeLabel}] ${symbol} → ${STATUS_LABEL[status]}`;
      const bodyParts: string[] = [`"${ruleName}"`];
      if (status === 'Disarmed' && delta.reason) {
        bodyParts.push(delta.reason);
      }

      fireToast(addToast, prefs, status, title, bodyParts.join(' | '));
    });

    return () => {
      positionHandle.close();
      armedHandle.close();
    };
  }, [addToast, prefs]);
}
