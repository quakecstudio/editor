# qs-sound

The preview host's sound engine (P3-27, #241): the sounds the mixer knows and loads, the
channels a sound plays on, its place in stereo from the listener, the ambient and static
sounds, and the integer mixer that writes 16-bit stereo frames, as id's `snd_dma.c`,
`snd_mix.c` and `snd_mem.c` play them; and the files a music track number names, as
QuakeSpasm looks them up.

- **Layer:** L4 bridges. It depends on `qs-wav` (L0) for the WAV files and `qc-vm` (L1) for
  C's conversions.
- **Spec sections:** SND-1 to SND-5 of [`sound.md`](../../docs/spec/engine/sound.md) and
  RRE-3.1 of [`rerelease.md`](../../docs/spec/engine/rerelease.md).
- **Ported from:** id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`:
  `WinQuake/snd_dma.c` (`S_StartSound`, `SND_PickChannel`, `SND_Spatialize`, `S_StopSound`,
  `S_StopAllSounds`, `S_StaticSound`, `S_UpdateAmbientSounds`, `S_Update`, `S_Update_`,
  `S_LocalSound`, `S_FindName`, `S_PrecacheSound`), `WinQuake/snd_mix.c` (`S_PaintChannels`,
  `SND_PaintChannelFrom8`, `SND_PaintChannelFrom16`, `SND_InitScaletable`,
  `S_TransferPaintBuffer`) and `WinQuake/snd_mem.c` (`S_LoadSound`, `ResampleSfx`). Each
  file's header lists its functions.
- **Shipped:** yes.

## Public shape

- `SoundTable::precache(name, speed, read)`: the sound named `name`, known from now on and
  loaded through `read`, the game filesystem, resampled to `speed` samples per second
  (SND-1.3 to SND-1.6); `loaded(id)` says why one has no data. `SoundData::resample` is
  `ResampleSfx` on its own.
- `Mixer::new(MixerSpec)`: the channels, cleared; `precache_ambients` loads the water and
  sky sounds (SND-1).
- `Mixer::start_sound(table, rand, Start)`, `stop_sound`, `local_sound` and `static_sound`:
  `S_StartSound` and its kin (SND-2, SND-4.1, SND-4.2); `rand` is the host's seeded source
  for the skip of SND-2.5.
- `Mixer::update(table, listener, leaf, frametime)`: `S_Update` for one server tick: the
  ambient channels follow the leaf's levels, every channel is spatialized, static sounds
  combine, and the mixer paints `mixahead` seconds past the frames the tick played (SND-3,
  SND-4.3, SND-4.4, SND-5.1, SND-5.5). `take_output` gives the frames painted since the last
  call, left, right pairs; `take_messages` what the mixer would print.
- `Output` and `NullOutput`: where the frames go; the null output counts them for a headless
  run.
- `track_files(track, extensions)`: the files `music/track<NN>.<ext>` a CD track number names
  (RRE-3.1).

## What it is not, yet

The frames go nowhere but the null output: the preview host app (P3-02) plays them through
`cpal`, and decodes music tracks through `symphonia`, both chosen there, since neither is
Quake behaviour. The WAV reader and the mixer stay ports (`qs-wav`, this crate) because the
engine differential compares the mixed frames with id's engine sample for sample (SND-5.4).
The engine oracle's audio buffers (P3-23, #237) are not compared yet: every statement is
pinned by its own test against the arithmetic id's source states.

## Invariants the tests pin

- A channel never reads past its sound: a sample past the data is silence (SND-1.6).
- Every arithmetic step is the C's: truncation toward zero where a float is stored in an
  int, arithmetic shifts of negative sums, the scale table for 8-bit samples (SND-5.3).
- `S_StopSound` scans channels 0 to 7, as id's loop bound is the dynamic count (SND-2.6).
