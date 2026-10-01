import { useState } from 'react';

import { FeePinToggles } from 'components/tokens/FeePinToggles';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import { HOST_SHAPES_TAG, type IxPatternTarget, type StageMatcher, type TagStage } from 'hooks/useIxPatternTarget';
import { tagLabel } from 'lib/flow/tapeClassify';
import { tagField, useStrategyRegistry, type StrategyRegistry } from 'lib/strategy/registry';
import { TAG_NAME_RE } from 'lib/strategy/tagsDoc';

/** A matcher's registry title and summary, for a switch or a caption. */
function matcherTitle(reg: StrategyRegistry | undefined, m: StageMatcher): string {
  return tagField(reg, m)?.title ?? m;
}

/**
 * The "add to @tag (matcher)" caption, the matcher switch and the fee pins - the part
 * every stage shares, whoever owns the tag. The caption states exactly what a badge
 * click below writes.
 */
export function TagStageControls({ stage, disabled = false }: { stage: TagStage; disabled?: boolean }) {
  const { data: reg } = useStrategyRegistry();
  const options = stage.matchers.map((m) => ({
    value: m,
    label: matcherTitle(reg, m),
    title: tagField(reg, m)?.summary ?? m,
  }));
  return (
    <>
      <span className="text-[11px] text-text">
        click adds to <span className="font-mono">{tagLabel(stage.tagName)}</span> ({matcherTitle(reg, stage.matcher)})
      </span>
      {stage.matchers.length > 1 && (
        <ToggleGroup
          size="sm"
          tone="neutral"
          aria-label="Which matcher a badge click writes"
          value={stage.matcher}
          onChange={stage.setMatcher}
          options={options}
        />
      )}
      {stage.matcher === 'ix_shape' && (
        <FeePinToggles mask={stage.feePins} onChange={stage.setFeePins} disabled={disabled} />
      )}
      {stage.saving && <span className="text-[11px] text-text-dim">Saving...</span>}
      {stage.error && <span className="text-[11px] text-red">{stage.error}</span>}
    </>
  );
}

/** Said wherever the lines read the host's bare key set instead of a fingerprint tag. */
const HOST_SHAPES_NOTE = `${tagLabel(HOST_SHAPES_TAG)} = the host's exact ix shapes only, not a fingerprint tag (no program, marker, wallet, creator, cluster, side or sticky).`;

/** Not a legal tag name, so it cannot collide with one. */
const NEW_TAG = '-new';

/** The target's tags plus "new tag...", which opens a name field. */
function TagPicker({ target }: { target: IxPatternTarget }) {
  const { data: reg } = useStrategyRegistry();
  const [draft, setDraft] = useState<string | null>(null);
  const builtin = reg?.tags.builtin.map((b) => b.name) ?? [];
  if (draft !== null) {
    const ok = TAG_NAME_RE.test(draft) && !builtin.includes(draft);
    return (
      <span className="inline-flex items-center gap-1">
        <Input
          fieldSize="sm"
          className="w-28 font-mono"
          value={draft}
          autoFocus
          placeholder="tag name"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && ok) {
              target.setTagName(draft);
              setDraft(null);
            } else if (e.key === 'Escape') setDraft(null);
          }}
        />
        <span className={`text-[10px] ${ok ? 'text-text-dim' : 'text-red'}`}>
          {ok ? 'Enter: the first click creates it' : 'a-z, 0-9, _ (1 to 24), not a built-in class'}
        </span>
      </span>
    );
  }
  const names = target.tagNames.includes(target.tagName) ? target.tagNames : [...target.tagNames, target.tagName];
  return (
    <Select
      fieldSize="sm"
      value={target.tagName}
      onChange={(e) => (e.target.value === NEW_TAG ? setDraft('') : target.setTagName(e.target.value))}
      title="The tag a click writes and the chart classifies with"
      className="max-w-40 font-mono"
    >
      {names.map((n) => (
        <option key={n} value={n}>
          {tagLabel(n)}
          {target.tagNames.includes(n) ? '' : ' (new)'}
        </option>
      ))}
      <option value={NEW_TAG}>new tag...</option>
    </Select>
  );
}

/**
 * The strip above a chart's trades table: which fingerprint and which tag the badges
 * write to, with which matcher, and what a click there costs.
 *
 * There is no staging step. A click saves the fingerprint's `tags`, and every rule
 * bound to it reads the new tag - hence the named target and the loud active-rule
 * count. The target is normally the host's own fingerprint; the two ways it can be
 * something else - guessed from a key set, or picked away from the host - are both
 * called out, because from the badge alone they look like editing the row on screen.
 */
export function IxPatternBar({
  target,
  readOnly = false,
}: {
  target: IxPatternTarget;
  /** A stored run's frozen snapshot: its numbers were computed under those shapes,
   *  so they are not this chart's to change. */
  readOnly?: boolean;
}) {
  if (readOnly) {
    return (
      <span
        className="text-[10px] uppercase tracking-wide text-text-dim/60"
        title={`This chart shows a stored run's own ix shapes: the numbers were computed under them, so they are not editable here. ${HOST_SHAPES_NOTE}`}
      >
        run snapshot
      </span>
    );
  }

  const { fingerprints, targetId, setTargetId, activeRuleCount, inferred, offHost } = target;
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <Select
        fieldSize="sm"
        value={targetId ?? ''}
        onChange={(e) => setTargetId(e.target.value || null)}
        title="Fingerprint the badges write to"
        className="w-96 max-w-full"
      >
        <option value="">Fingerprint...</option>
        {fingerprints.map((f) => (
          <option key={f.id} value={f.id}>
            {f.name}
          </option>
        ))}
      </Select>
      {targetId ? (
        <>
          <TagPicker target={target} />
          <TagStageControls stage={target} />
        </>
      ) : (
        <span className="text-[11px] text-text-dim">
          {target.tag ? `${HOST_SHAPES_NOTE} ` : ''}Pick a fingerprint to make the badges add to one of its tags.
        </span>
      )}
      {inferred && (
        <span
          className="text-[11px] text-text-dim"
          title="This host has no fingerprint of its own, so the target was matched by its ix shapes. Confirm it before editing."
        >
          matched by shapes: confirm before editing
        </span>
      )}
      {offHost && (
        <span className="text-[11px] text-warning">
          Not this chart&rsquo;s fingerprint: badges and lines follow the picked one.
        </span>
      )}
      {activeRuleCount > 0 && (
        <span className="text-[11px] text-warning">
          {activeRuleCount} active rule{activeRuleCount === 1 ? '' : 's'} use this fingerprint: a click changes
          what they read.
        </span>
      )}
    </span>
  );
}
