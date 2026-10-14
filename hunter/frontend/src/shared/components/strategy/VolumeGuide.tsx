// The volume workflow picture. The Tags Guide opens this.

import { useState } from 'react';

import { Button } from 'components/ui/Button';
import { Modal } from 'components/ui/Modal';
import { Card, Chip, Chips, DownJoin, Fall, Lane, Pass } from './GuidePicture';

const SPLIT = ['Mark', 'Grow', 'Check'];
const ROWS = ['Ix structure', 'Ix core'];

const WALLETS: { name: string; marks: [string, string]; lines: string[]; keep: string }[] = [
  { name: 'Same slot', marks: ['1× buy', '2× buy'], lines: ['Same instant', 'Smaller sells ~60 s'], keep: 'After 20 coins' },
  { name: 'Below zero', marks: ['Sell', 'No buy'], lines: ['Bag goes negative'], keep: 'After 2 coins' },
  { name: 'Never sells', marks: ['10 coins', '20 buys'], lines: ['No sell anywhere'], keep: 'Keep' },
  { name: 'Stays in group', marks: ['3 coins', '80% here'], lines: ['Lives on this dev'], keep: 'Keep' },
];

export function VolumeWorkflow() {
  return (
    <div className="flex flex-col gap-4 pb-2">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span className="text-[13px] font-semibold text-text">Volume tag</span>
        <span className="text-[11px] text-text-dim">Built once, then only read</span>
        <span className="ml-auto flex items-center gap-3 text-[11px] text-text-mid">
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm bg-green" />
            Stored
          </span>
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm bg-red" />
            Left off
          </span>
        </span>
      </div>

      <div className="flex flex-col pl-10">
        <Pass
          n={1}
          title="Built once"
          hint={
            <div className="flex flex-wrap items-center gap-1">
              <span className="text-[11px] text-text-dim">The owner split, on the launch group's coins</span>
              <Chips names={SPLIT} />
            </div>
          }
        >
          <div className="grid gap-2 sm:grid-cols-2">
            <Card>
              <strong className="text-[12px] text-text">Rows</strong>
              <Chips names={ROWS} tone="keep" />
              <span className="text-[11px] text-text-dim">90% or more owner SOL on the group, 10 or more trades</span>
            </Card>
            <Card>
              <strong className="text-[12px] text-text">Creator</strong>
              <Chip tone="keep">Flag on</Chip>
              <Chip tone="drop">Address left off</Chip>
              <span className="text-[11px] text-text-dim">Creator is owner here</span>
            </Card>
          </div>
          <div className="flex flex-col gap-2">
            <strong className="text-[12px] text-text">Wallets that come back</strong>
            <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
              {WALLETS.map((rule) => (
                <Card key={rule.name}>
                  <strong className="text-[12px] text-text">{rule.name}</strong>
                  <div className="flex w-full gap-1">
                    {rule.marks.map((mark) => (
                      <span key={mark} className="flex-1 border border-white/25 bg-white/5 px-1 py-1 text-center text-[11px] text-text">
                        {mark}
                      </span>
                    ))}
                  </div>
                  {rule.lines.map((line) => (
                    <span key={line} className="text-[11px] text-text-dim">
                      {line}
                    </span>
                  ))}
                  <Chip tone="keep">{rule.keep}</Chip>
                </Card>
              ))}
            </div>
            <div className="flex flex-wrap gap-1">
              <Chip tone="keep">Recent launches</Chip>
              <Chip>90% return gap</Chip>
              <Chip tone="drop">This-coin wallet off</Chip>
            </div>
          </div>
        </Pass>
        <DownJoin />
        <Pass
          n={2}
          title="While a rule runs"
          hint={
            <div className="flex flex-wrap gap-1">
              <Chip>First yes wins</Chip>
              <Chip>Sticky looks forward</Chip>
              <Chip>Printed trade stays</Chip>
              <Chip tone="drop">Nothing written</Chip>
            </div>
          }
        >
          <div className="flex flex-col gap-2">
            <Lane ask={<Card><strong className="text-[12px] text-text">Stored row, stored wallet, creator, or sticky</strong></Card>} via="Yes">
              <Chip>Owner from here on</Chip>
            </Lane>
            <Fall />
            <Lane ask={<Card><strong className="text-[12px] text-text">No match</strong></Card>} via="Yes">
              <Chip>Outsider</Chip>
            </Lane>
          </div>
        </Pass>
      </div>
    </div>
  );
}

/** Opens the volume workflow. Used on the fingerprint Tags section. */
export function VolumeGuideButton({ className }: { className?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="subtle" size="xs" className={className} onClick={() => setOpen(true)} title="How the volume tag is built and how a rule reads it">
        Guide
      </Button>
      <Modal title="Guide: volume workflow" open={open} onClose={() => setOpen(false)} size="xl">
        <VolumeWorkflow />
      </Modal>
    </>
  );
}
