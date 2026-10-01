import { cn } from 'lib/cn';
import { ixLabelsActions } from 'lib/ixLabels';
// Deep imports: type-only w.r.t. lightweight-charts, so a host outside the chart
// chunk can draw the lens controls without loading the charting library.
import {
  EMPTY_LENS_MATCH,
  lensItemId,
  type LensMatch,
} from 'components/token-price-chart/lensTint';
import type { ChartLensSizeLabels } from 'components/token-price-chart/types';
import { FeePinToggles } from 'components/tokens/FeePinToggles';
import type { TokenHighlight } from 'components/tokens/useTokenHighlight';
import { formatFeePins } from 'lib/strategy/ixPatternRows';

/*
 * The highlight-lens controls (`useTokenHighlight`): the target button that arms a
 * lens from a row, and the chips that state and disarm the armed ones. One set, so
 * every table that arms a lens (the trades table, the Entry Context breakdown)
 * draws the same control.
 */

/** Target glyph for a highlight-lens toggle — reads as "find this everywhere". */
function LensIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden className="size-3">
      <circle cx="8" cy="8" r="3.25" stroke="currentColor" strokeWidth="1.4" />
      <path
        d="M8 1.5v2.2M8 12.3v2.2M1.5 8h2.2M12.3 8h2.2"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
    </svg>
  );
}

/** The glyph's box. A button and its spacer MUST share it: a row that renders one
 *  and a row that renders neither would start their content at different x, which
 *  reads as a ragged column. */
const LENS_SLOT = 'block size-3 shrink-0 p-px';

/** Holds the slot open on a row that has nothing to arm (no labels captured). */
export function LensSpacer() {
  return <span className={LENS_SLOT} aria-hidden />;
}

/**
 * The one control that arms a highlight lens. Lit while its target is the armed
 * one, so a row can say "this is what the chart is washing" without a legend.
 */
export function LensButton({
  armed,
  color,
  title,
  onClick,
}: {
  armed: boolean;
  color: string;
  title: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={armed}
      title={title}
      onClick={(e) => {
        // Several hosts make the row itself selectable; arming a lens must not
        // also move the table's selection.
        e.stopPropagation();
        onClick();
      }}
      className={cn(
        LENS_SLOT,
        'rounded transition focus:outline-none focus-visible:ring-1 focus-visible:ring-primary',
        armed ? 'opacity-100' : 'opacity-30 hover:opacity-90',
      )}
      style={{ color: armed ? color : undefined }}
    >
      <LensIcon />
    </button>
  );
}

/** Short address for a chip — the address itself is a column away. */
function shortAddr(addr: string): string {
  return addr.length > 12 ? `${addr.slice(0, 4)}…${addr.slice(-4)}` : addr;
}

/** `+4.21` / `-0.08` — sign always shown, because the sign is the point. */
function signedSol(match: LensMatch): string {
  const net = match.buySol - match.sellSol;
  return `${net >= 0 ? '+' : '−'}${Math.abs(net).toFixed(3)}`;
}

/**
 * One armed lens, stated in full: what is washed, how much of the token it is,
 * and the button that turns it off.
 *
 * The counts come from the CHART's own match (`onHighlightLensMatch`), not from a
 * second pass over the rows — a chip that quoted a different number from the wash
 * beside it would make the reader distrust both.
 */
function LensChip({
  color,
  label,
  title,
  match,
  onClear,
}: {
  color: string;
  label: string;
  title: string;
  match: LensMatch;
  onClear: () => void;
}) {
  const total = match.buys + match.sells;
  return (
    <span
      className="inline-flex max-w-full items-center gap-1.5 rounded border px-1.5 py-px font-mono text-[10px]"
      style={{ borderColor: `${color}99`, backgroundColor: `${color}1f`, color }}
      title={title}
    >
      <span className="truncate">{label}</span>
      <span className="text-text-dim">
        {total} tx ({match.buys}b/{match.sells}s) · net {signedSol(match)} SOL
      </span>
      <button
        type="button"
        onClick={onClear}
        title="Stop highlighting"
        className="leading-none opacity-70 hover:opacity-100"
      >
        ×
      </button>
    </span>
  );
}

/** The size-label switch: what prints under each wash. */
const SIZE_MODES: { mode: ChartLensSizeLabels; label: string; title: string }[] = [
  { mode: 'buys', label: 'buys', title: 'Beside each lane mark: the SOL its buys moved in that candle' },
  { mode: 'all', label: 'all', title: 'Beside each lane mark: +buy SOL and −sell SOL in that candle' },
  { mode: 'off', label: 'off', title: 'No numbers in the lane' },
];

function SizeLabelSwitch({
  mode,
  onChange,
}: {
  mode: ChartLensSizeLabels;
  onChange: (mode: ChartLensSizeLabels) => void;
}) {
  return (
    <span
      className="inline-flex items-center gap-1 font-mono text-[10px] text-text-dim"
      title="The number beside each mark in the highlight lane under the chart: one trade prints its SOL, several print the sum and (count). A taller mark moved more SOL. Hover a candle for each trade's size, fee and wallet."
    >
      sizes
      {SIZE_MODES.map((m) => (
        <button
          key={m.mode}
          type="button"
          title={m.title}
          aria-pressed={mode === m.mode}
          onClick={() => onChange(m.mode)}
          className={cn(
            'rounded px-1 leading-4',
            mode === m.mode ? 'bg-white/10 text-text' : 'hover:text-text',
          )}
        >
          {m.label}
        </button>
      ))}
    </span>
  );
}

/**
 * The armed highlight lenses for this token, shown wherever the trades panel is —
 * including with no candle selected, so the control that disarms a lens never
 * hides behind the table it is washing. Next to them, which fee fields a structure
 * click pins: shown before anything is armed, since it decides what the first
 * click arms.
 *
 * Counts are bar-aligned: they cover exactly the trades the chart could paint, so
 * dust legs the candles drop are absent here too.
 */
export function LensChips({ highlight }: { highlight: TokenHighlight }) {
  const { items, matches, unlabeled, remove, clear, sizeLabels, setSizeLabels, pinMask, setPinMask } = highlight;
  const anyStructure = items.some((i) => i.kind === 'structure');
  return (
    <div className="mb-2 flex flex-wrap items-center gap-2">
      {items.map((item) => {
        const match = matches.get(lensItemId(item)) ?? EMPTY_LENS_MATCH;
        if (item.kind === 'wallet') {
          return (
            <LensChip
              key={lensItemId(item)}
              color={item.color}
              label={shortAddr(item.key)}
              title={`${item.key} — every candle this wallet traded in is marked in its lane row, in this color`}
              match={match}
              onClear={() => remove(item)}
            />
          );
        }
        const text = item.labels ? ixLabelsActions([...item.labels]) : '';
        const pins = item.pins ? formatFeePins(item.pins) : '';
        return (
          <LensChip
            key={lensItemId(item)}
            color={item.color}
            label={`${text || 'ix structure'}${pins ? ` · ${pins}` : ''}`}
            title={
              `${text}${pins ? `
pinned: ${pins}` : ''}

Every candle carrying this EXACT ordered structure` +
              (pins ? ' with these fee readings' : ', any CU and fee,') +
              ` is marked in its lane row, in this color. View-only — no fingerprint or rule reads it.`
            }
            match={match}
            onClear={() => remove(item)}
          />
        );
      })}
      {anyStructure && unlabeled > 0 && (
        <span
          className="font-mono text-[10px] text-text-dim"
          title="Trades with no captured ix structure — a structure lens cannot match them"
        >
          {unlabeled} unlabeled
        </span>
      )}
      {items.length > 0 && <SizeLabelSwitch mode={sizeLabels} onChange={setSizeLabels} />}
      <FeePinToggles mask={pinMask} onChange={setPinMask} onto="the highlighted structure" label="highlight pin" />
      {items.length > 1 && (
        <button
          type="button"
          onClick={clear}
          className="text-[10px] text-text-dim hover:text-text"
          title="Stop highlighting everything"
        >
          clear all
        </button>
      )}
    </div>
  );
}
