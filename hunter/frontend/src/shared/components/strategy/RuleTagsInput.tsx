import { useId, useState, type KeyboardEvent } from 'react';

import { Input } from 'components/ui/Input';
import { TagChip } from './TagChip';
import { RULE_TAG_LIMIT, RULE_TAG_MAX_LEN, tagInputToChip } from 'lib/strategy/tags';

export interface RuleTagsInputProps {
  value: string[];
  onChange: (next: string[]) => void;
  /** Existing tags across all rules — autocomplete source. */
  suggestions?: string[];
  disabled?: boolean;
}

/**
 * Chip input for a rule's tags. Enter / comma / blur commits; Backspace on an
 * empty field pops the last chip.
 *
 * Only trims + lowercases what you type — the canonical grammar (separators,
 * illegal characters, dedupe, sort) is the **server's**
 * (`normalize_tags`), and the saved rule re-renders from the response. That is
 * deliberate: a second copy of the grammar here would be one more thing to keep
 * in step. Over-cap input is left for the server to reject so there is exactly
 * one authority on the limit; the counter below just warns first.
 */
export function RuleTagsInput({ value, onChange, suggestions = [], disabled }: RuleTagsInputProps) {
  const [draft, setDraft] = useState('');
  const listId = useId();

  const commit = (raw: string) => {
    const tag = tagInputToChip(raw);
    setDraft('');
    if (!tag || value.includes(tag)) return;
    onChange([...value, tag]);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      commit(draft);
      return;
    }
    if (e.key === 'Backspace' && draft === '' && value.length > 0) {
      onChange(value.slice(0, -1));
    }
  };

  const overLimit = value.length > RULE_TAG_LIMIT;
  const tooLong = value.filter((t) => t.length > RULE_TAG_MAX_LEN);

  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-1">
        {value.map((tag) => (
          <TagChip
            key={tag}
            tag={tag}
            onRemove={disabled ? undefined : () => onChange(value.filter((t) => t !== tag))}
          />
        ))}
        <Input
          fieldSize="sm"
          value={draft}
          disabled={disabled}
          list={listId}
          placeholder={value.length ? 'add tag…' : 'fam:scalper, stage:paper-test…'}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          onBlur={() => commit(draft)}
          className="w-44"
        />
        <datalist id={listId}>
          {suggestions
            .filter((t) => !value.includes(t))
            .map((t) => (
              <option key={t} value={t} />
            ))}
        </datalist>
      </div>
      {(overLimit || tooLong.length > 0) && (
        <span className="text-[10px] text-warning">
          {overLimit && `at most ${RULE_TAG_LIMIT} tags per rule. `}
          {tooLong.length > 0 && `“${tooLong[0]}” is over ${RULE_TAG_MAX_LEN} characters.`}
        </span>
      )}
    </div>
  );
}
