/**
 * Staging tape (Flow Discovery): the draft tag the overlay, badges and clicks read
 * instead of a saved fingerprint. The cart owns Apply.
 *
 * Its own module so a route can type the tape without statically importing
 * `TokenTradeChart` (that file pulls `lightweight-charts`).
 */
export type { TagTape as ChartTapeStaging } from 'hooks/useIxPatternTarget';
