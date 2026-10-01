import { Checkbox } from 'components/ui/Checkbox';
import type { IxPatternFeeField, IxPatternFeeMask } from 'lib/strategy/ixPatternRows';

/** Each fee field's toggle; `onto` names what the clicked tx's value lands on. */
const FEE_PIN_TOGGLES: { field: IxPatternFeeField; label: string; title: (onto: string) => string }[] = [
  {
    field: 'cu_limit',
    label: 'cu_limit',
    title: (onto) => `Copy the clicked tx's cu_limit onto ${onto}. Off (the default) = the ix shape alone, any limit.`,
  },
  {
    field: 'cu_price',
    label: 'cu_price',
    title: (onto) =>
      `Copy the clicked tx's cu_price onto ${onto}. Many clients recompute this per transaction: pin it only when you have seen it hold.`,
  },
  {
    field: 'tip_lamports',
    label: 'tip',
    title: (onto) => `Copy the clicked tx's tip onto ${onto}. A tip is an auction bid and almost never a stable identity.`,
  },
];

/**
 * Sticky fee-field modifiers for an ix-shape click. Checking cu_limit then clicking
 * a tx copies that tx's ix shape plus its cu_limit - not the other two. All off = the
 * shape alone (any budget). The tag stage and the chart highlight each own a mask.
 */
export function FeePinToggles({
  mask,
  onChange,
  disabled = false,
  onto = 'the added shape',
  label = 'pin',
}: {
  mask: IxPatternFeeMask;
  onChange: (next: IxPatternFeeMask) => void;
  disabled?: boolean;
  /** What the copied values land on, for the hover text. */
  onto?: string;
  /** The strip's caption. */
  label?: string;
}) {
  return (
    <span
      className="inline-flex flex-wrap items-center gap-1.5"
      title={`Fee fields copied from the clicked tx onto ${onto}. Default off = any budget.`}
    >
      <span className="text-[9px] uppercase tracking-wide text-text-dim/60">{label}</span>
      {FEE_PIN_TOGGLES.map(({ field, label: fieldLabel, title }) => (
        <label key={field} className="inline-flex cursor-pointer items-center gap-0.5" title={title(onto)}>
          <Checkbox
            boxSize="sm"
            checked={!!mask[field]}
            disabled={disabled}
            onChange={() => onChange({ ...mask, [field]: !mask[field] })}
            aria-label={`${label}: ${fieldLabel} from the clicked tx`}
          />
          <span className="font-mono text-[10px] text-text-dim">{fieldLabel}</span>
        </label>
      ))}
    </span>
  );
}
