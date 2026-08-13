// Shared "scope by saved fingerprint" block — the searchable picker + the
// matched-fingerprint info panel (axis chips + a link to the fingerprint) or
// the manual-mode hint when nothing's picked. Used by Flow Discovery, Grouped
// Sweep, and the Creation Stats dashboard so all three read identically; only
// the help copy differs per page (what "scoped" means there), passed in by
// the caller rather than hard-coded here.

import type { ReactNode } from 'react';
import { useMemo } from 'react';
import { Link } from 'react-router-dom';
import { Badge } from 'components/ui/Badge';
import { LinkIcon } from 'components/ui/icons';
import { formatWithCommas } from 'utils/format';
import { SearchableSelect, type SearchableSelectOption } from 'components/ui/SearchableSelect';
import { LabelTip } from './LabelTip';
import {
  FingerprintOptionBody,
  fingerprintParamsCell,
  fingerprintParamsSearchText,
  fingerprintSelectLabel,
} from './FingerprintParamsSummary';
import { fingerprintsHref } from 'lib/strategy/nav';
import type { Fingerprint } from 'lib/strategy/types';
import type { HelpTip } from 'lib/strategy/strategyHelp';

export interface FingerprintScopeControlProps {
  fingerprints: Fingerprint[];
  /** Selected fingerprint id, or `null` for "manual" (no scope). */
  value: string | null;
  /** `''` when the user clears back to manual. */
  onChange: (id: string) => void;
  /** `LabelTip` help content for the field label. */
  tip: HelpTip;
  label?: string;
  emptyOptionLabel?: string;
  /** One-liner shown next to the matched-fingerprint badge — what "scoped"
   *  concretely does on this page (e.g. "…are swept" / "…are analyzed"). */
  scopedDescription: ReactNode;
  /** Shown instead of the badge panel when nothing is selected. */
  manualHint: ReactNode;
  /** Badge caption prefix. Defaults to `"engine match"`. */
  badgeLabel?: string;
  /** Number of tokens the selected fingerprint matches, or `null` when unknown /
   *  still loading. Rendered as a chip next to the badge. The matched-tokens
   *  affordance (chip + "View matches") only appears when `onViewMatches` is
   *  passed — the shared control stays presentational; a `@lab` caller (see
   *  `useFingerprintMatches`) owns the query + modal. */
  matchedCount?: number | null;
  /** Show a "counting…" placeholder in the chip while the count loads. */
  matchedCountLoading?: boolean;
  /** Opens the caller's matched-tokens view. Presence of this callback is what
   *  enables the count chip + "View matches" button. */
  onViewMatches?: () => void;
  /** Start the lazy match-count fetch (hover the chip / focus the affordance). */
  onRequestMatchCount?: () => void;
  /** Look-back window (days) the match count/list covers — labels the chip so a
   *  window-scoped count doesn't read as all-time. Defaults to 30. */
  matchWindowDays?: number;
}

export function FingerprintScopeControl({
  fingerprints,
  value,
  onChange,
  tip,
  label = 'Scope by saved fingerprint',
  emptyOptionLabel = 'Manual group-by / filters below',
  scopedDescription,
  manualHint,
  badgeLabel = 'engine match',
  matchedCount,
  matchedCountLoading = false,
  onViewMatches,
  onRequestMatchCount,
  matchWindowDays = 30,
}: FingerprintScopeControlProps) {
  const byId = useMemo(() => {
    const map = new Map<string, Fingerprint>();
    for (const f of fingerprints) map.set(f.id, f);
    return map;
  }, [fingerprints]);
  const selected = (value && byId.get(value)) || null;

  const options: SearchableSelectOption<Fingerprint>[] = useMemo(
    () =>
      fingerprints.map((f) => ({
        value: f.id,
        label: fingerprintSelectLabel(f),
        searchText: fingerprintParamsSearchText(f),
        data: f,
      })),
    [fingerprints],
  );

  return (
    <div className="mb-3 flex flex-col gap-2 rounded border border-white/8 p-3">
      <label className="flex min-w-[16rem] flex-col gap-1 text-[11px] text-text-dim">
        <LabelTip
          tip={tip}
          className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
        >
          {label}
        </LabelTip>
        <SearchableSelect
          options={options}
          value={value}
          onChange={onChange}
          placeholder="Search fingerprints…"
          emptyOptionLabel={emptyOptionLabel}
          noResultsLabel="No fingerprints match"
          fieldSize="sm"
          renderOption={(opt) => <FingerprintOptionBody fp={opt.data} label={opt.label} />}
        />
      </label>
      {selected ? (
        <div className="flex flex-col gap-1.5">
          <div className="flex flex-wrap items-center gap-2">
            <Link
              to={fingerprintsHref(selected.id)}
              className="inline-flex items-center gap-1 rounded-md hover:opacity-90"
              title={`Open fingerprint "${selected.name}"`}
            >
              <Badge variant="info">
                {badgeLabel} · {selected.name}
              </Badge>
              <LinkIcon className="h-3.5 w-3.5 text-accent" />
            </Link>
            <span className="text-[10px] text-text-dim">{scopedDescription}</span>
            {onViewMatches && (
              <span
                className="inline-flex flex-wrap items-center gap-2"
                onPointerEnter={() => onRequestMatchCount?.()}
                onFocusCapture={() => onRequestMatchCount?.()}
              >
                <span
                  className="inline-flex items-center rounded border border-white/10 bg-surface px-1.5 py-0.5 font-mono text-[10px] leading-tight text-text-dim"
                  title={`Tokens matching this fingerprint, created in the last ${matchWindowDays} days`}
                >
                  {matchedCountLoading
                    ? 'counting…'
                    : matchedCount != null
                      ? `${formatWithCommas(matchedCount)} match · ${matchWindowDays}d`
                      : `match · ${matchWindowDays}d`}
                </span>
                <button
                  type="button"
                  onClick={onViewMatches}
                  disabled={matchedCount === 0}
                  className="text-[10px] font-semibold text-accent hover:underline disabled:cursor-not-allowed disabled:text-text-dim/50 disabled:no-underline"
                >
                  View matches
                </button>
              </span>
            )}
          </div>
          {fingerprintParamsCell(selected)}
        </div>
      ) : (
        <p className="text-[10px] text-text-dim">{manualHint}</p>
      )}
    </div>
  );
}
