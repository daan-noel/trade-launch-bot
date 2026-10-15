import { useMemo, useState } from 'react';

import { InfoTooltip } from 'components/ui/InfoTooltip';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import { cn } from 'lib/cn';
import { tagLabel } from 'lib/flow/tapeClassify';
import type { OwnerMarks, OwnerSplitParams } from 'types';
import {
  AUTO_TAG_FAMILIES,
  autoTagState,
  toggleAutoTag,
  type AutoTagFamilyId,
  type AutoTagRule,
} from './autoTagRules';

type Scope = 'token' | 'group';

const PANEL_HELP =
  'Auto-tag fills the owner-split list from the owner-split rules (_!___owner-split.md). ' +
  'Pick a family tab, pick whose trades the rules read (the previewed token, or every coin of the group), ' +
  'then click a rule to put the wallets it marks into the wallet list, and click it again to take them out. ' +
  'Nothing is saved until Apply.';

const SCOPE_HELP: Record<Scope, string> = {
  token: 'Read the rules on the token picked in the preview only.',
  group: 'Read the rules on every coin of this group.',
};

const STATE_CLASS = {
  on: 'border-warning bg-warning/20 text-warning',
  some: 'border-dashed border-warning/60 text-warning',
  off: 'border-white/15 text-text-mid hover:border-warning/50 hover:text-text',
} as const;

/**
 * The auto-tag buttons of the owner-split tag: one tab per rule family, one toggle
 * per rule. A toggle writes the draft's wallet list only; Apply saves it.
 */
export function AutoTagPanel({
  tagName,
  params,
  groupMarks,
  tokenMarks,
  tokenMint,
  wallets,
  onWalletsChange,
}: {
  tagName: string;
  /** The run's thresholds; `null` on a run cached before the rules existed. */
  params: OwnerSplitParams | null;
  groupMarks: OwnerMarks | null;
  /** The previewed token's marks; `null` when none is picked or the run lacks them. */
  tokenMarks: OwnerMarks | null;
  tokenMint: string | null;
  wallets: string[];
  onWalletsChange: (wallets: string[]) => void;
}) {
  const [family, setFamily] = useState<AutoTagFamilyId>('creation_slot');
  const [scopePick, setScopePick] = useState<Scope | null>(null);
  const scope: Scope = scopePick ?? (tokenMint ? 'token' : 'group');
  const marks = scope === 'token' ? tokenMarks : groupMarks;
  const listed = useMemo(() => new Set(wallets), [wallets]);
  const fam = AUTO_TAG_FAMILIES.find((f) => f.id === family) ?? AUTO_TAG_FAMILIES[0];
  const where = scope === 'token' ? 'this token' : 'this group';

  /** Why a rule cannot be clicked, or `null` when it can. */
  const blocked = (r: AutoTagRule): string | null => {
    if (!params || !groupMarks) return 'This run predates auto-tag. Run discovery again.';
    if (scope === 'token' && !tokenMint) return 'Pick a token in the preview, or switch to Group.';
    if (!marks) return 'This token has no marks in this run. Run discovery again.';
    if (marks[r.id] === undefined) {
      return r.market
        ? 'Off for this run: the lake market read failed, so snipers and never-sellers are unknown. Run discovery again.'
        : 'This run did not compute this rule. Run discovery again.';
    }
    if ((marks[r.id] ?? []).length === 0) return `No wallet matches on ${where}.`;
    return null;
  };

  return (
    <div className="rounded-lg border border-warning/30 bg-warning/4 p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1 text-xs font-semibold text-text">
          Auto-tag {tagLabel(tagName)}
          <InfoTooltip title="Auto-tag" body={PANEL_HELP} />
        </span>
        <ToggleGroup
          size="sm"
          tone="neutral"
          aria-label="Rule family"
          value={family}
          onChange={setFamily}
          options={AUTO_TAG_FAMILIES.map((f) => ({ value: f.id, label: f.label, title: f.help }))}
        />
        <ToggleGroup
          size="sm"
          tone="neutral"
          aria-label="Whose trades the rules read"
          value={scope}
          onChange={setScopePick}
          options={[
            { value: 'token', label: 'This token', title: SCOPE_HELP.token },
            { value: 'group', label: 'Group', title: SCOPE_HELP.group },
          ]}
        />
      </div>
      <p className="mb-2 text-[11px] leading-snug text-text-dim">{fam.help}</p>
      <div className="flex flex-wrap gap-2">
        {fam.rules.map((r) => {
          const ws = marks?.[r.id] ?? [];
          const why = blocked(r);
          const state = autoTagState(listed, ws);
          const listedN = ws.filter((w) => listed.has(w)).length;
          const status =
            state === 'on'
              ? `On: all ${ws.length} wallets are in the list. Click to take them out.`
              : state === 'some'
                ? `${listedN} of ${ws.length} wallets are in the list. Click to add the other ${ws.length - listedN}.`
                : `Click to add ${ws.length} wallet${ws.length === 1 ? '' : 's'} from ${where}.`;
          const help = params ? r.help(params) : '';
          return (
            <button
              key={r.id}
              type="button"
              aria-pressed={state === 'on'}
              disabled={why != null}
              onClick={() => marks && onWalletsChange(toggleAutoTag(wallets, marks, r.id))}
              title={`${r.label}\n\n${help}\n\n${why ?? status}`}
              className={cn(
                'inline-flex items-center gap-1.5 rounded border px-2 py-1 text-[11px] font-semibold transition',
                'disabled:cursor-not-allowed disabled:opacity-40',
                STATE_CLASS[state],
              )}
            >
              <span
                aria-hidden
                className={cn(
                  'size-2 rounded-full border border-current',
                  state === 'on' ? 'bg-current' : state === 'some' ? 'bg-current/40' : '',
                )}
              />
              {r.label}
              <span className="font-mono text-[10px] opacity-80">
                {state === 'some' ? `${listedN}/${ws.length}` : ws.length}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
