// A stage tree: phases (when in its life, one way) x situations (what happens now,
// re-picked on every print) = cells. Phases sit left and situations right, each in its
// own color, and the grid below uses the same two colors for its rows and columns. A
// cell says two things: BUY (a buy may start there) and its sell lines. A sell line is
// written for one cell, a whole column (every phase of a situation), a whole row (every
// situation of a phase) or every cell; a cell runs the narrowest it has.

import { useState } from 'react';

import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { CloseIcon, PlusIcon } from 'components/ui/icons';
import { cn } from 'lib/cn';
import { defaultRef } from 'lib/strategy/metricRef';
import { ROLE, wash } from 'lib/strategy/roleColors';
import {
  cellBuys,
  cellEntryFor,
  freshName,
  metricCond,
  newId,
  newLine,
  removeTreeName,
  renameTreeName,
  type CellEntry,
  type CellRef,
  type Cond,
  type Line,
  type RuleDoc,
  type Tree,
} from 'lib/strategy/ruleDoc';
import { cellTitle, lineExitLabel, lineSentence } from 'lib/strategy/sentences';
import type { StrategyRegistry } from 'lib/strategy/registry';
import { nameError } from 'lib/strategy/validate';
import type { CondContext } from './CondRow';
import { CondList, LineList, PartHeader } from './parts';

const findEntry = (t: Tree, p: string | null, s: string | null) => t.cells.find((c) => c.phase === p && c.situation === s);

function cloneLine(l: Line): Line {
  return { ...l, id: newId(), if: l.if.map((c): Cond => ({ ...c, id: newId() })) };
}

/** Where an entry's lines apply. */
type Scope = 'cell' | 'column' | 'row' | 'all';

function scopeOf(e: Pick<CellEntry, 'phase' | 'situation'>): Scope {
  if (e.phase != null && e.situation != null) return 'cell';
  if (e.situation != null) return 'column';
  if (e.phase != null) return 'row';
  return 'all';
}

/** The entry key a scope edits, for cell `p x s`. */
function scopeRef(scope: Scope, p: string, s: string): CellRef {
  if (scope === 'cell') return [p, s];
  if (scope === 'column') return [null, s];
  if (scope === 'row') return [p, null];
  return [null, null];
}

const SCOPE_WORD: Record<Scope, string> = { cell: 'own', column: 'from column', row: 'from row', all: 'from all' };

/** A cell's sells, short: `SELL burst`, `SELL burst +1`. */
function sellFace(entry: CellEntry | undefined): string | null {
  const lines = entry?.on.filter((l) => !l.off && l.sell) ?? [];
  if (!lines.length) return null;
  return `SELL ${lineExitLabel(lines[0])}${lines.length > 1 ? ` +${lines.length - 1}` : ''}`;
}

/** The grid: a row per phase, a column per situation. With `onSelect` it is the editor's;
 *  without, the read-only picture the In words view draws. */
export function TreeGrid({
  doc,
  reg,
  selected,
  onSelect,
  onToggleBuy,
  onAddSell,
}: {
  doc: RuleDoc;
  reg: StrategyRegistry | undefined;
  selected?: CellRef | null;
  onSelect?: (p: string, s: string) => void;
  onToggleBuy?: (p: string, s: string) => void;
  /** Give a cell with no sell line its first one. */
  onAddSell?: (p: string, s: string) => void;
}) {
  const t = doc.tree;
  if (!t) return null;
  const cell = (p: string, s: string) => {
    const entry = cellEntryFor(t, p, s);
    const inherited = entry != null && scopeOf(entry) !== 'cell';
    const face = sellFace(entry);
    const buys = cellBuys(doc.enter.cells, p, s);
    const isSel = selected?.[0] === p && selected?.[1] === s;
    const title = [
      cellTitle(p, s),
      buys ? 'BUY: a buy may start here' : 'no buy starts here',
      ...(entry?.on.filter((l) => !l.off).map((l) => lineSentence(reg, l)) ?? ['holds: no sell line']),
      inherited && entry ? `(the lines of ${cellTitle(entry.phase, entry.situation)})` : '',
    ]
      .filter(Boolean)
      .join('\n');
    return (
      <td key={s} className="p-0.5">
        <div
          role={onSelect ? 'button' : undefined}
          tabIndex={onSelect ? 0 : undefined}
          title={title}
          onClick={onSelect ? () => onSelect(p, s) : undefined}
          onKeyDown={onSelect ? (e) => (e.key === 'Enter' || e.key === ' ') && onSelect(p, s) : undefined}
          className={cn(
            'flex h-14 w-28 flex-col justify-between rounded border border-white/12 px-1.5 py-1 text-[10px]',
            onSelect && 'cursor-pointer hover:border-white/40',
            isSel && 'ring-2 ring-white/80',
          )}
          style={buys ? { backgroundColor: wash(ROLE.buy, 16), borderColor: wash(ROLE.buy, 60) } : undefined}
        >
          <div className="flex items-center gap-1">
            {onToggleBuy ? (
              <button
                type="button"
                className={cn('rounded border px-1 font-bold leading-tight', buys ? 'border-buy bg-buy/20 text-buy' : 'border-white/15 text-text-dim/60 hover:text-buy')}
                title={buys ? 'A buy may start here: click to stop it' : 'No buy starts here: click to allow it'}
                onClick={(e) => {
                  e.stopPropagation();
                  onToggleBuy(p, s);
                }}
              >
                {buys ? 'BUY' : '+ buy'}
              </button>
            ) : (
              buys && <span className="font-bold text-buy">BUY</span>
            )}
            {inherited && entry && face && <span className="ml-auto text-[9px] text-text-dim">{SCOPE_WORD[scopeOf(entry)]}</span>}
          </div>
          {face ? (
            <span className={cn('truncate font-mono', inherited ? 'text-sell/70' : 'font-semibold text-sell')}>
              {face}
            </span>
          ) : onAddSell ? (
            <button
              type="button"
              className="self-start rounded px-0.5 text-text-dim/60 hover:text-sell"
              title="Add a sell line to this cell"
              onClick={(e) => {
                e.stopPropagation();
                onAddSell(p, s);
              }}
            >
              + sell
            </button>
          ) : (
            <span className="text-text-dim/50">hold</span>
          )}
        </div>
      </td>
    );
  };
  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-3 text-[11px] text-text-dim">
        <span className="inline-flex items-center gap-1">
          <span className="rounded border border-buy bg-buy/20 px-1 text-[10px] font-bold text-buy">BUY</span> a buy may start here
        </span>
        <span className="inline-flex items-center gap-1">
          <span className="font-mono font-semibold text-sell">SELL</span> sells while we hold and the coin is here
        </span>
        <span>no sell line = hold (only Always and TP/SL sell)</span>
      </div>
      <div className="overflow-x-auto">
        <table className="border-separate border-spacing-0 text-[11px]">
          <thead>
            <tr>
              <th className="px-1 text-left align-bottom text-[10px] font-normal">
                <span style={{ color: ROLE.stage }}>phase</span>
                <span className="text-text-dim"> \ </span>
                <span style={{ color: ROLE.situation }}>situation</span>
              </th>
              {t.situations.map((s) => (
                <th key={s.id} className="px-1 text-left font-mono font-semibold" style={{ color: ROLE.situation }}>
                  {s.name}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {t.phases.map((p) => (
              <tr key={p.id}>
                <th className="px-1 pr-2 text-left font-mono font-semibold" style={{ color: ROLE.stage }}>
                  {p.name}
                </th>
                {t.situations.map((s) => cell(p.name, s.name))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

/** Phases or situations: one colored column, names in order, each with its conditions.
 *  The last has none (runs to the end / catches the rest), so a new one goes in before it. */
function AxisColumn({
  which,
  doc,
  patch,
  ctx,
}: {
  which: 0 | 1;
  doc: RuleDoc;
  patch: (f: (d: RuleDoc) => RuleDoc) => void;
  ctx: CondContext;
}) {
  const t = doc.tree!;
  const color = which === 0 ? ROLE.stage : ROLE.situation;
  const items = which === 0 ? t.phases.map((p) => ({ id: p.id, name: p.name, conds: p.until })) : t.situations.map((s) => ({ id: s.id, name: s.name, conds: s.when }));
  const word = which === 0 ? 'until' : 'when';
  const setTree = (f: (tree: Tree) => Tree) => patch((d) => (d.tree ? { ...d, tree: f(d.tree) } : d));
  const setConds = (i: number, conds: Cond[]) =>
    setTree((tree) =>
      which === 0
        ? { ...tree, phases: tree.phases.map((p, j) => (j === i ? { ...p, until: conds } : p)) }
        : { ...tree, situations: tree.situations.map((s, j) => (j === i ? { ...s, when: conds } : s)) },
    );
  const move = (i: number, dir: -1 | 1) =>
    setTree((tree) => {
      const swap = <T,>(xs: T[]) => {
        const j = i + dir;
        if (j < 0 || j >= xs.length) return xs;
        const next = [...xs];
        [next[i], next[j]] = [next[j], next[i]];
        return next;
      };
      return which === 0 ? { ...tree, phases: swap(tree.phases) } : { ...tree, situations: swap(tree.situations) };
    });
  const add = () =>
    setTree((tree) => {
      const at = Math.max(0, items.length - 1);
      if (which === 0) {
        const name = freshName('phase', tree.phases.map((p) => p.name));
        const until = [metricCond({ metric: 'm_state.age_sec' })];
        return { ...tree, phases: [...tree.phases.slice(0, at), { id: newId(), name, until }, ...tree.phases.slice(at)] };
      }
      const name = freshName('situation', tree.situations.map((s) => s.name));
      const first = ctx.reg.families.flatMap((f) => f.metrics).find((m) => m.tags !== 'required');
      const when = [metricCond(first ? defaultRef(first) : { metric: '' })];
      return { ...tree, situations: [...tree.situations.slice(0, at), { id: newId(), name, when }, ...tree.situations.slice(at)] };
    });
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-lg border px-2.5 py-2" style={{ borderColor: color, backgroundColor: wash(color, 4) }}>
      <PartHeader ctx={ctx} part={which === 0 ? 'phases' : 'situations'} title={which === 0 ? 'PHASES' : 'SITUATIONS'}>
        <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={add}>
          <PlusIcon className="size-3" /> {which === 0 ? 'phase' : 'situation'}
        </Button>
      </PartHeader>
      <p className="-mt-1 text-[11px] text-text-dim">
        {which === 0
          ? 'When in its life. The coin moves down this list and never back: a phase ends when its until holds.'
          : 'What happens now, re-checked on every print and tick. Top to bottom: the first whose when holds is the situation.'}
      </p>
      {items.map((it, i) => {
        const last = i === items.length - 1;
        const nErr = nameError(it.name, 'Name');
        return (
          <div key={it.id} className="flex flex-col gap-1 rounded-md border border-white/10 bg-bg-card/60 px-2 py-1.5">
            <div className="flex flex-wrap items-center gap-2">
              <span className="w-4 text-right text-[12px] font-semibold tabular-nums text-text-dim">{i + 1}</span>
              <Input
                fieldSize="sm"
                className="w-32 font-mono font-semibold"
                style={{ color }}
                value={it.name}
                disabled={ctx.disabled}
                onChange={(e) => patch((d) => renameTreeName(d, which, it.name, e.target.value))}
              />
              {last && <span className="text-[11px] text-text-dim">{which === 0 ? 'last: runs to the end' : 'last: everything else'}</span>}
              <div className="ml-auto flex items-center gap-0.5">
                {items.length > 1 && (
                  <>
                    <IconButton variant="ghost" size="sm" disabled={ctx.disabled} onClick={() => move(i, -1)} title="Move up">
                      ↑
                    </IconButton>
                    <IconButton variant="ghost" size="sm" disabled={ctx.disabled} onClick={() => move(i, 1)} title="Move down">
                      ↓
                    </IconButton>
                  </>
                )}
                <IconButton
                  variant="ghost"
                  size="sm"
                  disabled={ctx.disabled || items.length === 1}
                  onClick={() => patch((d) => removeTreeName(d, which, it.name))}
                  title="Remove, with every sell line and buy cell that names it"
                  aria-label="Remove"
                >
                  <CloseIcon />
                </IconButton>
              </div>
            </div>
            {nErr && <p className="text-[11px] text-red">{nErr}</p>}
            {(!last || it.conds.length > 0) && (
              <CondList
                conds={it.conds}
                onChange={(cs) => setConds(i, cs)}
                ctx={ctx}
                metricOnly={which === 0}
                word={(j) => (j === 0 ? word : 'and')}
                empty="Add a condition."
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

/** The selected cell: may a buy start here, and the sell lines it runs, written for this
 *  cell, its column, its row or every cell. */
function CellPane({
  doc,
  patch,
  ctx,
  phase,
  situation,
  onToggleBuy,
}: {
  doc: RuleDoc;
  patch: (f: (d: RuleDoc) => RuleDoc) => void;
  ctx: CondContext;
  phase: string;
  situation: string;
  onToggleBuy: (p: string, s: string) => void;
}) {
  const t = doc.tree!;
  const running = cellEntryFor(t, phase, situation);
  const [scope, setScope] = useState<Scope>(running ? scopeOf(running) : 'cell');
  const [sp, ss] = scopeRef(scope, phase, situation);
  const entry = findEntry(t, sp, ss);
  const setCells = (f: (cells: CellEntry[]) => CellEntry[]) => patch((d) => (d.tree ? { ...d, tree: { ...d.tree, cells: f(d.tree.cells) } } : d));
  const create = (on: Line[]) => setCells((cells) => [...cells, { id: newId(), phase: sp, situation: ss, on }]);
  const buys = cellBuys(doc.enter.cells, phase, situation);
  const scopes: { key: Scope; label: string }[] = [
    { key: 'cell', label: 'this cell' },
    { key: 'column', label: `column: ${situation}, every phase` },
    { key: 'row', label: `row: ${phase}, every situation` },
    { key: 'all', label: 'every cell' },
  ];
  // A narrower entry replaces this scope's lines in this cell.
  const shadowed = running != null && entry != null && running !== entry;
  return (
    <div className="flex flex-col gap-2 rounded-md border border-white/25 px-3 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-mono text-[12px] font-semibold">
          <span style={{ color: ROLE.stage }}>{phase}</span>
          <span className="text-text-dim"> x </span>
          <span style={{ color: ROLE.situation }}>{situation}</span>
        </span>
        <label className="flex items-center gap-1 text-[11px] text-text-dim">
          <input type="checkbox" className="accent-buy" checked={buys} disabled={ctx.disabled} onChange={() => onToggleBuy(phase, situation)} />
          <span className={cn(buys && 'font-semibold text-buy')}>a buy may start here</span>
        </label>
      </div>
      <div className="flex flex-wrap items-center gap-1 text-[11px]">
        <span className="text-text-dim">sell lines for</span>
        {scopes.map((o) => {
          const has = findEntry(t, ...scopeRef(o.key, phase, situation)) != null;
          return (
            <button
              key={o.key}
              type="button"
              onClick={() => setScope(o.key)}
              className={cn('rounded border px-1.5 py-0.5', scope === o.key ? 'border-white/60 bg-white/10 text-text' : 'border-white/15 text-text-dim hover:text-text')}
            >
              {o.label}
              {has && <span className="text-sell"> ●</span>}
            </button>
          );
        })}
      </div>
      <p className="text-[11px] text-text-dim">
        {scope === 'cell'
          ? 'Only this cell. They replace any column, row or every-cell lines here.'
          : scope === 'all'
            ? 'Every cell that has no lines of its own, of its column or of its row.'
            : `Every cell of this ${scope} that has no lines of its own.`}
        {running && running !== entry && ` Now this cell runs the lines of ${cellTitle(running.phase, running.situation)}.`}
      </p>
      {shadowed && (
        <p className="text-[11px] text-warning">
          This cell does not run these lines: {cellTitle(running.phase, running.situation)} replaces them here.
        </p>
      )}
      {entry ? (
        <>
          <LineList lines={entry.on} onChange={(on) => setCells((cells) => cells.map((c) => (c === entry ? { ...c, on } : c)))} ctx={ctx} stages={[]} cell addLabel="sell line" />
          <div>
            <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => setCells((cells) => cells.filter((c) => c !== entry))}>
              remove these lines
            </Button>
          </div>
        </>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => create([newLine()])}>
            <PlusIcon className="size-3" /> sell line
          </Button>
          {scope === 'cell' && running && running.on.length > 0 && (
            <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => create(running.on.map(cloneLine))}>
              <PlusIcon className="size-3" /> start from a copy of {cellTitle(running.phase, running.situation)}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

export function TreeEditor({
  doc,
  patch,
  ctx,
  onRemove,
}: {
  doc: RuleDoc;
  patch: (f: (d: RuleDoc) => RuleDoc) => void;
  /** Held-side context (position reads allowed). */
  ctx: CondContext;
  onRemove: () => void;
}) {
  const [sel, setSel] = useState<[string, string] | null>(null);
  const t = doc.tree;
  if (!t) return null;
  const phaseNames = t.phases.map((p) => p.name);
  const situationNames = t.situations.map((s) => s.name);
  const current = sel && phaseNames.includes(sel[0]) && situationNames.includes(sel[1]) ? sel : null;
  const all: [string, string][] = phaseNames.flatMap((p) => situationNames.map((s): [string, string] => [p, s]));
  const toggleBuy = (p: string, s: string) =>
    patch((d) => {
      const next = all.filter(([a, b]) => cellBuys(d.enter.cells, a, b) !== (a === p && b === s));
      // No buy cell at all would read back as "every cell" (an empty list): keep one.
      if (next.length === 0) return d;
      const cells: CellRef[] = next.length === all.length ? [] : next;
      return { ...d, enter: { ...d.enter, cells } };
    });
  const addSell = (p: string, s: string) => {
    patch((d) => (d.tree ? { ...d, tree: { ...d.tree, cells: [...d.tree.cells, { id: newId(), phase: p, situation: s, on: [newLine()] }] } } : d));
    setSel([p, s]);
  };
  const beforeBuy: CondContext = { ...ctx, beforeBuy: true };
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2 border-b border-white/12 pb-2">
        <span className="text-[12px] font-semibold tracking-wide" style={{ color: ROLE.stage }}>
          Stage tree
        </span>
        <span className="text-[11px] text-text-dim">
          On every print: find the <span style={{ color: ROLE.stage }}>phase</span>, then the <span style={{ color: ROLE.situation }}>situation</span>; that cell says if a buy may start and which lines sell (after Always).
        </span>
        <Button variant="subtle" size="xs" className="ml-auto" disabled={ctx.disabled} onClick={onRemove}>
          remove tree
        </Button>
      </div>
      <div className="grid items-start gap-3 lg:grid-cols-2">
        <AxisColumn which={0} doc={doc} patch={patch} ctx={beforeBuy} />
        <AxisColumn which={1} doc={doc} patch={patch} ctx={ctx} />
      </div>
      <div className="flex flex-col gap-2">
        <PartHeader ctx={ctx} part="cells" title="CELLS">
          {doc.enter.cells.length > 0 && (
            <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => patch((d) => ({ ...d, enter: { ...d.enter, cells: [] } }))} title="Let a buy start in every cell">
              buy in every cell
            </Button>
          )}
        </PartHeader>
        <TreeGrid
          doc={doc}
          reg={ctx.reg}
          selected={current}
          onSelect={(p, s) => setSel([p, s])}
          onToggleBuy={ctx.disabled ? undefined : toggleBuy}
          onAddSell={ctx.disabled ? undefined : addSell}
        />
        {current ? (
          <CellPane key={`${current[0]}|${current[1]}`} doc={doc} patch={patch} ctx={ctx} phase={current[0]} situation={current[1]} onToggleBuy={toggleBuy} />
        ) : (
          <p className="text-[11px] italic text-text-dim/70">Click a cell to edit its sell lines.</p>
        )}
      </div>
    </div>
  );
}
