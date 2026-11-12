# Map WADs and WAD3

Prefix: `WAD3` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the texture WADs a map names and how the FitzQuake lineage reads their textures
for a BSP's external textures, WAD2 and Half-Life's WAD3, from QuakeSpasm (QS):
`Quake/wad.c`, `Quake/wad.h`, `Quake/gl_model.c` and `Quake/common.c` at
sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`, read, not ported (GoldSrc's
own source is not a porting upstream, `docs/porting.md`). Requirements MAP-24 and MAP-5
([`plan/requirements.yaml`](../../plan/requirements.yaml)). Implemented by
`crates/qs-wad`'s `map_wad` module and `crates/qs-bsp`'s `wads` module and
`Bsp::read_with_wads` ([#331](https://github.com/quakecstudio/editor/issues/331)).

A WAD3 has WAD2's layout ([`wad2.md`](wad2.md), WAD-1.1, WAD-2.1): a 12-byte header, lumps,
and a directory of 32-byte entries. Its textures may carry their own palette. Two entry points
read WADs: id's and QS's `W_LoadWadFile` reads `gfx.wad`, the engine's 2D graphics, and takes
WAD2 only, as WAD-1.2 says, which stays true; QS's `W_AddWadFile` reads the WADs a map names,
WAD2 or WAD3, as this section says. Integers are little-endian, read through `LittleLong`.

## WAD3-1 Header

**Source:** QS `Quake/wad.c`, `W_AddWadFile`, `W_LoadWadFile`; `Quake/wad.h`, `WADID`,
`WADID_VALVE`, `wadinfo_t`.

### Behaviour

- **WAD3-1.1** `W_AddWadFile` reads the 12-byte header and takes its first four bytes as one
  integer, `id = LittleLong (header.identification[0] | (header.identification[1] << 8) | ...)`,
  and goes on only when `id` is `WADID` (`WAD2`) or `WADID_VALVE` (`WAD3`): otherwise
  `Con_Warning ("%s is not a valid WAD\n", name);` and the WAD is not used. A file shorter
  than 12 bytes leaves part of the header unread, which is undefined; this spec fixes the
  answer: it is not a valid WAD. `MapWad::read` fails with `MapWadError::NotValid`.
- **WAD3-1.2** A negative lump count or directory offset is not valid,
  `if (numlumps < 0 || infotableofs < 0)` with `%s is not a valid WAD (%i lumps, %i info table
  offset)`, and a WAD with no lumps is not used, `WAD file %s has no lumps, ignored`.
  `MapWad::read` fails with `MapWadError::BadDirectory` and `MapWadError::NoLumps`.
- **WAD3-1.3** QS's `W_LoadWadFile`, which reads `gfx.wad`, fails unless the file starts
  `WAD2` (`Sys_Error ("Wad file %s doesn't have WAD2 id\n",filename)`), as id's does
  (WAD-1.2): a WAD3 is never `gfx.wad`. `qs_wad::Wad::read` stays id's and fails a WAD3.

## WAD3-2 Directory

**Source:** QS `Quake/wad.c`, `W_AddWadFile`, `W_CleanupName`; `Quake/common.c`, `FS_fread`,
`FS_fseek`.

### Behaviour

- **WAD3-2.1** The directory is `numlumps` entries of 32 bytes at `infotableofs`, each
  `lumpinfo_t` as WAD-2.1 lays it out: `filepos`, `disksize`, `size`, `type`, `compression`,
  two pad bytes and a 16-byte `name`. QS reads it with `FS_fseek (fh, infotableofs, SEEK_SET);`
  and `FS_fread (lumps, 1, numlumps * sizeof (lumpinfo_t), fh);`.
- **WAD3-2.2** Each name is cleaned in place, `W_CleanupName (info->name, info->name);`: the
  bytes up to the first NUL, at most 16, with `A` to `Z` lowered, and the rest NUL, as
  WAD-2.5 says.
- **WAD3-2.3** Each entry is then made to fit the file, in this order: when it runs past the
  file but its `disksize` would not, `if (info->filepos + info->size > fh->length &&
  !(info->filepos + disksize > fh->length)) info->size = disksize;`. Then, when it still
  starts before the file or runs past it, `if (info->filepos < 0 || info->size < 0 ||
  info->filepos + info->size > fh->length)`: one that starts past the file or has a negative
  size gets `info->filepos = 0; info->size = q_max (0, info->size - info->filepos);`, which,
  the offset being 0 by then, is its size or 0; any other gets
  `info->size = q_max (0, info->size - info->filepos);`, its size less its offset. QS adds in
  `int` and `long`; `qs-wad` adds in 64 bits, so no sum overflows. `MapWad::entries` gives the
  fixed entries.
- **WAD3-2.4** `FS_fread` reads only to the end of the file, so a directory that runs past it
  leaves the rest of the `malloc`ed array unread, which is undefined; this spec fixes the
  answer: only the entries the file holds whole are entries.

## WAD3-3 Reading a texture

**Source:** QS `Quake/gl_model.c`, `Mod_LoadWadTexture`; `Quake/wad.c`, `W_GetLumpinfoList`,
`W_GetLumpinfo`; `Quake/wad.h`, `TYP_MIPTEX`, `TYP_MIPTEX_PALETTE`; `Quake/bspfile.h`,
`miptex_t`.

### Behaviour

- **WAD3-3.1** A texture is found by its name: `W_GetLumpinfo` cleans the name asked for as
  WAD3-2.2 does and returns the first entry whose cleaned name is the same
  (`if (!strcmp(clean, lump_p->name))`). `MapWad::find`.
- **WAD3-3.2** The entry must be a texture: `if (!info || (info->type != TYP_MIPTEX &&
  (wad->id != WADID_VALVE || info->type != TYP_MIPTEX_PALETTE)))` the lookup fails with
  `Missing texture %s in WAD, using BSP.`, so type 68 in either WAD, or 67 in a WAD3. QS reads
  the 40-byte `miptex_t` at `filepos`; a width or height of 0 (`Zero sized texture %s in
  %s!`) or not a multiple of 16 (`Texture %s (%d x %d) is not 16 aligned`) fails the lookup
  too. A texture has its own palette when `pal = wad->id == WADID_VALVE && info->type ==
  TYP_MIPTEX_PALETTE;`. `MapWad::texture` returns `None` where the lookup fails.
- **WAD3-3.3** The pixels are level 0's, `pixels = mt.width * mt.height;` from
  `pixels_pos = info->filepos + sizeof (miptex_t);`. A palette texture's palette follows the
  four mip levels, `palette_pos = pixels_pos + pixels / 64 * 85;`: 2 bytes of colour count
  (`palette = 2;`), read only when `if ((pixels / 64 * 85 + 2) <= info->size)`, and then
  `palette += colors * 3;`. The test measures from the lump's start, 40 bytes before the
  pixels. A count the file does not hold leaves `colors` unread, which is undefined; this spec
  fixes the answer: 0 colours. A product of `width * height` that does not fit an `int` is
  undefined; this spec fixes the answer: no texture.
- **WAD3-3.4** When the texture runs past the lump, `if (lump_bytes > info->size)` with
  `lump_bytes` the mip levels and palette (or the pixels alone without a palette), it is cut
  against the lump's size, counted from the lump's start: `lump_bytes = info->size;`,
  `palette = q_min(palette, q_max(0, lump_bytes - pixels / 64 * 85));` and
  `pixels = q_min(pixels, lump_bytes);`. The pixels and the palette are then read with
  `FS_fseek` and `FS_fread`, which stop at the end of the file, not of the lump: what lies
  between the lump's end and the file's is read too, and what is past the file is not, the
  hunk keeping zeros there. `WadTexture::pixels` and `WadTexture::palette` are the bytes read.
- **WAD3-3.5** An entry whose `filepos` is negative after WAD3-2.3 makes `FS_fseek` fail
  (`if (offset < 0)` returns -1), so QS reads the header from wherever the file was left; and a
  header the file does not hold whole is read in part. This spec fixes the answer for both: no
  texture.
- **WAD3-3.6** The texture's name and size are its own header's,
  `memcpy (tx->name, mt.name, sizeof (tx->name)); tx->width = mt.width;`, not the directory
  entry's it was found by. `WadTexture::name` is the header's name up to its first NUL.

## WAD3-4 Which WADs a BSP uses

**Source:** QS `Quake/gl_model.c`, `Mod_LoadWadFiles`, `Mod_LoadTextures`,
`Mod_LoadBrushModel`, `external_textures`; `Quake/wad.c`, `W_LoadWadList`,
`W_GetLumpinfoList`; `Quake/common.c`, `COM_ParseEx`, `COM_FileBase`, `COM_AddExtension`.

### Behaviour

- **WAD3-4.1** `Mod_LoadTextures` loads the WADs once, `wads = Mod_LoadWadFiles (loadmodel);`,
  and only when `external_textures` is set (its default is `"1"`) and the model is the world
  (`if (strcmp (mod->name, sv.modelname)) return NULL;`). `Mod_LoadWadFiles` reads the entity
  text, which `Mod_LoadBrushModel` loads before the textures (and which a `.ent` file can
  replace, BSP-19), token by token with `COM_Parse`: whitespace (any byte up to `' '`, and,
  `char` being signed, any byte of 0x80 or more) and `//` and `/* */` comments are skipped,
  a quoted string is one token, and `{ } ( ) ' :` are tokens of their own. The first token
  must be `{`; then each key loses a leading `_` and its trailing spaces, and the first key
  that is then `wad` gives its value as the list (`if (!strcmp ("wad", key)) return
  W_LoadWadList (value);`). Text that ends first, or a world with no such key, gives none.
  `qs_bsp::wads::wad_key`.
- **WAD3-4.2** `W_LoadWadList` splits the value at `;` and, for each name, stopping at an
  empty one (`for (name = newnames; name && *name;)`), takes its base, `COM_FileBase`: the
  bytes between the last `/` or `\` and the last `.`, or `?model?` when that is shorter than
  2 bytes; adds `.wad` unless that base already ends in it,
  `if (strcmp(COM_FileGetExtension(path), extension + 1) != 0)` in `COM_AddExtension`, where
  `COM_FileGetExtension` is the text after the last `.` that is not the last byte; and opens that file through the search path, else the same name under
  `gfx/` (`memcpy (filename, "gfx/", 4);`), each within `MAX_QPATH`. A file that does not open
  or is not used (WAD3-1) is skipped. Each WAD it uses goes to the front of the list
  (`wad->next = wads; wads = wad;`), so `W_GetLumpinfoList` searches the last one named first.
  `qs_bsp::wads::wad_files` gives the two names for each, in the value's order.
- **WAD3-4.3** For each texture whose first offset is 0 (`from_wad = mt->offsets[0] == 0;`),
  `Mod_LoadTextures` asks `Mod_LoadWadTexture` (WAD3-3) and, when a WAD has it, uses its
  pixels, palette, name and size in place of the BSP's (`goto _load_texture;`), for any BSP
  version; the sky test then reads the WAD texture's name (BSP-22.4). When no WAD has it, the
  BSP's own is used (BSP-22.5). `Bsp::read_with_wads` takes the opened WADs in the value's
  order and does this with `BspLoader::FitzQuake`; id's loaders read no WAD.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `WAD3_MAGIC` | `WAD3` | QS `Quake/wad.h`, `WADID_VALVE` |
| `WAD_TYP_MIPTEX_PALETTE` | 67 | QS `Quake/wad.h`, `#define TYP_MIPTEX_PALETTE 67` |

### Test vectors

See [`vectors/wad3/`](vectors/wad3/README.md).
