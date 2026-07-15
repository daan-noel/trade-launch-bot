//! Portfolio admission for a single-rule simulate's candidate list — shared by
//! `tpsl_sniper_1`/`tpsl_sniper_2`/`swing_1::backtest` (three byte-identical
//! copies before this dedup — parity plan B3).

use chrono::{DateTime, Utc};

/// Apply the rule's concurrency / total-token caps to entry-time-sorted
/// candidates, mirroring how the live run admits tokens: a token is skipped when
/// `max_concurrent_tokens` are still open at its entry, and admission stops once
/// `max_total_tokens` have been selected. Generic over the per-token result
/// payload `T` — this fn only ever looks at the entry/exit instants alongside it.
pub fn select_simulated_tokens<T>(
    candidates: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, T)>,
    max_concurrent_tokens: Option<usize>,
    max_total_tokens: Option<usize>,
) -> Vec<T> {
    let mut active_exits: Vec<Option<DateTime<Utc>>> = Vec::new();
    let mut results: Vec<T> = Vec::new();
    let mut selected_count: usize = 0;

    for (entry_time, exit_time, result) in candidates {
        if let Some(total_max) = max_total_tokens {
            if selected_count >= total_max {
                break;
            }
        }

        active_exits.retain(|active_exit| match active_exit {
            Some(exit_time) => *exit_time > entry_time,
            None => true,
        });

        if let Some(max_open) = max_concurrent_tokens {
            if active_exits.len() >= max_open {
                continue;
            }
        }

        active_exits.push(exit_time);
        results.push(result);
        selected_count += 1;
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn no_caps_admits_everything_in_order() {
        let candidates =
            vec![(t(0), Some(t(10)), "a"), (t(5), Some(t(15)), "b"), (t(20), None, "c")];
        let out = select_simulated_tokens(candidates, None, None);
        assert_eq!(out, vec!["a", "b", "c"]);
    }

    #[test]
    fn max_total_stops_admission_after_n() {
        let candidates =
            vec![(t(0), Some(t(10)), "a"), (t(5), Some(t(15)), "b"), (t(20), None, "c")];
        let out = select_simulated_tokens(candidates, None, Some(2));
        assert_eq!(out, vec!["a", "b"]);
    }

    #[test]
    fn max_concurrent_skips_a_token_while_the_cap_is_full() {
        // "a" open [0,10), "b" open [5,15) — both alive at entry_time=8, so with a
        // cap of 1 "b" (2nd overlapping) is skipped; "c" enters at 20, after "a"
        // and "b" both closed, so it is admitted.
        let candidates = vec![(t(0), Some(t(10)), "a"), (t(8), Some(t(15)), "b"), (t(20), None, "c")];
        let out = select_simulated_tokens(candidates, Some(1), None);
        assert_eq!(out, vec!["a", "c"]);
    }

    #[test]
    fn exit_time_none_counts_as_open_forever() {
        // "a" never exits (still open) so it always occupies a concurrency slot.
        let candidates = vec![(t(0), None, "a"), (t(5), Some(t(6)), "b")];
        let out = select_simulated_tokens(candidates, Some(1), None);
        assert_eq!(out, vec!["a"]);
    }
}
