# The rerelease's engine behaviour

Prefix: `RRE` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes what the 2021 rerelease's QuakeC and data expect of an engine beyond id's: the
localization of the strings the game prints, the `localsound` builtin and its message, the
achievement message, and music as files rather than CD tracks, as QuakeSpasm implements them
in `Quake/common.c`, `pr_cmds.c`, `cl_parse.c` and `bgmusic.c` of
[sezero/quakespasm](https://github.com/sezero/quakespasm) at commit
`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5` (ADR-0018). The rerelease's own engine has no
public source; QuakeSpasm's reading of its data is the one described. The builtins the
rerelease's QuakeC calls are BUILTIN-18's rerelease rows and ENGT-3; the FTEQCC constructs
it compiles with are FTE-1 onward. To be implemented by the preview host (P3-26, P3-27).

## RRE-1 Localization

**Source:** `Quake/common.c`, `COM_HashString`, `LOC_LoadFile`, `LOC_GetRawString`,
`LOC_GetString`, `LOC_ParseArg`, `LOC_HasPlaceholders`, `LOC_Format`; `Quake/pr_cmds.c`,
`PF_VarString`, `PF_GetStringArg`, `PF_WriteString`.

The rerelease's game text is keyed: the QuakeC passes `$key` strings and the engine looks
them up in `localization/loc_english.txt`, read from the game directory or from
`QuakeEX.kpf`, the zip in the rerelease's base directory (FS-5).

### Behaviour

- **RRE-1.1** The file is read in full, a UTF-8 byte order mark skipped; each line has its
  leading blanks skipped, and a line starting with `/` is a comment (`//`; a lone `/` is
  reported and skipped). A line with `=` is an entry: the key is the text before the first
  `=` with trailing whitespace trimmed, and the value the text after it with leading
  whitespace skipped, an opening `"` dropped, and ending at the next unescaped `"` or, without
  one, at the line's end with trailing blanks trimmed. In the value `\n`, `\t`, `\v`, `\b`
  and `\f` are the control characters, `\"` and `\'` the quotes, and any other `\c` is `c`
  with a warning. A line with neither is ignored.
- **RRE-1.2** Entries are kept by the FNV-1a hash of the key (`0x811c9dc5`, multiplied by
  `0x01000193` after each byte) in an open-addressed table of twice the entry count; a key
  listed twice keeps the first entry found by probing, which is the first in the file.
- **RRE-1.3** `LOC_GetRawString (key)` is the value for a `key` that starts with `$` (the
  `$` dropped) when the table has it, else none; `LOC_GetString` is that value or the key
  itself, so a plain string passes through. With no file loaded nothing is localized.
- **RRE-1.4** A localized format may hold placeholders `{}` or `{N}` (a decimal `N`, 0 by
  default); `LOC_HasPlaceholders` is true when one is found, with no file loaded false.
  `LOC_Format` copies the format, replacing each placeholder by argument `N` (through the
  caller's getter, itself localized by RRE-1.3), into a buffer of `len` bytes, truncating an
  argument that does not fit (`LOC_Format: overflow at argument #n`) and the rest
  (`LOC_Format: overflow`); the output is always terminated.
- **RRE-1.5** `PF_VarString (first)`, which `bprint`, `sprint`, `centerprint`, `dprint`,
  `error`, `objerror` and `print` read their text with: the first argument is localized; when
  it has placeholders the result is `LOC_Format` of it with argument `N` taken from parameter
  `first + 1 + N` (empty past `argc`); otherwise every parameter from `first` is localized and
  appended, into 1024 bytes (`PF_VarString: overflow (string truncated)`), with a warning
  past 255. `WriteString` localizes the one string it writes.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `RRE_LOC_FILE` | `localization/loc_english.txt` | `common.c`, `LOC_Init` |
| `RRE_KPF` | `QuakeEX.kpf` | `common.c`, `LOC_LoadFile` |
| `RRE_VARSTRING_LEN` | 1024 | `pr_cmds.c`, `PF_VarString` |
| `RRE_FNV_OFFSET` | `0x811c9dc5` | `common.c`, `COM_HashString` |
| `RRE_FNV_PRIME` | `0x01000193` | `common.c`, `COM_HashString` |

## RRE-2 Local sounds and achievements

**Source:** `Quake/pr_cmds.c`, `PF_localsound`; `Quake/cl_parse.c`, `CL_ParseLocalSound`,
`CL_ParseServerMessage`; `Quake/protocol.h`.

### Behaviour

- **RRE-2.1** `localsound (client, sample)` (builtin 80 under the rerelease's table,
  BUILTIN-18) writes `svc_localsound` (56) to that client alone: a field byte, then the
  sound's precache index as a byte, or a short when `SND_LARGESOUND` (bit 4) is set; a
  non-client entity prints `tried to localsound to a non-client` and sends nothing.
- **RRE-2.2** The client reads the index (at or past `MAX_SOUNDS` is the error
  `CL_ParseLocalSound: <n> > MAX_SOUNDS`) and plays the sound as SND-2.7, on the view entity
  at full volume.
- **RRE-2.3** `svc_achievement` (52) carries a string; the client reads and ignores it
  (`Ignoring svc_achievement (<id>)`), since achievements are the rerelease platform's.

## RRE-3 Music

**Source:** `Quake/bgmusic.c`, `BGM_PlayCDtrack`, `BGM_Play`, `BGM_Stop`; `WinQuake/cl_parse.c`,
`svc_cdtrack`.

### Behaviour

- **RRE-3.1** `svc_cdtrack` (CLIENT-1.4) names a track number and a loop track; id's client
  plays the CD track. The preview host has no CD: it plays the file `music/track<NN>.<ext>`
  (`NN` the track as two decimal digits) from the game's search path, trying the extensions
  of the codecs it has and taking the file from the search path entry of highest priority,
  looping; no file prints `Couldn't find a cdrip for track <n>` and plays nothing.
- **RRE-3.2** The rerelease ships its music as `music/track02.ogg` and up in its base
  directory and the mission packs' directories (FS-5); a map's `worldspawn` `sounds` key is
  the track number the server sends.
- **RRE-3.3** A new track stops the one playing; `BGM_Stop` stops it; the volume is
  `bgmvolume`.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `RRE_MUSIC_DIR` | `music` | `bgmusic.c`, `MUSIC_DIRNAME` |
| `RRE_TRACK_FORMAT` | `track%02d` | `bgmusic.c`, `BGM_PlayCDtrack` |
