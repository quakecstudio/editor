# WAV sounds

Prefix: `WAV` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the subset of RIFF WAVE files the engine reads, loop points included, as
implemented by `WinQuake/snd_mem.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. Every sound the game plays is a `.wav` under
`sound/`; ambient sounds (`ambience/*.wav`) loop from a point a cue chunk gives. Implemented
by `crates/qs-wav` ([#235](https://github.com/quakecstudio/editor/issues/235)).

`GetWavinfo` reads the file through a few global pointers: `iff_data` where a chunk search
starts, `iff_end` the end of the file, `last_chunk` where the next search continues, and
`data_p` the read position. `GetLittleShort` and `GetLittleLong` read signed 16-bit and
32-bit little-endian integers at `data_p` and advance it. Offsets below are from the start
of the chunk named, whose first byte is its 4-byte id.

## WAV-1 Chunks

**Source:** `WinQuake/snd_mem.c`, `FindNextChunk` and `FindChunk`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **WAV-1.1** A chunk is a 4-byte id, a signed 32-bit little-endian length, and that many
  bytes of data. The next chunk starts `8 + ((length + 1) & ~1)` bytes after the chunk's
  start: the data is padded to an even length.
- **WAV-1.2** Finding a chunk walks the chunks from a start offset to the end of the file
  and returns the first whose id equals the four bytes asked for, compared exactly (case
  matters). The lengths of enclosing chunks, including the `RIFF` chunk's, are not used:
  every search runs to the end of the file.
- **WAV-1.3** A search stops, finding nothing, at a chunk whose length is negative, or when
  the next chunk would start at or past the end of the file.
- **WAV-1.4** A search also stops, finding nothing, when fewer than 8 bytes are left for a
  chunk header. id's code reads the header past the end of the buffer, which is undefined;
  this spec fixes the answer as not found.
- **WAV-1.5** The next chunk's offset is computed without overflow: a length of 2147483647
  puts the next chunk past the end of the file, so the search stops. In id's code
  `length + 1` overflows a signed integer there, which is undefined.

## WAV-2 Reading the header

**Source:** `WinQuake/snd_mem.c`, `GetWavinfo`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`GetWavinfo` returns a `wavinfo_t`: `rate`, `width` (bytes per sample), `channels`,
`loopstart`, `samples` and `dataofs` (the offset of the first sample). On the first three
errors below it prints the message and returns the structure zeroed, and `S_LoadSound` then
rejects it as WAV-3.1 says.

| Chunk | Offset | Size | Field |
| --- | --- | --- | --- |
| `RIFF` | 8 | 4 | The form type, `WAVE` |
| `fmt ` | 8 | 2 | Format tag; 1 is PCM |
| `fmt ` | 10 | 2 | `channels` |
| `fmt ` | 12 | 4 | `rate`, samples per second |
| `fmt ` | 22 | 2 | Bits per sample |
| `cue ` | 32 | 4 | `loopstart`: the sample offset of the first cue point |
| `LIST` | 24 | 4 | The number of samples in the loop |
| `LIST` | 28 | 4 | `mark`, when the chunk holds a loop length |
| `data` | 4 | 4 | The chunk's length, in bytes |
| `data` | 8 | | The first sample |

### Behaviour

- **WAV-2.1** The file must contain a `RIFF` chunk, found by WAV-1 from the start of the
  file, whose bytes 8 to 11 are `WAVE`. Otherwise reading fails with the error
  `Missing RIFF/WAVE chunks`. The `RIFF` chunk need not be the first chunk in the file, and
  one that ends before byte 12 is missing.
- **WAV-2.2** Every later search starts 12 bytes after the start of the `RIFF` chunk.
- **WAV-2.3** A `fmt ` chunk must be found, or reading fails with the error
  `Missing fmt chunk`. Its format tag must be 1, or reading fails with the error
  `Microsoft PCM format only`.
- **WAV-2.4** `channels` is the signed 16-bit value at offset 10 of the `fmt ` chunk, `rate`
  the signed 32-bit value at 12 and `width` the signed 16-bit value at 22 divided by 8, with
  C integer division truncating toward zero (16 bits give 2, 12 give 1, 7 give 0). The `fmt `
  chunk's own length is not checked: a short chunk's fields come from the bytes after it.
- **WAV-2.5** A `fmt ` field that lies past the end of the file fails reading with the error
  `Missing fmt chunk`. id's code reads past the buffer, which is undefined; this spec fixes
  the answer as an error.
- **WAV-2.6** If a `cue ` chunk is found, `loopstart` is the signed 32-bit value at offset 32
  of it, the sample offset of the first cue point; the number of cue points and the chunk's
  length are not checked. With no `cue ` chunk, `loopstart` is -1. A value past the end of
  the file is read as if there were no `cue ` chunk (id's code reads past the buffer, which
  is undefined).
- **WAV-2.7** After a `cue ` chunk, a `LIST` chunk is searched for from the chunk after the
  `cue ` chunk. If one is found and its bytes 28 to 31 are `mark`, the loop's sample count
  `samples = loopstart + n`, where `n` is the signed 32-bit value at offset 24. Bytes 28 to 31
  past the end of the file mean no `mark`.
- **WAV-2.8** A `data` chunk must be found, or reading fails with the error
  `Missing data chunk`. id's code prints the message and returns what it has read, so the
  engine then plays the bytes at the start of the file as samples; this spec fixes the
  answer as an error.
- **WAV-2.9** `width` 0 fails reading with the error `Sound <name> has a bad sample width`.
  id's code divides the `data` length by it, which on x86 stops the program with a divide
  error.
- **WAV-2.10** The `data` chunk's sample count is its length divided by `width`, truncating
  toward zero, and `dataofs` is 8 bytes after the start of the chunk. When WAV-2.7 gave a
  non-zero `samples`, the data's count must be at least that, or reading fails with the
  error `Sound <name> has a bad loop length` (fatal in id's engine); otherwise `samples` is
  the data's count.
- **WAV-2.11** A loop length from WAV-2.7 that is negative, or whose sum overflows a signed
  32-bit integer, fails reading with the error `Sound <name> has a bad loop length`. id's
  code overflows a signed integer, which is undefined, or keeps a negative length the
  mixer cannot play; this spec fixes both as an error.
- **WAV-2.12** The `data` chunk's length is not checked against the file. `samples` counts
  what the chunk declares; a reader gets only the sample bytes that lie inside the file,
  and the samples after them are silence. id's code reads past the buffer there, which is
  undefined.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `WAV_CHUNK_HEADER_SIZE` | 8 | `WinQuake/snd_mem.c`, `FindNextChunk`, `data_p += 4` and `GetLittleLong` |
| `WAV_FORMAT_PCM` | 1 | `WinQuake/snd_mem.c`, `GetWavinfo`, `format != 1` |
| `WAV_CUE_LOOPSTART_OFFSET` | 32 | `WinQuake/snd_mem.c`, `GetWavinfo`, `data_p += 32` after the `cue ` chunk |
| `WAV_LIST_LENGTH_OFFSET` | 24 | `WinQuake/snd_mem.c`, `GetWavinfo`, `data_p += 24` in the `LIST` chunk |
| `WAV_LIST_MARK_OFFSET` | 28 | `WinQuake/snd_mem.c`, `GetWavinfo`, `strncmp (data_p + 28, "mark", 4)` |
| `WAV_NO_LOOP` | -1 | `WinQuake/snd_mem.c`, `GetWavinfo`, `info.loopstart = -1` |

### Test vectors

See the table in WAV-3.

## WAV-3 What the engine plays

**Source:** `WinQuake/snd_mem.c`, `S_LoadSound` and `ResampleSfx`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`S_LoadSound` loads `sound/<name>` through the game filesystem, calls `GetWavinfo`, and
resamples the samples to the output rate with `ResampleSfx`. Resampling and mixing belong to
the preview host's sound system, not to the reader.

### Behaviour

- **WAV-3.1** A sound plays only if `channels` is 1. Any other value, including the 0 a
  failed `GetWavinfo` leaves, makes the engine print `<name> is a stereo sample` and not
  load the sound (not fatal).
- **WAV-3.2** A sample is 8-bit unsigned with 128 as silence when `width` is 1, and 16-bit
  signed little-endian when `width` is 2. Any other `width` fails with the error
  `<name> has a bad sample width`: `ResampleSfx` reads any width but 2 as 8-bit samples while
  keeping the width as given, so id's engine has no single meaning for it, and this spec
  fixes the answer as an error.
- **WAV-3.3** `rate` must be greater than 0, or loading fails with the error
  `<name> has a bad sample rate`. id's code divides the sample count by `rate` divided by
  the output rate and converts the result to an integer, which for 0 or less is undefined
  or a negative allocation; this spec fixes the answer as an error.
- **WAV-3.4** `loopstart` -1 means the sound does not loop; any other value is the sample
  the sound loops back to, and is not checked against `samples`.

### Test vectors

Checked on 2026-10-16 against id's code: `GetLittleShort`, `GetLittleLong`,
`FindNextChunk`, `FindChunk` and `GetWavinfo` copied unchanged from the pinned commit,
built with the system C compiler (Apple clang 21, arm64), with `Con_Printf` and `Sys_Error` printing their message.
Each vector is a file built by a script; the result is the `wavinfo_t` as `rate`, `width`,
`channels`, `loopstart`, `samples` and `dataofs`, or the message printed.

| Input | Result | Statements |
| --- | --- | --- |
| `RIFF`/`WAVE`, `fmt ` of 16 bytes (PCM, 1 channel, 11025, 8 bits), `data` of 5 bytes | 11025, 1, 1, -1, 5, 44 | WAV-1.1, WAV-2.1, WAV-2.4, WAV-2.10 |
| The same with 16 bits | 11025, 2, 1, -1, 2, 44 | WAV-2.4, WAV-2.10 |
| A `junk` chunk of 3 bytes before `fmt ` | 11025, 1, 1, -1, 5, 56 | WAV-1.1, WAV-1.2 |
| A `cue ` chunk with sample offset 3 before `data` | loopstart 3, samples 5 | WAV-2.6 |
| The same, then a `LIST` chunk with `mark` and length 2 | loopstart 3, samples 5 | WAV-2.7, WAV-2.10 |
| The same with a `mark` length of 4 (3 + 4 > 5) | `Sound t.wav has a bad loop length` | WAV-2.10 |
| Form type `WAVX` | `Missing RIFF/WAVE chunks` | WAV-2.1 |
| No `fmt ` chunk | `Missing fmt chunk` | WAV-2.3 |
| Format tag 3 | `Microsoft PCM format only` | WAV-2.3 |
| No `data` chunk | `Missing data chunk`, then the partial header 11025, 1, 1, -1, 0, 0 | WAV-2.8 |
| 2 channels | 11025, 1, 2, -1, 5, 44 (the engine then rejects it) | WAV-2.4, WAV-3.1 |
| A `data` length of 1000 in a 50-byte file | 11025, 1, 1, -1, 1000, 44 | WAV-2.12 |

`qs-wav` gives the same values for every row, and the errors of WAV-2.1, WAV-2.3, WAV-2.8 and
WAV-2.10 where id's code prints or stops; its unit tests check each row. The same build and
`qs-wav` read all 226 sounds in LibreQuake's `lite` `id1` (the `librequake-lite` test
asset, extracted with `qs-vfs`) with identical `wavinfo_t` values and sample bytes: rates
from 11025 to 64000, widths 1 and 2, one channel, and 18 with a cue loop point.
