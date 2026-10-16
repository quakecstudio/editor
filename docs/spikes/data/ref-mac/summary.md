| Host | Prototype | Runs | Phase | Frames | p50 ms | p95 ms | p99 ms | max ms | Over 8.33 ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ref-mac | egui | 3 | type | 1923 | 1.42 | 1.80 | 2.06 | 2.84 | 0 |
| ref-mac | egui | 3 | scroll | 3600 | 1.35 | 1.81 | 2.07 | 2.25 | 0 |
| ref-mac | egui | 3 | multicursor | 768 | 1.66 | 2.10 | 2.22 | 3.11 | 0 |
| ref-mac | egui | 3 | ime | 420 | 1.45 | 1.78 | 1.88 | 2.08 | 0 |
| ref-mac | egui | 3 | diff | 1464 | 1.33 | 1.78 | 1.95 | 3.52 | 0 |
| ref-mac | egui | 3 | **all** | 8175 | 1.39 | 1.87 | 2.12 | 3.52 | 0 |
| ref-mac | egui-noviewport | 3 | type | 1923 | 1.37 | 1.79 | 2.12 | 3.13 | 0 |
| ref-mac | egui-noviewport | 3 | scroll | 3600 | 1.26 | 1.71 | 1.97 | 2.36 | 0 |
| ref-mac | egui-noviewport | 3 | multicursor | 768 | 1.55 | 2.02 | 2.16 | 3.53 | 0 |
| ref-mac | egui-noviewport | 3 | ime | 420 | 1.26 | 1.71 | 1.89 | 2.13 | 0 |
| ref-mac | egui-noviewport | 3 | diff | 1464 | 1.29 | 1.65 | 1.89 | 3.51 | 0 |
| ref-mac | egui-noviewport | 3 | **all** | 8175 | 1.31 | 1.80 | 2.05 | 3.53 | 0 |
| ref-mac | gpui | 3 | type | 1923 | 4.19 | 5.51 | 5.93 | 6.37 | 0 |
| ref-mac | gpui | 3 | scroll | 3600 | 3.96 | 5.17 | 6.03 | 7.26 | 0 |
| ref-mac | gpui | 3 | multicursor | 768 | 4.75 | 6.23 | 7.25 | 8.61 | 2 |
| ref-mac | gpui | 3 | ime | 420 | 4.15 | 5.39 | 8.19 | 11.93 | 4 |
| ref-mac | gpui | 3 | diff | 1464 | 3.99 | 5.04 | 5.52 | 7.08 | 0 |
| ref-mac | gpui | 3 | **all** | 8175 | 4.09 | 5.44 | 6.15 | 11.93 | 6 |
| ref-mac | gpui-noviewport | 3 | type | 1923 | 2.81 | 3.33 | 3.55 | 4.95 | 0 |
| ref-mac | gpui-noviewport | 3 | scroll | 3600 | 2.49 | 3.49 | 3.92 | 5.10 | 0 |
| ref-mac | gpui-noviewport | 3 | multicursor | 768 | 3.50 | 4.24 | 4.59 | 7.17 | 0 |
| ref-mac | gpui-noviewport | 3 | ime | 420 | 2.85 | 3.27 | 6.70 | 9.45 | 3 |
| ref-mac | gpui-noviewport | 3 | diff | 1464 | 2.64 | 3.17 | 3.71 | 4.66 | 0 |
| ref-mac | gpui-noviewport | 3 | **all** | 8175 | 2.68 | 3.67 | 4.18 | 9.45 | 3 |

| Host | Prototype | p99 ms, all measured phases | Threshold 8.33 ms |
| --- | --- | --- | --- |
| ref-mac | egui | 2.12 | pass |
| ref-mac | egui-noviewport | 2.05 | pass |
| ref-mac | gpui | 6.15 | pass |
| ref-mac | gpui-noviewport | 4.18 | pass |
