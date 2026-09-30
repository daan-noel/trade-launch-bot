import { ToggleGroup } from 'components/ui/ToggleGroup';

/** Which tokens a token table lists: every one in the pool, or the ones that pass the filters. */
export type TokenView = 'pool' | 'pass';

/** The Pool / Pass filters switch of a token table, each side with its token count. */
export function TokenViewToggle({
  value,
  onChange,
  pool,
  pass,
}: {
  value: TokenView;
  onChange: (v: TokenView) => void;
  pool: number;
  pass: number;
}) {
  return (
    <ToggleGroup
      size="sm"
      aria-label="Tokens listed"
      value={value}
      onChange={onChange}
      options={[
        { value: 'pool', label: 'Pool', count: pool, title: 'Every token with a buy in the token pool.' },
        { value: 'pass', label: 'Pass filters', count: pass, title: 'Tokens with a buy that passes the filters.' },
      ]}
    />
  );
}
