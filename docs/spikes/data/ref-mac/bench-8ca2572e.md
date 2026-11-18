# Budget rows on the reference Mac at 8ca2572e (P4-24)

Measured on 2026-11-17 on the reference Mac (Mac17,2, Apple M5) at commit
`8ca2572e` on `main`, with Justin's GOG copies of Quake read where they are:

```
QS_BENCH_GAME=~/Games/quake-gog/app \
QS_BENCH_RERELEASE=<a folder linking id1 from the original and mg1 from the rerelease> \
QS_BENCH_HOST=ref-mac cargo xtask bench
```

`find ~/Games -newer <a file made before the run>` printed nothing afterwards: neither
install was changed.

The four rows P4-24 asks for, as `cargo xtask bench` printed them:

| Row | Statistic | Measured | Budget |
| --- | --- | ---: | ---: |
| `save_to_game` | p95 | 73.76 ms | 1.000 s |
| `open_project_and_type` | p95 | 223.52 ms | 1.000 s |
| `keystroke` | p99 | 192.46 us | 8.33 ms |
| `check_after_typing` | p99 | 408.29 us | 8.33 ms |

Each is within its budget. `save_to_game` is the windowed preview host: a save in the editor
until the next frame the game draws.

The run's own verdict also holds each row to no more than 10% over the last passing run on
this machine, and by that it marked `keystroke` (192.46 us against 127.67 us) and
`check_after_typing` (408.29 us against 356.21 us). Those are the 99th percentiles of 200
samples taken while other work ran on the machine; the means of the same samples were 101.89
us for `keystroke` (106.91 us in the earlier run) and 290.20 us for `check_after_typing`
(267.74 us). `full_compile_e1m1`, which P4-24 does not ask for, measured a median of 8.475 s
against its 8 s budget.
