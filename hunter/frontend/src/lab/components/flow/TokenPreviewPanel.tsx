import { useDeferredValue, useMemo, useState, type UIEvent } from 'react';

import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Input } from 'components/ui/Input';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import type { ChartTapeStaging } from 'components/tokens/chartTapeStaging';
import type { TokenHighlight } from 'components/tokens/useTokenHighlight';
import { lensItemId } from 'components/token-price-chart/lensTint';
import { useGetFlowLensHitsQuery } from '@lab/store/labEndpoints';
import { apiErrorMessage } from 'store/baseApi';
import type { FlowDiscoveryTokenGross, TokenDetailRecord } from 'types';
import { lensHitItems, lensRoster, tokenLensHits } from './tokenLensHits';

/** Cards drawn at first, and added each time the list is scrolled near its end:
 *  a group can hold thousands of tokens. */
const CARD_PAGE = 200;

function fmt(n: number, digits = 1): string {
  if (!Number.isFinite(n)) return '—';
  return n.toFixed(digits);
}

/** Ranked member-token picker (cheap gross_sol roster, no trade payload) +
 *  the per-token candlestick preview for whichever one is picked. Only the
 *  picked token's full trade history is ever fetched — the roster itself
 *  can't show a real vol/organic split (that needs the trade-level
 *  classifier), which is exactly what picking a token and reading the chart
 *  below gives you instead.
 *
 *  While a highlight is armed, `flow-discovery/lens-hits` says which coins carry
 *  each item: a chip per item filters the list to coins carrying every chip
 *  switched on, and each card shows a dot per item it carries, in the item's wash
 *  color. The query lives here, not on the page, so its answer repaints only this
 *  panel; the list reads the deferred items, so a toggle paints its button first.
 *
 *  The preview is {@link LazyTokenTradeChart}: overlay, badges and highlight
 *  follow the cart's selected list via {@link ChartTapeStaging}. */
export function TokenPreviewPanel({
  tokens,
  selectedMint,
  onSelect,
  detail,
  tape,
  highlight = null,
  runId,
  groupKey,
}: {
  tokens: FlowDiscoveryTokenGross[];
  selectedMint: string | null;
  onSelect: (mint: string) => void;
  detail: TokenDetailRecord | null | undefined;
  tape: ChartTapeStaging;
  /** The page-owned highlight, shared with the ranked table; default the chart's own. */
  highlight?: TokenHighlight | null;
  /** The run and group `tokens` belong to: what the lens-hits read is scoped to. */
  runId: string | null;
  groupKey: Record<string, unknown>;
}) {
  const [mintQuery, setMintQuery] = useState('');
  /** Chips switched on, by `lensItemId`; a coin must carry every one. */
  const [required, setRequired] = useState<ReadonlySet<string>>(() => new Set());
  const [matchingOnly, setMatchingOnly] = useState(true);

  const liveItems = highlight?.items;
  const items = useDeferredValue(useMemo(() => liveItems ?? [], [liveItems]));
  // Which group coins carry each armed item: the chips and dots.
  const lensItemsBody = useMemo(() => lensHitItems(items), [items]);
  const lensHitsQ = useGetFlowLensHitsQuery(
    { run_id: runId ?? '', group_key: groupKey, items: lensItemsBody },
    { skip: !runId || lensItemsBody.length === 0 },
  );
  const lensHitsLoading = lensHitsQ.isFetching;
  const lensHitsError = lensHitsQ.error ? apiErrorMessage(lensHitsQ.error) : null;
  const hits = items.length > 0 ? (lensHitsQ.data?.by_mint ?? null) : null;
  // A disarmed item's chip is gone, so it stops filtering.
  const requiredNow = useMemo(() => {
    const ids = new Set(items.map(lensItemId));
    return new Set([...required].filter((id) => ids.has(id)));
  }, [items, required]);

  const filteredTokens = useMemo(() => {
    const q = mintQuery.trim().toLowerCase();
    const searched = q ? tokens.filter((t) => t.mint_address.toLowerCase().includes(q)) : tokens;
    return lensRoster(searched, items, hits, requiredNow, matchingOnly);
  }, [tokens, mintQuery, items, hits, requiredNow, matchingOnly]);
  // A new list starts back at one page of cards.
  const [shown, setShown] = useState(CARD_PAGE);
  const [shownFor, setShownFor] = useState(filteredTokens);
  if (shownFor !== filteredTokens) {
    setShownFor(filteredTokens);
    setShown(CARD_PAGE);
  }
  const onListScroll = (e: UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    if (shown < filteredTokens.length && el.scrollTop + el.clientHeight > el.scrollHeight - 300) {
      setShown((n) => n + CARD_PAGE);
    }
  };

  /** Roster coins carrying each item, by `lensItemId`. */
  const chipCounts = useMemo(() => {
    const m = new Map<string, number>();
    if (!hits) return m;
    for (const t of tokens) {
      const coin = hits[t.mint_address];
      if (!coin) continue;
      for (const id of Object.keys(coin)) m.set(id, (m.get(id) ?? 0) + 1);
    }
    return m;
  }, [tokens, hits]);

  const toggleChip = (id: string) => {
    setRequired((cur) => {
      const next = new Set(cur);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    setMatchingOnly(true);
  };

  const filtering = hits != null && matchingOnly;
  const emptyText = !filtering
    ? 'No tokens match.'
    : requiredNow.size > 1
      ? `No token has all ${requiredNow.size} — turn one off.`
      : requiredNow.size === 1
        ? 'No token has this one.'
        : 'No token has any highlighted item.';

  return (
    <div className="rounded border border-white/8 p-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <span className="text-xs font-semibold text-text-mid">
          Token preview ·{' '}
          {filteredTokens.length !== tokens.length
            ? `${filteredTokens.length.toLocaleString()} / ${tokens.length.toLocaleString()}`
            : tokens.length.toLocaleString()}{' '}
          ranked
        </span>
      </div>
      {items.length > 0 && (
        <div className="mb-2 flex flex-wrap items-center gap-1.5">
          {items.map((item) => {
            const id = lensItemId(item);
            const on = requiredNow.has(id);
            const n = chipCounts.get(id);
            return (
              <button
                key={id}
                type="button"
                onClick={() => toggleChip(id)}
                title={on ? 'Stop requiring this item' : 'Show only tokens that have this item'}
                className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 font-mono text-[10px] transition ${
                  on ? 'text-text' : 'border-white/10 text-text-mid hover:border-white/25'
                }`}
                style={on ? { borderColor: item.color, backgroundColor: `${item.color}26` } : undefined}
              >
                <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: item.color }} />
                <span className="max-w-[24ch] truncate">{item.label}</span>
                <span className="text-text-dim">
                  · {lensHitsLoading && !hits ? '…' : `${n ?? 0}/${tokens.length}`}
                </span>
              </button>
            );
          })}
          <span className="ml-auto inline-flex overflow-hidden rounded border border-white/10 text-[10px]">
            {([false, true] as const).map((v) => (
              <button
                key={String(v)}
                type="button"
                onClick={() => setMatchingOnly(v)}
                className={`px-2 py-0.5 transition ${
                  matchingOnly === v ? 'bg-white/10 text-text' : 'text-text-dim hover:text-text'
                }`}
              >
                {v ? 'Matching' : 'All'}
              </button>
            ))}
          </span>
          {lensHitsError && <span className="w-full text-[10px] text-red">{lensHitsError}</span>}
        </div>
      )}
      <div className="flex flex-wrap gap-3">
        <div className="flex min-h-120 w-56 shrink-0 flex-col gap-1">
          <Input
            type="search"
            value={mintQuery}
            onChange={(e) => setMintQuery(e.target.value)}
            placeholder="Search mint…"
            className="h-7 text-[11px]"
          />
          {/* Absolute so the roster fills the chart's height without setting it. */}
          <div className="relative min-h-0 flex-1">
          <div className="absolute inset-0 flex flex-col gap-1 overflow-y-auto pr-1" onScroll={onListScroll}>
            {filteredTokens.length === 0 && (
              <p className="px-2 py-1 text-[10px] text-text-dim">{emptyText}</p>
            )}
            {filteredTokens.slice(0, shown).map((t) => {
              const carried = tokenLensHits(t.mint_address, items, hits);
              return (
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
              <div className="flex items-center gap-1 text-text-dim">
                <span className="min-w-0 flex-1 truncate">
                  {fmt(t.gross_sol)}◎ · {t.n_trades.toLocaleString()} trades
                </span>
                {carried.map(({ item, trades }) => (
                  <span
                    key={lensItemId(item)}
                    title={`${item.label} · ${trades.toLocaleString()} trades`}
                    className="size-2 shrink-0 rounded-full"
                    style={{ backgroundColor: item.color }}
                  />
                ))}
              </div>
            </div>
              );
            })}
          </div>
          </div>
        </div>
        <div className="min-w-0 flex-1">
          {selectedMint ? (
            detail ? (
              <LazyTokenTradeChart
                detail={detail}
                tape={tape}
                highlight={highlight}
                tableId="flow_preview_trades"
              />
            ) : (
              <p className="text-[11px] text-text-dim">Loading token…</p>
            )
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
