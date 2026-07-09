import { useMemo } from 'react';
import { getTimezoneSelectOptions } from 'components/token-price-chart/chartTimezone';
import { useTimezone } from 'context/TimezoneContext';
import { Select } from 'components/ui/Select';
import { cn } from 'lib/cn';

type TimezoneSelectProps = {
  className?: string;
  disabled?: boolean;
};

export function TimezoneSelect({ className, disabled }: TimezoneSelectProps) {
  const { timezone, setTimezone } = useTimezone();
  const options = useMemo(() => getTimezoneSelectOptions(timezone), [timezone]);

  return (
    <Select
      value={timezone}
      disabled={disabled}
      onChange={(e) => setTimezone(e.target.value)}
      title="Display timezone"
      // Keep the toolbar-pill look (semibold, max-width, hover) layered over the
      // shared field base via Select's merged className.
      className={cn(
        'max-w-[14rem] truncate rounded-lg !border-white/6 !bg-white/3 font-semibold text-text-dim transition-colors hover:text-text focus:ring-1 focus:ring-primary/30',
        disabled && 'cursor-not-allowed opacity-40',
        className,
      )}
    >
      {options.map((opt) => (
        <option key={opt.id} value={opt.id}>
          {opt.label}
        </option>
      ))}
    </Select>
  );
}
