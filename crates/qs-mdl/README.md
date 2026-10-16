# qs-mdl

Reads Quake's models other than brush models.

- **Alias models** (`mdl`): `Mdl::read` is id's `Mod_LoadAliasModel`, with
  `Mod_LoadAliasSkin`, `Mod_LoadAliasSkinGroup`, `Mod_LoadAliasFrame` and
  `Mod_LoadAliasGroup`, plus the bounds and vertex checks id's engine leaves out. Its checks
  run in id's order with id's messages (`MdlError::message`); skins and frame vertices borrow
  the file's bytes. It also gives what the renderers derive: vertex positions, the seam's
  skin offset, normal indexes past the table, and the limits of GLQuake's loader a model
  exceeds. Ported from `WinQuake/model.c` at the commit pinned in
  [`docs/porting.md`](../../docs/porting.md).
- **Sprites** (`spr`): `Sprite::read` is id's `Mod_LoadSpriteModel`, with
  `Mod_LoadSpriteFrame` and `Mod_LoadSpriteGroup`, plus the bounds checks id's engine leaves
  out. It returns a zero-copy view whose frames borrow the file's bytes, and rejects
  malformed input with a typed `SprError`, never a panic. Ported from `WinQuake/model.c` at
  the commit pinned in [`docs/porting.md`](../../docs/porting.md).
- **MD5 models** (`md5`): the 2021 rerelease's enhanced models. `Md5Mesh::read` and
  `Md5Anim::read` read `.md5mesh` and `.md5anim` files, `Md5Anim::check_mesh` checks that
  an animation fits its mesh, and `Md5Anim::pose` gives a joint's pose in a frame. id's
  rerelease engine is not published, so the format follows QuakeSpasm-Spiked, the engine
  that reads the rerelease's files, cited for description only; nothing is ported from it.
  Where its checks are missing or wrong, the spec fixes a stricter answer.

- **Layer:** L0 formats, with no dependencies. The `qs-vfs` example `walk` decodes every
  `.mdl`, `.spr`, `.md5mesh` and `.md5anim` of an install with it.
- **Spec sections:** MDL-1 to MDL-5 in [`mdl.md`](../../docs/spec/mdl.md); SPR-1 and SPR-2
  in [`spr.md`](../../docs/spec/spr.md); MD5-1 to MD5-3 in
  [`md5.md`](../../docs/spec/md5.md).
- **Shipped:** yes.

## Invariants

Property tests in `src/mdl/tests.rs`, `src/spr/tests.rs` and `src/md5/tests.rs`, and the
fuzz target `fuzz/fuzz_targets/qs_mdl.rs` (`cargo xtask fuzz qs-mdl`), which also writes its
input over the spec's `base` alias model:

- Arbitrary bytes never panic the alias model reader, the sprite reader, the MD5 tokenizer or
  either MD5 reader (MDL-1 to MDL-5, SPR-1, SPR-2, MD5-1 to MD5-3).
- Every triangle of an alias model names vertices it has, and every frame has `numverts`
  vertices (MDL-4.3, MDL-5.2).
- Every sprite frame's pixels are exactly `width * height` bytes inside the file (SPR-2.2,
  SPR-2.4).
- Every MD5 accessor answers for each index below its count and for none above (MD5-2.8,
  MD5-3.6).

## Differential result

The factory in `src/mdl/fixtures.rs` builds the spec's `base` alias model byte for byte, and
every vector in [`vectors/mdl/`](../../docs/spec/vectors/mdl/README.md) gives the result its
README records for id's loader. On 2026-10-16 id's `Mod_LoadAliasModel`, built from the pinned
commit, and `qs-mdl` loaded the 81 alias models of LibreQuake's `lite` release alike: header,
texture coordinates, triangles and every frame entry's name and bounds (the run is in
[`../qs-bsp/README.md`](../qs-bsp/README.md#differential-result)).

id's sprite loader, built from the pinned commit, and `qs-mdl` agree on every vector in
SPR-2 and on the three sprites of LibreQuake's `lite` `id1` (2026-10-16; the method is in
the spec's test vectors). The MD5 reader has no oracle: its vectors were written from
QuakeSpasm-Spiked's code, and no real MD5 file was available to this project's machines.
The run on the rerelease's MD5 models is part of P1-30
([#277](https://github.com/quakecstudio/editor/issues/277)).
