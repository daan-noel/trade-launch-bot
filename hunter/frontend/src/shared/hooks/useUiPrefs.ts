import { useStoredField } from 'hooks/useLocalStorage';
import { STORAGE_KEYS, type UiToggles } from 'lib/storage';

/**
 * Open/closed state of one collapsible panel, persisted in the shared
 * `mt:ui.accordion` map. `id` is a logical id from `ACCORDION_IDS`, never a full
 * storage key — the map owns the `mt:` prefix.
 *
 * Persist only when the open state is a **user chrome preference**. An accordion
 * whose default follows the data ("open when there are no runs yet") must stay
 * unpersisted, or the first visit's data shape sticks forever.
 */
export function useAccordionOpen(
  id: string,
  defaultOpen = true,
): [boolean, (open: boolean | ((prev: boolean) => boolean)) => void] {
  return useStoredField<boolean>(STORAGE_KEYS.uiAccordion, id, defaultOpen);
}

/**
 * One app-wide show/hide switch from the shared `mt:ui.toggles` blob. A switch
 * that means the same thing on two surfaces (Rules board and Simulate's rules
 * list) uses the same field, so it is one product preference, not two.
 */
export function useUiToggle<K extends keyof UiToggles>(
  field: K,
  defaultValue: NonNullable<UiToggles[K]>,
): [
  NonNullable<UiToggles[K]>,
  (v: NonNullable<UiToggles[K]> | ((prev: NonNullable<UiToggles[K]>) => NonNullable<UiToggles[K]>)) => void,
] {
  return useStoredField<NonNullable<UiToggles[K]>>(STORAGE_KEYS.uiToggles, field, defaultValue);
}
