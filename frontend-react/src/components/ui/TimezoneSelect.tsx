import { useMemo } from 'react';
import { getTimezoneSelectOptions } from 'components/token-price-chart/chartTimezone';
import { useTimezone } from 'context/TimezoneContext';
import { cn } from 'lib/cn';

type TimezoneSelectProps = {
  className?: string;
  disabled?: boolean;
};

export function TimezoneSelect({ className, disabled }: TimezoneSelectProps) {
  const { timezone, setTimezone } = useTimezone();
  const options = useMemo(() => getTimezoneSelectOptions(timezone), [timezone]);

  return (
    <select
      value={timezone}
      disabled={disabled}
      onChange={(e) => setTimezone(e.target.value)}
      title="Display timezone"
      className={cn(
        'max-w-[14rem] truncate rounded-lg border border-white/6 bg-white/3 px-2 py-1 text-[11px] font-semibold text-text-dim transition-colors hover:text-text focus:outline-none focus:ring-1 focus:ring-primary/30',
        disabled && 'cursor-not-allowed opacity-40',
        className,
      )}
    >
      {options.map((opt) => (
        <option key={opt.id} value={opt.id}>
          {opt.label}
        </option>
      ))}
    </select>
  );
}
