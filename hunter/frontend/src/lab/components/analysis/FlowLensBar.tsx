import { useMemo, useState } from 'react';

import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { Switch } from 'components/ui/Switch';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import { cn } from 'lib/cn';
import {
  formatPatternsJson,
  kindOf,
  parsePastedGrains,
  parsePastedPatterns,
  patternGroups,
  toPatternRow,
  UNGROUPED,
  type IxPattern,
  type IxPatternSetKind,
} from 'lib/flow/ixPatternSets';
import { patternRowKey } from 'lib/strategy/ixPatternRows';
import { metricConfigWithList, metricConfigWithWorkingTemplates } from 'lib/strategy/registry';
import { toggleWorkingTemplate } from 'lib/strategy/templateGrain';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetFingerprintsQuery,
  useUpdateFingerprintMutation,
} from 'store/sharedEndpoints';
import type { FlowSide } from 'lib/flow/classifyFlow';
import type { TraderFlowLens } from './useTraderFlowLens';

const shortAddr = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

const KIND_OPTIONS: { value: IxPatternSetKind; label: string; title: string }[] = [
  {
    value: 'templates',
    label: 'Templates',
    title: 'Grain ids (program|CU|ATA|N|S|F) — harvest working-template vocabulary',
  },
  {
    value: 'exact',
    label: 'Exact',
    title: 'Full ix_labels sequences, optional fee pins — same as tagged/dump',
  },
];

/**
 * The Trader Analysis **flow lens** control strip: which analysis-owned pattern
 * set every chart on the page classifies vol/non-vol with, and how.
 *
 * A wallet study has no fingerprint, so the chart stack's usual source for
 * patterns is empty and the vol/non-vol overlay never draws. This bar supplies
 * that fact from `ix_pattern_sets` instead. Kind is chosen at create (Templates
 * or Exact); the set picker is the switch. Charts, the Vol column, and badge
 * clicks all follow the selected set's kind.
 *
 * Nothing here can reach a rule: a set is analysis-only, and the one path into
 * the engine is the explicit copy-to-fingerprint below.
 */
export function FlowLensBar({ lens, wallet }: { lens: TraderFlowLens; wallet: string | null }) {
  const [pasteOpen, setPasteOpen] = useState(false);
  const [pasteText, setPasteText] = useState('');
  const [newName, setNewName] = useState('');
  const [newKind, setNewKind] = useState<IxPatternSetKind>('templates');

  const { set, sets, groups, enabledGroups, keys } = lens;
  const kind = set ? kindOf(set) : newKind;
  const isTemplates = kind === 'templates';
  const classifying = keys?.size ?? 0;
  const storedCount = isTemplates ? (set?.working_templates.length ?? 0) : (set?.patterns.length ?? 0);

  const parsedPatterns = useMemo(
    () => (!isTemplates && pasteText.trim() ? parsePastedPatterns(pasteText) : null),
    [isTemplates, pasteText],
  );
  const parsedGrains = useMemo(
    () => (isTemplates && pasteText.trim() ? parsePastedGrains(pasteText) : null),
    [isTemplates, pasteText],
  );

  const pasteReady = isTemplates
    ? (parsedGrains?.grains.length ?? 0) > 0
    : (parsedPatterns?.patterns.length ?? 0) > 0;

  const applyPaste = async (mode: 'replace' | 'merge') => {
    if (isTemplates) {
      if (!parsedGrains || parsedGrains.grains.length === 0) return;
      if (!set) {
        await lens.createSet(newName.trim() || defaultSetName(wallet, newKind), newKind, [], parsedGrains.grains);
      } else {
        const next =
          mode === 'replace'
            ? parsedGrains.grains
            : mergeGrains(set.working_templates, parsedGrains.grains);
        await lens.saveTemplates(next);
      }
    } else {
      if (!parsedPatterns || parsedPatterns.patterns.length === 0) return;
      if (!set) {
        await lens.createSet(
          newName.trim() || defaultSetName(wallet, newKind),
          newKind,
          parsedPatterns.patterns,
          [],
        );
      } else {
        const next =
          mode === 'replace'
            ? parsedPatterns.patterns
            : mergeExact(set.patterns, parsedPatterns.patterns);
        await lens.savePatterns(next);
      }
    }
    setPasteText('');
    setPasteOpen(false);
  };

  return (
    <div className="mb-3 rounded-md border border-white/8 bg-white/2 p-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="info" size="sm">
          Flow lens
        </Badge>

        <Select
          fieldSize="sm"
          value={lens.setId ?? ''}
          onChange={(e) => lens.selectSet(e.target.value || null)}
          title="Pattern set every chart on this page classifies with"
          className="max-w-[22rem]"
        >
          <option value="">No lens — charts show no vol/non-vol lines</option>
          {sets.map((s) => {
            const k = kindOf(s);
            const n = k === 'templates' ? s.working_templates.length : s.patterns.length;
            return (
              <option key={s.id} value={s.id}>
                {s.name} ({n} {k === 'templates' ? 'grain' : 'pattern'}
                {n === 1 ? '' : 's'} · {k})
                {s.wallet_address ? ` · ${shortAddr(s.wallet_address)}` : ''}
              </option>
            );
          })}
        </Select>

        {set ? (
          <>
            <Badge variant={isTemplates ? 'success' : 'info'} size="sm">
              {isTemplates ? 'Templates' : 'Exact'}
            </Badge>
            <span
              className="font-mono text-[11px] text-text-dim"
              title={
                isTemplates
                  ? 'Grains currently classifying / grains in the set'
                  : 'Patterns currently classifying / patterns in the set'
              }
            >
              {classifying}/{storedCount} classifying
            </span>
            <Button
              size="xs"
              variant="ghost"
              onClick={() => setPasteOpen((v) => !v)}
              title={isTemplates ? 'Paste grain ids into this set' : 'Paste ix_labels sequences into this set'}
            >
              {pasteOpen ? 'Close paste' : isTemplates ? 'Paste grains' : 'Paste patterns'}
            </Button>
            <Button
              size="xs"
              variant="ghost"
              onClick={() => {
                const text = isTemplates
                  ? JSON.stringify(set.working_templates, null, 2)
                  : formatPatternsJson(set.patterns);
                void navigator.clipboard?.writeText(text);
              }}
              title="Copy the whole set as re-pastable JSON"
            >
              Copy JSON
            </Button>
            <RenameControl lens={lens} />
            <Button
              size="xs"
              variant="danger"
              onClick={() => void lens.deleteSet()}
              title="Delete this pattern set"
            >
              Delete
            </Button>
          </>
        ) : (
          <>
            <ToggleGroup
              size="sm"
              tone="neutral"
              aria-label="New set vocabulary"
              value={newKind}
              onChange={setNewKind}
              options={KIND_OPTIONS}
            />
            <Input
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={defaultSetName(wallet, newKind)}
              className="w-[220px] font-normal normal-case tracking-normal"
            />
            <Button
              size="xs"
              variant="primary"
              onClick={() =>
                void lens.createSet(newName.trim() || defaultSetName(wallet, newKind), newKind)
              }
            >
              New set
            </Button>
            <Button size="xs" variant="ghost" onClick={() => setPasteOpen((v) => !v)}>
              {pasteOpen ? 'Close paste' : newKind === 'templates' ? 'Paste grains' : 'Paste patterns'}
            </Button>
          </>
        )}

        <span className="mx-1 h-4 w-px bg-white/10" />

        <SideControl lens={lens} />

        <span className="mx-1 h-4 w-px bg-white/10" />

        {/* Structural-only is the lens' default and the reason it answers a
            different question than the engine's own split — see `contagion`. */}
        <label className="flex items-center gap-1.5 text-[11px] text-text-dim">
          <Switch
            checked={lens.contagion}
            onChange={lens.setContagion}
            label="Wallet contagion"
          />
          <span
            title="ON = the engine's own rule: one structural match tags that wallet, and every later trade of it counts as volume (the creator seeds it). OFF = each trade judged by its own ix_labels / grain alone — what you want when the question is which STRUCTURES surround a moment."
          >
            Contagion
          </span>
        </label>
        <label className="flex items-center gap-1.5 text-[11px] text-text-dim">
          <Switch
            checked={lens.excludeSelf}
            onChange={lens.setExcludeSelf}
            label="Exclude the studied wallet"
            disabled={!wallet}
          />
          <span title="Keep the studied trader's own trades out of the split, so the lines describe what happened AROUND them.">
            Exclude self
          </span>
        </label>

        {lens.saving && <span className="text-[11px] text-text-dim">Saving…</span>}
        {lens.error && <span className="text-[11px] text-red">{lens.error}</span>}
      </div>

      {set && isTemplates && set.working_templates.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
            Grains
          </span>
          {set.working_templates.map((g) => (
            <button
              key={g}
              type="button"
              onClick={() => void lens.saveTemplates(toggleWorkingTemplate(set.working_templates, g))}
              className="rounded-full border border-green/40 bg-green/15 px-2 py-0.5 font-mono text-[11px] text-green transition-colors hover:border-red/50 hover:bg-red/15 hover:text-red"
              title="Remove this grain"
            >
              {g}
            </button>
          ))}
        </div>
      )}

      {/* Group narrowing — one launch client at a time, without re-pasting. Exact only. */}
      {set && !isTemplates && groups.length > 1 && (
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
            Groups
          </span>
          {groups.map((g) => {
            const on = !enabledGroups || enabledGroups.has(g);
            const count = set.patterns.filter((p) => (p.group ?? UNGROUPED) === g).length;
            return (
              <button
                key={g}
                type="button"
                onClick={() => lens.toggleGroup(g)}
                className={cn(
                  'rounded-full border px-2 py-0.5 text-[11px] transition-colors',
                  on
                    ? 'border-primary/50 bg-primary/15 text-primary'
                    : 'border-white/10 bg-transparent text-text-dim hover:text-text',
                )}
                title={`${count} pattern${count === 1 ? '' : 's'}`}
              >
                {g} · {count}
              </button>
            );
          })}
        </div>
      )}

      {pasteOpen && (
        <div className="mt-2">
          <textarea
            value={pasteText}
            onChange={(e) => setPasteText(e.target.value)}
            rows={6}
            spellCheck={false}
            placeholder={
              kind === 'templates'
                ? 'Paste ["Axiom Trade|CU", "GMGN|ATA"]\nor one grain id per line'
                : 'Paste [["Compute Budget: SetComputeUnitLimit","…"], …]\n' +
                  'or [{ "tool": "Axiom Trade", "ix_labels": ["…"], "cu_limit": 300000 }, …]\n' +
                  'or a { "patterns": [ … ] } file'
            }
            className="w-full rounded-md border border-white/10 bg-black/30 p-2 font-mono text-[11px] text-text outline-none focus:border-primary/50"
          />
          <div className="mt-1.5 flex flex-wrap items-center gap-2 text-[11px]">
            {isTemplates ? (
              <>
                {parsedGrains?.error && <span className="text-red">{parsedGrains.error}</span>}
                {parsedGrains && !parsedGrains.error && (
                  <span className="text-text-dim">
                    {parsedGrains.accepted} grain{parsedGrains.accepted === 1 ? '' : 's'}
                    {parsedGrains.duplicates > 0 && ` · ${parsedGrains.duplicates} duplicate`}
                  </span>
                )}
              </>
            ) : (
              <>
                {parsedPatterns?.error && <span className="text-red">{parsedPatterns.error}</span>}
                {parsedPatterns && !parsedPatterns.error && (
                  <span className="text-text-dim">
                    {parsedPatterns.accepted} pattern{parsedPatterns.accepted === 1 ? '' : 's'}
                    {parsedPatterns.duplicates > 0 && ` · ${parsedPatterns.duplicates} duplicate`}
                    {parsedPatterns.skipped > 0 && ` · ${parsedPatterns.skipped} skipped`}
                    {parsedPatterns.patterns.length > 0 &&
                      ` · groups: ${patternGroups(parsedPatterns.patterns).join(', ')}`}
                  </span>
                )}
              </>
            )}
            <span className="grow" />
            <Button
              size="xs"
              variant="primary"
              disabled={!pasteReady}
              onClick={() => void applyPaste('replace')}
            >
              {set ? 'Replace set' : 'Create set'}
            </Button>
            {set && (
              <Button
                size="xs"
                variant="ghost"
                disabled={!pasteReady}
                onClick={() => void applyPaste('merge')}
                title="Keep the set's current entries and add the new ones"
              >
                Merge in
              </Button>
            )}
          </div>
        </div>
      )}

      {set && storedCount > 0 && <PromoteToFingerprint setKind={kind} set={set} />}
    </div>
  );
}

/** Default name for a set created while studying a wallet. */
function defaultSetName(wallet: string | null, kind: IxPatternSetKind): string {
  const who = wallet ? `${shortAddr(wallet)} ` : '';
  return `${who}${kind === 'templates' ? 'templates' : 'exact'}`;
}

function mergeGrains(current: string[], incoming: string[]): string[] {
  const seen = new Set(current);
  const out = [...current];
  for (const g of incoming) {
    if (seen.has(g)) continue;
    seen.add(g);
    out.push(g);
  }
  return out;
}

/** Union by labels+pins identity, incoming groups/pins winning on a re-paste. */
function mergeExact(current: IxPattern[], incoming: IxPattern[]): IxPattern[] {
  const byKey = new Map(incoming.map((p) => [patternRowKey(toPatternRow(p)), p]));
  const kept = current.map((p) => byKey.get(patternRowKey(toPatternRow(p))) ?? p);
  const keptKeys = new Set(kept.map((p) => patternRowKey(toPatternRow(p))));
  return [...kept, ...incoming.filter((p) => !keptKeys.has(patternRowKey(toPatternRow(p))))];
}

/** Leg narrowing: Both / Buy / Sell.
 *
 * `ix_labels` carry no direction — an aggregator's structure is byte-identical
 * on the buy and on the sell that unwinds it — so one pattern key matches both
 * legs and an unnarrowed line sums two opposite events. Narrowing is a filter
 * over TRADES, not over patterns: no set edit, and it composes with the group
 * chips (Axiom-buy vs Axiom-sell falls out of the two together). */
function SideControl({ lens }: { lens: TraderFlowLens }) {
  const options: { value: FlowSide | null; label: string; title: string }[] = [
    {
      value: null,
      label: 'Both',
      title: 'Classify every leg — the engine’s own behavior. One pattern counts a matched structure buying AND the same structure selling.',
    },
    {
      value: 'buy',
      label: 'Buy',
      title: 'Only matched BUYS count as volume — the crowd impulse a trade joins.',
    },
    {
      value: 'sell',
      label: 'Sell',
      title: 'Only matched SELLS count as volume — the exit liquidity a trade absorbs.',
    },
  ];
  return (
    <div className="flex items-center gap-1">
      <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">Side</span>
      <div className="flex overflow-hidden rounded-md border border-white/10">
        {options.map((o) => {
          const on = lens.side === o.value;
          return (
            <button
              key={o.label}
              type="button"
              onClick={() => lens.setSide(o.value)}
              title={o.title}
              className={cn(
                'px-2 py-0.5 text-[11px] transition-colors',
                on ? 'bg-primary/20 text-primary' : 'text-text-dim hover:text-text',
              )}
            >
              {o.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}
function RenameControl({ lens }: { lens: TraderFlowLens }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState('');
  if (!lens.set) return null;
  if (!editing) {
    return (
      <Button
        size="xs"
        variant="ghost"
        onClick={() => {
          setName(lens.set?.name ?? '');
          setEditing(true);
        }}
      >
        Rename
      </Button>
    );
  }
  return (
    <span className="inline-flex items-center gap-1">
      <Input
        value={name}
        onChange={(e) => setName(e.target.value)}
        className="w-[200px] font-normal normal-case tracking-normal"
        autoFocus
      />
      <Button
        size="xs"
        variant="primary"
        onClick={() => {
          void lens.renameSet(name);
          setEditing(false);
        }}
      >
        Save
      </Button>
      <Button size="xs" variant="link" onClick={() => setEditing(false)}>
        Cancel
      </Button>
    </span>
  );
}

/**
 * The one path from study to something the engine reads: copy the lens into the
 * matching fingerprint field (exact → m_flow_ix.ix_patterns with fees;
 * templates → m_burst_slot.working_templates).
 *
 * Deliberately explicit and one-directional. A lens is a guess under
 * examination; a fingerprint's lists are what live rules classify with, so the
 * crossing is a decision, never a side effect of editing a lens. Group labels
 * have no home on a fingerprint and are dropped.
 */
function PromoteToFingerprint({
  setKind,
  set,
}: {
  setKind: IxPatternSetKind;
  set: { patterns: IxPattern[]; working_templates: string[] };
}) {
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const [updateFingerprint, { isLoading }] = useUpdateFingerprintMutation();
  const [targetId, setTargetId] = useState('');
  const [status, setStatus] = useState<string | null>(null);

  const target = fingerprints.find((f) => f.id === targetId) ?? null;
  const isTemplates = setKind === 'templates';
  const n = isTemplates ? set.working_templates.length : set.patterns.length;

  const copy = async () => {
    if (!target) return;
    setStatus(null);
    try {
      const prev = target.metric_config ?? {};
      const metric_config = isTemplates
        ? metricConfigWithWorkingTemplates(prev, set.working_templates)
        : metricConfigWithList(
            prev,
            set.patterns.map(toPatternRow),
            'tagged',
          );
      await updateFingerprint({
        id: target.id,
        body: {
          name: target.name,
          // The whole criteria map round-trips: a PUT replaces the row, so an
          // omitted axis would silently WIDEN what this fingerprint matches. Same
          // reason `wildcard` is sent — omitted it defaults to false, turning a
          // match-everything row into a criterion-less one.
          criteria: target.criteria,
          wildcard: target.wildcard,
          metric_config,
        },
      }).unwrap();
      setStatus(
        `Copied ${n} ${isTemplates ? 'grain' : 'pattern'}${n === 1 ? '' : 's'} to ${target.name}`,
      );
    } catch (e) {
      setStatus(apiErrorMessage(e as never, 'Failed to copy to fingerprint'));
    }
  };

  return (
    <div className="mt-2 flex flex-wrap items-center gap-2 border-t border-white/7 pt-2">
      <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
        Copy to fingerprint
      </span>
      <Select
        fieldSize="sm"
        value={targetId}
        onChange={(e) => {
          setTargetId(e.target.value);
          setStatus(null);
        }}
        className="max-w-[16rem]"
      >
        <option value="">Pick a fingerprint…</option>
        {fingerprints.map((f) => (
          <option key={f.id} value={f.id}>
            {f.name}
          </option>
        ))}
      </Select>
      <Button
        size="xs"
        variant="ghost"
        disabled={!target || isLoading}
        onClick={() => void copy()}
        title={
          isTemplates
            ? "Replace that fingerprint's working_templates with this lens' grains."
            : "Replace that fingerprint's ix_patterns with this lens' exact rows (fees kept)."
        }
      >
        {isTemplates ? 'Copy grains' : 'Copy patterns'}
      </Button>
      <span className="text-[11px] text-warning">
        {isTemplates
          ? 'Replaces its working_templates — harvest rules bound to it change meaning.'
          : 'Replaces its ix_patterns — every rule bound to it changes meaning.'}
      </span>
      {status && <span className="text-[11px] text-text-dim">{status}</span>}
    </div>
  );
}
