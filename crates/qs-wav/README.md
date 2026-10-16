# qs-wav

Reads the RIFF WAVE files Quake plays its sounds from, loop points included. `get_wavinfo` is
id's `GetWavinfo` with the bounds checks id's engine leaves out: it finds the `RIFF`/`WAVE`,
`fmt `, `cue `, `LIST` and `data` chunks the engine reads and returns their values as a
`WavInfo`, and rejects malformed input with a typed `WavError`, never a panic.
`s_load_sound` adds the checks of id's `S_LoadSound` (one channel) and those this project
fixes where id's code is undefined (a width of 1 or 2 bytes, a positive rate), and returns
the sample bytes as a zero-copy `Sound`. Resampling and mixing are the sound system's.
`s_load_sound_path` and `sound_name` say where `S_LoadSound` looks: `sound/<name>`, never
anywhere else (WAV-3.5).
Ported from `WinQuake/snd_mem.c` at the commit pinned in
[`docs/porting.md`](../../docs/porting.md).

- **Layer:** L0 formats, with no dependencies. The `qs-vfs` example `walk` decodes every
  `.wav` under an install's `sound/` with it.
- **Spec sections:** WAV-1 to WAV-3 in [`wav.md`](../../docs/spec/wav.md).
- **Shipped:** yes.

## Invariants

Property tests in `src/tests.rs` and the fuzz target `fuzz/fuzz_targets/qs_wav.rs`
(`cargo xtask fuzz qs-wav`):

- Arbitrary bytes never panic the reader or `Sound::sample` (WAV-1 to WAV-3).
- A sound's sample bytes always lie inside the file, whatever its `data` chunk declares
  (WAV-2.12).

## Differential result

id's `GetWavinfo` and its chunk functions, built from the pinned commit, and `qs-wav` agree
on every vector in WAV-3 and on all 226 sounds of LibreQuake's `lite` `id1` (2026-10-16;
the method is in the spec's test vectors).
