import { useEffect, useState } from 'react';
import type { AppSettings } from 'services/api';
import {
  NumberField,
  SettingsPanel,
  SettingsPanelIntro,
  SettingsRows,
} from './SettingsPrimitives';

interface TradingSectionProps {
  settings: AppSettings;
  saving: boolean;
  update: (patch: Partial<AppSettings>) => void;
  setError: (msg: string) => void;
}

export function TradingSection({ settings, saving, update, setError }: TradingSectionProps) {
  const [buySlipText, setBuySlipText] = useState('');
  const [sellSlipText, setSellSlipText] = useState('');
  const [maxSolText, setMaxSolText] = useState('');

  useEffect(() => {
    setBuySlipText(
      settings.buy_slippage_bps != null ? String(settings.buy_slippage_bps / 100) : '',
    );
  }, [settings.buy_slippage_bps]);

  useEffect(() => {
    setSellSlipText(
      settings.sell_slippage_bps != null ? String(settings.sell_slippage_bps / 100) : '',
    );
  }, [settings.sell_slippage_bps]);

  useEffect(() => {
    setMaxSolText(settings.max_committed_sol != null ? String(settings.max_committed_sol) : '');
  }, [settings.max_committed_sol]);

  function commitBuySlippage() {
    const raw = buySlipText.trim();
    if (raw === '') {
      if (settings.buy_slippage_bps !== null) update({ buy_slippage_bps: null });
      return;
    }
    const pct = parseFloat(raw);
    if (!Number.isFinite(pct) || pct < 0 || pct > 50) {
      setError('Buy slippage must be between 0 and 50% (0 = no limit)');
      return;
    }
    const bps = Math.round(pct * 100);
    if (bps !== settings.buy_slippage_bps) update({ buy_slippage_bps: bps });
  }

  function commitSellSlippage() {
    const raw = sellSlipText.trim();
    if (raw === '') {
      if (settings.sell_slippage_bps !== null) update({ sell_slippage_bps: null });
      return;
    }
    const pct = parseFloat(raw);
    if (!Number.isFinite(pct) || pct < 0 || pct > 50) {
      setError('Sell slippage must be between 0 and 50% (0 or blank = no limit)');
      return;
    }
    const bps = Math.round(pct * 100);
    if (bps !== settings.sell_slippage_bps) update({ sell_slippage_bps: bps });
  }

  function commitMaxCommittedSol() {
    const raw = maxSolText.trim();
    if (raw === '') {
      if (settings.max_committed_sol != null) update({ max_committed_sol: null });
      return;
    }
    const sol = parseFloat(raw);
    if (!Number.isFinite(sol) || sol <= 0) {
      setError('Max committed SOL must be a positive number');
      return;
    }
    if (sol !== settings.max_committed_sol) update({ max_committed_sol: sol });
  }

  return (
    <SettingsPanel>
      <SettingsPanelIntro
        title="Trading"
        description="Slippage and exposure ceiling for manual trades and the bot."
        tip={{
          body: 'Buy default is 5% when blank. Sell default is no limit so bot exits still clear during a rapid dump. Max committed SOL is a hard ceiling on open real positions.',
        }}
      />
      <SettingsRows>
        <div className="flex flex-wrap gap-5 py-3.5">
          <NumberField
            label="Buy slippage %"
            hint="Blank = server default (5%). 0 = no limit."
            min={0}
            max={50}
            step={0.1}
            placeholder="5"
            value={buySlipText}
            disabled={saving}
            onChange={setBuySlipText}
            onCommit={commitBuySlippage}
          />
          <NumberField
            label="Sell slippage %"
            hint="Blank or 0 = no limit (always fills)."
            min={0}
            max={50}
            step={0.1}
            placeholder="no limit"
            value={sellSlipText}
            disabled={saving}
            onChange={setSellSlipText}
            onCommit={commitSellSlippage}
          />
          <NumberField
            label="Max committed SOL"
            hint="Hard ceiling on open real positions. Blank = no limit."
            min={0}
            step={0.01}
            placeholder="no limit"
            value={maxSolText}
            disabled={saving}
            onChange={setMaxSolText}
            onCommit={commitMaxCommittedSol}
          />
        </div>
      </SettingsRows>
    </SettingsPanel>
  );
}
