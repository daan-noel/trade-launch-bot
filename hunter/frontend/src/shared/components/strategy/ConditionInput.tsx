import { useEffect, useRef, useState } from 'react';

import { Input } from 'components/ui/Input';
import { cn } from 'lib/cn';
import { parseConditions, formatConditions, type Condition } from 'lib/strategy/grammar';
import { unitSuffix, type MetricUnit } from 'lib/strategy/registry';

export interface ConditionInputProps {
  /** The metric's current conditions (controlled). */
  value: Condition[];
  /** Called with the new conditions whenever the text parses cleanly. */
  onChange: (conds: Condition[]) => void;
  /** The metric's unit (drives the field adornment + chip suffix + hint). */
  unit: MetricUnit;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
}

/**
 * One metric's condition editor: a grammar input (`">10, <=30"`, `"1..40"`,
 * `"!=0"`), the parsed conditions echoed as chips beneath it, and a red
 * underline + unit-aware hint when a fragment is malformed. Text-first: the
 * field holds the raw string while editing and only emits `onChange` when the
 * whole string parses; on blur an invalid field snaps back to the last valid
 * value (mirrors the numeric `Input` draft behavior).
 */
export function ConditionInput({
  value,
  onChange,
  unit,
  disabled,
  placeholder,
  className,
}: ConditionInputProps) {
  const canonical = formatConditions(value);
  const [text, setText] = useState(canonical);
  const [focused, setFocused] = useState(false);
  const lastCanonical = useRef(canonical);

  // Resync from the external value when it changes out from under us (e.g. a
  // Promote pre-fill) — but never while the field is focused, so typing isn't
  // clobbered.
  useEffect(() => {
    if (!focused && canonical !== lastCanonical.current) setText(canonical);
    lastCanonical.current = canonical;
  }, [canonical, focused]);

  const parsed = parseConditions(text);
  const invalid = parsed === null;
  const suffix = unitSuffix(unit);

  const handleChange = (raw: string) => {
    setText(raw);
    const next = parseConditions(raw);
    if (next !== null) onChange(next);
  };

  return (
    <div className={cn('flex flex-col gap-1', className)}>
      <Input
        fieldSize="sm"
        value={text}
        disabled={disabled}
        placeholder={placeholder ?? '>10, <=30'}
        unit={suffix}
        aria-invalid={invalid}
        onChange={(e) => handleChange(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          if (invalid) setText(canonical); // snap back to the last valid text
        }}
        className={cn(invalid && 'border-red/70 focus:border-red')}
      />
      {invalid ? (
        <span className="text-[11px] text-red">
          malformed — try {'>'}10, {'<='}30, 1..40, !=0 (in {unit})
        </span>
      ) : parsed.length > 0 ? (
        <div className="flex flex-wrap gap-1">
          {parsed.map((c, i) => (
            <span
              key={i}
              className="rounded bg-white/6 px-1.5 py-0.5 font-mono text-[11px] text-text-dim"
            >
              {c.operator} {c.value}
              {suffix}
            </span>
          ))}
        </div>
      ) : null}
    </div>
  );
}
