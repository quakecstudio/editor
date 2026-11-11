# qs-wad

Reads WAD2 lump archives: `gfx.wad`, which holds the engine's 2D graphics, and the texture
WADs map tools read. `Wad::read` is id's `W_LoadWadFile` with the bounds checks id's engine
leaves out: it returns a zero-copy view whose lumps borrow the file's bytes, and rejects
malformed input with a typed `WadError`, never a panic. `Wad::find` is `W_GetLumpinfo`,
`Wad::lump` is `W_GetLumpNum`, `w_cleanup_name` is `W_CleanupName`, and `QPic`, `Conchars`
and `MipTexHeader` decode the lumps the engine and map tools read. Ported from
`WinQuake/wad.c` and `draw.c` at the commit pinned in
[`docs/porting.md`](../../docs/porting.md).

`map_wad` reads the texture WADs a map names as QuakeSpasm does, WAD2 and Half-Life's WAD3,
described from QuakeSpasm, not ported: `MapWad::read` is its `W_AddWadFile`, the directory
with its fixes, and `MapWad::texture` its `Mod_LoadWadTexture`, a WAD3 texture's own palette
included. `qs-bsp` reads a BSP's external textures from them (`Bsp::read_with_wads`).

- **Layer:** L0 formats, with no dependencies. `qs-vfs` finds `gfx.wad` with it.
- **Spec sections:** WAD-1 to WAD-4 in [`wad2.md`](../../docs/spec/wad2.md); WAD3-1 to WAD3-3
  in [`wad3.md`](../../docs/spec/wad3.md).
- **Not this crate's:** WAD-1.4 (finding `gfx.wad` on the search path), the filesystem's, in
  `qs-vfs`.
- **Shipped:** yes.

## Invariants

Property tests in `src/tests.rs` and the fuzz target `fuzz/fuzz_targets/qs_wad.rs`
(`cargo xtask fuzz qs-wad`):

- Arbitrary bytes never panic the reader, its lookups or the lump decoders (WAD-1 to WAD-4),
  nor the map WAD reader and its textures, as given or as a WAD3 (WAD3-1 to WAD3-3).
- Every lump is found by its own name in any ASCII case (WAD-3.1).
- Cleaning a name is idempotent and never gives more than 16 bytes (WAD-2.5).
