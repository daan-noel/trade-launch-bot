import { useMemo, useState } from 'react';

import { IconButton } from 'components/ui/IconButton';
import { PlusIcon } from 'components/ui/icons';
import { Modal } from 'components/ui/Modal';
import { SearchableSelect, type SearchableSelectOption } from 'components/ui/SearchableSelect';
import { cn } from 'lib/cn';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useGetStrategyRulesQuery,
  useCreateFingerprintMutation,
} from 'store/sharedEndpoints';
import { copyFingerprintIds } from 'lib/strategy/copyRule';
import { FingerprintForm } from './FingerprintForm';
import {
  FingerprintOptionBody,
  fingerprintParamsSearchText,
  fingerprintSelectLabel,
} from './FingerprintParamsSummary';
import type { Fingerprint, FingerprintDraft } from 'lib/strategy/types';

export interface FingerprintPickerProps {
  value: string | null;
  onChange: (id: string) => void;
  disabled?: boolean;
  className?: string;
  /** `copy` lists every fingerprint, including ones a copy rule owns. */
  pool?: 'rules' | 'copy';
}

/**
 * Fingerprint selector for the rule editor: searchable dropdown of saved
 * fingerprints (with used-by count) plus "+ new" to create one inline. The
 * dropdown shows the axis chips and searches them; the closed input stays the
 * name. Full-width chips under the field still live in `FingerprintParamsById`
 * (see RuleEditor's fingerprint + TP/SL row).
 */
export function FingerprintPicker({
  value,
  onChange,
  disabled,
  className,
  pool = 'rules',
}: FingerprintPickerProps) {
  const { data: fps = [], isLoading } = useGetFingerprintsQuery();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const hidden = useMemo(() => (pool === 'copy' ? new Set<string>() : copyFingerprintIds(rules)), [pool, rules]);
  const [createFp, { isLoading: creating }] = useCreateFingerprintMutation();
  const [open, setOpen] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const options: SearchableSelectOption<Fingerprint>[] = useMemo(
    () =>
      fps.filter((f) => !hidden.has(f.id)).map((f) => ({
        value: f.id,
        label: fingerprintSelectLabel(f),
        searchText: fingerprintParamsSearchText(f),
        data: f,
      })),
    [fps, hidden],
  );

  const submit = async (draft: FingerprintDraft) => {
    setErr(null);
    try {
      const created = await createFp(draft).unwrap();
      onChange(created.id);
      setOpen(false);
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Create failed');
    }
  };

  return (
    <>
      <div className={cn('flex min-w-0 items-center gap-2', className)}>
        <SearchableSelect
          options={options}
          value={value}
          onChange={onChange}
          disabled={disabled || isLoading}
          placeholder={isLoading ? 'loading…' : 'Search fingerprints…'}
          noResultsLabel="No fingerprints match"
          fieldSize="sm"
          menuMinWidth={640}
          className="min-w-0 flex-1"
          renderOption={(opt) => <FingerprintOptionBody fp={opt.data} label={opt.label} />}
        />
        <IconButton
          variant="success"
          size="md"
          disabled={disabled}
          className="shrink-0"
          onClick={() => setOpen(true)}
          label="New"
          title="New fingerprint"
        >
          <PlusIcon />
        </IconButton>
      </div>

      <Modal title="New fingerprint" open={open} onClose={() => setOpen(false)}>
        <FingerprintForm
          onSubmit={submit}
          onCancel={() => setOpen(false)}
          submitting={creating}
          error={err}
        />
      </Modal>
    </>
  );
}
