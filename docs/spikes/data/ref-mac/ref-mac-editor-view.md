# Editor view frame times: ref-mac

| Phase | Frames | p50 ms | p95 ms | p99 ms | Max ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| warmup | 119 | 0.55 | 0.73 | 0.84 | 21.14 |
| type | 641 | 0.94 | 1.75 | 2.03 | 3.87 |
| scroll | 1200 | 1.03 | 1.94 | 2.24 | 2.67 |
| multicursor | 256 | 1.02 | 1.64 | 2.01 | 2.61 |
| ime | 140 | 0.96 | 1.91 | 2.20 | 2.23 |

The bar is a p99 of at most 8.33 ms in every phase but the warm-up: passed.
