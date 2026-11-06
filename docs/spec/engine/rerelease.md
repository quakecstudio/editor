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

- **RRE-1.1** **Withdrawn** 2026-11-03: it said an unquoted value has its trailing blanks
  trimmed, which `LOC_LoadFile`'s in-place parse does not quite do. Replaced by RRE-1.7.
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
- **RRE-1.5** **Withdrawn** 2026-11-03: it listed `print` among the builtins that read their
  text with `PF_VarString`; QuakeSpasm has no `print` builtin. Replaced by RRE-1.9.
- **RRE-1.6** The preview host localizes as RRE-1.9 says only with a file loaded (one that
  holds an entry); with none, the print builtins' text is id's joined text (BUILTIN-7.3,
  BUILTIN-11) and `WriteString` writes its string as it is, so a progs that is not the
  rerelease's runs exactly as on id's engine. QuakeSpasm with no file still cuts the joined text
  to 1023 bytes; id's engine has no such cut, and this is ours to choose, as the server
  simulation follows id's engine (ADR-0018).
- **RRE-1.7** `LOC_LoadFile` reads the file's text up to its first zero, skips a UTF-8 byte
  order mark, and parses each line in place: the blanks (space, tab) that start it are skipped;
  a line starting with `/` holds nothing (`//` a comment, a lone `/` "malformed comment"); a
  line with an `=` is an entry whose key is the text from the line's start to the first `=`,
  the white space before it (`q_isspace`: space, `\t`, `\n`, `\v`, `\f`, `\r`) trimmed by
  writing a zero; its value starts after the `=` and the white space after it, an opening `"`
  dropped, and is copied onto itself: `\n`, `\t`, `\v`, `\b`, `\f` are those control characters,
  `\"` and `\'` the quote, any other `\c` is `c` ("unrecognized escape sequence"), a `\` that is
  the line's last byte is itself, and an unescaped `"` writes a zero and ends it. A line with
  neither is passed over. Each line's end, its `\n`, is then overwritten with a zero and the
  parse goes on after it. The copy is not terminated where it ends, so an unquoted value an
  escape shortened keeps the source's last bytes up to the line's end: `b = x\ny` is `x`, a line
  feed, `yy`. An unquoted value has its trailing blanks trimmed by `while (value_dst != value &&
  q_isblank(value_dst[-1])) { *value_dst = 0; value_dst--; }`, which zeroes the byte after each
  blank and so keeps the last blank (`ab` with two trailing spaces reads as `ab` with one);
  and when no escape shortened the value
  its first write is the line's own `\n`, after which `if (*cursor)` does not advance and the
  file is read no further. A `\r` before the line feed stays in an unquoted value.
- **RRE-1.9** `PF_VarString (first)`, which `error`, `objerror`, `bprint`, `sprint`,
  `centerprint` and `dprint` read their text with (its six callers in `pr_cmds.c`): the first
  argument is localized; when it has placeholders the result is `LOC_Format` of it with
  argument `N` taken from parameter `first + 1 + N` (empty past `argc`); otherwise every
  parameter from `first` is localized and appended, into 1024 bytes (`PF_VarString: overflow
  (string truncated)`), with a warning past 255. `WriteString` writes the one string it is
  given localized (`MSG_WriteString (WriteDest(), LOC_GetString (G_STRING (OFS_PARM1)))`),
  so its bytes, and the message's count of them, are the localized string's and its zero.
- **RRE-1.10** `LOC_LoadFile` looks for the file on the search path (`COM_LoadMallocFile`,
  FS-3), else opens `<com_basedir>/<file>`, else the member `<file>` of
  `<com_basedir>/QuakeEX.kpf` (FS-5.3, FS-5.5); a base directory file that opens but is empty
  (`sz <= 0`) is `goto fail`, with no look in the archive, as is an archive that is not a zip or
  has no such member: "Couldn't load '<file>' from '<basedir>'", and nothing is localized.
  QuakeSpasm also tries the user directory (`host_parms->userdir`) after each; QuakeC Studio
  models no user directories (FS-5.4).
- **RRE-1.8** `COM_HashString` XORs each byte of the key as a `char`, `hash ^= *str++`, which
  x86 signs, so a byte from 0x80 up is XORed sign-extended (`0xFFFFFFE9` for 0xE9) before the
  multiply.

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
  sound's precache index as a byte, or a short when `SND_LARGESOUND` (bit 4) is set. A
  non-client entity prints `tried to localsound to a non-client` and sends nothing; a sample
  not in the server's precache list prints `SV_LocalSound: <sample> not precached` and sends
  nothing; an index of 256 or more sends nothing under protocol 15 (`PROTOCOL_NETQUAKE`,
  CLIENT-1.1) and sets `SND_LARGESOUND` under the FitzQuake protocols; and a client message
  with fewer than 4 bytes free (`cursize > maxsize-4`) drops it.
- **RRE-2.2** The client reads the index (at or past `MAX_SOUNDS` is the error
  `CL_ParseLocalSound: <n> > MAX_SOUNDS`) and plays the sound as SND-2.7, on the view entity
  at full volume.
- **RRE-2.3** `svc_achievement` (52) carries a string; the client reads and ignores it
  (`Ignoring svc_achievement (<id>)`), since achievements are the rerelease platform's.
- **RRE-2.4** `PF_localsound` reads its client with `G_EDICTNUM(OFS_PARM0)`, QuakeSpasm's
  `NUM_FOR_EDICT(G_EDICT(o))`: the value is a byte offset from the first edict, divided by
  `pr_edict_size` with C's truncating `/` and not checked to fall on an edict, so an offset
  inside an edict is that edict and -1 is the world; a number below 0 or at or past
  `num_edicts` is `Host_Error ("NUM_FOR_EDICT: bad pointer")`.
- **RRE-2.5** The preview host's client holds id's 256 sound precaches (`CLIENT_MAX_SOUNDS`,
  CLIENT-1.1), not QuakeSpasm's 2048, so its RRE-2.2 bound is 256. A negative index, from a short
  under `SND_LARGESOUND`, passes QuakeSpasm's `sound_num >= MAX_SOUNDS` check and reads
  `cl.sound_precache[sound_num]` before the array, which is undefined; the client makes it
  the same error, `CL_ParseLocalSound: <n> > MAX_SOUNDS`. The client does not check that
  the index has a precache: it passes the index on in its event, as it does `svc_sound`'s.

## RRE-3 Music

**Source:** `Quake/bgmusic.c`, `BGM_PlayCDtrack`, `BGM_Play`, `BGM_Stop`; `WinQuake/cl_parse.c`,
`svc_cdtrack`.

### Behaviour

- **RRE-3.1** **Withdrawn** 2026-11-06: the preview host plays a CD image's track first, as
  QuakeSpasm plays a CD first; replaced by RRE-3.4.
- **RRE-3.2** The rerelease ships its music as `music/track02.ogg` and up in its base
  directory and the mission packs' directories (FS-5); a map's `worldspawn` `sounds` key is
  the track number the server sends.
- **RRE-3.3** A new track stops the one playing; `BGM_Stop` stops it; the volume is
  `bgmvolume`.
- **RRE-3.4** `svc_cdtrack` (CLIENT-1.4) names a track number and a loop track; QuakeSpasm
  plays `(byte)cl.cdtrack`, always looping (`BGM_PlayCDtrack ((byte)cl.cdtrack, true)`):
  first from the CD (`CDAudio_Play`), which in the preview host is the CD image of SND-6.4;
  else the file `music/track<NN>.<ext>` (`NN` the track as two decimal digits) from the
  game's search path, trying the extensions of the codecs it has and taking the file from the
  search path entry of highest priority; else it prints `Couldn't find a cdrip for track <n>`
  and plays nothing. The preview host has no codec for the files yet: it plays the CD image's
  track or nothing, while P3-34 is open.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `RRE_MUSIC_DIR` | `music` | `bgmusic.c`, `MUSIC_DIRNAME` |
| `RRE_TRACK_FORMAT` | `track%02d` | `bgmusic.c`, `BGM_PlayCDtrack` |

## RRE-4 The rerelease's builtins

**Source:** `Quake/pr_edict.c`, `exbuiltins`, `PR_PatchRereleaseBuiltins`, `PR_LoadProgs`;
`Quake/pr_cmds.c`, `pr_builtin`, `PF_finalefinished`, `PF_CheckPlayerEXFlags`,
`PF_walkpathtogoal`, `PF_localsound`, `PF_Fixme`.

The rerelease's QuakeC declares `bprint`, `sprint`, `centerprint`, `localsound` and its other
own builtins by name, `#0:ex_NAME` (BUILTIN-17.3, ENGT-2.16); an engine gives each a number when
it loads the progs. The preview host runs the rerelease's progs as QuakeSpasm does.

### Behaviour

- **RRE-4.1** When the progs loads, `PR_PatchRereleaseBuiltins` goes through `exbuiltins` in
  order and, for each row, gives the function `ED_FindFunction` finds by the row's name (the
  first of that name) the row's `first_statement` when it has the row's old one: `centerprint`
  from -90, `bprint` from -91 and `sprint` from -92 (Update-1's numbers) to -73, -23 and -24;
  and from 0, `ex_centerprint` to -73, `ex_bprint` to -23, `ex_sprint` to -24,
  `ex_finaleFinished` to -79, `ex_localsound` to -80, `ex_draw_point` to `ex_draw_cylinder`
  to -81 to -89, `ex_CheckPlayerEXFlags` to -90, `ex_walkpathtogoal` to -91, and
  `ex_bot_movetopoint` and `ex_bot_followentity` both to -92. A function of another name or
  another `first_statement` is left alone.
- **RRE-4.2** QuakeSpasm's table runs past id's 78 to 92: 79 `PF_finalefinished`, 90
  `PF_CheckPlayerEXFlags` and 91 `PF_walkpathtogoal` return 0 (`G_FLOAT(OFS_RETURN) = 0`,
  `PATH_ERROR` for the last); 80 `PF_localsound` (RRE-2.1); and 81 to 89 and 92 `PF_Fixme`,
  the run error `unimplemented builtin`. The preview host puts 79 to 92 in its table only for
  a progs RRE-4.1 changed, so another progs keeps id's 79 builtins (LIMIT-5.1) and its
  `BadBuiltin` past them.
