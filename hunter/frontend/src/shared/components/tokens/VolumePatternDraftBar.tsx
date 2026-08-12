import { useMemo, useState } from 'react';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Select } from 'components/ui/Select';
import {
  useVolumePatternDraft,
  useVolumePatternLocked,
} from 'context/VolumePatternDraftContext';
import { patternKey, cleanPatterns } from 'lib/flow/volumePatterns';
import {
  metricConfigWithVolumePatterns,
  volumeIxPatternsFromConfig,
} from 'lib/strategy/registry';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetFingerprintsQuery,
  useGetStrategyRulesQuery,
  useUpdateFingerprintMutation,
} from 'store/sharedEndpoints';
import type { Fingerprint } from 'lib/strategy/types';

/** Same-membership test on two pattern sets (order carries no meaning here —
 *  order matters INSIDE a pattern, which `patternKey` already preserves). */
function samePatternSet(
  a: readonly (readonly string[])[],
  b: readonly (readonly string[])[],
): boolean {
  if (a.length !== b.length) return false;
  const bKeys = new Set(b.map(patternKey));
  return a.every((p) => bKeys.has(patternKey(p)));
}

/**
 * The staging strip above a chart's trades table: open a `volume_ix_patterns`
 * draft, see how it differs from what was saved, and persist it to one
 * fingerprint.
 *
 * Saving is deliberately a separate, explicitly-targeted act. `metric_config` is
 * NOT part of fingerprint identity, so a write does not fork the row — it lands
 * on the same id, and every rule bound to it silently starts classifying flow
 * differently. Hence: no implicit save, an explicit fingerprint pick, and a loud
 * count of the ACTIVE rules a write would change under.
 */
export function VolumePatternDraftBar({
  savedKeys,
}: {
  /** The host's own saved keys — what a fresh draft is seeded from. */
  savedKeys: ReadonlySet<string> | null;
}) {
  const draft = useVolumePatternDraft();
  const locked = useVolumePatternLocked();
  const editing = draft.patterns != null;

  const { data: fingerprints = [] } = useGetFingerprintsQuery(undefined, { skip: !editing });
  const { data: rules = [] } = useGetStrategyRulesQuery(undefined, { skip: !editing });
  const [updateFingerprint, { isLoading: saving }] = useUpdateFingerprintMutation();

  // Preselect the fingerprint the draft was seeded FROM, when exactly one row
  // carries that pattern set — otherwise the target is ambiguous and the user
  // picks. Never guess into a write that changes live rules.
  const seededMatches = useMemo(() => {
    if (!draft.base) return [];
    return fingerprints.filter((f) =>
      samePatternSet(volumeIxPatternsFromConfig(f.metric_config), draft.base ?? []),
    );
  }, [fingerprints, draft.base]);

  const [pickedId, setPickedId] = useState<string | null>(null);
  const targetId =
    pickedId ?? (seededMatches.length === 1 ? seededMatches[0].id : null);
  const target = fingerprints.find((f) => f.id === targetId) ?? null;

  const activeRuleCount = useMemo(
    () =>
      targetId
        ? rules.filter((r) => r.fingerprint_id === targetId && r.is_active).length
        : 0,
    [rules, targetId],
  );

  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  if (locked) {
    return (
      <span
        className="text-[10px] uppercase tracking-wide text-text-dim/60"
        title="This chart shows a stored run's own volume_ix_patterns — the numbers were computed under them, so they are not editable here."
      >
        run snapshot
      </span>
    );
  }

  if (!editing) {
    return (
      <Button
        variant="link"
        size="xs"
        onClick={() => draft.open(savedKeys)}
        title="Stage volume_ix_patterns by clicking the Vol badge on any trade row — every chart redraws against the draft until you discard it."
      >
        Edit volume patterns
      </Button>
    );
  }

  const count = draft.patterns?.length ?? 0;
  const { added, removed, dirty } = draft.diff;

  async function handleSave(fp: Fingerprint) {
    setError(null);
    setSaved(null);
    try {
      await updateFingerprint({
        id: fp.id,
        body: {
          name: fp.name,
          cu_limit: fp.cu_limit,
          cu_price: fp.cu_price,
          init_buy_lamports: fp.init_buy_lamports,
          max_cost_lamports: fp.max_cost_lamports,
          spendable_lamports_in: fp.spendable_lamports_in,
          first_slot_buy_lamports: fp.first_slot_buy_lamports,
          first_slot_sell_lamports: fp.first_slot_sell_lamports,
          bucket_size_amount: fp.bucket_size_amount,
          ix_labels: fp.ix_labels,
          metric_config: metricConfigWithVolumePatterns(cleanPatterns(draft.patterns ?? [])),
        },
      }).unwrap();
      draft.commit();
      setSaved(`Saved to “${fp.name}”.`);
    } catch (e) {
      setError(apiErrorMessage(e as never, 'Failed to save volume patterns'));
    }
  }

  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <Badge variant={dirty ? 'warning' : 'info'} size="sm">
        Pattern draft
      </Badge>
      <span className="font-mono text-[11px] text-text-dim">
        {count} pattern{count === 1 ? '' : 's'}
        {dirty && ` · +${added} / −${removed} vs saved`}
      </span>

      <Button
        variant="link"
        size="xs"
        onClick={draft.reset}
        disabled={!dirty}
        title="Back to the patterns this draft was seeded from"
      >
        Reset
      </Button>
      <Button
        variant="link"
        size="xs"
        onClick={draft.close}
        title="Discard the draft — every chart goes back to its own saved patterns"
      >
        Discard
      </Button>

      <Select
        fieldSize="sm"
        value={targetId ?? ''}
        onChange={(e) => {
          setPickedId(e.target.value || null);
          setSaved(null);
          setError(null);
        }}
        title="Fingerprint this draft would be written to"
        className="max-w-[16rem]"
      >
        <option value="">Save to…</option>
        {fingerprints.map((f) => (
          <option key={f.id} value={f.id}>
            {f.name}
          </option>
        ))}
      </Select>
      <Button
        variant="primary"
        size="xs"
        disabled={!target || saving || !dirty}
        onClick={() => target && handleSave(target)}
      >
        {saving ? 'Saving…' : 'Save'}
      </Button>

      {activeRuleCount > 0 && (
        <span className="text-[11px] text-warning">
          {activeRuleCount} active rule{activeRuleCount === 1 ? '' : 's'} use this
          fingerprint — saving changes how they classify flow.
        </span>
      )}
      {saved && <span className="text-[11px] text-green">{saved}</span>}
      {error && <span className="text-[11px] text-red">{error}</span>}
    </span>
  );
}
