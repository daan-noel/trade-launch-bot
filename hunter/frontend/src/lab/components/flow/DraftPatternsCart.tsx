import { Fragment, useState } from 'react';

import { clearPrompt, IxPatternRowsEditor } from 'components/strategy/IxPatternsEditor';
import { LabelTip } from 'components/strategy/LabelTip';
import { tagSentence } from 'lib/strategy/tagsDoc';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { EmptyState } from 'components/ui/EmptyState';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { CheckIcon, CloseIcon, EditIcon, LinkIcon, SpinnerIcon, TrashIcon } from 'components/ui/icons';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import type { StageMatcher } from 'hooks/useIxPatternTarget';
import type { FlowTag } from 'lib/flow/classifyFlow';
import { flowTagOf, tagLabel } from 'lib/flow/tapeClassify';
import {
  formatFeePins,
  patternRowKey,
  rowPinsFee,
  serializeIxPatternRows,
  type IxPatternRow,
} from 'lib/strategy/ixPatternRows';
import { tagField, useStrategyRegistry } from 'lib/strategy/registry';
import { DISCOVERY_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { TAG_NAME_RE, withTagListValue, withTagShape } from 'lib/strategy/tagsDoc';
import { isProgramWorkingId, toggleWorkingTemplate } from 'lib/strategy/templateGrain';
import type { Fingerprint } from 'lib/strategy/types';

/** The matchers the cart stages: the exact ix shape, or the template vocabulary
 *  (grain ids under `ix_template`, bare program names under `program`). */
export type CartMatcher = Exclude<StageMatcher, 'wallet'>;

export const CART_MATCHERS: readonly CartMatcher[] = ['ix_shape', 'ix_template', 'program'];

/**
 * `doc` with tag `name`'s staged matcher(s) set to the draft - `rows` for `ix_shape`,
 * `ids` for `ix_template` (grains) and `program` (bare names) - every other matcher,
 * option and tag kept. Adds before it removes, so a tag whose list is swapped whole
 * never passes through "no matcher" and loses its options. Every step goes through
 * `withTagShape` / `withTagListValue`, the tags document's one writer.
 */
export function withTagDraft(
  doc: unknown,
  name: string,
  rows: readonly IxPatternRow[] | null,
  ids: readonly string[] | null,
): Record<string, unknown> {
  const saved = flowTagOf(doc, name);
  let out: unknown = doc;
  if (rows) {
    for (const r of rows) out = withTagShape(out, name, r);
    const keep = new Set(rows.map(patternRowKey));
    for (const r of saved?.match.ix_shape ?? []) {
      if (!keep.has(patternRowKey(r))) out = withTagShape(out, name, r, true);
    }
  }
  if (ids) {
    for (const k of ['ix_template', 'program'] as const) {
      const want = ids.filter((id) => (k === 'program') === isProgramWorkingId(id));
      for (const v of want) out = withTagListValue(out, name, k, v);
      for (const v of saved?.match[k] ?? []) {
        if (!want.includes(v)) out = withTagListValue(out, name, k, v, true);
      }
    }
  }
  return (out ?? {}) as Record<string, unknown>;
}

const NEW_TAG = '-new';

/** Staging "cart" for one fingerprint tag: an accent-elevated panel that reads as the
 *  page's deliverable. Checked rows from the ranked table land here as chips; Apply
 *  writes them into the named tag's matcher on the fingerprint. Raw JSON editing is
 *  one toggle away. */
export function DraftPatternsCart({
  draftPatterns,
  onChange,
  currentPatterns,
  draftWorking,
  onWorkingChange,
  currentWorking,
  targetFp,
  tagName,
  onTagNameChange,
  matcher,
  onMatcherChange,
  previewTag,
  applying,
  onApply,
}: {
  draftPatterns: IxPatternRow[];
  onChange: (patterns: IxPatternRow[]) => void;
  /** The tag's saved `ix_shape`. */
  currentPatterns: IxPatternRow[];
  /** Draft grain ids / program names. */
  draftWorking: string[];
  onWorkingChange: (ids: string[]) => void;
  /** The tag's saved `ix_template` + `program`. */
  currentWorking: string[];
  targetFp: Fingerprint | null;
  /** The tag Apply writes into (`@tagName`). */
  tagName: string;
  /** Switching reseeds the draft from that tag - the page owns that. */
  onTagNameChange: (name: string) => void;
  /** Which matcher a check writes. `ix_template` / `program` share one draft. */
  matcher: CartMatcher;
  onMatcherChange: (m: CartMatcher) => void;
  /** The tag as Apply would save it - saved options, draft matcher. */
  previewTag: FlowTag;
  applying: boolean;
  onApply: () => void;
}) {
  const [rawEdit, setRawEdit] = useState(false);
  const [newTag, setNewTag] = useState<string | null>(null);
  const { data: reg } = useStrategyRegistry();
  const title = (k: string) => tagField(reg, k)?.title ?? k;
  const builtin = reg?.tags.builtin.map((b) => b.name) ?? [];

  const isTemplate = matcher !== 'ix_shape';
  const draftNorm = serializeIxPatternRows(draftPatterns);
  const savedNorm = serializeIxPatternRows(currentPatterns);
  const stagedCount = isTemplate ? draftWorking.length : draftNorm.length;
  const dirty = isTemplate
    ? JSON.stringify([...draftWorking].sort()) !== JSON.stringify([...currentWorking].sort())
    : JSON.stringify(draftNorm) !== JSON.stringify(savedNorm);
  const tagNames = Object.keys(targetFp?.tags ?? {});
  const names = tagNames.includes(tagName) ? tagNames : [...tagNames, tagName];
  const writes = isTemplate
    ? `${tagLabel(tagName)} (${title('ix_template')} / ${title('program')})`
    : `${tagLabel(tagName)} (${title('ix_shape')})`;

  const applyLabel = applying
    ? 'Applying…'
    : isTemplate && !targetFp
      ? 'Pick a fingerprint to save templates'
      : targetFp
        ? `Save ${writes} on "${targetFp.name}"`
        : `Create & bind fingerprint with ${writes}`;

  const applyDisabled = applying || (isTemplate && !targetFp) || (!targetFp && stagedCount === 0);
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
              {names.map((n) => (
                <option key={n} value={n}>
                  {tagLabel(n)}
                  {tagNames.includes(n) ? '' : ' (new)'}
                </option>
              ))}
              <option value={NEW_TAG}>new tag...</option>
            </Select>
          )}
          <ToggleGroup
            size="sm"
            tone="neutral"
            aria-label="Which matcher a checked row writes"
            value={matcher}
            onChange={onMatcherChange}
            options={CART_MATCHERS.map((m) => ({
              value: m,
              label: title(m),
              title: tagField(reg, m)?.summary ?? m,
            }))}
          />
          <Badge variant={stagedCount > 0 ? 'accent' : 'neutral'} size="sm" pill>
            {stagedCount} staged
          </Badge>
          {targetFp && (
            <span className="text-[10px] text-text-dim">
              {isTemplate ? currentWorking.length : savedNorm.length} saved
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
                if (!window.confirm(isTemplate ? 'Clear all staged templates?' : clearPrompt(draftPatterns))) return;
                if (isTemplate) onWorkingChange([]);
                else onChange([]);
              }}
              title={isTemplate ? 'Delete all staged templates' : 'Delete all staged shapes'}
            >
              <TrashIcon className="h-3 w-3" />
              Delete all
            </Button>
          )}
          {!isTemplate && (
            <Button
              variant="link"
              size="xs"
              onClick={() => setRawEdit((v) => !v)}
              title={rawEdit ? 'Back to chip view' : 'Edit raw JSON rows (labels + optional fee pins)'}
            >
              <EditIcon className="h-3 w-3" />
              {rawEdit ? 'Done editing' : 'Edit raw'}
            </Button>
          )}
        </span>
      </div>

      {isTemplate ? (
        stagedCount === 0 ? (
          <EmptyState
            compact
            message={
              <>
                No templates staged. Check rows in the ranked table: each shape maps to its{' '}
                {matcher === 'program' ? 'program' : 'program|CU|ATA|N|S|F grain'}.
              </>
            }
          />
        ) : (
          <div className="flex flex-wrap gap-1">
            {draftWorking.map((id) => (
              <button
                key={id}
                type="button"
                onClick={() => onWorkingChange(toggleWorkingTemplate(draftWorking, id))}
                className="inline-flex items-center gap-1 rounded border border-green/40 bg-green/10 px-1.5 py-0.5 font-mono text-[10px] text-text-hi hover:border-red/50 hover:bg-red/10"
                title={`Remove from the draft (${title(isProgramWorkingId(id) ? 'program' : 'ix_template')})`}
              >
                {id}
                <span aria-hidden className="text-text-dim/60">
                  ×
                </span>
              </button>
            ))}
          </div>
        )
      ) : rawEdit ? (
        <IxPatternRowsEditor rows={draftPatterns} onChange={onChange} />
      ) : stagedCount === 0 ? (
        <EmptyState
          compact
          message={<>No shapes staged. Check rows in the ranked table below.</>}
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
        </ul>
      )}

      <p className="mt-2 rounded border border-white/8 bg-white/3 px-2 py-1.5 text-[11px] text-text-dim">
        After Apply: {tagSentence({ ...previewTag, id: '' })} Side, sticky and the creation-slot option are the
        tag&rsquo;s own; edit them on the fingerprint.
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
