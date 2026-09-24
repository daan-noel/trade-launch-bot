"""The week tape in memory: chain-ordered prints, one contiguous run per token."""
import numpy as np, pyarrow.parquet as pq, pandas as pd

DAY0 = 1788220800000  # 2026-09-01 00:00:00 UTC in ms


class Tape:
    def __init__(self, path='wk_prints.parquet', cols=None):
        cols = cols or ['mint', 'slot', 't_ms', 'reserve_lamports', 'amount_lamports', 'side', 'wallet_id',
                        'payer_id', 'proxied', 'build']
        # mint and build arrive dictionary-encoded: factorize reads the codes, and a 20-million-row
        # tape does not materialise two string columns
        T = pq.read_table(path, columns=cols, read_dictionary=[c for c in ('mint', 'build') if c in cols])
        mint = T.column('mint').to_pandas()
        codes, uniques = pd.factorize(mint, sort=False)
        self.mints = np.asarray(uniques)
        self.code = codes.astype(np.int32)
        # runs
        chg = np.nonzero(np.diff(self.code))[0] + 1
        self.start = np.concatenate(([0], chg))
        self.end = np.concatenate((chg, [len(codes)]))
        assert len(self.start) == len(self.mints), 'a token is split across runs'
        self.t = T.column('t_ms').to_numpy().astype(np.float64) / 1000.0
        self.slot = T.column('slot').to_numpy()
        self.v = T.column('reserve_lamports').to_numpy().astype(np.float64) / 1e9
        if 'amount_lamports' in cols:
            self.sol = T.column('amount_lamports').to_numpy().astype(np.float64) / 1e9
        if 'side' in cols:
            self.side = T.column('side').to_numpy().astype(np.int8)
        if 'wallet_id' in cols:
            self.wallet = T.column('wallet_id').to_numpy()
        if 'payer_id' in cols:
            self.payer = T.column('payer_id').to_pandas().fillna(-1).to_numpy().astype(np.int64)
        if 'proxied' in cols:
            self.proxied = T.column('proxied').to_numpy()
        if 'build' in cols:
            bc, bu = pd.factorize(T.column('build').to_pandas(), sort=False)
            self.build = bc.astype(np.int32)
            self.builds = np.asarray(bu)
        self.n = len(self.t)
        self.day = ((T.column('t_ms').to_numpy() - DAY0) // 86400000).astype(np.int16)
        # run id per print
        self.run_of = np.repeat(np.arange(len(self.start), dtype=np.int32), self.end - self.start)

    def run(self, r):
        return slice(self.start[r], self.end[r])

    def local(self, i):
        """(run index, local index within the run) for a global print index."""
        r = self.run_of[i]
        return r, i - self.start[r]
