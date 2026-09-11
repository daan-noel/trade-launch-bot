"""Hot-tape node, step 34: the HOLDOUT - the frozen sentence on days it has never seen.

Every threshold on the sentence was read off the study tape, which ends 2026-09-06 11:59:59 UTC:
E (evidence 1.12), the bracket (1.13), holders >= 368 and age >= 158 s (1.14). Nothing is
re-fitted here. `cvx_holdout_export.py` builds the lake's days 09-03..09-10 in the study tape's
exact format (every shared row identical on 09-04 and 09-05; creation times identical), and this
file books the SAME code (`cvx_hot_perm2.run`) on it with fires only from 09-06 12:00 on - the
three days before are warm-up, so each coin's holder book and seller history start at its birth.

The six node wallets are dropped by ADDRESS here (the lake has no wallet_dict ids); coins created
before the warm-up start are booked and reported separately, their holder book being partial.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import pandas as pd

from cvx import DAY0
from tape import Tape
from cvx_hot_event import LOCAL, NODE_NAME
import cvx_hot_perm2 as P

T_MIN = datetime(2026, 9, 6, 12, 0, tzinfo=timezone.utc).timestamp()
WARMUP = datetime(2026, 9, 3, tzinfo=timezone.utc).timestamp()


def main() -> None:
    t0 = time.time()
    T = Tape(str(data_file("cvx_holdout_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    W = pd.read_parquet(data_file("cvx_holdout_wallets.parquet"))
    R = pd.read_csv(LOCAL / "solo-traders.csv", usecols=["wallet_address", "node", "use_it"])
    R = R[(R.node == NODE_NAME) & (R.use_it == "yes")]
    node = W[W.address.isin(R.wallet_address)]
    is_node = np.isin(T.wallet, node.wallet_id.to_numpy())
    w8 = int(node[node.address.str.startswith("8fStGV")].wallet_id.iloc[0])
    tok = pd.read_parquet(data_file("cvx_holdout_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    held = T.t >= T_MIN
    days = (T.t[held].max() - T_MIN) / 86400.0
    print("holdout tape %s prints (%s in the holdout)  node wallets found %d / %d  holdout "
          "%.2f days  %ds" % (f"{T.n:,}", f"{int(held.sum()):,}", len(node), len(R), days,
                              time.time() - t0), flush=True)
    F = P.run(T, c_s, is_node, w8, P.CELLS, t_min=T_MIN)
    F["full_hist"] = c_s[F.run.to_numpy()] >= WARMUP
    F.to_parquet(data_file("cvx_hot_holdout.parquet"), index=False)
    print("fires %s  (coins born before the warm-up: %s)  %ds"
          % (f"{len(F):,}", f"{int((~F.full_hist).sum()):,}", time.time() - t0), flush=True)
    print("\n##### HOLDOUT, every coin")
    P.report(F, days, P.CELLS)
    print("\n##### HOLDOUT, coins born inside the tape (full holder book)")
    P.report(F[F.full_hist], days, P.CELLS)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
