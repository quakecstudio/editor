# id1 maps in the renderer on the reference Mac (P2-152)

Measured on 2026-11-02 with this change's viewer on the reference Mac (Apple M5, built-in
Liquid Retina XDR display, 120 Hz), in the viewer's benchmark mode: each map opened from
Justin's GOG copy of Quake, presented without vsync, the camera at the player's start turning
slowly for six seconds from the first frame, then the average frame rate, the slowest frame
and the CPU time each frame's work took (the simulation step, the relighting and the paint
callback's recording, without the wait for the display) printed.

```
for m in start e1m1 ... dm6; do
  cargo run --release -p qs-render --example view -- \
    -basedir ~/Games/quake-gog/app -benchmark 6 -position 2700 200 "$m"
done
```

`-position 2700 200` opens the window on the built-in display. macOS paces a window's
redraws at its display's refresh rate whatever the present mode, so the frame rate shows the
renderer keeping up with 120 Hz and the work time its headroom: the external 5K panel runs at
60 Hz and pins any window on it at 60. The slowest frame of each run is near its start, when
the map's textures go to the GPU.

Every map averages 119 fps or more, the display's refresh rate within the measurement's
rounding, with the frames' CPU work under 1 ms on average and under 8.33 ms at its longest,
so the criterion "every id1 map renders at 120 Hz or more on the reference Mac" holds with
room to spare.

| Map | Average fps | Frames in 6 s | Slowest frame (ms) | Work per frame (ms), average | Work per frame (ms), longest |
| --- | --- | --- | --- | --- | --- |
| start | 119 | 716 | 51.7 | 0.42 | 3.44 |
| e1m1 | 119 | 717 | 40.9 | 0.33 | 2.74 |
| e1m2 | 120 | 718 | 45.2 | 0.31 | 1.08 |
| e1m3 | 119 | 716 | 49.8 | 0.64 | 2.46 |
| e1m4 | 119 | 717 | 40.8 | 0.22 | 1.65 |
| e1m5 | 119 | 716 | 46.4 | 0.70 | 4.03 |
| e1m6 | 119 | 717 | 45.1 | 0.28 | 3.01 |
| e1m7 | 119 | 716 | 50.7 | 0.26 | 0.79 |
| e1m8 | 120 | 718 | 46.9 | 0.47 | 1.44 |
| e2m1 | 119 | 716 | 49.1 | 0.64 | 3.36 |
| e2m2 | 119 | 717 | 45.6 | 0.28 | 3.54 |
| e2m3 | 120 | 718 | 48.0 | 0.24 | 1.43 |
| e2m4 | 119 | 717 | 48.6 | 0.33 | 2.31 |
| e2m5 | 119 | 716 | 52.7 | 0.29 | 2.25 |
| e2m6 | 120 | 719 | 51.3 | 0.30 | 2.61 |
| e2m7 | 120 | 718 | 46.0 | 0.36 | 2.50 |
| e3m1 | 120 | 721 | 47.7 | 0.28 | 1.90 |
| e3m2 | 119 | 716 | 55.8 | 0.22 | 1.34 |
| e3m3 | 120 | 721 | 51.5 | 0.23 | 1.71 |
| e3m4 | 120 | 718 | 45.6 | 0.26 | 4.09 |
| e3m5 | 119 | 717 | 45.0 | 0.29 | 2.08 |
| e3m6 | 119 | 717 | 49.0 | 0.29 | 2.15 |
| e3m7 | 119 | 717 | 46.3 | 0.24 | 1.27 |
| e4m1 | 119 | 717 | 46.4 | 0.28 | 2.58 |
| e4m2 | 119 | 717 | 44.7 | 0.23 | 1.27 |
| e4m3 | 120 | 718 | 42.2 | 0.39 | 3.03 |
| e4m4 | 120 | 718 | 51.3 | 0.34 | 1.99 |
| e4m5 | 120 | 718 | 43.1 | 0.39 | 3.07 |
| e4m6 | 119 | 717 | 49.2 | 0.25 | 1.56 |
| e4m7 | 119 | 717 | 43.1 | 0.33 | 2.99 |
| e4m8 | 119 | 716 | 53.8 | 0.31 | 2.26 |
| end | 120 | 719 | 44.1 | 0.17 | 0.81 |
| dm1 | 119 | 717 | 48.2 | 0.20 | 1.67 |
| dm2 | 119 | 717 | 43.7 | 0.13 | 1.52 |
| dm3 | 119 | 717 | 47.5 | 0.19 | 0.56 |
| dm4 | 119 | 716 | 38.9 | 0.20 | 0.58 |
| dm5 | 119 | 716 | 50.6 | 0.20 | 0.56 |
| dm6 | 119 | 717 | 48.4 | 0.12 | 0.54 |
