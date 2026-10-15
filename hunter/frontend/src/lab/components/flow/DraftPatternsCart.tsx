import { Fragment, useState } from 'react';

import { clearPrompt, IxPatternRowsEditor } from 'components/strategy/IxPatternsEditor';
import { LabelTip } from 'components/strategy/LabelTip';
import { TagKindBadge, TagNameOptions } from 'components/strategy/TagKind';
import { tagSentence } from 'lib/strategy/tagsDoc';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { EmptyState } from 'components/ui/EmptyState';
import { IconButton } from 'components/ui/IconButton';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { CheckIcon, CloseIcon, EditIcon, LinkIcon, SpinnerIcon, TrashIcon } from 'components/ui/icons';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { FeePinToggles } from 'components/tokens/FeePinToggles';
import { STAGE_MATCHER_TITLE, type StageMatcher } from 'hooks/useIxPatternTarget';
import type { FlowTag } from 'lib/flow/classifyFlow';
import { flowTagOf, tagLabel } from 'lib/flow/tapeClassify';
import {
  formatFeePins,
  patternRowKey,
  rowIsCore,
  rowPinsFee,
  serializeIxPatternRows,
  type IxPatternFeeMask,
  type IxPatternRow,
} from 'lib/strategy/ixPatternRows';
import { tagField, tagFieldTip, useStrategyRegistry } from 'lib/strategy/registry';
import { DISCOVERY_FIELD_HELP } from 'lib/strategy/strategyHelp';
import {
  TAG_NAME_RE,
  withTagListValue,
  withTagShape,
  withTagSwitches,
  type TagSwitches,
} from 'lib/strategy/tagsDoc';
import type { Fingerprint } from 'lib/strategy/types';

/** The grains a discovery check writes. Both are `ix_shape` rows. */
export type CartMatcher = Exclude<StageMatcher, 'wallet'>;

export const CART_MATCHERS: readonly CartMatcher[] = ['ix_shape', 'ix_core'];

/**
 * `doc` with tag `name`'s `ix_shape` set to `rows` — exact structures and ix cores
 * together — its `wallet` list to `wallets`, and (when given) its sticky and creator
 * switches, every other matcher, option and tag kept. Adds before it removes, so a list swapped whole never passes through "no
 * matcher".
 */
export function withTagDraft(
  doc: unknown,
  name: string,
  rows: readonly IxPatternRow[],
  wallets: readonly string[] = [],
  switches?: TagSwitches,
): Record<string, unknown> {
  const saved = flowTagOf(doc, name);
  let out: unknown = doc;
  for (const r of rows) out = withTagShape(out, name, r);
  for (const w of wallets) out = withTagListValue(out, name, 'wallet', w);
  const keep = new Set(rows.map(patternRowKey));
  for (const r of saved?.match.ix_shape ?? []) {
    if (!keep.has(patternRowKey(r))) out = withTagShape(out, name, r, true);
  }
  for (const w of saved?.match.wallet ?? []) {
    if (!wallets.includes(w)) out = withTagListValue(out, name, 'wallet', w, true);
  }
  if (switches) out = withTagSwitches(out, name, switches);
  return (out ?? {}) as Record<string, unknown>;
}

const NEW_TAG = '-new';

/** The tag switches the cart edits, each explained by its registry field. */
const SWITCH_KEYS: readonly (keyof TagSwitches)[] = ['sticky', 'creator'];

/** Staging "cart" for one fingerprint tag: an accent-elevated panel that reads as the
 *  page's deliverable. Checked rows from the ranked table land here as chips; Apply
 *  writes them into the named tag's matcher on the fingerprint. Raw JSON editing is
 *  one toggle away. */
export function DraftPatternsCart({
  draftPatterns,
  onChange,
  currentPatterns,
  draftWallets,
  onWalletsChange,
  currentWallets,
  targetFp,
  tagName,
  onTagNameChange,
  matcher,
  onMatcherChange,
  feePins,
  onFeePinsChange,
  switches,
  savedSwitches,
  onSwitchesChange,
  previewTag,
  applying,
  onApply,
}: {
  draftPatterns: IxPatternRow[];
  onChange: (patterns: IxPatternRow[]) => void;
  /** The tag's saved `ix_shape`. */
  currentPatterns: IxPatternRow[];
  /** Staged `wallet` list - a trades-table click under the wallet matcher. */
  draftWallets: string[];
  onWalletsChange: (wallets: string[]) => void;
  /** The tag's saved `wallet` list. */
  currentWallets: string[];
  targetFp: Fingerprint | null;
  /** The tag Apply writes into (`@tagName`). */
  tagName: string;
  /** Switching reseeds the draft from that tag - the page owns that. */
  onTagNameChange: (name: string) => void;
  /** Which grain a ranked-table check writes. */
  matcher: CartMatcher;
  onMatcherChange: (m: CartMatcher) => void;
  /** Fee fields a check copies: off a ranked row's one budget, or off a clicked tx. */
  feePins: IxPatternFeeMask;
  onFeePinsChange: (mask: IxPatternFeeMask) => void;
  /** The tag's sticky and creator switches as staged, and as saved. */
  switches: TagSwitches;
  savedSwitches: TagSwitches;
  onSwitchesChange: (sw: TagSwitches) => void;
  /** The tag as Apply would save it. */
  previewTag: FlowTag;
  applying: boolean;
  onApply: () => void;
}) {
  const [rawEdit, setRawEdit] = useState(false);
  const [newTag, setNewTag] = useState<string | null>(null);
  const { data: reg } = useStrategyRegistry();
  const builtin = reg?.tags.builtin.map((b) => b.name) ?? [];

  const draftNorm = serializeIxPatternRows(draftPatterns);
  const savedNorm = serializeIxPatternRows(currentPatterns);
  const stagedCount = draftNorm.length + draftWallets.length;
  const savedCount = savedNorm.length + currentWallets.length;
  const dirty =
    JSON.stringify(draftNorm) !== JSON.stringify(savedNorm) ||
    JSON.stringify([...draftWallets].sort()) !== JSON.stringify([...currentWallets].sort()) ||
    switches.sticky !== savedSwitches.sticky ||
    switches.creator !== savedSwitches.creator;
  const tagNames = Object.keys(targetFp?.tags ?? {});
  const names = tagNames.includes(tagName) ? tagNames : [...tagNames, tagName];
  const writes = `${tagLabel(tagName)} (${STAGE_MATCHER_TITLE[matcher]})`;

  const applyLabel = applying
    ? 'Applying…'
    : targetFp
      ? `Save ${writes} on "${targetFp.name}"`
      : `Create & bind fingerprint with ${writes}`;

  const applyDisabled = applying || (!targetFp && stagedCount === 0);
  const newOk = newTag !== null && TAG_NAME_RE.test(newTag) && !builtin.includes(newTag);

  return (
    <div className="rounded-lg border border-accent/30 bg-accent/4 p-3 shadow-[0_0_0_1px_color-mix(in_srgb,var(--color-accent)_10%,transparent)]">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <span className="inline-flex flex-wrap items-center gap-2">
          <LabelTip tip={DISCOVERY_FIELD_HELP.draftPatterns} className="text-xs font-semibold text-text">
            Draft for
          </LabelTip>
          {newTag !== null ? (
            <span className="inline-flex items-center gap-1">
              <Input
                fieldSize="sm"
                className="w-28 font-mono"
                value={newTag}
                autoFocus
                placeholder="tag name"
                onChange={(e) => setNewTag(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && newOk) {
                    onTagNameChange(newTag);
                    setNewTag(null);
                  } else if (e.key === 'Escape') setNewTag(null);
                }}
              />
              <span className={`text-[10px] ${newOk ? 'text-text-dim' : 'text-red'}`}>
                {newOk ? 'Enter: Apply creates it' : 'a-z, 0-9, _ (1 to 24), not a built-in class'}
              </span>
            </span>
          ) : (
            <Select
              fieldSize="sm"
              className="max-w-40 font-mono"
              value={tagName}
              title="The fingerprint tag Apply writes into"
              onChange={(e) => (e.target.value === NEW_TAG ? setNewTag('') : onTagNameChange(e.target.value))}
            >
              <TagNameOptions names={names} known={tagNames} />
              <option value={NEW_TAG}>new tag...</option>
            </Select>
          )}
          <TagKindBadge name={tagName} />
          {SWITCH_KEYS.map((k) => {
            const f = tagField(reg, k);
            return (
              <label key={k} className="inline-flex items-center gap-1 text-[11px] text-text-mid">
                <input
                  type="checkbox"
                  className="accent-accent"
                  checked={switches[k]}
                  onChange={(e) => onSwitchesChange({ ...switches, [k]: e.target.checked })}
                />
                {f?.title ?? k}
                <InfoTooltip title={f?.title} body={tagFieldTip(f)} />
              </label>
            );
          })}
          <ToggleGroup
            size="sm"
            tone="neutral"
            aria-label="Which matcher a checked row writes"
            value={matcher}
            onChange={onMatcherChange}
            options={CART_MATCHERS.map((m) => ({
              value: m,
              label: STAGE_MATCHER_TITLE[m],
              title:
                m === 'ix_core'
                  ? 'One ix core: the list without compute budget, System, token-account, memo and Lighthouse, pump verbs merged'
                  : 'One exact ix structure: the whole instruction list, in order',
            }))}
          />
          <FeePinToggles
            mask={feePins}
            onChange={onFeePinsChange}
            onto="a ranked row with one budget, or a clicked tx"
          />
          <Badge variant={stagedCount > 0 ? 'accent' : 'neutral'} size="sm" pill>
            {stagedCount} staged
          </Badge>
          {targetFp && (
            <span className="text-[10px] text-text-dim">
              {savedCount} saved
            </span>
          )}
          {dirty && (
            <Badge variant="warning" size="sm" pill>
              unsaved
            </Badge>
          )}
        </span>
        <span className="inline-flex items-center gap-2">
          {!rawEdit && stagedCount > 0 && (
            <Button
              variant="link"
              size="xs"
              className="text-red hover:text-red"
              onClick={() => {
                if (!window.confirm(clearPrompt(draftPatterns))) return;
                onChange([]);
                onWalletsChange([]);
              }}
              title="Delete all staged rows"
            >
              <TrashIcon className="h-3 w-3" />
              Delete all
            </Button>
          )}
          <Button
              variant="link"
              size="xs"
              onClick={() => setRawEdit((v) => !v)}
              title={rawEdit ? 'Back to chip view' : 'Edit raw JSON rows (labels + optional fee pins)'}
            >
              <EditIcon className="h-3 w-3" />
              {rawEdit ? 'Done editing' : 'Edit raw'}
            </Button>
        </span>
      </div>

      {rawEdit ? (
        <IxPatternRowsEditor rows={draftPatterns} onChange={onChange} />
      ) : stagedCount === 0 ? (
        <EmptyState
          compact
          message={<>Nothing staged. Check rows in the ranked table below, or click trades in the preview.</>}
          action={
            <button
              type="button"
              className="text-[11px] font-semibold text-accent hover:underline"
              onClick={() => {
                onChange([...draftPatterns, { labels: [] }]);
                setRawEdit(true);
              }}
            >
              or add one manually
            </button>
          }
        />
      ) : (
        <ul className="flex max-h-64 flex-col gap-1.5 overflow-y-auto pr-1">
          {draftPatterns.map((p, i) => {
            const labels = p.labels.map((s) => s.trim()).filter(Boolean);
            if (labels.length === 0) return null;
            const pin = formatFeePins(p);
            return (
              <li key={i} className="flex items-center gap-2 rounded border border-white/8 bg-white/3 px-2 py-1.5">
                <span className="w-4 shrink-0 font-mono text-[9px] text-text-dim/60">{i + 1}</span>
                <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1">
                  {labels.map((label, k) => (
                    <Fragment key={k}>
                      {k > 0 && <span className="text-[10px] text-text-dim/40">›</span>}
                      <span className="rounded bg-white/6 px-1.5 py-0.5 font-mono text-[10px] text-text-mid">{label}</span>
                    </Fragment>
                  ))}
                  {rowIsCore(p) && (
                    <span className="shrink-0 rounded bg-accent/15 px-1 font-mono text-[9px] text-accent" title="ix core">
                      core
                    </span>
                  )}
                  {rowPinsFee(p) && (
                    <span
                      className="shrink-0 rounded bg-accent/15 px-1 font-mono text-[9px] text-accent"
                      title={`pinned to ${pin}: a trade must carry these exactly`}
                    >
                      {pin || 'fee'}
                    </span>
                  )}
                </div>
                <IconButton
                  variant="danger"
                  size="sm"
                  type="button"
                  onClick={() => onChange(draftPatterns.filter((_, j) => j !== i))}
                  title="Remove shape"
                  aria-label="Remove shape"
                >
                  <CloseIcon />
                </IconButton>
              </li>
            );
          })}
          {draftWallets.map((w) => (
            <li key={w} className="flex items-center gap-2 rounded border border-white/8 bg-white/3 px-2 py-1.5">
              <span className="shrink-0 rounded bg-white/6 px-1 font-mono text-[9px] text-text-dim">wallet</span>
              <span className="min-w-0 flex-1">
                <AddressDisplay address={w} kind="account" />
              </span>
              <IconButton
                variant="danger"
                size="sm"
                type="button"
                onClick={() => onWalletsChange(draftWallets.filter((x) => x !== w))}
                title="Remove wallet"
                aria-label="Remove wallet"
              >
                <CloseIcon />
              </IconButton>
            </li>
          ))}
        </ul>
      )}

      <p className="mt-2 rounded border border-white/8 bg-white/3 px-2 py-1.5 text-[11px] text-text-dim">
        After Apply: {tagSentence({ ...previewTag, id: '' })} Side is the tag&rsquo;s own; edit it on the
        fingerprint.
      </p>

      <Button
        variant="primary"
        size="md"
        className="mt-3 w-full"
        disabled={applyDisabled}
        onClick={onApply}
        title={applyLabel}
      >
        {applying ? (
          <SpinnerIcon className="h-4 w-4" />
        ) : targetFp ? (
          <CheckIcon className="h-4 w-4" />
        ) : (
          <LinkIcon className="h-4 w-4" />
        )}
        {applyLabel}
      </Button>

      {targetFp && Object.keys(targetFp.tags ?? {}).length > 0 && (
        <details className="mt-2 text-[10px] text-text-dim">
          <summary className="cursor-pointer">Saved tags</summary>
          <pre className="mt-1 overflow-x-auto rounded bg-black/20 p-2 font-mono">
            {JSON.stringify(targetFp.tags, null, 2)}
          </pre>
        </details>
      )}
    </div>
  );
}
