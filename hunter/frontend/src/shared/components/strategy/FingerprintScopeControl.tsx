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
import { SearchableSelect, type SearchableSelectOption } from 'components/ui/SearchableSelect';
import { LabelTip } from './LabelTip';
import { fingerprintParamsCell } from './FingerprintParamsSummary';
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
}: FingerprintScopeControlProps) {
  const selected = (value && fingerprints.find((f) => f.id === value)) || null;

  const options: SearchableSelectOption<Fingerprint>[] = useMemo(
    () =>
      fingerprints.map((f) => ({
        value: f.id,
        label: `${f.name || f.id.slice(0, 8)}${f.used_by != null ? ` · used by ${f.used_by}` : ''}`,
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
          </div>
          {fingerprintParamsCell(selected)}
        </div>
      ) : (
        <p className="text-[10px] text-text-dim">{manualHint}</p>
      )}
    </div>
  );
}
