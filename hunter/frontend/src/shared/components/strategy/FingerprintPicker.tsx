import { useState } from 'react';

import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { PlusIcon } from 'components/ui/icons';
import { Modal } from 'components/ui/Modal';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useCreateFingerprintMutation,
} from 'store/sharedEndpoints';
import { FingerprintForm } from './FingerprintForm';
import { fingerprintParamsCell } from './FingerprintParamsSummary';
import type { FingerprintDraft } from 'lib/strategy/types';

export interface FingerprintPickerProps {
  value: string | null;
  onChange: (id: string) => void;
  disabled?: boolean;
}

/**
 * Fingerprint selector for the rule editor: a dropdown of existing fingerprints
 * (with each one's used-by count) plus an inline "+ new" that opens the create
 * form in a modal and auto-selects the result. Reused across live + lab.
 */
export function FingerprintPicker({ value, onChange, disabled }: FingerprintPickerProps) {
  const { data: fps = [], isLoading } = useGetFingerprintsQuery();
  const [createFp, { isLoading: creating }] = useCreateFingerprintMutation();
  const [open, setOpen] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const selected = fps.find((f) => f.id === value);

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
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <Select
          fieldSize="sm"
          value={value ?? ''}
          disabled={disabled || isLoading}
          onChange={(e) => onChange(e.target.value)}
          className="min-w-[16rem]"
        >
          <option value="" disabled>
            {isLoading ? 'loading…' : 'select a fingerprint…'}
          </option>
          {fps.map((f) => (
            <option key={f.id} value={f.id}>
              {f.name || f.id.slice(0, 8)}
              {f.used_by != null ? ` · used by ${f.used_by}` : ''}
            </option>
          ))}
        </Select>
        <IconButton
          variant="success"
          size="md"
          disabled={disabled}
          onClick={() => setOpen(true)}
          label="New"
          title="New fingerprint"
        >
          <PlusIcon />
        </IconButton>
      </div>
      {selected && fingerprintParamsCell(selected)}

      <Modal title="New fingerprint" open={open} onClose={() => setOpen(false)}>
        <FingerprintForm
          onSubmit={submit}
          onCancel={() => setOpen(false)}
          submitting={creating}
          error={err}
        />
      </Modal>
    </div>
  );
}
