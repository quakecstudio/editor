# Code editor budgets: ref-mac

A project of 25 files of 2000 lines and `defs.qc`.

Open the project and show a file: 223 ms, budget 1000 ms: met.

| Phase | Frames | p50 ms | p99 ms | Max ms | Budget |
| --- | ---: | ---: | ---: | ---: | --- |
| Type | 43 | 0.15 | 1.61 | 1.61 | every frame at most 8.33 ms: met |
| Check | 4112 | 0.12 | 0.14 | 85.63 | every frame at most 8.33 ms: missed |
