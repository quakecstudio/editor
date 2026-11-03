# id1 maps in the renderer on the reference Mac (P2-152)

Measured on 2026-11-02 at 4f8b26a5 on the reference Mac (Apple M5, built-in Liquid Retina
XDR display, 120 Hz), with the viewer's benchmark mode: each map opened from Justin's GOG
copy of Quake, presented without vsync, the camera at the player's start turning slowly for
six seconds, the average frame rate and the slowest frame printed when the time was up.

```
for m in start e1m1 ... dm6; do
  cargo run --release -p qs-render --example view -- \
    -basedir ~/Games/quake-gog/app -benchmark 6 -position 2700 200 "$m"
done
```

`-position 2700 200` opens the window on the built-in display; on the external 5K display,
a 60 Hz panel, macOS paces the frames at 60 whatever the present mode. The slowest frame of
each run is its first, which loads the map's textures to the GPU.

Every map averages 119 fps or more, the display's refresh rate within the measurement's
rounding, so the criterion "every id1 map renders at 120 Hz or more on the reference Mac"
holds.

| Map | Average fps | Frames in 6 s | Slowest frame (ms) |
| --- | --- | --- | --- |
| start | 123 | 739 | 44.8 |
| e1m1 | 122 | 730 | 59.5 |
| e1m2 | 119 | 717 | 45.3 |
| e1m3 | 119 | 715 | 56.4 |
| e1m4 | 119 | 718 | 55.7 |
| e1m5 | 121 | 728 | 48.3 |
| e1m6 | 119 | 718 | 49.3 |
| e1m7 | 120 | 718 | 42.2 |
| e1m8 | 119 | 716 | 43.1 |
| e2m1 | 119 | 715 | 55.7 |
| e2m2 | 119 | 716 | 54.4 |
| e2m3 | 120 | 718 | 46.6 |
| e2m4 | 119 | 717 | 46.8 |
| e2m5 | 119 | 714 | 47.2 |
| e2m6 | 119 | 715 | 52.4 |
| e2m7 | 119 | 717 | 43.4 |
| e3m1 | 119 | 717 | 54.1 |
| e3m2 | 119 | 717 | 49.9 |
| e3m3 | 119 | 715 | 56.0 |
| e3m4 | 119 | 714 | 55.7 |
| e3m5 | 120 | 719 | 42.7 |
| e3m6 | 119 | 717 | 52.7 |
| e3m7 | 119 | 717 | 56.8 |
| e4m1 | 119 | 717 | 48.5 |
| e4m2 | 119 | 716 | 54.6 |
| e4m3 | 119 | 715 | 54.7 |
| e4m4 | 119 | 715 | 54.8 |
| e4m5 | 120 | 718 | 43.4 |
| e4m6 | 120 | 719 | 39.6 |
| e4m7 | 119 | 715 | 55.2 |
| e4m8 | 119 | 716 | 45.8 |
| end | 119 | 717 | 47.4 |
| dm1 | 119 | 716 | 46.9 |
| dm2 | 119 | 716 | 49.5 |
| dm3 | 119 | 715 | 49.2 |
| dm4 | 119 | 717 | 48.8 |
| dm5 | 119 | 716 | 47.2 |
| dm6 | 119 | 717 | 47.6 |
