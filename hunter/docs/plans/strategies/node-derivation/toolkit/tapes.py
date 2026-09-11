"""Load a tape with its instrument wallets marked.

A tape is chain-ordered curve prints, one contiguous run per coin (study-kernel/tape.py). Two are
registered: the STUDY tape, where every threshold is read, and the HOLDOUT tape, the lake's days
after it in the same format (lake_export.py), where a change is only confirmed. A new export is a
new entry in TAPES.

The instrument wallets are the node's members. They mark prints as NODE so every public fact can
exclude them; they never become a term (law 20). The study tape keys wallets by wallet_dict id
(a Postgres lookup); a lake tape keys them by its own code, with the address map beside it.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone

import numpy as np
import pandas as pd

from .paths import HUNTER, LOCAL, data

COLS = ["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side", "wallet_id", "build"]
# name -> (prints, tokens, wallet map or None for wallet_dict ids, first fire time)
TAPES = {
    "study": ("cvx_prints.parquet", "cvx_tok.parquet", None, None),
    "holdout": ("cvx_holdout_prints.parquet", "cvx_holdout_tok.parquet",
                "cvx_holdout_wallets.parquet",
                datetime(2026, 9, 6, 12, 0, tzinfo=timezone.utc).timestamp()),
    # the holdout at the grain a live engine meets: every leg of a multi-leg transaction
    "holdout_legs": ("cvx_holdlegs_prints.parquet", "cvx_holdlegs_tok.parquet",
                     "cvx_holdlegs_wallets.parquet",
                     datetime(2026, 9, 6, 12, 0, tzinfo=timezone.utc).timestamp()),
}


@dataclass
class Session:
    name: str
    T: object                      # study-kernel Tape; T.day is the day index from cvx.DAY0
    c_s: np.ndarray                # coin creation time (s) per run
    t_min: float                   # fires only at t >= t_min (a holdout's start)
    days: float                    # days of fire window
    ids: dict = field(default_factory=dict)   # wallet id -> address prefix (6), the instruments
    is_node: np.ndarray = None     # per print: printed by an instrument wallet

    def wallet(self, prefix: str) -> int:
        """The id of the instrument wallet whose address starts with `prefix`."""
        for i, p in self.ids.items():
            if p.startswith(prefix[:6]):
                return i
        raise KeyError(prefix)


def roster(node: str) -> list[str]:
    """Addresses of the roster's wallets for one decision node (use_it == yes)."""
    R = pd.read_csv(LOCAL / "solo-traders.csv", usecols=["wallet_address", "node", "use_it"])
    return R[(R.node == node) & (R.use_it == "yes")].wallet_address.tolist()


def _database_url() -> str:
    for line in (HUNTER / ".env").read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing from hunter/.env")


def _ids_wallet_dict(addresses) -> dict[int, str]:
    import psycopg2
    conn = psycopg2.connect(_database_url())
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SELECT id, left(address, 6) FROM public.wallet_dict WHERE address = ANY(%s)",
                (list(addresses),))
    rows = cur.fetchall()
    conn.close()
    return {int(i): str(p) for i, p in rows}


def _ids_map(wfile, addresses) -> dict[int, str]:
    W = pd.read_parquet(data(wfile))
    W = W[W.address.isin(list(addresses))]
    return {int(i): str(a)[:6] for i, a in zip(W.wallet_id, W.address)}


def load(name: str, addresses=()) -> Session:
    from cvx import DAY0
    from tape import Tape
    prints, tokf, wfile, t_min = TAPES[name]
    T = Tape(str(data(prints)), cols=COLS)
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    tok = pd.read_parquet(data(tokf)).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    t_min = -np.inf if t_min is None else t_min
    days = (T.t.max() - max(T.t.min(), t_min)) / 86400.0
    ids = {}
    if len(addresses):
        ids = _ids_wallet_dict(addresses) if wfile is None else _ids_map(wfile, addresses)
    return Session(name, T, c_s, t_min, days, ids, np.isin(T.wallet, list(ids)))
