# mid-tape: scripts of rule 1's derivation

The record behind [../mid-tape-rule-1.md](../mid-tape-rule-1.md). Each script opens with
its step and the question it answers. Evidence: [_!___evidence.md](../../_!___evidence.md)
5.8. Playbook: [_!___derive.md](../../_!___derive.md).

Every script imports `_paths` first. Run from this folder: `python mt_p15.py`. The study
tape's wallet ids are `wallet_dict` ids, so scripts that name the node's wallets need
`DATABASE_URL` in `hunter/.env` (a read of `wallet_dict`, nothing else).

| step | script | question | evidence |
| --- | --- | --- | --- |
| 3-5 | `mt_p15.py` | who pays, shape, 9Uq8GV's trigger, DELAY at 115 ms | 5.8 |
