# Rule search — validate the robustness upgrades

The mechanisms (latency ladder, spread gate, trimmed block ranking, sibling z,
admission gate, entry bands, expectancy floor, token-n floor, ablation /
quartile / exit-efficiency diagnostics) are implemented and documented in
[rule-search-method.md](../plans/strategies/rule-search-method.md) §2–§4.
Which metrics and which clock the champion uses is
[rule-search-habit.md](rule-search-habit.md) — this file is only the robustness
gates already in the method.

Open work is proving the gates on real cohorts:

- Grade each upgrade with the method's §5 same-form ablation: freeze
  fingerprint, range, buy, fill, cost, copycat, incumbent; one cut source /
  gate per run; promoted g4 / g8 / g12 rules are the ablation incumbents.
- Re-run the fingerprints the pilots killed for latency or spread (g2, g6,
  g8 v1, g12) and confirm the new gates refuse or downgrade them without an
  operator reading the numbers.
- Watch whether the 10% admission share and the 0.25 spread discount need
  tuning — both are constants in `hunter-lab`'s `rule_search` (`cuts.rs`,
  `report.rs`), chosen from the pilot history, not fitted.
