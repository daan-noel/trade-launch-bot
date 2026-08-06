import { useMemo, useState } from 'react';

import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Input } from 'components/ui/Input';
import type { FlowDiscoveryTokenGross, TradeRecord } from 'types';

import { FlowPreviewChart } from './FlowPreviewChart';

function fmt(n: number, digits = 1): string {
  if (!Number.isFinite(n)) return '—';
  return n.toFixed(digits);
}

/** Ranked member-token picker (cheap gross_sol roster, no trade payload) +
 *  the per-token candlestick preview for whichever one is picked. Only the
 *  picked token's full trade history is ever fetched — the roster itself
 *  can't show a real vol/organic split (that needs the trade-level
 *  classifier), which is exactly what picking a token and reading the chart
 *  below gives you instead. */
export function TokenPreviewPanel({
  tokens,
  selectedMint,
  onSelect,
  trades,
  tradesLoading,
  creatorWallet,
  athPriceInSol,
  isMigrated,
  tokenCreatedAt,
  patternKeys,
  onTogglePattern,
}: {
  tokens: FlowDiscoveryTokenGross[];
  selectedMint: string | null;
  onSelect: (mint: string) => void;
  trades: TradeRecord[];
  tradesLoading: boolean;
  creatorWallet: string | null;
  athPriceInSol: number | null;
  isMigrated: boolean;
  /** Token `created_at` (ISO) — zero point for the chart tooltip's "+age". */
  tokenCreatedAt: string | null;
  patternKeys: ReadonlySet<string>;
  onTogglePattern: (labels: string[]) => void;
}) {
  const [mintQuery, setMintQuery] = useState('');
  const filteredTokens = useMemo(() => {
    const q = mintQuery.trim().toLowerCase();
    if (!q) return tokens;
    return tokens.filter((t) => t.mint_address.toLowerCase().includes(q));
  }, [tokens, mintQuery]);
  return (
    <div className="rounded border border-white/8 p-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <span className="text-xs font-semibold text-text-mid">
          Token preview ·{' '}
          {mintQuery.trim()
            ? `${filteredTokens.length.toLocaleString()} / ${tokens.length.toLocaleString()}`
            : tokens.length.toLocaleString()}{' '}
          ranked
        </span>
        {selectedMint && tradesLoading && (
          <span className="text-[10px] text-text-dim">Loading trades…</span>
        )}
      </div>
      <div className="flex flex-wrap gap-3">
        <div className="flex w-56 shrink-0 flex-col gap-1">
          <Input
            type="search"
            value={mintQuery}
            onChange={(e) => setMintQuery(e.target.value)}
            placeholder="Search mint…"
            className="h-7 text-[11px]"
          />
          <div className="flex max-h-70 flex-col gap-1 overflow-y-auto pr-1">
            {filteredTokens.length === 0 && (
              <p className="px-2 py-1 text-[10px] text-text-dim">No tokens match.</p>
            )}
            {filteredTokens.map((t) => (
            <div
              key={t.mint_address}
              role="button"
              tabIndex={0}
              onClick={() => onSelect(t.mint_address)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  onSelect(t.mint_address);
                }
              }}
              className={`cursor-pointer rounded border px-2 py-1 text-left text-[10px] transition ${
                t.mint_address === selectedMint
                  ? 'border-accent/50 bg-accent/10 text-text'
                  : 'border-white/8 text-text-mid hover:border-white/20'
              }`}
            >
              <AddressDisplay
                address={t.mint_address}
                truncate={false}
                kind="token"
                iconSize="lg"
                stopPropagation={false}
              />
              <div className="text-text-dim">
                {fmt(t.gross_sol)}◎ · {t.n_trades.toLocaleString()} trades
              </div>
            </div>
          ))}
          </div>
        </div>
        <div className="min-w-0 flex-1">
          {selectedMint ? (
            <FlowPreviewChart
              trades={trades}
              patternKeys={patternKeys}
              onTogglePattern={onTogglePattern}
              creatorWallet={creatorWallet}
              athPriceInSol={athPriceInSol}
              isMigrated={isMigrated}
              tokenCreatedAt={tokenCreatedAt}
            />
          ) : (
            <p className="text-[11px] text-text-dim">
              Pick a token to preview its vol/non-vol split — updates live as you toggle
              structures below.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
