import { cn } from 'lib/cn';
import { HelpTip, Term, Terms } from './HelpText';
import { COLUMN_HELP, type Count, type Help, passPct } from './summaryHelp';

/** One count before and after the filters. */
export interface PassCell extends Count {
  /** Hover text on the numbers, in words: `120 of his 300 buys in the pool pass`. */
  tip?: string;
}

export interface PassRow {
  key: string;
  label: string;
  /** A second line under the label: what the row is, in plain words. */
  sub?: string;
  /** What the row counts and which filters it uses: the label's info popover. */
  help?: Help;
  /** Which filters the row uses: every one, or the Any IXs ones only. */
  uses: 'every' | 'any';
  /** Under the filters used: how many are set, how many skipped. */
  usesNote?: string;
  /** One cell per group, in group order; `null` = not counted for this row. */
  cells: (PassCell | null)[];
  /** Hover text on a `null` cell: why it is not counted. */
  missingTip?: string;
  /** Marks the row's first "pass" cell: the number the page is about. */
  highlight?: boolean;
}

const n = (v: number) => v.toLocaleString();

/**
 * A summary table of counts before and after the filters. Per row: what it is, the
 * filters it uses, then per group (Buys, Tokens) three columns, all / pass / %.
 * `help` is the table's "How to read" popover; all text comes from `summaryHelp.ts`.
 */
export function PassTable({
  groups,
  rows,
  help,
}: {
  /** What is counted: Buys, Tokens. */
  groups: readonly string[];
  rows: readonly PassRow[];
  help: Help;
}) {
  const head = 'px-3 text-[10px] font-bold uppercase tracking-wider text-text-dim';
  return (
    <div className="overflow-x-auto rounded-md border border-white/8">
      <table className="w-full text-xs">
        <thead className="bg-white/3">
          <tr>
            <th rowSpan={2} className="px-3 text-left text-[11px] font-normal text-text-dim">
              How to read
              <HelpTip title="How to read this table" help={help} className="ml-1" />
            </th>
            <th rowSpan={2} className={cn(head, 'border-l border-white/8 text-left')}>
              Filters used
            </th>
            {groups.map((g) => (
              <th
                key={g}
                colSpan={3}
                className="border-l border-white/8 px-3 pt-1.5 text-center text-[11px] font-bold text-text"
              >
                {g}
              </th>
            ))}
          </tr>
          <tr>
            {groups.map((g) => (
              <SubHeads key={g} className={cn(head, 'text-right')} />
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.key} className="border-t border-white/8">
              <td className="px-3 py-1.5">
                <div className="font-semibold text-text">
                  {r.label}
                  {r.help && <HelpTip title={r.label} help={r.help} className="ml-1" />}
                </div>
                {r.sub && (
                  <div className="text-[11px] text-text-dim">
                    <Terms text={r.sub} />
                  </div>
                )}
              </td>
              <td className="border-l border-white/8 px-3 py-1.5 text-[11px]">
                <div className="whitespace-nowrap">
                  {r.uses === 'every' && (
                    <>
                      <Term>Target IXs</Term>
                      <span className="text-text-dim"> + </span>
                    </>
                  )}
                  <Term>Any IXs</Term>
                  {r.uses === 'any' && <span className="text-text-dim"> only</span>}
                </div>
                {r.usesNote && <div className="text-text-dim">{r.usesNote}</div>}
              </td>
              {r.cells.map((c, i) => (
                <Cells key={i} c={c} strong={!!r.highlight && i === 0} missingTip={r.missingTip} />
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function SubHeads({ className }: { className: string }) {
  return (
    <>
      <th title={COLUMN_HELP.all} className={cn(className, 'border-l border-white/8 pb-1.5')}>
        all
      </th>
      <th title={COLUMN_HELP.pass} className={cn(className, 'pb-1.5')}>
        pass
      </th>
      <th title={COLUMN_HELP.pct} className={cn(className, 'pb-1.5')}>
        %
      </th>
    </>
  );
}

function Cells({ c, strong, missingTip }: { c: PassCell | null; strong: boolean; missingTip?: string }) {
  const num = 'px-3 py-1.5 text-right font-mono tabular-nums';
  if (!c) {
    return (
      <td colSpan={3} title={missingTip} className={cn(num, 'border-l border-white/8 text-center text-text-dim')}>
        -
      </td>
    );
  }
  const tip = c.tip ?? (c.pass == null ? missingTip : undefined);
  return (
    <>
      <td title={tip} className={cn(num, 'border-l border-white/8 text-text-dim')}>{n(c.all)}</td>
      <td title={tip} className={cn(num, strong ? 'bg-primary/10 font-bold text-primary' : 'text-text')}>
        {c.pass == null ? '-' : n(c.pass)}
      </td>
      <td title={tip} className={cn(num, 'text-text-dim')}>{passPct(c)}</td>
    </>
  );
}
