# qs-mdl

Reads Quake's models other than brush models.

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

Alias models (`.mdl`) join this crate in P5-02
([#143](https://github.com/quakecstudio/editor/issues/143)).

- **Layer:** L0 formats, with no dependencies. The `qs-vfs` example `walk` decodes every
  `.spr`, `.md5mesh` and `.md5anim` of an install with it.
- **Spec sections:** SPR-1 and SPR-2 in [`spr.md`](../../docs/spec/spr.md); MD5-1 to MD5-3
  in [`md5.md`](../../docs/spec/md5.md).
- **Shipped:** yes.

## Invariants

Property tests in `src/spr/tests.rs` and `src/md5/tests.rs`, and the fuzz target
`fuzz/fuzz_targets/qs_mdl.rs` (`cargo xtask fuzz qs-mdl`):

- Arbitrary bytes never panic the sprite reader, the MD5 tokenizer or either MD5 reader
  (SPR-1, SPR-2, MD5-1 to MD5-3).
- Every sprite frame's pixels are exactly `width * height` bytes inside the file (SPR-2.2,
  SPR-2.4).
- Every MD5 accessor answers for each index below its count and for none above (MD5-2.8,
  MD5-3.6).

## Differential result

id's sprite loader, built from the pinned commit, and `qs-mdl` agree on every vector in
SPR-2 and on the three sprites of LibreQuake's `lite` `id1` (2026-10-16; the method is in
the spec's test vectors). The MD5 reader has no oracle: its vectors were written from
QuakeSpasm-Spiked's code, and no real MD5 file was available to this project's machines.
The run on the rerelease's MD5 models is part of P1-30
([#277](https://github.com/quakecstudio/editor/issues/277)).
