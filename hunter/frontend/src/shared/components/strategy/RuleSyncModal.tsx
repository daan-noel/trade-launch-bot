import { useCallback, useMemo, useState } from 'react';

import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Modal } from 'components/ui/Modal';
import { Input, Textarea } from 'components/ui/Input';
import { SearchIcon } from 'components/ui/icons';
import { RuleModeFilter } from './RuleModeFilter';
import { RuleTagFilter } from './RuleTagFilter';
import { EMPTY_TAG_FILTER, matchesTagFilter, type TagFilterState } from 'lib/strategy/tags';
import { matchesModeFilter, type ModeFilter } from 'lib/strategy/mode';
import { formatDurationShort } from 'utils/format';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useApplyStrategyBundleMutation,
  useLazyExportStrategyBundleQuery,
  usePreviewStrategyBundleMutation,
  refetchAllRuleLists,
} from 'store/sharedEndpoints';
import { useDispatch } from 'react-redux';
import type {
  BundleApplied,
  BundleFieldChange,
  BundleItemStatus,
  BundlePlan,
  StrategyRule,
} from 'lib/strategy/types';

/**
 * Copy rules between this box and the other one (workstation lab <-> EC2 live).
 *
 * Two panes, because the operation has two halves and only one of them is
 * dangerous. **Copy** serializes the selected rules plus the fingerprints they
 * need. **Paste** previews first: what a bundle would change on THIS box, field by
 * field, before anything is written. That preview is the whole point — a paste you
 * cannot read first fails exactly the way the hand-written SQL it replaces does.
 *
 * What travels and what does not is the backend's contract
 * (`trading_core::api::handlers::strategies::rule_bundle`), not this component's:
 * arming, enabled and trade_mode stay with the box, so an imported rule always
 * lands idle and paper and has to be promoted here deliberately.
 */
export interface RuleSyncModalProps {
  open: boolean;
  onClose: () => void;
  /** Rules currently on the board — the Copy pane's selection source. */
  rules: StrategyRule[];
  /** Pre-selected rule ids (the row you opened this from). */
  initialSelection?: string[];
}

const STATUS_LABEL: Record<BundleItemStatus, string> = {
  identical: 'identical',
  changed: 'changed',
  new: 'new',
  reuse_existing: 'reuse existing',
  duplicate: 'duplicate',
  conflict: 'conflict',
};

const STATUS_VARIANT: Record<BundleItemStatus, BadgeVariant> = {
  identical: 'neutral',
  changed: 'warning',
  new: 'success',
  reuse_existing: 'info',
  duplicate: 'neutral',
  conflict: 'danger',
};

/** Pane heading. `SectionDivider` is a bare rule with no slot for a label, and
 *  every heading here is a one-line hint the operator reads before acting. */
function Heading({ children }: { children: React.ReactNode }) {
  return (
    <div className="mb-1.5 border-b border-white/8 pb-1 text-[11px] font-semibold uppercase tracking-wider text-text-dim">
      {children}
    </div>
  );
}

/** Compact one-line rendering of a JSON value for the diff rows. Long values are
 *  clipped rather than wrapped: a `params` blob would otherwise push every other
 *  row off the screen, and the point of the row is *that* the field changed. */
function preview(v: unknown): string {
  if (v === null || v === undefined) return '—';
  const s = typeof v === 'string' ? v : JSON.stringify(v);
  return s.length > 220 ? `${s.slice(0, 220)}…` : s;
}

function ChangeRow({ c }: { c: BundleFieldChange }) {
  return (
    <div className="grid grid-cols-[7rem_1fr] gap-x-2 py-0.5 text-[11px]">
      <span className="font-semibold text-text-dim">{c.field}</span>
      <span className="min-w-0">
        <span className="block break-all text-red/80">- {preview(c.from)}</span>
        <span className="block break-all text-green/90">+ {preview(c.to)}</span>
      </span>
    </div>
  );
}

function PlanItem({
  title,
  status,
  note,
  changes,
  localUpdatedAt,
  incomingUpdatedAt,
}: {
  title: string;
  status: BundleItemStatus;
  note: string | null;
  changes: BundleFieldChange[];
  localUpdatedAt: string | null;
  incomingUpdatedAt: string;
}) {
  // Which side is older is the one thing a diff cannot tell you and you always want
  // to know: overwriting a row this box edited more recently is usually the mistake.
  const stale = localUpdatedAt !== null && localUpdatedAt > incomingUpdatedAt;
  return (
    <div className="rounded-md border border-white/8 bg-white/2 px-2.5 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant={STATUS_VARIANT[status]} size="sm">
          {STATUS_LABEL[status]}
        </Badge>
        <span className="text-[12px] font-semibold text-text">{title}</span>
        {stale && (
          <Badge variant="warning" size="sm" title="this box's row is newer than the bundle's">
            yours is newer
          </Badge>
        )}
      </div>
      {note && <div className="mt-1 text-[11px] text-text-dim">{note}</div>}
      {changes.length > 0 && <div className="mt-1.5">{changes.map((c) => <ChangeRow key={c.field} c={c} />)}</div>}
    </div>
  );
}

/** "3h 12m ago", reusing the ONE duration formatter. `null` on an unparseable
 *  stamp rather than a bogus span. */
function editedAgo(iso: string): string | null {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return null;
  return `${formatDurationShort((Date.now() - t) / 1000)} ago`;
}

/** How a rule on THIS box stands against the pasted bundle. `unknown` = no
 *  preview yet; `absent` = the bundle does not mention it at all. */
type SyncStand = 'unknown' | 'differs' | 'in-sync' | 'absent';

const STAND_BADGE: Record<Exclude<SyncStand, 'unknown'>, { label: string; variant: BadgeVariant }> = {
  differs: { label: 'differs', variant: 'warning' },
  'in-sync': { label: 'in sync', variant: 'success' },
  absent: { label: 'not in bundle', variant: 'neutral' },
};

export function RuleSyncModal({ open, onClose, rules, initialSelection }: RuleSyncModalProps) {
  const dispatch = useDispatch();
  const [selected, setSelected] = useState<Set<string>>(() => new Set(initialSelection ?? []));
  const [exported, setExported] = useState('');
  const [pasted, setPasted] = useState('');
  const [plan, setPlan] = useState<BundlePlan | null>(null);
  const [applied, setApplied] = useState<BundleApplied | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  // Picker filters are LOCAL state, deliberately not `useTagFilter`/`useModeFilter`:
  // those sync to `?tags=` / `?mode=`, which the Rules board behind this modal already
  // owns — sharing them would make narrowing the picker silently re-filter the board.
  const [query, setQuery] = useState('');
  const [modeFilter, setModeFilter] = useState<ModeFilter>('all');
  const [tagFilter, setTagFilter] = useState<TagFilterState>(EMPTY_TAG_FILTER);
  const [showDisabled, setShowDisabled] = useState(false);
  const [diffOnly, setDiffOnly] = useState(false);

  const [runExport, exportState] = useLazyExportStrategyBundleQuery();
  const [runPreview, previewState] = usePreviewStrategyBundleMutation();
  const [runApply, applyState] = useApplyStrategyBundleMutation();

  // Fingerprint names are searchable because a rule is often known by the shape it
  // arms on rather than by its own label.
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const fpNameById = useMemo(
    () => new Map(fingerprints.map((f) => [f.id, f.name.toLowerCase()])),
    [fingerprints],
  );

  /** Where each of MY rules stands against the pasted bundle, once previewed. */
  const standById = useMemo(() => {
    const out = new Map<string, SyncStand>();
    if (!plan) return out;
    for (const r of plan.rules) {
      // `new` means the bundle has a rule this box lacks — it cannot appear in this
      // list at all, so only the other two statuses map onto a row here.
      out.set(r.id, r.status === 'identical' ? 'in-sync' : 'differs');
    }
    return out;
  }, [plan]);

  const standOf = useCallback(
    (id: string): SyncStand => (plan ? (standById.get(id) ?? 'absent') : 'unknown'),
    [plan, standById],
  );

  // Everything except the tag filter, so the tag chips' counts don't collapse the
  // moment one is clicked (the contract `RuleTagFilter` documents).
  const preTag = useMemo(
    () => (showDisabled ? rules : rules.filter((r) => r.is_enabled)),
    [rules, showDisabled],
  );

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return preTag
      .filter((r) => matchesModeFilter(r.trade_mode, modeFilter))
      .filter((r) => matchesTagFilter(r.tags, tagFilter))
      .filter((r) => !diffOnly || standOf(r.id) !== 'in-sync')
      .filter(
        (r) =>
          !q ||
          r.rule_name.toLowerCase().includes(q) ||
          r.tags?.some((t) => t.includes(q)) ||
          (fpNameById.get(r.fingerprint_id) ?? '').includes(q),
      )
      // Newest edit first: the reason this modal is open is that something just
      // changed, so the rule you want is the one you touched last.
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
  }, [preTag, query, modeFilter, tagFilter, diffOnly, standOf, fpNameById]);

  /** Fingerprints the current selection drags along — the same derivation the
   *  backend does, surfaced because it is otherwise invisible. */
  const selectedFpCount = useMemo(() => {
    const source = selected.size ? rules.filter((r) => selected.has(r.id)) : rules;
    return new Set(source.map((r) => r.fingerprint_id)).size;
  }, [rules, selected]);

  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const allVisibleSelected = visible.length > 0 && visible.every((r) => selected.has(r.id));
  const selectAllVisible = () =>
    setSelected((prev) => {
      const next = new Set(prev);
      // One button, both ways: with every visible row already picked the only useful
      // action is to drop them, and a separate "deselect" button would be dead half
      // the time.
      for (const r of visible) {
        if (allVisibleSelected) next.delete(r.id);
        else next.add(r.id);
      }
      return next;
    });

  const doExport = useCallback(async () => {
    setErr(null);
    setCopied(false);
    try {
      const bundle = await runExport(selected.size ? [...selected] : undefined).unwrap();
      const text = JSON.stringify(bundle, null, 2);
      setExported(text);
      // Clipboard access is denied outside a secure context (plain-http LAN), so the
      // textarea below stays the real deliverable and this is the convenience.
      try {
        await navigator.clipboard.writeText(text);
        setCopied(true);
      } catch {
        setCopied(false);
      }
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'export failed');
    }
  }, [runExport, selected]);

  /** Parse locally first: a malformed paste is the operator's typo, not a round trip. */
  const parsePasted = (): unknown | null => {
    try {
      return JSON.parse(pasted);
    } catch {
      setErr('that is not valid JSON — paste the whole block, braces included');
      return null;
    }
  };

  const doPreview = useCallback(async () => {
    setErr(null);
    setApplied(null);
    setPlan(null);
    const body = parsePasted();
    if (body === null) return;
    try {
      setPlan(await runPreview(body).unwrap());
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'preview failed');
    }
    // `parsePasted` reads `pasted` and sets `err`; both are covered by the deps below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runPreview, pasted]);

  const doApply = useCallback(async () => {
    setErr(null);
    const body = parsePasted();
    if (body === null) return;
    try {
      const result = await runApply(body).unwrap();
      setApplied(result);
      setPlan(result.plan);
      // The rules board is served by several list queries (score scopes, modes); the
      // shared refetch is the one that covers them all.
      refetchAllRuleLists(dispatch);
    } catch (e) {
      // A blocked apply comes back 409 with the re-planned diff, so show that rather
      // than a bare message — the conflict is the information.
      const data = (e as { data?: { plan?: BundlePlan } })?.data;
      if (data?.plan) setPlan(data.plan);
      setErr(apiErrorMessage(e as never) ?? 'apply failed');
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runApply, pasted, dispatch]);

  const busy = exportState.isFetching || previewState.isLoading || applyState.isLoading;
  const canApply = !!plan && !plan.blocked && plan.writes > 0 && !applyState.isLoading;

  return (
    <Modal title="Sync rules between boxes" open={open} onClose={onClose} size="xl">
      <div className="space-y-4">
        {/* ── Copy ─────────────────────────────────────────────── */}
        <section>
          <Heading>Copy from this box</Heading>
          <p className="mb-2 text-[11px] text-text-dim">
            Pick the rules to send. The fingerprints they use come along automatically —
            selecting none exports every rule on this box.
          </p>

          {/* Filters. Each narrows the list only; none of them changes what a
              selected rule exports. */}
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <span className="relative">
              <SearchIcon className="pointer-events-none absolute left-2 top-1/2 size-3 -translate-y-1/2 text-text-dim" />
              <Input
                fieldSize="sm"
                className="w-56 pl-7"
                placeholder="Search name, tag, or fingerprint"
                value={query}
                onChange={(e) => setQuery(e.currentTarget.value)}
              />
            </span>
            <RuleModeFilter rules={preTag} value={modeFilter} onChange={setModeFilter} />
            <Button
              variant="ghost"
              size="xs"
              active={showDisabled}
              onClick={() => setShowDisabled((v) => !v)}
              title="Disabled rules are archived — hidden here by default, as on the board"
            >
              Disabled
            </Button>
            {plan && (
              <Button
                variant="ghost"
                size="xs"
                active={diffOnly}
                onClick={() => setDiffOnly((v) => !v)}
                title="Hide rules the pasted bundle already matches"
              >
                Differs only
              </Button>
            )}
            <div className="grow" />
            <Button variant="link" size="xs" disabled={!visible.length} onClick={selectAllVisible}>
              {allVisibleSelected ? 'Clear visible' : `Select ${visible.length} visible`}
            </Button>
          </div>
          <RuleTagFilter rules={preTag} filter={tagFilter} onChange={setTagFilter} />

          <div className="mt-2 max-h-44 overflow-y-auto rounded-md border border-white/8">
            {visible.length === 0 && (
              <div className="px-2.5 py-3 text-[11px] text-text-dim">
                No rule matches these filters.
              </div>
            )}
            {visible.map((r) => {
              const stand = standOf(r.id);
              const edited = editedAgo(r.updated_at);
              return (
                <label
                  key={r.id}
                  className="flex cursor-pointer items-center gap-2 border-b border-white/5 px-2.5 py-1 text-[12px] last:border-b-0 hover:bg-white/4"
                >
                  <input
                    type="checkbox"
                    checked={selected.has(r.id)}
                    onChange={() => toggle(r.id)}
                  />
                  <span className="truncate">{r.rule_name}</span>
                  <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'} size="sm">
                    {r.trade_mode}
                  </Badge>
                  {!r.is_enabled && (
                    <Badge variant="neutral" size="sm">
                      disabled
                    </Badge>
                  )}
                  {stand !== 'unknown' && (
                    <Badge variant={STAND_BADGE[stand].variant} size="sm">
                      {STAND_BADGE[stand].label}
                    </Badge>
                  )}
                  <span className="grow" />
                  {/* The sort key, shown — an ordering you cannot see reads as random. */}
                  {edited && (
                    <span className="shrink-0 text-[10px] tabular-nums text-text-dim">{edited}</span>
                  )}
                </label>
              );
            })}
          </div>

          <div className="mt-2 flex flex-wrap items-center gap-2">
            <Button variant="primary" size="sm" disabled={busy} onClick={() => void doExport()}>
              {selected.size ? `Copy ${selected.size} rule(s)` : 'Copy all rules'}
            </Button>
            <span className="text-[11px] text-text-dim">
              {selected.size || rules.length} rule(s) · {selectedFpCount} fingerprint(s)
            </span>
            {copied && <span className="text-[11px] text-green">copied to clipboard</span>}
          </div>
          {diffOnly && (
            <p className="mt-1 text-[10px] text-text-dim">
              &ldquo;not in bundle&rdquo; means the pasted bundle never mentions that rule — which
              is only &ldquo;the other box lacks it&rdquo; if that bundle was a full export.
            </p>
          )}
          {exported && (
            <Textarea
              className="mt-2 w-full font-mono"
              rows={5}
              readOnly
              value={exported}
              onFocus={(e) => e.currentTarget.select()}
            />
          )}
        </section>

        {/* ── Paste ────────────────────────────────────────────── */}
        <section>
          <Heading>Paste from the other box</Heading>
          <Textarea
            className="w-full font-mono"
            rows={5}
            placeholder="paste a bundle here, then Preview"
            value={pasted}
            onChange={(e) => setPasted(e.currentTarget.value)}
          />
          <div className="mt-2 flex items-center gap-2">
            <Button variant="ghost" size="sm" disabled={!pasted.trim() || busy} onClick={() => void doPreview()}>
              Preview changes
            </Button>
            <Button variant="primary" size="sm" disabled={!canApply} onClick={() => void doApply()}>
              {plan ? `Apply ${plan.writes} change(s)` : 'Apply'}
            </Button>
            <span className="text-[11px] text-text-dim">
              Preview writes nothing. Imported rules land idle and paper.
            </span>
          </div>
        </section>

        {err && (
          <div className="rounded-md border border-red/40 bg-red/10 px-2.5 py-2 text-[12px] text-red">
            {err}
          </div>
        )}

        {applied && (
          <div className="rounded-md border border-green/40 bg-green/10 px-2.5 py-2 text-[12px] text-green">
            Applied — fingerprints {applied.fingerprints_inserted} new /{' '}
            {applied.fingerprints_updated} updated, rules {applied.rules_inserted} new /{' '}
            {applied.rules_updated} updated, {applied.skipped} unchanged.
          </div>
        )}

        {plan && (
          <section className="space-y-2">
            <Heading>
              {plan.blocked
                ? 'Blocked — nothing was written'
                : plan.writes === 0
                  ? 'Nothing to do — this box already matches'
                  : `${plan.writes} change(s) to apply`}
            </Heading>
            {plan.blockers.length > 0 && (
              <ul className="list-disc rounded-md border border-red/40 bg-red/10 py-1.5 pl-6 pr-2 text-[11px] text-red">
                {plan.blockers.map((b) => (
                  <li key={b}>{b}</li>
                ))}
              </ul>
            )}
            {plan.fingerprints.map((f) => (
              <PlanItem
                key={f.id}
                title={`fingerprint · ${f.name}`}
                status={f.status}
                note={f.note}
                changes={f.changes}
                localUpdatedAt={f.local_updated_at}
                incomingUpdatedAt={f.incoming_updated_at}
              />
            ))}
            {plan.rules.map((r) => (
              <PlanItem
                key={r.id}
                title={`rule · ${r.rule_name}`}
                status={r.status}
                note={r.note}
                changes={r.changes}
                localUpdatedAt={r.local_updated_at}
                incomingUpdatedAt={r.incoming_updated_at}
              />
            ))}
          </section>
        )}
      </div>
    </Modal>
  );
}
