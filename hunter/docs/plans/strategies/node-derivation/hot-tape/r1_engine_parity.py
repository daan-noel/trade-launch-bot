"""Rule 1 and rule 1b in the engine, ticket by ticket against the frozen reference (step 3 of
hunter/docs/roadmap/hot-tape-rule-1-engine-plan.md).

The engine side is `hunter/lab/examples/hot_tape_rule1_parity.rs`: the lab's lake load, run_replay,
LagMs(115) and the engine cost kernel, one row per position with its prints named by
(slot, tx_index, leg). This script writes that test's inputs and reads its output.

  python r1_engine_parity.py prep TAPE
      data/r1p_rule.json          rule 1 in the engine's rule grammar, from r1x_rule_any_study_exact.json
      data/r1b_rule.json          rule 1b: rule 1's entry, r1b_exit.RULE_B's exit (m_position.room_taken)
      data/r1p_TAPE_mints.txt     every coin the reference tape holds, born inside it
      data/r1p_TAPE_creators.csv  mint,creator address (r1_creators.parquet, the reference's creators)
      data/r1p_TAPE_tick0.txt     the tape's first print in us: the replay's grid anchor
  python r1_engine_parity.py compare TAPE ENGINE_CSV [REF]
      matches the engine rows to REF_TAPE.parquet (r1_ref, or r1b_ref from r1b_exit.py ref) on the
      trigger print and reports every difference in the entry fill, the exit print, the reason and
      the SOL; then the two books
  python r1_engine_parity.py book TAPE ENGINE_CSV [BUY_SOL]
      the engine book alone, in the tape's fire window, at the buy size the run used
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import data_file

import json
import sys

import numpy as np
import pandas as pd

from cvx import DAY0
from r1_exact import Tape, bootstrap, CLIP
from toolkit.book import ledger

REASON = {"TakeProfit": "tp", "StopLoss": "sl"}


def why(label: str) -> str:
    """The engine's exit label in the reference's words."""
    s = str(label)
    if s in REASON:
        return REASON[s]
    if s.startswith("held"):
        return "time"
    if s.startswith("room_taken"):
        return "head"
    return s


def engine_rule() -> dict:
    R = json.loads(data_file("r1x_rule_any_study_exact.json").read_text())
    e = R["entry"]
    tp, sl, clock = R["exit"]
    c = lambda key: [{"operator": e[key][0], "value": float(e[key][1])}]  # noqa: E731
    assert e["vres"][0] == "<="
    return {
        "entry": {
            "m_flow_window": {"window_size_prints": 1, "sell": c("ssize")},
            "m_print_wallet": {"since_buy": c("shold")},
            "m_build_window": {"window_size_sec": 5, "unique_builds": c("nb5")},
            "m_price_lifetime": {"stall": c("stall")},
            "m_state": {"time": c("age"),
                        # liquidity is vsol - 30 on the curve: reserve <= 100 is liquidity <= 70
                        "liquidity": [{"operator": "<=", "value": float(e["vres"][1]) - 30.0}],
                        "on_curve": [{"operator": "=", "value": 1.0}]},
            "m_crowd_after_age": {"after_age_sec": 0, "non_creator_buyers": c("hold_n")},
        },
        "exit": {"m_position": {"held": [{"operator": ">=", "value": float(clock)}]}},
        "take_profit": float(tp),
        "stop_loss": float(sl),
        "reentry": {"cooldown_sec": 0, "max_episodes_per_token": 1000},
    }


def engine_rule_b() -> dict:
    """Rule 1's entry; exit at RULE_B's share of the room to the wall, its stop and its clock."""
    from r1b_exit import RULE_B
    r = engine_rule()
    del r["take_profit"]
    r["stop_loss"] = float(RULE_B["sl"])
    r["exit"] = {"m_position": {"held": [{"operator": ">=", "value": float(RULE_B["clock"])}],
                                "room_taken": [{"operator": ">=", "value": 100.0 * RULE_B["head"]}]}}
    return r


def prep(name: str) -> None:
    data_file("r1p_rule.json").write_text(json.dumps(engine_rule(), indent=1) + "\n")
    data_file("r1b_rule.json").write_text(json.dumps(engine_rule_b(), indent=1) + "\n")
    T = Tape(name)
    born = np.isfinite(T.created_us)
    mints = T.mints[born]
    data_file("r1p_%s_mints.txt" % name).write_text("\n".join(mints) + "\n")
    cr = pd.read_parquet(data_file("r1_creators.parquet")).drop_duplicates("mint").set_index("mint")
    c = cr.creator.reindex(mints)
    print("%s: %d coins born inside the tape, %d without a creator" % (name, len(mints), int(c.isna().sum())))
    c.dropna().rename("creator").rename_axis("mint").reset_index().to_csv(
        data_file("r1p_%s_creators.csv" % name), index=False)
    data_file("r1p_%s_tick0.txt" % name).write_text("%d\n" % int(T.t_us.min()))


def engine_frame(T: Tape, eng: pd.DataFrame) -> pd.DataFrame:
    code = {mm: i for i, mm in enumerate(T.mints)}
    return pd.DataFrame({"y": eng.pnl_sol.to_numpy(), "run": eng.mint.map(code).to_numpy(),
                         "why": eng.reason.map(why).to_numpy(),
                         "day": (eng.trig_t_us.to_numpy() // 1_000_000 - DAY0) // 86400})


def book(name: str, csv: str, b: str = "0.2") -> None:
    T = Tape(name)
    eng = pd.read_csv(csv)
    E = engine_frame(T, eng[(eng.trig_t_us >= T.t_min_us) & (eng.trig_t_us < T.t_max_us)])
    L = ledger(E, T.days, float(b))
    lo_, hi_, _ = bootstrap(E, float(b))
    print(name, csv, {k: L.get(k) for k in ("n", "pct", "sol", "solday", "pos", "worst", "body", "top1",
                                           "maxcoin", "h1", "h2")}, "ci %+.2f..%+.2f" % (lo_, hi_))


def compare(name: str, csv: str, ref_name: str = "r1_ref") -> None:
    pd.set_option("display.width", 250)
    T = Tape(name)
    ref = pd.read_parquet(data_file("%s_%s.parquet" % (ref_name, name)))
    eng = pd.read_csv(csv)
    lo, hi = T.t_min_us, T.t_max_us
    eng_w = eng[(eng.trig_t_us >= lo) & (eng.trig_t_us < hi)].copy()
    print("engine positions %d, inside the fire window %d; reference tickets %d"
          % (len(eng), len(eng_w), len(ref)))
    amb = eng_w[(eng_w.trig_n != 1) | (eng_w.fill_n != 1) | (eng_w.exit_n != 1)]
    print("rows whose print is not pinned to one leg: %d" % len(amb))
    k = ["mint", "trig_slot", "trig_tx", "trig_leg"]
    r = ref.rename(columns={"trigger_slot": "trig_slot", "trigger_tx": "trig_tx", "trigger_leg": "trig_leg"})
    m = eng_w.merge(r, on=k, how="outer", indicator=True)
    only_e = m[m._merge == "left_only"]
    only_r = m[m._merge == "right_only"]
    both = m[m._merge == "both"].copy()
    print("tickets on the same trigger print: %d   engine only: %d   reference only: %d"
          % (len(both), len(only_e), len(only_r)))
    both["why_e"] = both.reason.map(why)
    checks = {
        "entry fill print": (both.fill_slot_x != both.fill_slot_y) | (both.fill_tx_x != both.fill_tx_y)
        | (both.fill_leg_x != both.fill_leg_y),
        "exit print": (both.exit_slot_x != both.exit_slot_y) | (both.exit_tx_x != both.exit_tx_y)
        | (both.exit_leg_x != both.exit_leg_y),
        "reason": both.why_e != both.why,
        "SOL (> 1e-9)": (both.pnl_sol - both.pnl_sol_engine_kernel).abs() > 1e-9,
    }
    for label, bad in checks.items():
        print("  %-18s differ on %d" % (label, int(bad.sum())))
    print("  max |SOL difference| %.3g" % float((both.pnl_sol - both.pnl_sol_engine_kernel).abs().max()))
    cols = ["mint", "trig_slot", "trig_tx", "trig_leg", "trig_t_us_x", "reason", "pnl_sol"]
    if len(only_e):
        print("\nengine only:\n", only_e[cols].head(20).to_string(index=False))
    if len(only_r):
        print("\nreference only:\n", only_r[["mint", "trig_slot", "trig_tx", "trig_leg", "why",
                                             "pnl_sol_engine_kernel"]].head(20).to_string(index=False))
    for label, bad in checks.items():
        if bad.any():
            print("\n%s differs:\n" % label, both[bad][[
                "mint", "trig_slot", "trig_tx", "fill_slot_x", "fill_tx_x", "fill_leg_x", "fill_slot_y",
                "fill_tx_y", "fill_leg_y", "exit_slot_x", "exit_tx_x", "exit_slot_y", "exit_tx_y",
                "reason", "why", "pnl_sol", "pnl_sol_engine_kernel"]].head(20).to_string(index=False))

    # the two books, scored by the one ledger
    code = {mm: i for i, mm in enumerate(T.mints)}
    E = engine_frame(T, eng_w)
    R = pd.DataFrame({"y": ref.pnl_sol_engine_kernel.to_numpy(), "run": ref.mint.map(code).to_numpy(),
                      "why": ref.why.to_numpy(), "day": ref.day.to_numpy()})
    rows = []
    for label, F in (("engine", E), ("python", R)):
        L = ledger(F, T.days, CLIP)
        lo_, hi_, p0 = bootstrap(F)
        rows.append(dict(book=label, **{k2: L.get(k2) for k2 in ("n", "pct", "sol", "pos", "worst", "body",
                                                                 "top1", "maxcoin", "h1", "h2", "win")},
                         ci="%+.2f..%+.2f" % (lo_, hi_)))
    print("\n" + pd.DataFrame(rows).to_string(index=False))
    print("engine exits:", E.why.value_counts().to_dict(), " python exits:", R.why.value_counts().to_dict())


if __name__ == "__main__":
    {"prep": prep, "compare": compare, "book": book}[sys.argv[1]](*sys.argv[2:])
