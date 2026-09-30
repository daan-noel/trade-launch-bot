import { useMemo } from 'react';
import { Terms } from './HelpText';
import type { AreaKey, HisClass, Overlap, Zone } from '@lab/lib/entryContext/overlap';
import { overlapLayout, REGIONS, type Region } from '@lab/lib/entryContext/overlapLayout';

type Cls = HisClass | 'none';

const COLOR = {
  pool: 'var(--color-info)',
  pass: 'var(--color-secondary)',
  his: 'var(--color-primary)',
} as const;

/** The short label drawn beside a count. */
const CLASS_LABEL: Record<Cls, string> = {
  point: 'at a point',
  signal: 'signal, fails',
  bought: 'no signal',
  none: 'he never bought',
};

const ZONE_WORDS: Record<Zone, string> = {
  pass: 'have a buy that passes the filters',
  pool: 'are in the pool, with no buy that passes the filters',
  outside: 'have no buy made with the selected IXs',
};

const CLASS_WORDS: Record<Cls, string> = {
  point: 'he bought at a point: right after the selected IXs, with the filters passing',
  signal: 'he bought right after the selected IXs, but the filters fail on his buy',
  bought: 'he bought, but not right after the selected IXs',
  none: 'he never bought',
};

const CLASS_FILL: Record<Cls, string> = {
  point: COLOR.his,
  signal: 'var(--color-accent)',
  bought: 'var(--color-text)',
  none: 'var(--color-text-mid)',
};

const count = (v: number) => v.toLocaleString();

/** The one-sentence meaning of an area, with its count. */
export function areaSentence(key: AreaKey, n: number): string {
  const [zone, cls] = key.split(':') as [Zone, Cls];
  return `${count(n)} tokens ${ZONE_WORDS[zone]}, and ${CLASS_WORDS[cls]}.`;
}

const ZONE_LINE: Record<Zone, string> = {
  pass: 'The token has a buy made with the selected IXs that passes the filters.',
  pool: 'The token has buys made with the selected IXs, but none passes the filters.',
  outside: 'The token has no buy made with the selected IXs.',
};

const CLASS_LINE: Record<Cls, string> = {
  point: 'He bought right after such a buy, and the filters pass on it.',
  signal: 'He bought right after such a buy, but the filters fail on it.',
  bought: 'He bought the token, but not right after such a buy.',
  none: 'He never bought the token.',
};

/** One step a line: the time, who buys, what the filters say; the last line is the result. */
const EXAMPLE: Partial<Record<AreaKey, readonly string[]>> = {
  'pool:none': [
    '10:00:00  Wallet A buys with the selected IXs. The filters fail.',
    'He never buys this token.',
    'Result: no point on the token, and no buy of his.',
  ],
  'pass:none': [
    '10:00:00  Wallet A buys with the selected IXs. The filters pass: a point.',
    'He never buys this token.',
    'Result: the market offered a point, and he did not take it.',
  ],
  'pass:point': [
    '10:00:00  Wallet A buys with the selected IXs. The filters pass: a point.',
    '10:00:01  He buys, right after Wallet A.',
    'Result: he bought at the point.',
  ],
  'pass:signal': [
    '10:00:00  Wallet A buys with the selected IXs. The filters pass: a point. He does not buy.',
    '10:03:00  Wallet B buys with the selected IXs. The filters fail.',
    '10:03:01  He buys, right after Wallet B.',
    'Result: the token has a point, but the buy he followed is not one.',
  ],
  'pass:bought': [
    '10:00:00  Wallet A buys with the selected IXs. The filters pass: a point. He does not buy.',
    '10:05:00  He buys. No buy with the selected IXs lands just before his.',
    'Result: the token has a point, but his buy does not follow any buy made with the selected IXs.',
  ],
  'pool:signal': [
    '10:00:00  Wallet A buys with the selected IXs. The filters fail.',
    '10:00:01  He buys, right after Wallet A.',
    'Result: he followed the selected IXs, and no buy on the token passes the filters.',
  ],
  'pool:bought': [
    '10:00:00  Wallet A buys with the selected IXs. The filters fail.',
    '10:05:00  He buys. No buy with the selected IXs lands just before his.',
    'Result: no point on the token, and his buy does not follow the selected IXs.',
  ],
  'outside:bought': [
    '10:00:00  He buys.',
    'No wallet buys this token with the selected IXs in the range.',
    'Result: the token is outside the pool; he bought it for another reason.',
  ],
};

/** The hover text of a count: one fact a line, then an example. */
function areaTip(key: AreaKey, n: number): string {
  const [zone, cls] = key.split(':') as [Zone, Cls];
  const example = EXAMPLE[key];
  return [
    `${count(n)} tokens`,
    '',
    `Market: ${ZONE_LINE[zone]}`,
    `Him: ${CLASS_LINE[cls]}`,
    ...(example ? ['', 'Example:', ...example.map((line) => `  ${line}`)] : []),
    ...(n > 0 ? ['', 'Click to list them.'] : []),
  ].join('\n');
}

const WIDTH = 780;
const HEIGHT = 380;
const LINE = 22;
/** The strip under the drawing, for the counts of the parts too small to hold them. */
const STRIP = 104;

/** The areas each part of the drawing holds, top to bottom. */
const REGION_AREAS: Record<Region, readonly AreaKey[]> = {
  poolOnly: ['pool:none'],
  passOnly: ['pass:none'],
  passHis: ['pass:point', 'pass:signal', 'pass:bought'],
  poolHis: ['pool:point', 'pool:signal', 'pool:bought'],
  outside: ['outside:point', 'outside:signal', 'outside:bought'],
};

/** A part's name, over its counts in the strip. */
const REGION_NAME: Record<Region, string> = {
  poolOnly: 'Pool only',
  passOnly: 'Passing only',
  passHis: 'Passing + his',
  poolHis: 'Pool only + his',
  outside: 'His, outside the pool',
};

const clsOf = (key: AreaKey) => key.split(':')[1] as Cls;
/** The drawn width of a count and its label. */
const rowWidth = (key: AreaKey, n: number) => count(n).length * 8.5 + 5 + CLASS_LABEL[clsOf(key)].length * 5.8;

/**
 * His tokens against the market's, as three overlapping sets: the pool (tokens
 * with a buy made with the selected IXs), the tokens that pass the filters inside
 * it, and the tokens he bought. Drawn to scale (`overlapLayout`): each shape's area
 * and each shared area follow the token counts. A part's counts sit inside it when
 * they fit, and in a strip under the drawing with a line to the part when they do
 * not; an empty area shows nothing. Each count is a button that lists its tokens.
 * The legend says what each shape and each count label means, in the drawing's colors.
 */
export function OverlapDiagram({
  areas,
  pool,
  pass,
  his,
  target,
  selected,
  onSelect,
}: {
  areas: Overlap;
  /** Tokens in the pool, in the passing set, and of his. */
  pool: number;
  pass: number;
  his: number;
  /** Which IXs are selected, in words: `the pattern set "6Vo3"`. */
  target: string;
  selected: AreaKey | null;
  /** The clicked area; the selected one again clears it. */
  onSelect: (key: AreaKey | null) => void;
}) {
  const { at, inside, strip } = useMemo(() => {
    const parts = REGIONS.map((region) => ({
      region,
      keys: REGION_AREAS[region].filter((k) => areas[k].length > 0),
    })).filter((p) => p.keys.length > 0);
    const size = (keys: readonly AreaKey[]) => keys.reduce((n, k) => n + areas[k].length, 0);
    const at = overlapLayout(
      { pool, pass, his, hisInPass: size(REGION_AREAS.passHis), hisOutside: size(REGION_AREAS.outside) },
      WIDTH,
      HEIGHT,
      Object.fromEntries(
        parts.map((p) => [
          p.region,
          { w: Math.max(...p.keys.map((k) => rowWidth(k, areas[k].length))) + 14, h: p.keys.length * LINE + 6 },
        ]),
      ),
    );
    const placed = parts.map((p) => ({ ...p, spot: at.spots[p.region] }));
    return {
      at,
      inside: placed.filter((p) => p.spot?.fits),
      // Left to right as the parts lie, so the lines do not cross.
      strip: placed.filter((p) => !p.spot?.fits).sort((x, y) => (x.spot?.x ?? WIDTH) - (y.spot?.x ?? WIDTH)),
    };
  }, [areas, pool, pass, his]);

  const rows = (keys: readonly AreaKey[], x: number, y: number) =>
    keys.map((key, i) => {
      const n = areas[key].length;
      const on = selected === key;
      const pick = () => onSelect(on ? null : key);
      return (
        <g
          key={key}
          role="button"
          tabIndex={0}
          aria-pressed={on}
          onClick={pick}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') pick();
          }}
          style={{ cursor: 'pointer' }}
        >
          <title>{areaTip(key, n)}</title>
          <text x={x} y={y + i * LINE} textAnchor="middle" fontSize={11} textDecoration={on ? 'underline' : undefined}>
            <tspan fontSize={14} fontWeight={700} fill={CLASS_FILL[clsOf(key)]}>
              {count(n)}
            </tspan>
            <tspan dx={5} fill={on ? 'var(--color-text)' : 'var(--color-text-mid)'} fontWeight={on ? 700 : 400}>
              {CLASS_LABEL[clsOf(key)]}
            </tspan>
          </text>
        </g>
      );
    });

  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT + (strip.length > 0 ? STRIP : 0)}`}
        className="w-full max-w-[860px] flex-1 basis-[560px]"
        role="group"
        aria-label="His tokens against the market's"
      >
        {at.pool && <ellipse {...at.pool} fill={COLOR.pool} fillOpacity={0.06} stroke={COLOR.pool} strokeOpacity={0.6} />}
        {at.pass && <ellipse {...at.pass} fill={COLOR.pass} fillOpacity={0.08} stroke={COLOR.pass} strokeOpacity={0.6} />}
        {at.his && <ellipse {...at.his} fill={COLOR.his} fillOpacity={0.12} stroke={COLOR.his} strokeOpacity={0.8} />}

        {inside.map((p) => (
          <g key={p.region}>{rows(p.keys, p.spot!.x, p.spot!.y - ((p.keys.length - 1) * LINE) / 2 + 5)}</g>
        ))}
        {strip.map((p, i) => {
          const x = (WIDTH * (i + 0.5)) / strip.length;
          return (
            <g key={p.region}>
              {p.spot && (
                <>
                  <line x1={x} y1={HEIGHT + 4} x2={p.spot.x} y2={p.spot.y} stroke="var(--color-text-mid)" strokeOpacity={0.7} />
                  <circle cx={p.spot.x} cy={p.spot.y} r={2.5} fill="var(--color-text)" />
                </>
              )}
              <text x={x} y={HEIGHT + 18} textAnchor="middle" fontSize={10} fontWeight={700} fill="var(--color-text-mid)">
                {REGION_NAME[p.region]}
              </text>
              {rows(p.keys, x, HEIGHT + 40)}
            </g>
          );
        })}
      </svg>
      <div className="flex min-w-[260px] flex-1 flex-col gap-3 text-[11px] text-text-mid">
        <Legend
          title="The three shapes"
          rows={[
            [
              COLOR.pool,
              `In pool · ${count(pool)}`,
              ['Tokens with a buy made with the selected IXs.', 'The buyer can be any wallet.'],
            ],
            [
              COLOR.pass,
              `Pass filters · ${count(pass)}`,
              ['Tokens in the pool where at least one such buy passes the filters.', 'Always inside the pool.'],
            ],
            [COLOR.his, `His tokens · ${count(his)}`, ['Tokens he bought in the range.']],
          ]}
        />
        <Legend
          title="The numbers: his buy on the token"
          rows={[
            [
              CLASS_FILL.point,
              CLASS_LABEL.point,
              ['He bought right after a buy made with the selected IXs.', 'The filters pass on that buy.'],
            ],
            [
              CLASS_FILL.signal,
              CLASS_LABEL.signal,
              [
                'He bought right after such a buy.',
                'The filters fail on that buy.',
                'Inside Pass filters: another such buy on the token passes, and he did not follow it.',
              ],
            ],
            [
              CLASS_FILL.bought,
              CLASS_LABEL.bought,
              [
                'He bought the token, but not right after such a buy.',
                'Inside Pass filters: the token has a passing buy, and his buy is not a reaction to it.',
              ],
            ],
            [CLASS_FILL.none, CLASS_LABEL.none, ['No buy of his on the token.']],
          ]}
        />
        <div className="flex flex-col gap-0.5 text-text-dim">
          <span>Drawn to scale: each shape and each overlap is as large as its token count.</span>
          <span>A part too small for its numbers shows them under the drawing, with a line to the part.</span>
          <span>The selected IXs are {target}.</span>
        </div>
      </div>
    </div>
  );
}

/** One entry per row: its name on the first line, then one fact a line. */
function Legend({ title, rows }: { title: string; rows: readonly (readonly [string, string, readonly string[]])[] }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim">{title}</span>
      {rows.map(([color, name, lines]) => (
        <div key={name} className="flex items-baseline gap-1.5">
          <span className="size-2 shrink-0 rounded-full" style={{ background: color }} />
          <div className="flex flex-col gap-0.5">
            <span className="font-semibold" style={{ color }}>
              {name}
            </span>
            {lines.map((line) => (
              <span key={line}>
                <Terms text={line} />
              </span>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
