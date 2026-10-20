# Archives and assets

**Crates:** `qs-pak`, `qs-wad`, `qs-wav`, `qs-mdl` (L0). **Specs:** PAK, WAD, WAV, MDL, SPR, MD5.

## Context and design philosophy

Four L0 crates read the game's archive and asset files exactly as the engine does, so every
higher layer (the game filesystem, the preview host, the renderer, the map editor's texture
browser) sees the same bytes and the same accept/reject answers as id's engine:

| Crate | Reads | Spec | Ported from |
| --- | --- | --- | --- |
| `qs-pak` | PAK archives: header, directory, entry data, directory CRC and the "modified game" check | [`pak.md`](../../../spec/pak.md) PAK-1 to PAK-5 | `WinQuake/common.c` `COM_LoadPackFile`, half of `COM_FindFile` (`crates/qs-pak/src/lib.rs:5`) |
| `qs-wad` | WAD2 lump archives, name cleanup and lookup, and the lumps the engine decodes (qpic, conchars, miptex header) | [`wad2.md`](../../../spec/wad2.md) WAD-1 to WAD-4 | `wad.c` and `draw.c` `Draw_Character` (`crates/qs-wad/src/lib.rs:5-6`) |
| `qs-wav` | The RIFF WAVE subset `GetWavinfo` reads and what `S_LoadSound` accepts | [`wav.md`](../../../spec/wav.md) WAV-1 to WAV-3 | `snd_mem.c` (`crates/qs-wav/src/lib.rs:5`) |
| `qs-mdl` | Alias models (`.mdl`), sprites (`.spr`), and the rerelease's `.md5mesh`/`.md5anim` | [`mdl.md`](../../../spec/mdl.md) MDL-1 to MDL-5, [`spr.md`](../../../spec/spr.md) SPR-1 to SPR-2, [`md5.md`](../../../spec/md5.md) MD5-1 to MD5-3 | `model.c` for `mdl.rs` and `spr.rs` (`crates/qs-mdl/src/mdl.rs:5`, `spr.rs:5`); `md5.rs` is described from QuakeSpasm-Spiked, not ported (`md5.rs:5-8`) |

The format behaviour lives in `docs/spec/`; this document does not restate it. What it
records is the shape the four crates share and where they differ.

Guiding principles, from `CLAUDE.md` and `docs/porting.md`:

- Port the behaviour, not the C: no globals, errors as `Err` values, every wrap and float
  comparison written out with an `#[expect]` citing the spec statement.
- Shipped crates never panic; arbitrary bytes give a value or a typed error (each crate's
  README Invariants list and its proptests).
- Where id's C is undefined (an error path that reads past a buffer, a zero rate), the spec
  fixes a deterministic answer and the crate implements that answer (for example PAK-1.3,
  PAK-2.5 to 2.7, PAK-2.9, PAK-3.4; the WAV width/rate/stereo checks in
  `crates/qs-wav/README.md:7-9`).

## Components

### Reader shape

Every crate is a pure function from bytes to a parsed value. None touches the filesystem;
`qs-pak` is the one exception that accepts a caller-supplied `Read + Seek` stream
(`Pak::read_from`, `Pak::read_entry`, `crates/qs-pak/src/lib.rs:298`, `:401`).

| Crate | Entry point | Result | Ownership |
| --- | --- | --- | --- |
| `qs-pak` | `Pak::read(&[u8])`, `Pak::read_from(&mut R)` | `Pak` (header, `Vec<Entry>`, directory CRC, file length) | owned; names are copied |
| `qs-wad` | `Wad::read(&'a [u8])` | `Wad<'a>` borrowing the file, `Vec<Lump>` | zero-copy |
| `qs-wav` | `get_wavinfo(&[u8])`, `s_load_sound(&[u8])` | `WavInfo`, `Sound<'a>` borrowing sample bytes | zero-copy |
| `qs-mdl` | `Mdl::read`, `Sprite::read` | `Mdl<'a>`, `Sprite<'a>`; skins, frames and pixels borrow the file | zero-copy |
| `qs-mdl` | `Md5Mesh::read`, `Md5Anim::read` | owned token-derived structures (`BTreeMap`-backed sparse lists) | owned |

Lookups are split from loading where id splits them: `Pak::find` and `Pak::entry_data` are the
archive half of `COM_FindFile`, while the search path itself is `qs-vfs`
([game-filesystem](game-filesystem.md)). PAK-1.4 and WAD-1.4 are
explicitly disowned by these crates and implemented in `qs-vfs` (`crates/qs-pak/README.md`,
`crates/qs-wad/README.md`).

### Derived values

Beyond parsing, the crates expose what the renderers and the engine derive from a file, so
higher layers do not reimplement it: PAK directory CRC and `is_modified` (PAK-4); WAD
conchars glyphs (WAD-4); `Sound::sample` converting 8-bit to signed 16-bit as `ResampleSfx`
does (`crates/qs-wav/src/lib.rs:253-268`); MDL size ratio, vertex positions, seam offset, bad
normal indexes and GLQuake limits exceeded (`Mdl::size`, `position`, `skin_s`,
`bad_normals`, `gl_limits`); sprite bounds; MD5 pose evaluation and mesh/animation
compatibility (`Md5Anim::pose`, `Md5Anim::check_mesh`).

## Error model

Each crate has its own error enum with no `thiserror`:

| Error | Shape | Message style |
| --- | --- | --- |
| `PakError` | `Copy`; `Io(ErrorKind)` for stream errors | text meant to follow a path |
| `WadError` | not `Copy`: `NotFound` owns a `Vec<u8>` | text meant to follow a path |
| `WavError` | `Copy` | text meant to follow a path |
| `MdlError` | `message(name)` builds id's full message | id's wording |
| `SprError` | `Display` only; `WrongVersion` written to follow a file name (`crates/qs-mdl/src/spr.rs:51-52`) | id's wording |
| `Md5Error` | `Display` only | QSS wording |

Check order follows id's so the first error reported is the one the engine would report
(for example `qs-pak` count, then size, then bounds, `crates/qs-pak/src/lib.rs:189-212`; MDL
header checks in id's order, `crates/qs-mdl/src/mdl.rs:461-481`).

## Verification

- Statement-cited unit tests per crate, fixture factories whose `Default` is the spec's base
  vector (`crates/qs-*/src/fixtures.rs`, `crates/qs-mdl/src/*/fixtures.rs`).
- Proptests in every crate; fuzz targets `fuzz/fuzz_targets/{qs_pak,qs_wad,qs_wav,qs_mdl}.rs`.
- Differential results recorded in READMEs: id's `GetWavinfo` on the WAV-3 vectors and 226
  LibreQuake sounds (`crates/qs-wav/README.md:29-33`); id's alias and sprite loaders on every
  vector (`crates/qs-mdl/README.md`). MD5 has no oracle (`crates/qs-mdl/README.md:58-61`).
- L0 readiness passed at `110ce43` with every crate at 100% spec citation and 100% mutation
  for `qs-pak`, `qs-wad`, `qs-wav`, `qs-mdl` (`docs/readiness/p1-r0.md:59-68`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Implementation source | Port id's C for PAK, WAD, WAV, MDL, SPR | Write from format documentation | The project's core principle: model what the engine executes (`CLAUDE.md`, ADR-0002 allows porting GPL source) |
| MD5 source | Describe QuakeSpasm-Spiked, port nothing; stricter answers where QSS is lax | Port QSS; follow id Tech 4 | id's rerelease engine is unpublished (`crates/qs-mdl/README.md`); `md5.md` fixes MD5-1.7, 1.9, 2.5, 2.10, 3.4, 3.8 as stricter answers |
| Undefined C behaviour | Fixed, deterministic errors in the spec | Reproduce x86 crash behaviour | `.claude/rules/specs-are-source-of-truth.md`: the spec states the answer for UB |
| `qs-pak` ownership | Owned `Pak`, copied names | Zero-copy like `qs-wad` | [inferred] supports `read_from` over a stream where the whole file is not in memory |
| `qs-pak` CRC dependency | Reuse `qc_progs::crc` | Own CRC copy | [inferred] one CRC-16 implementation in L0 (`crates/qs-pak/src/lib.rs:26`) |
| WAD2 LZSS | Named (`WAD_CMP_LZSS`), never decompressed | Implement LZSS | WAD-2.7 (id's loader does not decompress) |
| Error crate | Hand-written enums | `thiserror` | [inferred] no dependency for a few `Display` impls |
| Magic check | One 32-bit comparison | Byte-by-byte | Stated in code: "for fuzzers" (`crates/qs-pak/src/lib.rs:161`, `crates/qs-mdl/src/md5.rs:432`) |

## Open questions

### Resolved
1. L0 readiness (P1-R0, #53) passed at `110ce43`; surviving mutants killed in P1-57 (#358).

### Deferred
1. **`Md5Anim::frame` allocates from an unchecked file count**. `numAnimatedComponents`
   is read as an unchecked `u64` (`crates/qs-mdl/src/md5.rs:637-638`). With `numJoints 0` and no
   `frame` blocks, `Md5Anim::read` accepts a tiny file, and `frame(0)` then runs
   `vec![0.0; num_animated_components]` (`md5.rs:693-701`), which aborts on allocation failure or
   panics on capacity overflow. Neither the proptest (`crates/qs-mdl/src/md5/tests.rs:736-743`)
   nor the fuzz target (`fuzz/fuzz_targets/qs_mdl.rs`, which calls `pose` but not `frame`) calls
   `frame` on arbitrary input. MD5-3.6 says a frame never given is all zero but sets no bound on
   the count. Options: bound the count by the remaining input or a stated limit at read time
   (needs a spec statement), return a sparse or lazily-zero view, or make `frame` fallible. Not
   yet filed in `plan/`.
2. `Md5Anim::read` skips the 32-bit `MD5V` magic check that `Md5Mesh::read` does
   (`crates/qs-mdl/src/md5.rs:432-440` vs `:623-626`); both still require the `MD5Version`
   token, so the effect is limited to how leading bytes are treated. It also does not
   range-check hierarchy parent indexes (`md5.rs:644`); MD5-3.9 checks parents only against a
   mesh.
3. `qs-wad` indexes a fixed `&[u8; 32]` chunk with `raw[12]`, `raw[13]` in shipped code
   (`crates/qs-wad/src/lib.rs:267-268`). In bounds by type, but it is the `[]` form
   `CLAUDE.md` asks to avoid.
4. `Conchars::glyph` silently leaves zeros for a row out of range (`crates/qs-wad/src/lib.rs:423-425`).
5. `qs-wav` writes `// WAV-2.4:` without the `spec` prefix in shipped code (`crates/qs-wav/src/lib.rs:189`, `:220`); the ratchet does not count these.
6. The `qs_wav` fuzz target uses an unchecked `dataofs + data.len()` in its assertion (`fuzz/fuzz_targets/qs_wav.rs:19`).
7. `qs-mdl`'s Cargo description names sprites and MD5 but not alias models (`crates/qs-mdl/Cargo.toml:3`).
8. Error types are inconsistent across the four crates (`Copy` or not, `message(name)` or
   `Display` only). CONV-17 asks for typed errors but not for one shape.
9. `le_i32` is reimplemented privately in `qs-pak` (`lib.rs:420`), `qs-wad` (`lib.rs:479`) and
   `qs-wav` (`lib.rs:330`); `from_xxd` test helpers are duplicated across crates.
10. Planned writers: a PAK writer in `qs-pak` (P4-27, #206, open); a WAD2 writer and miptex
    encoder in `qs-wad` (P6-15, #209, open); a WAD3 reader beside WAD2 (P5-12, #331, open).
11. `pak.md` PAK-5 cites QuakeSpasm by unpinned `master` URLs, unlike `bsp.md`'s pinned commit.

## References

- Specs: [`pak.md`](../../../spec/pak.md), [`wad2.md`](../../../spec/wad2.md),
  [`wav.md`](../../../spec/wav.md), [`mdl.md`](../../../spec/mdl.md),
  [`spr.md`](../../../spec/spr.md), [`md5.md`](../../../spec/md5.md)
- Porting process: [`docs/porting.md`](../../../porting.md)
- Readiness: [`docs/readiness/p1-r0.md`](../../../readiness/p1-r0.md)
- Plan: P1-18 (#42), P1-29 (#235), P5-02 (#143), P1-57 (#358); open P4-27 (#206), P5-12 (#331), P6-15 (#209)
