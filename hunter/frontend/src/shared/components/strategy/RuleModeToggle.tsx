import { useState } from 'react';

import { apiErrorMessage } from 'store/baseApi';
import { useUpdateStrategyRuleMutation } from 'store/sharedEndpoints';
import type { StrategyRule, TradeMode } from 'lib/strategy/types';
import { ModeBadge } from './ModeBadge';

/**
 * A rule's mode badge as a button: click confirms, then flips paper <-> real.
 * A flip starts a new run; positions already open keep the mode they opened in.
 */
export function RuleModeToggle({
  rule,
  disabled,
  onError,
}: {
  rule: StrategyRule;
  disabled?: boolean;
  onError?: (message: string) => void;
}) {
  const [update] = useUpdateStrategyRuleMutation();
  const [busy, setBusy] = useState(false);
  const next: TradeMode = rule.trade_mode === 'real' ? 'paper' : 'real';

  const flip = async () => {
    const msg =
      next === 'real'
        ? `Switch "${rule.rule_name}" to REAL? New entries will spend real SOL on-chain.`
        : `Switch "${rule.rule_name}" to PAPER? New entries stop spending real SOL; open real positions keep their mode.`;
    if (!window.confirm(msg)) return;
    setBusy(true);
    try {
      await update({ id: rule.id, body: { trade_mode: next } }).unwrap();
    } catch (e) {
      const text = apiErrorMessage(e as never) ?? 'Mode switch failed';
      if (onError) onError(text);
      else window.alert(text);
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      type="button"
      disabled={disabled || busy}
      className="cursor-pointer rounded-full disabled:cursor-not-allowed disabled:opacity-60"
      title={`Switch "${rule.rule_name}" to ${next}`}
      aria-label={`Switch ${rule.rule_name} to ${next}`}
      onClick={(e) => {
        e.stopPropagation();
        void flip();
      }}
    >
      <ModeBadge mode={rule.trade_mode} />
    </button>
  );
}
