import { useEffect, useMemo, useState } from 'react';

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
import { tagField, useStrategyRegistry } from 'lib/strategy/registry';
import { isProgramWorkingId, toggleWorkingTemplate } from 'lib/strategy/templateGrain';
import { tagNames, withTagListValue, withTagShape } from 'lib/strategy/tagsDoc';
import { defaultTagName, tagLabel } from 'lib/flow/tapeClassify';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetFingerprintsQuery,
  useUpdateFingerprintMutation,
} from 'store/sharedEndpoints';
import type { FlowSide } from 'lib/flow/classifyFlow';
import { PreEntryProbeControls } from './PreEntryProbeControls';
import type { PreEntryProbe } from './usePreEntryProbe';
import type { TraderFlowLens } from './useTraderFlowLens';

const shortAddr = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

const KIND_OPTIONS: { value: IxPatternSetKind; label: string; title: string }[] = [
  {
    value: 'templates',
    label: 'Templates',
    title: 'Grain ids (program|CU|ATA|N|S|F) and program names: a tag\'s ix template / program matchers',
  },
  {
    value: 'exact',
    label: 'Exact',
    title: 'Full ix_labels sequences, optional fee pins: a tag\'s exact ix shape matcher',
  },
];

/**
 * The Trader Analysis **flow lens** control strip: which analysis-owned pattern set
 * every chart on the page reads as a tag (`@set` / `@!set`), and how.
 *
 * A wallet study has no fingerprint, so the chart stack has no tag to read and the
 * overlay never draws. This bar supplies one from `ix_pattern_sets` instead: the
 * narrowed set as matchers, with the lens' own side and sticky switches. Kind is
 * chosen at create (Templates or Exact); the set picker is the switch. Charts, the
 * tag column and badge clicks all follow the selected set.
 *
 * Nothing here can reach a rule: a set is analysis-only, and the one path into the
 * engine is the explicit add-to-fingerprint-tag below.
 */
export function FlowLensBar({
  lens,
  wallet,
  probe,
}: {
  lens: TraderFlowLens;
  wallet: string | null;
  /** The pre-entry probe asked WITH this lens — its set, its narrowing, its
   *  side, the same wallet excluded. Absent ⇒ the bar is the lens alone. */
  probe?: PreEntryProbe;
}) {
  const [pasteOpen, setPasteOpen] = useState(false);
  const [pasteText, setPasteText] = useState('');
  const [newName, setNewName] = useState('');
  const [newKind, setNewKind] = useState<IxPatternSetKind>('templates');
  const [copied, setCopied] = useState(false);

  const { set, sets, units, enabledUnits, keys } = lens;
  const { data: reg } = useStrategyRegistry();
  const stickyField = tagField(reg, 'sticky');
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

  // The box opens on the STORED set, not empty: a clipboard-only Copy left no way
  // to read what a lens actually holds. Same text `Copy JSON` writes (one
  // serializer), and it round-trips back through the parser, so the viewer and the
  // editor are the same control — read it, change a line, Replace.
  const togglePaste = () => {
    const next = !pasteOpen;
    setPasteOpen(next);
    if (next && set && storedCount > 0) setPasteText(setJson(set, isTemplates));
  };

  // A set swap while the box is open would leave another set's JSON on screen
  // looking like this one's.
  useEffect(() => {
    setPasteOpen(false);
    setPasteText('');
  }, [lens.setId]);

  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);

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
          <option value="">No lens - charts show no tag lines</option>
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
              active={pasteOpen}
              onClick={togglePaste}
              title={
                isTemplates
                  ? "Show this set's grain ids as JSON — edit them there, or paste new ones in"
                  : "Show this set's ix_labels sequences as JSON — edit them there, or paste new ones in"
              }
            >
              {pasteOpen ? 'Hide JSON' : 'Show JSON'}
            </Button>
            <Button
              size="xs"
              variant="ghost"
              onClick={() => {
                void navigator.clipboard?.writeText(setJson(set, isTemplates));
                setCopied(true);
              }}
              title="Copy the whole set to the clipboard as re-pastable JSON"
            >
              {copied ? 'Copied ✓' : 'Copy JSON'}
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
            <Button size="xs" variant="ghost" onClick={togglePaste}>
              {pasteOpen ? 'Close paste' : newKind === 'templates' ? 'Paste grains' : 'Paste patterns'}
            </Button>
          </>
        )}

        <span className="mx-1 h-4 w-px bg-white/10" />

        <SideControl lens={lens} />

        <span className="mx-1 h-4 w-px bg-white/10" />

        {/* Not sticky is the lens' default: it asks which STRUCTURES surround a
            moment, and a sticky wallet set answers which wallets ever matched. */}
        <label className="flex items-center gap-1.5 text-[11px] text-text-dim">
          <Switch
            checked={lens.contagion}
            onChange={lens.setContagion}
            label={stickyField?.title ?? 'Sticky'}
          />
          <span title={stickyField ? `${stickyField.summary}\n\nExample: ${stickyField.example}` : undefined}>
            {stickyField?.title ?? 'Sticky'}
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

      {/* Narrowing chips — one launch client (exact) or one build template
          (templates) at a time, without re-pasting. A click is VIEW state: the
          stored set never changes, so a muted chip can be brought back. Removing
          a grain from the set is the separate × — the two were one click here,
          which made narrowing a templates lens impossible. */}
      {set && units.length > (isTemplates ? 0 : 1) && (
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
            {isTemplates ? 'Grains' : 'Groups'}
          </span>
          {units.map((u) => {
            const on = !enabledUnits || enabledUnits.has(u);
            const count = isTemplates
              ? 0
              : set.patterns.filter((p) => (p.group ?? UNGROUPED) === u).length;
            return (
              <span
                key={u}
                className={cn(
                  'inline-flex items-center rounded-full border transition-colors',
                  on
                    ? isTemplates
                      ? 'border-green/40 bg-green/15 text-green'
                      : 'border-primary/50 bg-primary/15 text-primary'
                    : 'border-white/10 bg-transparent text-text-dim',
                )}
              >
                <button
                  type="button"
                  onClick={() => lens.toggleUnit(u)}
                  className={cn(
                    'py-0.5 pl-2.5 text-[11px]',
                    isTemplates ? 'pr-2 font-mono' : 'pr-2.5',
                    on ? '' : 'hover:text-text',
                  )}
                  title={
                    on
                      ? `Classifying${isTemplates ? '' : ` · ${count} pattern${count === 1 ? '' : 's'}`} — click to mute`
                      : `Muted${isTemplates ? '' : ` · ${count} pattern${count === 1 ? '' : 's'}`} — click to classify with it again`
                  }
                >
                  {isTemplates ? u : `${u} · ${count}`}
                </button>
                {isTemplates && (
                  <button
                    type="button"
                    onClick={() =>
                      void lens.saveTemplates(toggleWorkingTemplate(set.working_templates, u))
                    }
                    className={cn(
                      'self-stretch border-l px-2 text-[11px] leading-none transition-colors hover:bg-red/20 hover:text-red',
                      on ? 'border-green/40 text-green/60' : 'border-white/10 text-text-dim/60',
                    )}
                    title="Remove this grain from the set — a click on the id itself only mutes it"
                  >
                    ×
                  </button>
                )}
              </span>
            );
          })}
        </div>
      )}

      {pasteOpen && (
        <div className="mt-2">
          <div className="mb-1 flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
            <span className="text-[9px] font-bold uppercase tracking-widest">
              {isTemplates ? 'Grains' : 'Patterns'} JSON
            </span>
            <span>
              {set
                ? `The whole stored set — ${storedCount} ${isTemplates ? 'grain' : 'pattern'}${storedCount === 1 ? '' : 's'}, narrowing not applied. Edit and Replace to save it back.`
                : 'Paste a set to create one.'}
            </span>
          </div>
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
              variant="link"
              disabled={!pasteText}
              onClick={() => setPasteText('')}
              title="Empty the box to paste something else in"
            >
              Clear
            </Button>
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

      {probe && <PreEntryProbeControls probe={probe} />}

      {set && storedCount > 0 && <PromoteToFingerprint setKind={kind} set={set} />}
    </div>
  );
}

/** The set as re-pastable JSON — the ONE text both `Copy JSON` and the box show,
 *  so what you read is what the clipboard carries and what the parser accepts. */
function setJson(
  set: { patterns: IxPattern[]; working_templates: string[] },
  isTemplates: boolean,
): string {
  return isTemplates
    ? JSON.stringify(set.working_templates, null, 2)
    : formatPatternsJson(set.patterns);
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

/** Leg narrowing: Both / Buy / Sell - the lens tag's `side`.
 *
 * `ix_labels` carry no direction — an aggregator's structure is byte-identical
 * on the buy and on the sell that unwinds it - so one shape matches both legs and
 * an unnarrowed line sums two opposite events. It composes with the group chips
 * (Axiom-buy vs Axiom-sell falls out of the two together). */
function SideControl({ lens }: { lens: TraderFlowLens }) {
  const { data: reg } = useStrategyRegistry();
  const field = tagField(reg, 'side');
  const options: { value: FlowSide | null; label: string; title: string }[] = [
    { value: null, label: 'Both', title: 'buys and sells can carry the lens tag' },
    { value: 'buy', label: 'Buy', title: 'only buys can carry the lens tag' },
    { value: 'sell', label: 'Sell', title: 'only sells can carry the lens tag' },
  ];
  return (
    <div className="flex items-center gap-1">
      <span
        className="text-[9px] font-bold uppercase tracking-widest text-text-dim"
        title={field ? `${field.summary}\n\nExample: ${field.example}` : undefined}
      >
        {field?.title ?? 'Side'}
      </span>
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
 * The one path from study to something the engine reads: add the lens' entries to a
 * fingerprint tag - exact rows under `ix_shape` (pins kept), grain ids under
 * `ix_template`, bare program names under `program`.
 *
 * Deliberately explicit and one-directional. A lens is a guess under examination; a
 * fingerprint's tags are what live rules read, so the crossing is a decision, never
 * a side effect of editing a lens. It ADDS - what the tag already lists stays - and
 * group labels have no home on a tag and are dropped.
 */
function PromoteToFingerprint({
  setKind,
  set,
}: {
  setKind: IxPatternSetKind;
  set: { patterns: IxPattern[]; working_templates: string[] };
}) {
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const { data: reg } = useStrategyRegistry();
  const [updateFingerprint, { isLoading }] = useUpdateFingerprintMutation();
  const [targetId, setTargetId] = useState('');
  const [tagName, setTagName] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const target = fingerprints.find((f) => f.id === targetId) ?? null;
  const names = tagNames(target?.tags);
  const tag = tagName ?? defaultTagName(target?.tags);
  const isTemplates = setKind === 'templates';
  const n = isTemplates ? set.working_templates.length : set.patterns.length;
  const title = (k: string) => tagField(reg, k)?.title ?? k;
  const writes = isTemplates
    ? `${tagLabel(tag)} (${title('ix_template')} / ${title('program')})`
    : `${tagLabel(tag)} (${title('ix_shape')})`;

  const copy = async () => {
    if (!target) return;
    setStatus(null);
    let tags: unknown = target.tags;
    if (isTemplates) {
      for (const id of set.working_templates) {
        tags = withTagListValue(tags, tag, isProgramWorkingId(id) ? 'program' : 'ix_template', id);
      }
    } else {
      for (const p of set.patterns) tags = withTagShape(tags, tag, toPatternRow(p));
    }
    try {
      await updateFingerprint({
        id: target.id,
        body: {
          name: target.name,
          // The whole row round-trips: a PUT replaces it, so an omitted axis would
          // silently WIDEN what this fingerprint matches, and `wildcard` omitted
          // defaults to false, turning a match-everything row criterion-less.
          criteria: target.criteria,
          wildcard: target.wildcard,
          tags: tags as Record<string, unknown>,
        },
      }).unwrap();
      setStatus(`Added ${n} ${isTemplates ? 'id' : 'shape'}${n === 1 ? '' : 's'} to ${writes} on ${target.name}`);
    } catch (e) {
      setStatus(apiErrorMessage(e as never, 'Failed to add to the fingerprint tag'));
    }
  };

  return (
    <div className="mt-2 flex flex-wrap items-center gap-2 border-t border-white/7 pt-2">
      <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
        Add to fingerprint tag
      </span>
      <Select
        fieldSize="sm"
        value={targetId}
        onChange={(e) => {
          setTargetId(e.target.value);
          setTagName(null);
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
      {target && (
        <Select
          fieldSize="sm"
          value={tag}
          onChange={(e) => setTagName(e.target.value)}
          className="max-w-40 font-mono"
          title="The fingerprint tag the lens entries are added to"
        >
          {(names.includes(tag) ? names : [...names, tag]).map((t) => (
            <option key={t} value={t}>
              {tagLabel(t)}
              {names.includes(t) ? '' : ' (new)'}
            </option>
          ))}
        </Select>
      )}
      <Button
        size="xs"
        variant="ghost"
        disabled={!target || isLoading}
        onClick={() => void copy()}
        title={`Add this lens' ${n} entr${n === 1 ? 'y' : 'ies'} to ${writes}. What the tag already lists stays.`}
      >
        Add {n} to {tagLabel(tag)}
      </Button>
      {target && (
        <span className="text-[11px] text-warning">
          Saves {writes} on {target.name}: every rule reading it changes meaning.
        </span>
      )}
      {status && <span className="text-[11px] text-text-dim">{status}</span>}
    </div>
  );
}
