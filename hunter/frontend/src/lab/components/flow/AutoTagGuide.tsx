// The auto-tag workflow picture: how the panel turns the rules you switch on into the
// rows and wallets Apply saves. Every number comes from the run's params, every row
// picture from the list's own `RowPins`, every mark from the matcher's tables.

import { useState } from 'react';

import { Button } from 'components/ui/Button';
import { Modal } from 'components/ui/Modal';
import { Card, Chip, Chips, DownJoin, Fall, Lane, Pass } from 'components/strategy/GuidePicture';
import { cn } from 'lib/cn';
import {
  CORE_MARK_COUNT_HELP,
  CORE_MARK_FLAGS,
  CORE_MARK_HELP,
  coreLabels,
  coreMarks,
  coreMarksText,
  isCoreExtra,
} from 'lib/strategy/ixCore';
import { FEE_FIELD_MARK } from 'lib/strategy/ixPatternRows';
import type { OwnerSplitParams } from 'types';
import { AUTO_TAG_FAMILIES, EXACT_LEVEL, MATCH_LEVELS, levelRow, type LevelTrade } from './autoTagRules';
import { FEE_PINS, RowPins } from './AutoTagRowPins';

/** One buy as a sender builds it: the trade every picture below reads. */
const EXAMPLE: LevelTrade = {
  labels: [
    'Compute Budget: SetComputeUnitLimit',
    'Compute Budget: SetComputeUnitPrice',
    'Associated Token: CreateIdempotent',
    'Pump.Fun: BuyExactSolIn',
    'System Program: Transfer',
  ],
  side: 'buy',
  cu_limit: 200_000,
  cu_price: 20_000,
  tip_lamports: 5_000,
};

/** The row the "read a row" key decodes. */
const READ_LEVEL = 5;

function Mono({ children }: { children: string }) {
  return <span className="font-mono text-[11px] text-text">{children}</span>;
}

export function AutoTagWorkflow({ params }: { params: OwnerSplitParams | null }) {
  const core = coreLabels(EXAMPLE.labels);
  const marks = coreMarksText(coreMarks(EXAMPLE.labels));
  const readRow = levelRow(READ_LEVEL, EXAMPLE);
  const ownerPct = params ? `${params.row_min_owner_pct}%` : 'the run’s share';
  const minTrades = params ? `${params.row_min_trades}` : 'the run’s count';

  return (
    <div className="flex flex-col gap-4 pb-2">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span className="text-[13px] font-semibold text-text">Auto-tag</span>
        <span className="text-[11px] text-text-dim">Rules you switch on → rows and wallets Apply saves</span>
        <span className="ml-auto flex items-center gap-3 text-[11px] text-text-mid">
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm bg-green" />
            Saved
          </span>
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm bg-red" />
            Left off
          </span>
        </span>
      </div>

      <div className="flex flex-col pl-10">
        <Pass n={1} title="Mark owners" hint={<span className="text-[11px] text-text-dim">The rules you switch on</span>}>
          <div className="grid gap-2 sm:grid-cols-2">
            {AUTO_TAG_FAMILIES.map((f) => (
              <Card key={f.id}>
                <strong className="text-[12px] text-text">{f.label}</strong>
                <Chips names={f.rules.map((r) => r.label)} />
              </Card>
            ))}
          </div>
          <div className="flex flex-wrap gap-1">
            <Chip tone="keep">Their wallets’ trades = owner</Chip>
            <Chip>Create tx wallets: always owner on their coin</Chip>
          </div>
        </Pass>
        <DownJoin />

        <Pass n={2} title="Read each trade" hint={<span className="text-[11px] text-text-dim">One trade, three ways</span>}>
          <div className="grid gap-2 lg:grid-cols-3">
            <Card>
              <strong className="text-[12px] text-text">Ix structure</strong>
              <span className="text-[11px] text-text-dim">Every instruction, in order</span>
              {EXAMPLE.labels.map((l) => (
                <span key={l} className={cn('font-mono text-[10px]', isCoreExtra(l) ? 'text-text-dim' : 'text-accent')}>
                  {l}
                </span>
              ))}
            </Card>
            <Card>
              <strong className="text-[12px] text-text">Ix core</strong>
              <span className="text-[11px] text-text-dim">Extras removed; Buy, BuyV2, BuyExactSolIn = BUY</span>
              {core.map((l) => (
                <Mono key={l}>{l}</Mono>
              ))}
            </Card>
            <Card>
              <strong className="text-[12px] text-text">Marks</strong>
              <span className="text-[11px] text-text-dim">Which extras the core lost</span>
              <Mono>{marks}</Mono>
            </Card>
          </div>
          <Card>
            <strong className="text-[12px] text-text">Marks key</strong>
            <div className="grid w-full gap-x-4 gap-y-0.5 text-[11px] text-text-mid sm:grid-cols-2">
              {CORE_MARK_FLAGS.map(([name]) => (
                <span key={name}>
                  <b className="font-mono text-text">{name}</b> {CORE_MARK_HELP[name]}
                </span>
              ))}
              {(['T', 'A'] as const).map((k) => (
                <span key={k}>
                  <b className="font-mono text-text">{k}&lt;n&gt;</b> {CORE_MARK_COUNT_HELP[k]}
                </span>
              ))}
            </div>
            <div className="flex flex-wrap items-center gap-1">
              <span className="text-[11px] text-text-dim">A fee exists only with its mark:</span>
              {FEE_PINS.map(({ field, label }) => (
                <Chip key={field}>
                  {label} needs {FEE_FIELD_MARK[field]}
                </Chip>
              ))}
            </div>
          </Card>
        </Pass>
        <DownJoin />

        <Pass
          n={3}
          title={`${EXACT_LEVEL} keys per trade`}
          hint={<span className="text-[11px] text-text-dim">Loosest first. A key needing a fee the trade lacks is skipped.</span>}
        >
          <div className="flex flex-col gap-1">
            {MATCH_LEVELS.map((lv, i) => {
              const row = levelRow(i + 1, EXAMPLE);
              return (
                <div key={lv.help} className="grid grid-cols-[minmax(0,16rem)_1fr] items-center gap-2">
                  <span className="text-[11px] text-text-dim">{lv.help}</span>
                  {row ? <RowPins level={i + 1} row={row} /> : <Chip tone="drop">skipped</Chip>}
                </div>
              );
            })}
          </div>
        </Pass>
        <DownJoin />

        <Pass n={4} title="Test each key" hint={<span className="text-[11px] text-text-dim">Across every coin of the group</span>}>
          <div className="flex flex-col gap-2">
            <Lane
              ask={
                <Card>
                  <strong className="text-[12px] text-text">
                    Owner SOL ≥ {ownerPct} and ≥ {minTrades} trades
                  </strong>
                </Card>
              }
              via="Yes"
            >
              <Chip tone="keep">Candidate</Chip>
            </Lane>
            <Fall />
            <Lane ask={<Card><strong className="text-[12px] text-text">Fails</strong></Card>} via="Then">
              <Chip tone="drop">Left off</Chip>
              <Chip>Failed level 1 keys listed greyed, with the reason</Chip>
            </Lane>
          </div>
        </Pass>
        <DownJoin />

        <Pass n={5} title="Pick the fewest" hint={<span className="text-[11px] text-text-dim">Biggest owner SOL first</span>}>
          <div className="flex flex-wrap gap-1">
            <Chip tone="keep">Kept: catches owner trades no kept row catches</Chip>
            <Chip tone="drop">Skipped: every owner trade it catches is already caught</Chip>
          </div>
          <Card>
            <strong className="text-[12px] text-text">Wallets</strong>
            <span className="text-[11px] text-text-dim">
              Only wallets that come back: a sell with no buy or a transfer pair on {params ? `${params.comes_back_min_coins}+` : 'enough'} coins,
              or a never-seller, whose latest coin is inside the recent window
            </span>
          </Card>
        </Pass>
        <DownJoin />

        <Pass n={6} title="You decide" hint={<span className="text-[11px] text-text-dim">Nothing is saved until Apply</span>}>
          <div className="flex flex-wrap gap-1">
            <Chip>☑ tick / untick</Chip>
            <Chip>Apply</Chip>
            <Chip tone="keep">Saved on the tag</Chip>
          </div>
        </Pass>
      </div>

      <section className="flex flex-col gap-2 border-t border-white/8 pt-3" aria-label="Read a row">
        <strong className="text-[13px] text-text">Read a row</strong>
        {readRow && (
          <div className="flex flex-col gap-1 rounded border border-white/8 bg-white/3 px-2 py-1">
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-text-mid">{readRow.labels.join(' › ')}</span>
              <span className="font-mono text-[10px] text-text-dim">
                <b className="text-text">100%</b> owner · 83 tr · 83 coins
              </span>
            </div>
            <RowPins level={READ_LEVEL} row={readRow} />
          </div>
        )}
        <div className="grid gap-x-4 gap-y-0.5 text-[11px] text-text-mid sm:grid-cols-2">
          <span>
            <b className="text-text">CORE / EXACT L1-{EXACT_LEVEL}</b> the row’s match level
          </span>
          <span>
            <b className="text-text">BUY / SELL</b> the side it matches
          </span>
          <span>
            <b className="text-text">MARKS</b> the extras, pinned as a set
          </span>
          <span>
            <b className="text-text">CU LIMIT / CU PRICE / TIP</b> fee pins, exact values
          </span>
          <span>
            <b className="text-text">owner</b> share of the row’s SOL that is owner
          </span>
          <span>
            <b className="text-text">tr · coins</b> trades and coins it matches in the group
          </span>
        </div>
        <span className="text-[11px] text-text-dim">Hover any chip on the list for its meaning.</span>
      </section>

      <section className="flex flex-col gap-1 border-t border-white/8 pt-3" aria-label="Highlight">
        <strong className="text-[13px] text-text">Highlight ◈</strong>
        <span className="text-[11px] text-text-mid">
          Highlight a row and the other rows light up when one real trade could match both: same core, and no clash
          in side, marks or fees. A fee pin also needs its mark on the other row, so a row with no CU price
          instruction never lights under a CU price pin.
        </span>
      </section>
    </div>
  );
}

/** Opens the auto-tag workflow. */
export function AutoTagGuideButton({ params }: { params: OwnerSplitParams | null }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="subtle" size="xs" onClick={() => setOpen(true)} title="How auto-tag turns the rules into the rows and wallets Apply saves">
        Guide
      </Button>
      <Modal title="Guide: auto-tag workflow" open={open} onClose={() => setOpen(false)} size="xl">
        <AutoTagWorkflow params={params} />
      </Modal>
    </>
  );
}
