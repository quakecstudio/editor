# Sound

Prefix: `SND` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the sound engine the preview host plays through (P3-27, #241): the channels a sound
plays on, how one is started, stopped and placed in stereo, the ambient and static sounds, and
the mixer that writes the output samples, as implemented by `WinQuake/snd_dma.c`,
`snd_mix.c` and `snd_mem.c` in [id-Software/Quake](https://github.com/id-Software/Quake) at
commit `bf4ac424ce754894ac8f1dae6a3981954bc9852d`. The WAV file a sound comes from is
[`../wav.md`](../wav.md). To be implemented by `qs-sound`.

Time in this spec is counted in output sample frames (one `left`, `right` pair): `paintedtime`
is the frame the mixer has written up to and `soundtime` the frame the output device has played
up to. `shm->speed` is the output rate and `shm->channels` 1 or 2. A volume is an `int` from 0
to 255 unless a statement says otherwise, and a float assigned to an `int` truncates toward
zero. The listener is `cl.viewentity`, the player's own entity. The platform's output device,
its DMA buffer and the `-nosound`, `-simsound` and `loadas8bit` switches are not described: the
preview host mixes 16-bit stereo at one rate it chooses (SND-5.4) and the headless mode keeps
`soundtime` in step with the server clock (SND-5.5).

## SND-1 Channels and loaded sounds

**Source:** `WinQuake/sound.h`; `WinQuake/snd_dma.c`, `S_Init`, `S_FindName`,
`S_PrecacheSound`, `S_StopAllSounds`; `WinQuake/snd_mem.c`, `S_LoadSound`, `ResampleSfx`.

### Behaviour

- **SND-1.1** The mixer has `SND_MAX_CHANNELS` channels in one array: the `SND_NUM_AMBIENTS`
  ambient channels first, at the indices `SND_AMBIENT_WATER`, `SND_AMBIENT_SKY`,
  `SND_AMBIENT_SLIME` and `SND_AMBIENT_LAVA`; then the `SND_MAX_DYNAMIC_CHANNELS` dynamic
  channels; then the static channels, from index `SND_NUM_AMBIENTS + SND_MAX_DYNAMIC_CHANNELS`
  up to `total_channels`. `S_StopAllSounds` sets `total_channels` to that first static index,
  so no static sound is left, and clears every channel to zeros (no sound, volumes 0, `end`
  0).
- **SND-1.2** A channel holds the sound it plays (`sfx`, none when cleared), `leftvol` and
  `rightvol`, `end` (the `paintedtime` at which it ends), `pos` (the next sample of the sound
  to mix), `entnum` and `entchannel` (the entity and the entity channel it was started on),
  `origin`, `dist_mult` and `master_vol`.
- **SND-1.3** A sound is known by its name, at most `MAX_QPATH - 1` bytes (a longer one is the
  fatal error `Sound name too long: <name>`), in a table of at most `SND_MAX_SFX` names (a
  new name past that is the fatal error `S_FindName: out of sfx_t`); `S_PrecacheSound`
  returns the entry for a name, adding it when it is new, and loads its data when `precache`
  is not 0 (it is 1 by default).
- **SND-1.4** `S_LoadSound` reads the file `sound/<name>` through the game filesystem and
  `GetWavinfo` (WAV-1 to WAV-3). A missing file prints `Couldn't load sound/<name>` and the
  sound has no data; one with other than one channel prints `<name> is a stereo sample` and
  has no data. Either way the sound stays known and every later start of it is silent
  (SND-2.4).
- **SND-1.5** The samples are resampled to the output rate: `stepscale = rate / speed` (a
  `float` division of the file's rate by the output rate), `length = samples / stepscale`
  truncated to an int, and a `loopstart` that is not -1 is `loopstart / stepscale` truncated
  the same way. The sample width stays the file's (1 or 2 bytes) and the loaded sound is mono.
- **SND-1.6** Output sample `i` (0 to `length - 1`) is input sample `(i * fracstep) >> 8`
  where `fracstep = (int) (stepscale * 256)` and the product is accumulated in an int
  (`samplefrac += fracstep` each sample, starting at 0), so a rate above the output rate
  decimates and one below repeats samples. An 8-bit input sample `b` is read as
  `(b - 128) << 8`, a 16-bit one as its little-endian value; the output is the 16-bit value
  for width 2 and `sample >> 8` for width 1. With `stepscale == 1` and width 1 the output is
  `b - 128` directly, the same value.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `SND_MAX_CHANNELS` | 128 | `sound.h`, `MAX_CHANNELS` |
| `SND_MAX_DYNAMIC_CHANNELS` | 8 | `sound.h`, `MAX_DYNAMIC_CHANNELS` |
| `SND_NUM_AMBIENTS` | 4 | `bspfile.h`, `NUM_AMBIENTS` |
| `SND_AMBIENT_WATER` | 0 | `bspfile.h`, `AMBIENT_WATER` |
| `SND_AMBIENT_SKY` | 1 | `bspfile.h`, `AMBIENT_SKY` |
| `SND_AMBIENT_SLIME` | 2 | `bspfile.h`, `AMBIENT_SLIME` |
| `SND_AMBIENT_LAVA` | 3 | `bspfile.h`, `AMBIENT_LAVA` |
| `SND_MAX_SFX` | 512 | `snd_dma.c`, `MAX_SFX` |
| `SND_AMBIENT_WATER_SOUND` | `ambience/water1.wav` | `snd_dma.c`, `S_Init` |
| `SND_AMBIENT_SKY_SOUND` | `ambience/wind2.wav` | `snd_dma.c`, `S_Init` |

The slime and lava ambient channels have no sound: `S_Init` precaches only the two above, so
`ambient_sfx[AMBIENT_SLIME]` and `ambient_sfx[AMBIENT_LAVA]` stay null and those channels
are silent whatever a leaf's level for them (SND-4.3).

## SND-2 Starting and stopping a sound

**Source:** `WinQuake/snd_dma.c`, `S_StartSound`, `SND_PickChannel`, `S_StopSound`,
`S_LocalSound`.

`S_StartSound (entnum, entchannel, sfx, origin, fvol, attenuation)` is what a server
`svc_sound` message (CLIENT-4) and the client's own sounds call.

### Behaviour

- **SND-2.1** Nothing is started for a null `sfx` or while `nosound` is not 0. The master
  volume is `vol = (int) (fvol * 255)`.
- **SND-2.2** `SND_PickChannel (entnum, entchannel)` scans the dynamic channels in index order
  and takes the first whose `entnum` is the entity's and whose `entchannel` is the requested
  one, or any channel of that entity when the requested `entchannel` is -1; an `entchannel` of
  0 never takes over this way ("channel 0 never overrides"). Failing that it takes the channel
  with the least life left, `end - paintedtime`, the first on a tie, skipping a channel that
  plays a sound of the view entity when `entnum` is not the view entity ("don't let monster
  sounds override player sounds"). When every dynamic channel is skipped that way there is no
  channel and nothing is started. The channel taken loses its sound.
- **SND-2.3** The channel is cleared, then set: `origin`, `dist_mult = attenuation /
  SND_NOMINAL_CLIP_DIST`, `master_vol = vol`, `entnum` and `entchannel`; it is spatialized
  (SND-3). When both `leftvol` and `rightvol` come out 0 nothing more happens: the channel
  stays taken, with no sound.
- **SND-2.4** The sound's data is loaded (SND-1.4); when it has none the channel has no
  sound. Otherwise the channel plays it: `pos = 0` and `end = paintedtime + length`.
- **SND-2.5** Then the other dynamic channels are scanned in index order for one playing the
  same sound at `pos` 0 (an identical sound started this frame); for the first found,
  `skip = rand () % (int) (0.1 * speed)`, lowered to `end - 1` when it is not below `end`, is
  added to the new channel's `pos` and taken from its `end`, "to keep it from just making the
  first one louder". `rand ()` is the C library's; the preview host draws `skip` from its own
  seeded source (DIFF-1.4) so a run is deterministic, and the engine differential compares
  this case with `skip` reported, not matched.
- **SND-2.6** `S_StopSound (entnum, entchannel)` scans channels 0 to
  `SND_MAX_DYNAMIC_CHANNELS - 1` (the four ambient channels and the first four dynamic ones:
  the loop bound is the dynamic count, not the dynamic range) and the first whose `entnum`
  and `entchannel` match gets `end = 0` and no sound; nothing else is stopped.
- **SND-2.7** `S_LocalSound (name)` precaches `name` and starts it on the view entity, channel
  -1, at the origin, volume 1 and attenuation 1; a sound that cannot be cached prints
  `S_LocalSound: can't cache <name>`.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `SND_NOMINAL_CLIP_DIST` | 1000.0 | `snd_dma.c`, `sound_nominal_clip_dist` |
| `SND_PACKET_VOLUME` | 255 | `sound.h`, `DEFAULT_SOUND_PACKET_VOLUME` |
| `SND_PACKET_ATTENUATION` | 1.0 | `sound.h`, `DEFAULT_SOUND_PACKET_ATTENUATION` |

## SND-3 Spatialization

**Source:** `WinQuake/snd_dma.c`, `SND_Spatialize`.

### Behaviour

- **SND-3.1** A channel of the view entity plays at full volume: `leftvol = rightvol =
  master_vol`.
- **SND-3.2** Any other: `source_vec = origin - listener_origin`, normalised, with
  `dist = |source_vec| * dist_mult`; a zero-length vector stays zero. `dot = listener_right
  . source_vec`. With one output channel `lscale = rscale = 1`; with two, `rscale = 1 + dot`
  and `lscale = 1 - dot`. Then `rightvol = (int) (master_vol * (1 - dist) * rscale)` and
  `leftvol = (int) (master_vol * (1 - dist) * lscale)`, each raised to 0 when negative. So a
  sound at `dist` 1 or more (at or past `SND_NOMINAL_CLIP_DIST / attenuation` units) is silent,
  and one straight to the right is silent on the left.

## SND-4 Static and ambient sounds

**Source:** `WinQuake/snd_dma.c`, `S_StaticSound`, `S_UpdateAmbientSounds`, `S_Update`.

### Behaviour

- **SND-4.1** `S_StaticSound (sfx, origin, vol, attenuation)`, from `svc_spawnstaticsound`
  (CLIENT-4), does nothing for a null `sfx` and prints `total_channels == MAX_CHANNELS` and
  does nothing when every channel is taken. Otherwise the channel at `total_channels` is taken
  (`total_channels` grows by one) before the sound's data is loaded: a sound with no data
  leaves the channel empty but counted, and one whose `loopstart` is -1 prints
  `Sound <name> not looped` and does the same.
- **SND-4.2** A looped static sound sets the channel's `sfx`, `origin`, `master_vol = (int)
  vol`, `dist_mult = (attenuation / 64) / SND_NOMINAL_CLIP_DIST` and `end = paintedtime +
  length`, and spatializes it (SND-3).
- **SND-4.3** Each update, the ambient channels follow the leaf the listener is in (BSP-6):
  with no world model nothing changes; with no leaf (the listener outside the world) or
  `ambient_level` 0, every ambient channel loses its sound. Otherwise ambient channel `i`
  plays the ambient sound of that index (SND-1), at the target `vol = ambient_level *
  leaf.ambient_level[i]` (a float, 0 when below 8), and its `master_vol` moves toward `vol`
  by `frametime * ambient_fade` per update, as an int, never past `vol`; `leftvol` and
  `rightvol` are `master_vol`. The step truncates to an int, so an update shorter than
  `1 / ambient_fade` seconds (10 ms by default) moves the volume by 0: the preview host runs
  this update once per server tick (P3-02), never per drawn frame.
- **SND-4.4** `S_Update (origin, forward, right, up)` stores the listener, updates the ambient
  channels (SND-4.3), then spatializes every channel from the first dynamic one to
  `total_channels - 1` that has a sound. A static channel (index at or past the first static
  one) that is audible after that is combined with an earlier static channel playing the same
  sound ("so we don't mix five torches every frame"): its volumes are added to that channel's
  and its own become 0. The earlier channel is the one found last for this sound when it plays
  the same sound, else the first static channel before this one with the same sound; when
  none is found the channel keeps its volumes and becomes the one found last.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `SND_AMBIENT_LEVEL` | 0.3 | `snd_dma.c`, `ambient_level` default |
| `SND_AMBIENT_FADE` | 100 | `snd_dma.c`, `ambient_fade` default |
| `SND_AMBIENT_MIN_VOLUME` | 8 | `snd_dma.c`, `S_UpdateAmbientSounds` (`if (vol < 8) vol = 0`) |
| `SND_STATIC_ATTENUATION_DIVISOR` | 64 | `snd_dma.c`, `S_StaticSound` |

## SND-5 Mixing

**Source:** `WinQuake/snd_mix.c`, `S_PaintChannels`, `SND_PaintChannelFrom8`,
`SND_PaintChannelFrom16`, `SND_InitScaletable`, `S_TransferPaintBuffer`,
`S_TransferStereo16`; `WinQuake/snd_dma.c`, `S_Update_`, `GetSoundtime`.

### Behaviour

- **SND-5.1** **Withdrawn** 2026-11-03: it truncated `_snd_mixahead * speed` to an int before
  the sum; the sum is a single-precision float, truncated as a whole. Replaced by SND-5.6.
- **SND-5.2** `S_PaintChannels (endtime)` works in blocks of at most `SND_PAINTBUFFER_SIZE`
  frames. Each block starts as zeros and every channel from 0 to `total_channels - 1` with a
  sound and a volume other than 0 on either side is added into it, in index order: a channel
  whose data cannot be loaded is skipped. From the block's start, `count = min (end,
  ch.end) - ltime` frames are painted and `pos` advances by `count`; when `ltime` reaches
  `ch.end`, a looped sound (`loopstart` not -1) restarts with `pos = loopstart` and
  `ch.end = ltime + length - loopstart`, and any other stops (the channel loses its sound).
- **SND-5.3** An 8-bit sound adds `scaletable[vol >> 3][sample]` on each side, where
  `vol` is that side's volume capped at 255 and `scaletable[i][j] = (signed char) j * i * 8`;
  a 16-bit sound adds `(sample * vol) >> 8`, with `vol` not capped. The sums are 32-bit ints.
- **SND-5.4** A painted frame goes to the output as `(sum * (int) (volume * 256)) >> 8`,
  clamped to the 16-bit range, with `volume` the master volume setting (0.7 by default); an
  8-bit output takes the high byte plus 128. The preview host writes 16-bit stereo at its own
  rate; the engine differential compares the paint buffers of the two engines at the oracle's
  rate, 11025 Hz, sample for sample, since every step above is integer arithmetic on the same
  loaded data, except where SND-2.5 drew a skip.
- **SND-5.5** **Withdrawn** 2026-11-03: it advanced the headless clock by a truncated step per
  frame, which drifts behind the run's time by up to a frame per step. Replaced by SND-5.9.
- **SND-5.6** `S_Update_` mixes from `paintedtime`, first raised to `soundtime` when it fell
  behind ("overshot"), up to `endtime = soundtime + _snd_mixahead * speed`, a single-precision
  sum (`soundtime` and `speed` converted to float) stored in an unsigned int, lowered so that
  `endtime - soundtime` is at most the output buffer's frames.
- **SND-5.7** `SND_PaintChannelFrom8` and `SND_PaintChannelFrom16` write `paintbuffer[0]`
  onward whatever `ltime` is, so when a looped sound restarts inside a block (`ltime` past
  `paintedtime`) the frames after the restart are added over the block's first frames rather
  than at `ltime - paintedtime`, and the channel is silent for the rest of the block.
  QuakeSpasm (`snd_mix.c`, `paintbufferstart`) adds them at `ltime - paintedtime`. The preview
  host does so by default (`LoopPaint::Fixed`) and reproduces id's placement under
  `LoopPaint::Id`, which the engine differential selects.
- **SND-5.8** A looped sound whose `loopstart` is at or past its `length` (a loop point in
  the last input samples, truncated by the resampling of SND-1.5 to the length) never ends in
  id's engine: `ch.end = ltime + length - loopstart` does not pass `ltime`, so
  `S_PaintChannels` restarts it forever and the game hangs. The preview host stops the channel
  instead.
- **SND-5.9** `soundtime` is the frames the device has played; in the preview host's headless
  mode it is `(int) (elapsed * speed)`, with `elapsed` the seconds the run has stepped (a
  double), so a scripted run mixes the same frames on every machine.
- **SND-5.10** `S_TransferPaintBuffer` on a one-channel output steps through the paint buffer
  two ints at a time (`step = 3 - channels`), so each frame's left sum alone is written, scaled
  and clamped as in SND-5.4.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `SND_PAINTBUFFER_SIZE` | 512 | `snd_mix.c`, `PAINTBUFFER_SIZE` |
| `SND_DEFAULT_VOLUME` | 0.7 | `snd_dma.c`, `volume` default |
| `SND_MIXAHEAD` | 0.1 | `snd_dma.c`, `_snd_mixahead` default |
| `SND_ORACLE_SPEED` | 11025 | `snd_dma.c`, `desired_speed` |

## SND-6 Music

**Source:** sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`: `Quake/snd_dma.c`,
`S_RawSamples`; `Quake/snd_mix.c`, `S_PaintChannels`; `Quake/bgmusic.c`, `BGM_PlayCDtrack`,
`BGM_Stop`, `BGM_UpdateStream`, `BGM_Update`. id's engine plays its music from the CD drive,
which mixes it itself; QuakeSpasm streams it through the mixer, and the preview host does as
QuakeSpasm does.

### Behaviour

- **SND-6.1** `S_RawSamples (samples, rate, width, channels, data, volume)` raises `s_rawend` to
  `paintedtime`, then for `i` from 0 takes `src = i * scale`, `scale = (float) rate /
  shm->speed`, until `src` reaches `samples`, storing frame `src` at `s_rawend &
  (MAX_RAW_SAMPLES - 1)` multiplied by `intVolume = (int) (256 * volume)` and advancing
  `s_rawend`; a one-channel frame is both sides. `MAX_RAW_SAMPLES` is 8192. The preview host
  streams only 16-bit samples (`width` 2).
- **SND-6.2** `BGM_UpdateStream`, run before `S_Update` each frame, does nothing when the
  stream is not playing or "`bgmvolume.value <= 0`". Otherwise, while `s_rawend` is less than
  `paintedtime + MAX_RAW_SAMPLES`, it reads `bufferSamples * rate / shm->speed` frames, at
  most 16384 bytes, into `S_RawSamples` at `bgmvolume`. At the stream's end a looping stream
  rewinds, and a second end with nothing read between prints "`Stream keeps returning EOF.`"
  and stops; a stream that cannot rewind prints "`Stream seek error (%i), stopping.`"; one
  not looping stops; a read error prints "`Stream read error (%i), stopping.`" and stops.
  `BGM_Stop` closes the stream and sets `s_rawend = 0`. The preview host's seek error is -1.
- **SND-6.3** QuakeSpasm's `S_PaintChannels` adds the raw frames from `paintedtime` to the
  lesser of the block's end and `s_rawend`, when `s_rawend >= paintedtime`, after the
  channels, each side `s_rawsamples[s].left / 2` ("lower music by 6db to match sfx"). Its
  paint buffer is 256 times id's: `SND_PaintChannelFrom16` folds `snd_vol` into the channel's
  volume and drops id's `>> 8`, it halves the channels' sum ("reduce by 6dB"), and its
  transfer writes `val = *p / 256` with no `snd_vol`. So a sound effect comes out at half
  id's level and a music sample at `sample * bgmvolume / 2`, unscaled by `volume`. The preview
  host keeps id's paint and transfer (SND-5.4) and keeps QuakeSpasm's balance: each frame's
  music, `s_rawsamples[s] >> 8`, is added to the transferred `(sum * snd_vol) >> 8` before the
  clamp to 16 bits, our choice.
- **SND-6.4** QuakeSpasm's `CDAudio_Play` plays a disc in a CD drive; the preview host plays a
  CD image instead, our choice: a cue sheet naming one `BINARY` file, whose `AUDIO` tracks
  each run from their `INDEX 01` to the next track's `INDEX 00`, else its `INDEX 01`, the last
  to the file's end, an index `mm:ss:ff` being `(mm * 60 + ss) * 75 + ff` sectors of 2352
  bytes, `PREGAP` adding nothing to the file. Its audio is 16-bit little-endian stereo at
  44100 Hz. A sheet with no binary file, more than one file, or a time it cannot read is not
  used. The cue sheet is `--cd-image`, else `game.cue` in the game root, as GOG's release of
  Quake keeps its CD; `--nocdaudio` plays none. A capture for the view differential plays no
  music, since id's engine there has no CD.
- **SND-6.5** A mission pack plays its own CD from GOG's release, in place of `game.cue`:
  `gamea.cue` for Scourge of Armagon, with `-hipnotic` or `-game hipnotic`, and `gamed.cue`
  for Dissolution of Eternity, with `-rogue` or `-game rogue`, the directory's name matched
  without case. The names are GOG's, not id's: GOG's forum guide to extracting the music
  (`gog.com/forum/quake_series/my_oneclick_audioextractor/post1`, fetched 2026-11-10) says
  "GAMEA is for Mission Pack 1 - Scourge of Armagon ... the HIPNOTIC folder" and "GAMED
  Mission Pack 2 - Dissolution of Eternity ... the ROGUE folder".

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `SND_MAX_RAW_SAMPLES` | 8192 | `q_sound.h`, `MAX_RAW_SAMPLES` |
| `SND_CD_RATE` | 44100 | the CD audio format |
| `SND_CD_SECTOR` | 2352 | the CD audio format |
