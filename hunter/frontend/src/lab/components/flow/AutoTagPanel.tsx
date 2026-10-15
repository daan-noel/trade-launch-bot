import { useState, type CSSProperties } from 'react';

import { LENS_COLORS } from 'components/token-price-chart/constants';
import { LensButton } from 'components/tokens/LensControls';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { SpinnerIcon } from 'components/ui/icons';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import { cn } from 'lib/cn';
import { tagLabel } from 'lib/flow/tapeClassify';
import { parseIxPatternRow, patternRowKey, type IxPatternRow } from 'lib/strategy/ixPatternRows';
import type { OwnerRuleId, OwnerSplitParams, OwnerStage } from 'types';
import { AutoTagGuideButton } from './AutoTagGuide';
import { RowPins } from './AutoTagRowPins';
import {
  AUTO_TAG_FAMILIES,
  EVIDENCE_HELP,
  type AutoTagFamilyId,
  type AutoTagRule,
  type Evidence,
} from './autoTagRules';

/** Wallets listed before the list folds. */
const WALLETS_SHOWN = 60;

const PANEL_HELP =
  'Turn a rule on to label its wallets’ trades owner; the list below is what Apply would save. Guide shows the whole workflow.';

const EVIDENCE_CLASS: Record<Evidence, string> = {
  measured: 'bg-green/15 text-green',
  weak: 'bg-warning/15 text-warning',
  unproven: 'bg-white/8 text-text-dim',
};

const fmtSol = (n: number) => (n >= 100 ? n.toFixed(0) : n.toFixed(1));

/** The preview chart's highlight as the auto-tag list arms it - the same lenses the
 *  trades table and the ranked table arm (`useTokenHighlight`, owned by the page). */
export interface AutoTagLens {
  toggleRow: (row: IxPatternRow) => void;
  /** The row's own lens color, `null` = not armed. */
  rowColor: (row: IxPatternRow) => string | null;
  /** Colors of every armed row sharing a trade with this one. */
  rowColors: (row: IxPatternRow) => string[];
  toggleWallet: (wallet: string) => void;
  walletColor: (wallet: string) => string | null;
}

/** A lit item washes in its first lens color, the trades table's `--row-lens`. */
const lensWash = (colors: readonly string[]): { className?: string; style?: CSSProperties } =>
  colors.length > 0 ? { className: 'bg-(--row-lens)/16', style: { '--row-lens': colors[0] } as CSSProperties } : {};

/** A staged row's lens button: arms the row itself, the trades it would match on the tag. */
function RowLens({ lens, row }: { lens: AutoTagLens; row: IxPatternRow }) {
  const own = lens.rowColor(row);
  return (
    <LensButton
      armed={own != null}
      color={own ?? LENS_COLORS.structure[0]}
      litBy={lens.rowColors(row)[0] ?? null}
      title={
        own != null
            ? 'Stop highlighting this row'
            : 'Highlight every candle and trade this row matches on the preview'
      }
      onClick={() => lens.toggleRow(row)}
    />
  );
}

/**
 * The auto-tag panel of the owner-split tag: one tab per rule family, one toggle per
 * rule, and the stage the toggles produce (the rows and wallets Apply would save).
 */
export function AutoTagPanel({
  tagName,
  params,
  stage,
  loading,
  error,
  rules,
  onRulesChange,
  tokenMint,
  draftRowKeys,
  draftWallets,
  lens,
  onPickRows,
  onPickWallets,
}: {
  tagName: string;
  /** The run's thresholds; `null` on a run cached before the rules existed. */
  params: OwnerSplitParams | null;
  stage: OwnerStage | null;
  loading: boolean;
  error: string | null;
  rules: ReadonlySet<OwnerRuleId>;
  onRulesChange: (rules: Set<OwnerRuleId>) => void;
  tokenMint: string | null;
  draftRowKeys: ReadonlySet<string>;
  draftWallets: ReadonlySet<string>;
  /** The preview chart's highlight; absent = no lens buttons. Pass a memoized one. */
  lens?: AutoTagLens | null;
  /** Ticks (`on`) or unticks staged rows - one from a checkbox, all from the toggle. */
  onPickRows: (rows: readonly IxPatternRow[], on: boolean) => void;
  onPickWallets: (wallets: readonly string[], on: boolean) => void;
}) {
  const [family, setFamily] = useState<AutoTagFamilyId>('creation_slot');
  const [allWallets, setAllWallets] = useState(false);
  const fam = AUTO_TAG_FAMILIES.find((f) => f.id === family) ?? AUTO_TAG_FAMILIES[0];

  /** Why a rule cannot be switched, or `null` when it can. */
  const blocked = (r: AutoTagRule): string | null => {
    if (!params) return 'This run predates auto-tag. Run discovery again.';
    if (r.market && params.market_from == null) {
      return 'Off for this run: the lake market read failed, so snipers and never-sellers are unknown. Run discovery again.';
    }
    return null;
  };
  const flip = (id: OwnerRuleId) => {
    const next = new Set(rules);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onRulesChange(next);
  };
  const coin = tokenMint ? stage?.by_mint[tokenMint] : undefined;
  const wallets = stage?.wallets ?? [];
  const shownWallets = allWallets ? wallets : wallets.slice(0, WALLETS_SHOWN);
  // What the select-all toggle acts on: every row and savable wallet with a checkbox.
  const pickRows = (stage?.rows ?? []).flatMap((r) => parseIxPatternRow(r.row) ?? []);
  const pickWallets = wallets.filter((w) => w.reason == null).map((w) => w.wallet);
  const pickCount = pickRows.length + pickWallets.length;
  const allPicked =
    pickCount > 0 &&
    pickRows.every((r) => draftRowKeys.has(patternRowKey(r))) &&
    pickWallets.every((w) => draftWallets.has(w));
  const pickAll = (on: boolean) => {
    onPickRows(pickRows, on);
    onPickWallets(pickWallets, on);
  };

  return (
    <div className="rounded-lg border border-warning/30 bg-warning/4 p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1 text-xs font-semibold text-text">
          Auto-tag {tagLabel(tagName)}
          <InfoTooltip title="Auto-tag" body={PANEL_HELP} />
        </span>
        <AutoTagGuideButton params={params} />
        <ToggleGroup
          size="sm"
          tone="neutral"
          aria-label="Rule family"
          value={family}
          onChange={setFamily}
          options={AUTO_TAG_FAMILIES.map((f) => ({ value: f.id, label: f.label, title: f.help }))}
        />
        {loading && <SpinnerIcon className="h-3.5 w-3.5 text-text-dim" />}
        <button
          type="button"
          aria-pressed={allPicked}
          disabled={pickCount === 0}
          onClick={() => pickAll(!allPicked)}
          title={
            allPicked
              ? 'Untick every row and wallet below: Apply saves none of them.'
              : 'Tick every row and wallet below into the draft.'
          }
          className={cn(
            'ml-auto inline-flex items-center gap-1.5 rounded border px-2 py-1 text-[11px] font-semibold transition',
            'disabled:cursor-not-allowed disabled:opacity-40',
            allPicked ? 'border-warning bg-warning/20 text-warning' : 'border-white/15 text-text-mid hover:border-warning/50 hover:text-text',
          )}
        >
          {allPicked ? 'Unselect all' : 'Select all'}
        </button>
      </div>
      <p className="mb-2 text-[11px] leading-snug text-text-dim">{fam.help}</p>

      <div className="flex flex-wrap gap-2">
        {fam.rules.map((r) => {
          const why = blocked(r);
          const on = rules.has(r.id);
          const reach = stage?.reach[r.id];
          const help = params ? r.help(params) : '';
          return (
            <button
              key={r.id}
              type="button"
              aria-pressed={on}
              disabled={why != null}
              onClick={() => flip(r.id)}
              title={`${r.label}\n\n${help}\n\n${EVIDENCE_HELP[r.evidence]}\n\n${why ?? (on ? 'On: its wallets’ trades are owner. Click to turn off.' : 'Click to label its wallets’ trades owner.')}`}
              className={cn(
                'inline-flex items-center gap-1.5 rounded border px-2 py-1 text-[11px] font-semibold transition',
                'disabled:cursor-not-allowed disabled:opacity-40',
                on ? 'border-warning bg-warning/20 text-warning' : 'border-white/15 text-text-mid hover:border-warning/50 hover:text-text',
              )}
            >
              <span aria-hidden className={cn('size-2 rounded-full border border-current', on && 'bg-current')} />
              {r.label}
              <span className={cn('rounded px-1 text-[9px] font-medium', EVIDENCE_CLASS[r.evidence])}>{r.evidence}</span>
              <span className="font-mono text-[10px] font-normal opacity-80">
                {reach ? `${reach.coins} coins · ${reach.trades} tr` : '0'}
              </span>
            </button>
          );
        })}
      </div>

      {error && <p className="mt-2 text-[11px] text-red">{error}</p>}
      {stage && (
        <div className="mt-3 flex flex-col gap-2 border-t border-white/8 pt-2 text-[11px]">
          <p className="text-text-mid">
            Owner now: <b>{stage.owner_trades.toLocaleString()}</b> trades, <b>{fmtSol(stage.owner_sol)} SOL</b> of{' '}
            {fmtSol(stage.total_sol)} ({stage.total_sol > 0 ? ((stage.owner_sol / stage.total_sol) * 100).toFixed(0) : 0}%)
            across {stage.group_coins} coins
            {coin && (
              <>
                {' '}
                &middot; this coin: {coin[0]} of {coin[1]} trades
              </>
            )}
            <InfoTooltip
              title="Owner now"
              body="The trades the rules that are on label owner, plus the create transaction's wallets, which are owner on their coin whatever is on."
            />
          </p>

          <div>
            <p className="mb-1 inline-flex items-center gap-1 font-semibold text-text">
              Rows to save ({stage.rows.length})
              <InfoTooltip
                title="Rows"
                body={
                  params
                    ? `Every ix structure an owner trade used, read at the doc's seven match levels (core and side, up to the exact list and budget). ` +
                      `A row is saved when ${params.row_min_owner_pct}%+ of its SOL across the group is owner, over ${params.row_min_trades}+ trades. ` +
                      'The fewest rows that cover every owner trade they can are picked, the most owner SOL first. A row matches new wallets on new coins.'
                    : ''
                }
              />
            </p>
            <ul className="flex max-h-72 flex-col gap-1 overflow-y-auto pr-1">
              {stage.rows.map((r) => {
                const row = parseIxPatternRow(r.row);
                if (!row) return null;
                const k = patternRowKey(row);
                const wash = lens ? lensWash(lens.rowColors(row)) : {};
                return (
                  <li
                    key={k}
                    className={cn('flex items-center gap-2 rounded border border-white/8 px-2 py-1', wash.className ?? 'bg-white/3')}
                    style={wash.style}
                  >
                    {lens && <RowLens lens={lens} row={row} />}
                    <input
                      type="checkbox"
                      className="accent-accent"
                      checked={draftRowKeys.has(k)}
                      onChange={(e) => onPickRows([row], e.target.checked)}
                      title="In the draft. Untick to keep it out."
                    />
                    <div className="flex min-w-0 flex-1 flex-col gap-1">
                      <div className="flex min-w-0 items-center gap-2">
                        <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-text-mid" title={row.labels.join(' › ')}>
                          {row.labels.join(' › ')}
                        </span>
                        <span className="shrink-0 font-mono text-[10px] text-text-dim">
                          <span className="cursor-help" title="Owner share: the part of this row's SOL across the group that is owner.">
                            <b className="text-text">{r.owner_pct.toFixed(0)}%</b> owner
                          </span>
                          {' · '}
                          <span className="cursor-help" title="Trades across the group this row matches.">
                            {r.trades} tr
                          </span>
                          {' · '}
                          <span className="cursor-help" title="Coins of the group this row matches a trade on.">
                            {r.coins} coins
                          </span>
                        </span>
                      </div>
                      <RowPins level={r.level} row={row} />
                    </div>
                  </li>
                );
              })}
              {stage.failing_rows.map((r) => {
                const row = parseIxPatternRow(r.row);
                if (!row) return null;
                const wash = lens ? lensWash(lens.rowColors(row)) : {};
                return (
                  <li
                    key={`x-${patternRowKey(row)}`}
                    className={cn('flex items-center gap-2 rounded border border-white/5 px-2 py-1 opacity-50', wash.className)}
                    style={wash.style}
                    title={`Not saved: ${r.reason}`}
                  >
                    {lens && <RowLens lens={lens} row={row} />}
                    <span className="w-3.5 shrink-0 text-center text-text-dim">-</span>
                    <span className="min-w-0 flex-1 truncate font-mono text-[10px]">{row.labels.join(' › ')}</span>
                    <span className="shrink-0 text-[10px] text-text-dim">{r.reason}</span>
                  </li>
                );
              })}
              {stage.rows.length === 0 && stage.failing_rows.length === 0 && (
                <li className="text-text-dim">Turn a rule on to find rows.</li>
              )}
            </ul>
          </div>

          <div>
            <p className="mb-1 inline-flex items-center gap-1 font-semibold text-text">
              Wallets to save ({wallets.filter((w) => w.reason == null).length})
              <InfoTooltip
                title="Wallets"
                body={
                  params
                    ? `Only wallets that come back are saved: Sell with no buy or Transfer pair on ${params.comes_back_min_coins}+ coins of the group, or a never-seller. ` +
                      `A wallet is saved when its latest trade on the group is ${stage.recent_window_days} days or less before the group's last trade (the recent window); a wallet quiet for longer is likely retired. Creation-slot wallets are never saved: they are new on the next coin.`
                    : ''
                }
              />
              {stage.one_coin_wallets > 0 && (
                <span className="font-normal text-text-dim">
                  &middot; {stage.one_coin_wallets} on one coin only, not saved
                </span>
              )}
            </p>
            <ul className="flex max-h-48 flex-col gap-1 overflow-y-auto pr-1">
              {shownWallets.map((w) => {
                const own = lens?.walletColor(w.wallet) ?? null;
                const wash = lensWash(own ? [own] : []);
                return (
                <li
                  key={w.wallet}
                  className={cn(
                    'flex items-center gap-2 rounded border px-2 py-1',
                    w.reason ? 'border-white/5 opacity-50' : 'border-white/8',
                    wash.className ?? (w.reason ? undefined : 'bg-white/3'),
                  )}
                  style={wash.style}
                  title={w.reason ? `Not saved: ${w.reason}` : undefined}
                >
                  {lens && (
                    <LensButton
                      armed={own != null}
                      color={own ?? LENS_COLORS.wallet[0]}
                      title={
                        own != null
                            ? 'Stop highlighting this wallet'
                            : "Highlight this wallet's candles and trades on the preview"
                      }
                      onClick={() => lens.toggleWallet(w.wallet)}
                    />
                  )}
                  {w.reason ? (
                    <span className="w-3.5 shrink-0 text-center text-text-dim">-</span>
                  ) : (
                    <input
                      type="checkbox"
                      className="accent-accent"
                      checked={draftWallets.has(w.wallet)}
                      onChange={(e) => onPickWallets([w.wallet], e.target.checked)}
                      title="In the draft. Untick to keep it out."
                    />
                  )}
                  <span className="min-w-0 flex-1">
                    <AddressDisplay address={w.wallet} kind="account" />
                  </span>
                  <span className="shrink-0 text-[10px] text-text-dim">
                    {w.reason ?? `${w.rules.join(', ')} · ${w.coins} coins · last trade ${w.days_since.toFixed(1)} d before the group's last`}
                  </span>
                </li>
                );
              })}
              {wallets.length > WALLETS_SHOWN && (
                <li>
                  <button type="button" className="text-[10px] text-accent hover:underline" onClick={() => setAllWallets((v) => !v)}>
                    {allWallets ? 'Show fewer' : `Show all ${wallets.length}`}
                  </button>
                </li>
              )}
              {wallets.length === 0 && <li className="text-text-dim">Turn a Transfers rule on to find wallets.</li>}
            </ul>
          </div>
        </div>
      )}
    </div>
  );
}
