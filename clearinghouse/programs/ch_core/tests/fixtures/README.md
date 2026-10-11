# Oracle fixtures

`pyth_sol_usd_price_update_v2.bin`: raw data of the mainnet-beta account
`7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE` (Pyth sponsored SOL/USD push
feed, owner `rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ`), fetched with
`getAccountInfo` at slot 455,686,323. Public on-chain data, 134 bytes.

Decoded: verification level Full, feed id
`ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d`,
price 10,960,825,188, conf 2,468,469, exponent -8 ($109.608 ± $0.0247),
publish_time 1,791,729,342.

Used by `oracle.rs` unit tests and `ch_svm_tests/tests/pyth_oracle.rs` to pin
the parser to the real layout rather than to a layout written by the same
author as the parser.
