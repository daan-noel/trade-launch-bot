/**
 * The help text of the Entry Context summary tables and the Filters section,
 * written once. Both tabs' tables (His entries, Market) and the filter lines
 * render from here, so a row or column means the same wherever it shows.
 *
 * The rule they explain: an **In pool** row uses every filter; an **All** row uses
 * only the **Any IXs** filters, because the **Target IXs** ones check the selected
 * IXs, and a buy outside the pool has none (`EntryLogic.anyIx`).
 *
 * A help is labelled lines (one idea each, the same labels in the same order) and
 * an optional figure. `HelpText.tsx` renders it and styles the fixed terms:
 * `selected IXs`, `pool`, `Target IXs`, `Any IXs`.
 */

export interface HelpLine {
  /** COUNTS, FILTERS, WHY, NOW ... */
  label: string;
  text: string;
}

export interface Help {
  lines: HelpLine[];
  /** A mono, column-aligned figure drawn above the lines. */
  figure?: string;
}

/** A count before and after the filters; `pass` `null` = not read. */
export interface Count {
  all: number;
  pass: number | null;
}

const n = (v: number) => v.toLocaleString();

/** `40%`, or `-` when there is nothing to divide. */
export const passPct = (c: Count): string =>
  c.pass != null && c.all > 0 ? `${Math.round((c.pass / c.all) * 100)}%` : '-';

/** The live numbers of a cell, in words: `300 buys -> 120 pass = 40%`. */
const now = (c: Count, noun: string): string =>
  c.pass == null ? `${n(c.all)} ${noun}` : `${n(c.all)} ${noun} -> ${n(c.pass)} pass = ${passPct(c)}`;

/** Rows of `label  value  pct`, the columns aligned. */
function figure(rows: readonly (readonly [string, string, string?])[]): string {
  const w0 = Math.max(...rows.map((r) => r[0].length));
  const w1 = Math.max(...rows.map((r) => r[1].length));
  return rows.map(([a, b, c]) => `${a.padEnd(w0)}  ${b.padStart(w1)}${c ? `  ${c}` : ''}`).join('\n');
}

/** Hover lines of the three number columns. */
export const COLUMN_HELP = {
  all: 'How many are in this row, before the filters.',
  pass: 'How many of them pass the filters named in "Filters used". "-" = not read.',
  pct: 'pass divided by all. Example: 120 of 300 = 40%.',
} as const;

const TOKEN_RULE = 'A token passes when at least one of its buys passes.';

export const HIS_HELP = {
  table: (pool: Count, all: Count): Help => ({
    figure: figure([
      ['his buys in range', n(all.all)],
      ['  pass Any IXs filters', n(all.pass ?? 0), passPct(all)],
      ['  in pool', n(pool.all)],
      ['    pass all filters', n(pool.pass ?? 0), passPct(pool)],
    ]),
    lines: [
      { label: 'Rows', text: 'In pool = his buys right after the selected IXs. All = every buy of his.' },
      { label: 'Columns', text: 'all = how many. pass = how many pass the filters. % = pass / all.' },
      { label: 'Tokens', text: TOKEN_RULE },
      { label: 'Read it', text: 'A good filter keeps the In pool % high and cuts the Market one.' },
    ],
  }),
  pool: (poolWords: string, buys: Count): Help => ({
    lines: [
      { label: 'Counts', text: `His buys made right after a buy with the selected IXs (${poolWords}).` },
      { label: 'Filters', text: 'All of them: Target IXs + Any IXs.' },
      {
        label: 'Checked',
        text: 'Right after the buy he followed, not at his own buy: what his bot saw, and the same check the market gets.',
      },
      { label: 'Now', text: now(buys, 'buys') },
    ],
  }),
  poolOff: {
    lines: [{ label: 'Counts', text: 'Nothing. The Probe switch is off, so there is no pool.' }],
  } satisfies Help,
  all: (buys: Count): Help => ({
    lines: [
      { label: 'Counts', text: 'Every buy of his in the time range.' },
      { label: 'Filters', text: 'Any IXs only. Target IXs filters are skipped.' },
      { label: 'Checked', text: 'In pool buys right after the buy he followed, the others at his own buy.' },
      { label: 'Why', text: 'Outside the pool there are no selected IXs to check.' },
      { label: 'Now', text: now(buys, 'buys') },
    ],
  }),
} as const;

export const MARKET_HELP = {
  table: (pool: Count, tokens: number): Help => ({
    figure: figure([
      ['tokens traded in range', n(tokens)],
      ['  in pool', n(pool.all)],
      ['    pass all filters', n(pool.pass ?? 0), passPct(pool)],
    ]),
    lines: [
      { label: 'Rows', text: 'In pool = buys made with the selected IXs. All = every token traded.' },
      { label: 'Columns', text: 'all = how many. pass = how many pass the filters. % = pass / all.' },
      { label: 'Tokens', text: TOKEN_RULE },
      { label: 'Read it', text: 'Fewer passing tokens here = the filters fire less often without him.' },
    ],
  }),
  pool: (poolWords: string, buys: Count): Help => ({
    lines: [
      { label: 'Counts', text: `Every buy made with the selected IXs, by any wallet. The selected IXs are ${poolWords}.` },
      { label: 'Filters', text: 'Target IXs + Any IXs, checked right after each buy, as a follower sees it.' },
      { label: 'Left out', text: 'His SOL, Token and Time. A market buy has none of them.' },
      { label: 'Now', text: now(buys, 'buys') },
    ],
  }),
  all: (tokens: number): Help => ({
    lines: [
      { label: 'Counts', text: 'Every token traded in the time range.' },
      { label: 'Filters', text: 'Any IXs only.' },
      { label: 'Why "-"', text: 'Not read yet. The scan reads only buys made with the selected IXs.' },
      { label: 'Now', text: `${n(tokens)} tokens` },
    ],
  }),
  missing: 'Not read yet: the scan reads only buys made with the selected IXs.',
} as const;

/** The Market tab's chance strip: what a chance is, and what Max pause does. */
export const CHANCE_HELP = {
  chances: (a: {
    chances: number;
    passing: number;
    failEnds: boolean;
    ends: { fail: number; pause: number; last: number };
  }): Help => ({
    lines: [
      { label: 'What', text: 'One opportunity on one token: passing buys close together count once.' },
      {
        label: 'Ends at',
        text: a.failEnds
          ? 'A buy made with the selected IXs that fails the filters, or a pause longer than Max pause.'
          : 'A pause longer than Max pause.',
      },
      { label: 'Charts', text: 'Each chance is marked at its first passing buy. His buys stay marked.' },
      { label: 'Now', text: `${n(a.passing)} passing buys -> ${n(a.chances)} chances` },
      {
        label: 'Ended by',
        text: `a failing buy ${n(a.ends.fail)} · a pause ${n(a.ends.pause)} · the token's last passing buy ${n(a.ends.last)}`,
      },
    ],
  }),
  pause: (a: {
    /** `5 s`; `null` when the count never settles. */
    suggested: string | null;
    fallbackSecs: number;
    /** `min 1 · median 4 · max 90 s`; `null` with under two passing buys on a token. */
    pauses: string | null;
    /** Chances counted at each max pause, seconds. */
    counts: readonly (readonly [number, number])[];
  }): Help => ({
    figure: figure([['max pause', 'chances'], ...a.counts.map(([p, c]) => [`${p} s`, n(c)] as const)]),
    lines: [
      { label: 'What', text: 'Two passing buys further apart than this are two chances.' },
      {
        label: 'Suggested',
        text: a.suggested
          ? `${a.suggested}: from there the chance count stops moving.`
          : `None: the count never settles, so ${a.fallbackSecs} s is used.`,
      },
      ...(a.pauses ? [{ label: 'Pauses', text: `Between two passing buys in a row: ${a.pauses}.` }] : []),
      { label: 'Read it', text: 'Where the count stops moving in the table above, the choice does not matter.' },
    ],
  }),
} as const;

/** The two lines of the Filters section. */
export const FILTER_LINE_HELP = {
  ix: {
    lines: [
      { label: 'What', text: 'Filters that check the selected IXs.' },
      { label: 'Columns', text: 'Probe, Signal tx, Target, Earlier, Tx % jump.' },
      { label: 'Used by', text: 'In pool rows only.' },
      { label: 'Example', text: '"Target tx % > 50" = the selected IXs made over half the buys.' },
    ],
  },
  any: {
    lines: [
      { label: 'What', text: 'Filters that work without the selected IXs.' },
      { label: 'Columns', text: 'His SOL, Token, Time, All buys, All buy SOL, Top structure.' },
      { label: 'Used by', text: 'Every row: In pool and All.' },
      { label: 'Note', text: 'His SOL, Token and Time pick which of his buys count: they cut "all".' },
      { label: 'Example', text: '"All buys > 10" = more than 10 buys by anyone.' },
    ],
  },
} as const satisfies Record<string, Help>;
