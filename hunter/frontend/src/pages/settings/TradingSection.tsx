import { useEffect, useState } from 'react';
import type { AppSettings } from 'services/api';
import {
  NumberField,
  SettingsPanel,
  SettingsPanelIntro,
  SettingsRows,
  ToggleRow,
} from './SettingsPrimitives';

/** Smallest percent that survives the API's `Math.round(pct * 100)` as a non-zero
 *  bps value. Anything below rounds to `0`, which the backend rejects with a 400. */
const MIN_SLIPPAGE_PCT = 0.01;

/** `DEFAULT_SLIPPAGE_BPS` (2500 bps) as a percent — what a blank buy field means.
 *  Display only; the backend owns the value. */
const BUY_DEFAULT_PCT = 25;

interface TradingSectionProps {
  settings: AppSettings;
  saving: boolean;
  update: (patch: Partial<AppSettings>) => void;
  setError: (msg: string) => void;
}

/** Server bound on the copycat guard's memory (1 year); above it the API 400s. */
const MAX_DUPE_WINDOW_HOURS = 24 * 365;

export function TradingSection({ settings, saving, update, setError }: TradingSectionProps) {
  const [buySlipText, setBuySlipText] = useState('');
  const [sellSlipText, setSellSlipText] = useState('');
  const [maxSolText, setMaxSolText] = useState('');
  const [dupeWindowText, setDupeWindowText] = useState('');

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

  useEffect(() => {
    setDupeWindowText(String(settings.duplicate_identity_window_hours));
  }, [settings.duplicate_identity_window_hours]);

  // A typed percent is honored literally by the backend — no floor is applied on
  // the way in — so `0` (revert on any movement at all) is refused with a 400.
  // Blank is how you say "no opinion", and blank is what carries the per-side
  // policy: buy falls back to the server default, sell to no floor (sell all).
  // The API's percent→bps conversion is `Math.round(pct * 100)`, so anything under
  // 0.005% would round to a rejected 0 — hence `step`/`min` of 0.01.
  function commitBuySlippage() {
    const raw = buySlipText.trim();
    if (raw === '') {
      if (settings.buy_slippage_bps !== null) update({ buy_slippage_bps: null });
      return;
    }
    const pct = parseFloat(raw);
    if (!Number.isFinite(pct) || pct < MIN_SLIPPAGE_PCT || pct > 50) {
      setError(`Buy slippage must be between ${MIN_SLIPPAGE_PCT} and 50% (blank = default)`);
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
    if (!Number.isFinite(pct) || pct < MIN_SLIPPAGE_PCT || pct > 50) {
      setError(`Sell slippage must be between ${MIN_SLIPPAGE_PCT} and 50% (blank = no limit)`);
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

  // `0` is refused rather than treated as "off": the toggle is the off switch, so a
  // zero window could only produce a guard that reads as ON while blocking nothing.
  function commitDupeWindow() {
    const hours = parseInt(dupeWindowText.trim(), 10);
    if (!Number.isFinite(hours) || hours < 1 || hours > MAX_DUPE_WINDOW_HOURS) {
      setError(`Copycat memory must be between 1 and ${MAX_DUPE_WINDOW_HOURS} hours`);
      return;
    }
    if (hours !== settings.duplicate_identity_window_hours) {
      update({ duplicate_identity_window_hours: hours });
    }
  }

  return (
    <SettingsPanel>
      <SettingsPanelIntro
        title="Trading"
        description="Slippage and exposure ceiling for manual trades and the bot."
        tip={{
          body: `A typed percent is used exactly as typed — nothing is floored or rounded on the way in. Blank carries the per-side default: buy falls back to ${BUY_DEFAULT_PCT}% so an entry still lands on a fast mover, sell falls back to no limit so exits still clear during a rapid dump. 0 is not a value — it would revert on any movement at all — so it is refused; clear the field instead. Max committed SOL is a hard ceiling on open real positions.`,
        }}
      />
      <SettingsRows>
        <div className="flex flex-wrap gap-5 py-3.5">
          <NumberField
            label="Buy slippage %"
            hint={`Blank = ${BUY_DEFAULT_PCT}% default.`}
            min={MIN_SLIPPAGE_PCT}
            max={50}
            step={0.01}
            placeholder={String(BUY_DEFAULT_PCT)}
            value={buySlipText}
            disabled={saving}
            onChange={setBuySlipText}
            onCommit={commitBuySlippage}
          />
          <NumberField
            label="Sell slippage %"
            hint="Blank = no limit (sell all)."
            min={MIN_SLIPPAGE_PCT}
            max={50}
            step={0.01}
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
        <ToggleRow
          title="Skip copycat re-launches"
          description="Don't enter a token whose name + symbol was already traded on a different mint."
          tip="A scam re-launch keeps the name and icon and only changes the mint, so every per-token gate lets it through. Counts entry attempts, not fills — a buy that reverted still burns the identity. Paper and real keep separate memories, and a manual buy is recorded but never blocked. Turning it on starts from an empty memory (positions from before are not blocked); skips are logged with the mint."
          checked={settings.skip_duplicate_identity}
          disabled={saving}
          onChange={(skip_duplicate_identity) => update({ skip_duplicate_identity })}
        >
          <NumberField
            label="Memory (hours)"
            hint="Default 168 (7 days). Copycats land within hours; a long window bans common names."
            min={1}
            max={MAX_DUPE_WINDOW_HOURS}
            step={1}
            value={dupeWindowText}
            disabled={saving || !settings.skip_duplicate_identity}
            onChange={setDupeWindowText}
            onCommit={commitDupeWindow}
          />
        </ToggleRow>
      </SettingsRows>
    </SettingsPanel>
  );
}
