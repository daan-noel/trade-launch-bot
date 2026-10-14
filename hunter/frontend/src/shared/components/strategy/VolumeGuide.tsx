// The volume workflow picture. The Tags Guide opens this.

import { useState } from 'react';

import { Button } from 'components/ui/Button';
import { RefreshIcon } from 'components/ui/icons';
import { Modal } from 'components/ui/Modal';
import { Card, Chip, Chips, DownJoin, Fall, Lane, Pass } from './GuidePicture';

const LISTS = ['Private list', 'Many wallets', 'Dominates', 'Mostly here'];
const GATED = ['Kept template', 'On a timer', 'Fee payer'];
const OUTSIDER = ['Sniper', 'Rise stops', 'Sold into', 'Timer', 'Wide service', 'Service payer', 'One wallet', 'Used once'];
const THIS_COIN = [
  'Same transaction',
  'Instant buy',
  'Creation slot',
  'Creation-slot bundle',
  'Bought twice',
  'Several lists',
  'Several sells',
  'Rise continues',
  'Below zero',
  'Same amount',
  'Matching holder',
  'Fresh cash-out',
  'Beside an owner',
  'After silence',
  'First trade',
  'Same slot, under 20',
];

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
        <span className="text-[13px] font-semibold text-text">Volume tag on one coin</span>
        <span className="text-[11px] text-text-dim">Four passes, then reload</span>
        <span className="ml-auto flex items-center gap-3 text-[11px] text-text-mid">
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm bg-green" />
            Written next
          </span>
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm bg-red" />
            Left off
          </span>
        </span>
      </div>

      <div className="flex flex-col pl-10">
        <Pass n={1} title="Before this coin" hint={<span className="text-[11px] text-text-dim">Already loaded</span>}>
          <div className="grid gap-2 sm:grid-cols-2">
            <Card>
              <strong className="text-[12px] text-text">Lists</strong>
              <Chips names={LISTS} tone="keep" />
              <span className="text-[11px] text-text-dim">After the gate</span>
              <Chips names={GATED} tone="keep" />
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
              <Chip>List replaced</Chip>
              <Chip tone="drop">Older address off</Chip>
            </div>
          </div>
        </Pass>
        <DownJoin />
        <Pass
          n={2}
          title="On live"
          hint={
            <div className="flex flex-wrap gap-1">
              <Chip>First yes wins</Chip>
              <Chip>Sticky looks forward</Chip>
              <Chip>Printed trade stays</Chip>
            </div>
          }
        >
          <div className="flex flex-col gap-2">
            <Lane ask={<Card><strong className="text-[12px] text-text">Creator, saved wallet, or sticky</strong></Card>} via="Yes">
              <Chip>Owner from here on</Chip>
              <Chip tone="drop">Other lists left off</Chip>
            </Lane>
            <Fall />
            <Lane ask={<Card><strong className="text-[12px] text-text">Saved instruction list</strong></Card>} via="Yes">
              <Chip>Owner, sticky starts</Chip>
              <Chip tone="drop">Wallet left off</Chip>
            </Lane>
            <Fall />
            <Lane ask={<Card><strong className="text-[12px] text-text">Label the tape</strong></Card>} via="Next">
              <Chip>Pass 3</Chip>
            </Lane>
          </div>
        </Pass>
        <DownJoin />
        <Pass
          n={3}
          title="Label the tape"
          hint={
            <div className="flex flex-wrap gap-1">
              <Chip>Volume tag only</Chip>
              <Chip>Dump stays out</Chip>
            </div>
          }
        >
          <div className="flex flex-col gap-2">
            <Lane
              ask={
                <div className="flex flex-col gap-1.5">
                  <strong className="text-[12px] text-text">Outsider</strong>
                  <Chips names={OUTSIDER} />
                </div>
              }
              via="Yes"
            >
              <Chip>Outsider</Chip>
              <Chip tone="drop">Save nothing</Chip>
            </Lane>
            <Fall />
            <Lane
              ask={
                <div className="flex flex-col gap-1.5">
                  <strong className="text-[12px] text-text">Owner on this coin</strong>
                  <Chips names={THIS_COIN} />
                </div>
              }
              via="Yes"
            >
              <Chip>Owner here</Chip>
              <Chip tone="drop">Wallet left off</Chip>
              <Chip tone="drop">Plain list left off</Chip>
            </Lane>
            <Fall />
            <Lane
              ask={
                <div className="flex flex-col gap-1.5">
                  <strong className="text-[12px] text-text">A list</strong>
                  <Chips names={LISTS} />
                </div>
              }
              via="Yes"
            >
              <Chip>Those trades owner</Chip>
              <Chip tone="keep">List queued</Chip>
              <Chip tone="drop">Wallet left off</Chip>
            </Lane>
            <Fall />
            <Lane ask={<Card><strong className="text-[12px] text-text">No match</strong></Card>} via="Yes">
              <Chip>Outsider</Chip>
            </Lane>
          </div>
        </Pass>
        <DownJoin />
        <Pass n={4} title="Write the next fingerprint" hint={<span className="text-[11px] text-text-dim">Off the print, then reload</span>}>
          <div className="grid gap-2 sm:grid-cols-3">
            <Card>
              <strong className="text-[12px] text-text">On the print</strong>
              <Chip tone="keep">Append the list</Chip>
              <span className="text-[11px] text-text-dim">A queued list is added</span>
            </Card>
            <Card>
              <strong className="text-[12px] text-text">Off the loop</strong>
              <Chip tone="keep">Replace wallet list</Chip>
              <Chip tone="drop">This-coin wallet off</Chip>
              <span className="text-[11px] text-text-dim">Still passing, recent only</span>
            </Card>
            <Card>
              <strong className="text-[12px] text-text">Gate</strong>
              <Chip tone="keep">Cashed out, save</Chip>
              <Chip tone="drop">Held the bag, drop</Chip>
              <Chip>Under 5 coins, wait</Chip>
            </Card>
          </div>
          <div className="flex items-center gap-2 text-[12px] text-text">
            <RefreshIcon className="size-3.5" />
            Reload into pass 1
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
      <Button variant="subtle" size="xs" className={className} onClick={() => setOpen(true)} title="How the volume tag judges a trade and what it stores">
        Guide
      </Button>
      <Modal title="Guide: volume workflow" open={open} onClose={() => setOpen(false)} size="xl">
        <VolumeWorkflow />
      </Modal>
    </>
  );
}
