import { useMemo, useState, type KeyboardEvent } from 'react';
import { Button } from 'components/ui/Button';
import { Terms } from './HelpText';
import type { AreaKey, HisClass, Overlap, Zone } from '@lab/lib/entryContext/overlap';
import { overlapLayout, type Region } from '@lab/lib/entryContext/overlapLayout';

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

const WIDTH = 720;
const HEIGHT = 400;
const LINE = 22;
/** The strip under the drawing: one group of counts for each part. Kept when hidden, so the height holds. */
const STRIP = 100;
/** The width one group takes in the strip. */
const GROUP = 200;

/**
 * The parts of the drawing, each with its areas: `circles` are drawn as circles,
 * `rest` is the part's remaining tokens, shown as a count in text. A part of his
 * always lists its counts as one group under the drawing; a market part does only
 * when its count has no room inside it.
 */
const PARTS: readonly {
  region: Region;
  name: string;
  color: string;
  his: boolean;
  circles: readonly AreaKey[];
  rest: AreaKey;
}[] = [
  { region: 'poolOnly', name: 'Rest of In pool', color: COLOR.pool, his: false, circles: [], rest: 'pool:none' },
  { region: 'passOnly', name: 'Rest of Pass filters', color: COLOR.pass, his: false, circles: [], rest: 'pass:none' },
  {
    region: 'passHis',
    name: 'His tokens in Pass filters',
    color: COLOR.pass,
    his: true,
    circles: ['pass:point', 'pass:signal'],
    rest: 'pass:bought',
  },
  {
    region: 'poolHis',
    name: 'His tokens in pool only',
    color: COLOR.pool,
    his: true,
    circles: ['pool:point', 'pool:signal'],
    rest: 'pool:bought',
  },
  {
    region: 'outside',
    name: 'His tokens outside the pool',
    color: COLOR.his,
    his: true,
    circles: ['outside:point', 'outside:signal'],
    rest: 'outside:bought',
  },
];

const clsOf = (key: AreaKey) => key.split(':')[1] as Cls;
/** The drawn width of a count and its label. */
const rowWidth = (key: AreaKey, n: number) => count(n).length * 8.5 + 5 + CLASS_LABEL[clsOf(key)].length * 5.8;

/**
 * His tokens against the market's, drawn to one scale (`overlapLayout`): a fixed
 * area per token. Three ellipses: the pool, the passing tokens inside it, and his
 * tokens, overlapping the two as his tokens split. Inside his ellipse, his tokens
 * at a point and his tokens with a failing signal are circles in the part they
 * belong to; the rest of each part is his tokens with no signal. Under the
 * drawing, each part of his ellipse lists its counts as one group with its total,
 * and a line ties the group to the part. An empty area draws nothing. Each count
 * is a button that lists its tokens.
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
  // Whether the groups under the drawing, and the lines to them, are shown.
  const [labels, setLabels] = useState(false);
  const { at, parts } = useMemo(() => {
    const n = (key: AreaKey) => areas[key].length;
    const shown = PARTS.map((p) => ({ ...p, keys: [...p.circles, p.rest].filter((k) => n(k) > 0) })).filter(
      (p) => p.keys.length > 0,
    );
    const total = (region: Region) => shown.find((p) => p.region === region)?.keys.reduce((t, k) => t + n(k), 0) ?? 0;
    const at = overlapLayout(
      {
        pool,
        pass,
        his,
        hisInPass: total('passHis'),
        hisOutside: total('outside'),
        groups: PARTS.flatMap((p) => p.circles).map((key) => ({ key, n: n(key) })),
      },
      WIDTH,
      HEIGHT,
      // The box of the part's text; a part with none still gets a point for its line.
      Object.fromEntries(
        shown.map((p) => [p.region, n(p.rest) > 0 ? { w: rowWidth(p.rest, n(p.rest)) + 12, h: LINE + 2 } : { w: 6, h: 6 }]),
      ),
    );
    return {
      at,
      parts: shown.map((p) => {
        const spot = at.spots[p.region];
        return { ...p, spot, total: total(p.region), inside: n(p.rest) > 0 && spot?.fits === true };
      }),
    };
  }, [areas, pool, pass, his]);

  /** What makes a shape or a label a button for its area. */
  const button = (key: AreaKey) => {
    const on = selected === key;
    const pick = () => onSelect(on ? null : key);
    return {
      role: 'button',
      tabIndex: 0,
      'aria-pressed': on,
      onClick: pick,
      onKeyDown: (e: KeyboardEvent) => {
        if (e.key === 'Enter' || e.key === ' ') pick();
      },
      style: { cursor: 'pointer' },
    };
  };
  /** A count and its label on one line. */
  const row = (key: AreaKey, x: number, y: number, anchor: 'start' | 'middle') => {
    const on = selected === key;
    return (
      <text x={x} y={y} textAnchor={anchor} fontSize={11} textDecoration={on ? 'underline' : undefined}>
        <tspan fontSize={14} fontWeight={700} fill={CLASS_FILL[clsOf(key)]}>
          {count(areas[key].length)}
        </tspan>
        <tspan dx={5} fill={on ? 'var(--color-text)' : 'var(--color-text-mid)'} fontWeight={on ? 700 : 400}>
          {CLASS_LABEL[clsOf(key)]}
        </tspan>
      </text>
    );
  };

  // A circle holds its count when the digits fit, and its label too when that fits.
  const circles = at.bubbles.map((b) => {
    const digits = count(areas[b.key].length).length;
    const label = CLASS_LABEL[clsOf(b.key)];
    const size = 2 * b.r >= digits * 9.5 + 8 ? 16 : 2 * b.r >= digits * 6.5 + 4 ? 11 : 0;
    return { ...b, size, label, full: size === 16 && b.r >= 22 && 2 * b.r >= label.length * 4.4 + 6 };
  });
  // Left to right as the parts lie, so the lines do not cross.
  const groups = (labels ? parts.filter((p) => p.his || !p.inside) : []).sort((a, b) => (a.spot?.x ?? WIDTH) - (b.spot?.x ?? WIDTH));
  // The groups sit under his shape, inside the drawing's width.
  const span = (groups.length - 1) * GROUP;
  const firstGroup = Math.min(Math.max((at.his?.cx ?? WIDTH / 2) - span / 2, GROUP / 2), WIDTH - GROUP / 2 - span);
  const passTop = at.pass && at.pool ? at.pass.cy - at.pass.ry : 0;
  // The passing title sits over its ellipse when the pool leaves room there, else inside it.
  const passTitleY = at.pass && at.pool && passTop - (at.pool.cy - at.pool.ry) >= 24 ? passTop - 6 : passTop + 18;

  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
      <div className="flex max-w-[900px] flex-1 basis-[600px] flex-col items-start gap-1">
      <Button size="xs" variant="ghost" aria-pressed={labels} onClick={() => setLabels(!labels)}>
        {labels ? 'Hide lines and labels' : 'Show lines and labels'}
      </Button>
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT + STRIP}`}
        className="w-full"
        role="group"
        aria-label="His tokens against the market's"
      >
        {at.pool && <ellipse {...at.pool} fill={COLOR.pool} fillOpacity={0.06} stroke={COLOR.pool} strokeOpacity={0.6} />}
        {at.pass && <ellipse {...at.pass} fill={COLOR.pass} fillOpacity={0.08} stroke={COLOR.pass} strokeOpacity={0.6} />}
        {at.his && <ellipse {...at.his} fill={COLOR.his} fillOpacity={0.12} stroke={COLOR.his} strokeOpacity={0.9} />}

        {at.pool && <Title x={at.pool.cx} y={14} color={COLOR.pool} label="In pool" n={pool} />}
        {at.pass && <Title x={at.pass.cx} y={passTitleY} color={COLOR.pass} label="Pass filters" n={pass} />}
        {at.his && <Title x={at.his.cx} y={at.his.cy + at.his.ry + 16} color={COLOR.his} label="His tokens" n={his} />}

        {circles.map((b) => {
          const fill = CLASS_FILL[clsOf(b.key)];
          return (
            <g key={b.key} {...button(b.key)}>
              <title>{areaTip(b.key, areas[b.key].length)}</title>
              <circle
                cx={b.x}
                cy={b.y}
                r={b.r}
                fill={fill}
                fillOpacity={0.14}
                stroke={fill}
                strokeWidth={selected === b.key ? 3 : 1.5}
              />
              {b.size > 0 && (
                <text
                  x={b.x}
                  y={b.y + (b.full ? -1 : b.size * 0.35)}
                  textAnchor="middle"
                  fontSize={b.size}
                  fontWeight={800}
                  fill={fill}
                >
                  {count(areas[b.key].length)}
                </text>
              )}
              {b.full && (
                <text x={b.x} y={b.y + 12} textAnchor="middle" fontSize={9} fontWeight={600} fill={fill}>
                  {b.label}
                </text>
              )}
            </g>
          );
        })}
        {parts.map(
          (p) =>
            p.inside && (
              <g key={p.rest} {...button(p.rest)}>
                <title>{areaTip(p.rest, areas[p.rest].length)}</title>
                {row(p.rest, p.spot!.x, p.spot!.y + 5, 'middle')}
              </g>
            ),
        )}
        {/* With the groups hidden, a part with no room for its text shows the count alone. */}
        {!labels &&
          parts.map(
            (p) =>
              !p.inside &&
              p.spot &&
              areas[p.rest].length > 0 && (
                <g key={p.rest} {...button(p.rest)}>
                  <title>{areaTip(p.rest, areas[p.rest].length)}</title>
                  <text
                    x={p.spot.x}
                    y={p.spot.y + 4}
                    textAnchor="middle"
                    fontSize={13}
                    fontWeight={600}
                    fill="var(--color-text-dim)"
                    textDecoration={selected === p.rest ? 'underline' : undefined}
                  >
                    {count(areas[p.rest].length)}
                  </text>
                </g>
              ),
          )}
        {groups.map((p, i) => {
          const x = firstGroup + i * GROUP;
          return (
            <g key={p.region}>
              {p.spot && (
                <>
                  <line
                    x1={x}
                    y1={HEIGHT + 4}
                    x2={p.spot.x}
                    y2={p.spot.y + (p.inside ? 10 : 0)}
                    stroke={p.color}
                    strokeOpacity={0.7}
                  />
                  {!p.inside && <circle cx={p.spot.x} cy={p.spot.y} r={2.5} fill={p.color} />}
                </>
              )}
              <text x={x} y={HEIGHT + 18} textAnchor="middle" fontSize={11} fontWeight={700} fill={p.color}>
                {p.name} · {count(p.total)}
              </text>
              {p.keys.map((key, j) => (
                <g key={key} {...button(key)}>
                  <title>{areaTip(key, areas[key].length)}</title>
                  {row(key, x, HEIGHT + 40 + j * LINE, 'middle')}
                </g>
              ))}
            </g>
          );
        })}
      </svg>
      </div>
      <div className="flex min-w-[260px] flex-1 flex-col gap-3 text-[11px] text-text-mid">
        <Legend
          title="The three shapes"
          rows={[
            [COLOR.pool, 'In pool', ['Tokens with a buy made with the selected IXs.', 'The buyer can be any wallet.']],
            [
              COLOR.pass,
              'Pass filters',
              ['Tokens in the pool where at least one such buy passes the filters.', 'Always inside the pool.'],
            ],
            [
              COLOR.his,
              'His tokens',
              ['Tokens he bought in the range.', 'It overlaps the other two as his tokens split between them.'],
            ],
          ]}
        />
        <Legend
          title="Inside his tokens: his buy on the token"
          rows={[
            [
              CLASS_FILL.point,
              CLASS_LABEL.point,
              ['A circle.', 'He bought right after a buy made with the selected IXs.', 'The filters pass on that buy.'],
            ],
            [
              CLASS_FILL.signal,
              CLASS_LABEL.signal,
              [
                'A circle.',
                'He bought right after such a buy.',
                'The filters fail on that buy.',
                'Inside Pass filters: another such buy on the token passes, and he did not follow it.',
              ],
            ],
            [
              CLASS_FILL.bought,
              CLASS_LABEL.bought,
              [
                'The rest of his shape, outside the circles.',
                'He bought the token, but not right after such a buy.',
                'Inside Pass filters: the token has a passing buy, and his buy is not a reaction to it.',
              ],
            ],
            [CLASS_FILL.none, CLASS_LABEL.none, ['The rest of In pool and of Pass filters.', 'No buy of his on the token.']],
          ]}
        />
        <div className="flex flex-col gap-0.5 text-text-dim">
          <span>Drawn to scale: every shape, overlap and circle is as large as its token count.</span>
          <span>Under the drawing, each part of his shape lists its counts together, with a line to the part.</span>
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

function Title({ x, y, color, label, n }: { x: number; y: number; color: string; label: string; n: number }) {
  return (
    <text x={x} y={y} textAnchor="middle" fontSize={12} fontWeight={700} fill={color}>
      {label} · {count(n)}
    </text>
  );
}
