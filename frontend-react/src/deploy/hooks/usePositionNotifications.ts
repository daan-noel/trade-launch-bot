import { useEffect } from 'react';
import { connectAllPositionsChanged } from 'services/sse';
import { useToast, type ToastVariant } from 'components/ui/Toast';
import { useNotificationPrefs } from 'hooks/useNotificationPrefs';
import type { RuleNotifSnapshot } from 'types';

const STATUS_VARIANT: Record<string, ToastVariant> = {
  Arming: 'neutral',
  BuySubmitted: 'info',
  Holding: 'success',
  ExitPending: 'warning',
  End: 'neutral',
  ExitFailed: 'danger',
};

const STATUS_LABEL: Record<string, string> = {
  Arming: 'Arming',
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  End: 'Closed',
  ExitFailed: 'Exit failed',
};

function fmtSol(v: number) {
  return `${v}◎`;
}

function fmtCu(v: number) {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(0)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(0)}K`;
  return String(v);
}

function formatFpGroups(snapshot: RuleNotifSnapshot, fpParams: string[]): string {
  const parts: string[] = [];

  if (fpParams.includes('buy_tol')) {
    const segs: string[] = [];
    if (snapshot.p_token_initial_buy_sol != null) segs.push(`ib:${fmtSol(snapshot.p_token_initial_buy_sol)}`);
    segs.push(`tol:${snapshot.tolerance_pct}%`);
    if (segs.length) parts.push(segs.join(' '));
  }

  if (fpParams.includes('cu')) {
    const segs: string[] = [];
    if (snapshot.p_token_cu_limit != null) segs.push(`lim:${fmtCu(snapshot.p_token_cu_limit)}`);
    if (snapshot.p_token_cu_price != null) segs.push(`cu:${fmtCu(snapshot.p_token_cu_price)}`);
    if (segs.length) parts.push(segs.join(' '));
  }

  if (fpParams.includes('cost')) {
    const segs: string[] = [];
    if (snapshot.p_token_max_sol_cost != null) segs.push(`max:${fmtSol(snapshot.p_token_max_sol_cost)}`);
    if (snapshot.p_token_spendable_sol_in != null) segs.push(`spnd:${fmtSol(snapshot.p_token_spendable_sol_in)}`);
    if (segs.length) parts.push(segs.join(' '));
  }

  if (fpParams.includes('ix')) {
    const labels = snapshot.p_token_ix_labels;
    const count = labels.length;
    const shown = labels.slice(0, 3).join(',');
    const overflow = count > 3 ? `,+${count - 3}` : '';
    parts.push(count > 0 ? `ix:${count} [${shown}${overflow}]` : `ix:0`);
  }

  if (fpParams.includes('tp_sl')) {
    parts.push(`TP:${snapshot.p_exit_take_profit}% SL:-${snapshot.p_exit_stop_loss}%`);
  }

  return parts.join(' ');
}

/** Mounted once in AppLayout — subscribes to all position deltas app-wide and
 *  fires toasts according to the user's notification preferences. */
export function usePositionNotifications() {
  const { addToast } = useToast();
  const [prefs] = useNotificationPrefs();

  useEffect(() => {
    const handle = connectAllPositionsChanged((delta) => {
      if (delta.removed || !delta.position) return;

      const { position, ruleSnapshot, strategy } = delta;
      const status = position.status;

      if (!prefs.statuses.includes(status)) return;

      const tradeMode = ruleSnapshot?.trade_mode ?? position.strategy;
      const isReal = tradeMode === 'real';
      if (isReal && !prefs.realEnabled) return;
      if (!isReal && !prefs.paperEnabled) return;

      const ruleName = ruleSnapshot?.rule_name ?? `rule ${delta.ruleId.slice(0, 8)}`;
      const symbol = position.symbol ?? position.mint.slice(0, 8);
      const stratLabel = strategy.toUpperCase();
      const modeLabel = isReal ? 'real' : 'paper';

      const title = `[${stratLabel} · ${modeLabel}] ${symbol} → ${STATUS_LABEL[status] ?? status}`;

      const bodyParts: string[] = [`"${ruleName}"`];

      if (status === 'End' && position.pnl_percent != null) {
        const sign = position.pnl_percent >= 0 ? '+' : '';
        bodyParts.push(`${sign}${position.pnl_percent.toFixed(1)}% (${position.exit_reason ?? ''})`);
      }

      if (ruleSnapshot && prefs.fpParams.length > 0) {
        const fp = formatFpGroups(ruleSnapshot, prefs.fpParams);
        if (fp) bodyParts.push(fp);
      }

      const body = bodyParts.join(' | ');
      addToast(title, body, STATUS_VARIANT[status] ?? 'neutral');

      if (prefs.desktopEnabled && typeof Notification !== 'undefined' && Notification.permission === 'granted') {
        new Notification(title, { body, silent: true });
      }
    });

    return () => handle.close();
  }, [addToast, prefs]);
}
