import { Badge } from 'components/ui/Badge';
import { orderTagNames, tagLabel } from 'lib/flow/tapeClassify';
import { isOwnerSplitTag } from 'lib/strategy/tagsDoc';

/** What the owner-split tag is, in one place for every picker and badge. */
export const OWNER_SPLIT_TAG_TIP =
  'Owner split: whose money each trade is. Rules read @volume as owner money and @!volume as outsider money. ' +
  'Built from rows, wallets and the creator switch, and filled by the Flow Discovery auto-tag rules. ' +
  'Sticky: a wallet it matches carries it for the rest of that coin.';

/** What every other tag is. */
export const PLAIN_TAG_TIP =
  'Plain list: a trade carries it when any of its matchers holds. No auto-tag rules. Side and sticky are what you set.';

/** `owner split` or `plain list`, so a tag's kind reads at a glance next to its name. */
export function TagKindBadge({ name }: { name: string }) {
  return isOwnerSplitTag(name) ? (
    <Badge variant="warning" size="sm" pill title={OWNER_SPLIT_TAG_TIP}>
      owner split
    </Badge>
  ) : (
    <Badge variant="neutral" size="sm" pill title={PLAIN_TAG_TIP}>
      plain list
    </Badge>
  );
}

/** A tag picker's options: the owner-split tag in its own group above the plain
 *  lists. `known` = the names the fingerprint already defines; another is `(new)`. */
export function TagNameOptions({ names, known }: { names: readonly string[]; known: readonly string[] }) {
  const ordered = orderTagNames(names);
  const owner = ordered.filter(isOwnerSplitTag);
  const plain = ordered.filter((n) => !isOwnerSplitTag(n));
  const option = (n: string) => (
    <option key={n} value={n}>
      {tagLabel(n)}
      {known.includes(n) ? '' : ' (new)'}
    </option>
  );
  return (
    <>
      {owner.length > 0 && <optgroup label="Owner split">{owner.map(option)}</optgroup>}
      {plain.length > 0 && <optgroup label="Plain lists">{plain.map(option)}</optgroup>}
    </>
  );
}
