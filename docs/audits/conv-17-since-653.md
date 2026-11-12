# CONV-17 audit of the code changed since PR #653

P5-19 ([#869](https://github.com/quakecstudio/editor/issues/869)). Every shipped crate and app (layers.toml, the test-only `qs-bench`, `qs-differential` and `qs-tests` and the tooling `xtask` excluded), read for the code added or changed since PR #653 (`85af721480639ade94177298023021bdc08af770`) against [`docs/conventions.md`](../conventions.md), "Type design (CONV-17)".

## How it was read

- **The checks' findings.** `git diff --name-only 85af7214..main -- crates apps` gives the Rust source files changed since #653 (311 shipped files, tests, fixtures, examples and benches left out). Every entry of P5-18's baseline (`docs/audits/conv-17-baseline.tsv`) in one of them is a finding below, located by its item: a function's `fn` line, a field's line in its struct, and a magic number's first line in its function (the function's own line where the literal is written another way, such as `49.0`).
- **What no check finds**, searched in the same files:
  - named types for units: a visible `f32` or `f64` field or parameter named as a time, speed, angle, distance, volume, attenuation, field of view or gamma;
  - single-use one-line helpers: a private function whose body is one line, named only at its definition and one call in its crate;
  - duplicated code a generic or trait would remove: a function body of six or more lines written the same in two files.
  - IDs the bare-id rule's names miss: a visible integer field or parameter named as a number, handle, reference or serial (`num`, `number`, `handle`, `_ref`, `serial`); the `num*` counts this also finds are not IDs.

## Summary

| Issue | Crates | bool argument | String error | free function | bare ID | magic number | other | Baseline lines |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| P6-24 | `qs-client` | 13 | 0 | 7 | 4 | 367 | 2 | 391 |
| P6-25 | `qs-sim` | 12 | 2 | 17 | 14 | 126 | 4 | 171 |
| P6-26 | `qc-vm`, `qc-edict`, `qc-builtins`, `qc-trace` | 11 | 0 | 61 | 22 | 144 | 7 | 238 |
| P6-27 | `qc-lexer`, `qc-preproc`, `qc-syntax` | 29 | 0 | 18 | 0 | 182 | 3 | 229 |
| P6-28 | `qc-sema`, `qc-ide`, `qc-project`, `qc-engines`, `qc-codegen` | 34 | 0 | 31 | 12 | 115 | 8 | 192 |
| P6-29 | `qs-render` | 5 | 0 | 10 | 3 | 109 | 8 | 127 |
| P6-30 | `qs-bsp`, `qs-mdl`, `qs-wad`, `qs-pak`, `qs-map`, `qs-vfs`, `qs-sound`, `qs-paths`, `qs-checkpoint`, `qs-templates`, `qs-layout` | 2 | 1 | 11 | 5 | 182 | 5 | 201 |
| P6-31 | `desktop`, `qs-editor-view` | 22 | 0 | 22 | 6 | 79 | 6 | 129 |
| P6-32 | `quakec-engine`, `qs-engine`, `qs-bridge` | 4 | 0 | 16 | 3 | 53 | 0 | 76 |
| P6-33 | `qs-workbench`, `qs-agent`, `qs-models`, `qs-mcp`, `qs-commands`, `qs-git`, `cli`, `lsp` | 6 | 8 | 15 | 18 | 39 | 10 | 86 |
| | **Total** | 138 | 11 | 208 | 87 | 1396 | 53 | 1840 |

Every finding is filed in the issue its crate's row names, except those accepted under "What no check finds" with their reason. The proposed change for each rule:

- **bool-argument**: an enum naming the two choices, in place of the `bool` argument.
- **string-error**: an error enum with a variant per failure, in place of `String`.
- **free-fn-on-type**: a method on the type of its first parameter.
- **bare-id**: a newtype with a private field and a constructor, in place of the raw integer.
- **magic-number**: a named `const` with a doc comment (citing the spec statement where the value comes from one).

## What no check finds

### Units

| Where | Name | Verdict | Why |
| --- | --- | --- | --- |
| `apps/desktop/src/map_scene.rs:65` | `yaw: f32` | filed | a `Degrees` (or `Seconds`) newtype: an editor-side angle or time with no C declaration to follow |
| `apps/quakec-engine/src/game.rs:213` | `fov_x: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `apps/quakec-engine/src/game/debug.rs:31` | `frametime: f64` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `apps/quakec-engine/src/settings.rs:17` | `frame_gamma: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `apps/quakec-engine/src/settings.rs:19` | `volume: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qc-builtins/src/mock.rs:487` | `last_check_time: f64` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/client.rs:350` | `volume: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/client.rs:352` | `attenuation: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/entity.rs:51` | `time: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/entity.rs:74` | `msgtime: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/hud.rs:152` | `time: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/hud.rs:160` | `faceanimtime: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/hud.rs:174` | `completed_time: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/menu.rs:201` | `gamma: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/menu.rs:205` | `bgmvolume: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/menu.rs:207` | `volume: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/menu.rs:209` | `forwardspeed: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/menu.rs:211` | `m_pitch: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/menu.rs:271` | `host_time: f64` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/menu.rs:273` | `realtime: f64` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/scene.rs:67` | `time: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/view.rs:101` | `gamma: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/view.rs:122` | `dmg_time: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/view.rs:126` | `faceanimtime: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/view.rs:160` | `time: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-client/src/view.rs:162` | `frametime: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-render/src/camera.rs:14` | `yaw: f32` | filed | a `Degrees` (or `Seconds`) newtype: an editor-side angle or time with no C declaration to follow |
| `crates/qs-render/src/camera.rs:16` | `pitch: f32` | filed | a `Degrees` (or `Seconds`) newtype: an editor-side angle or time with no C declaration to follow |
| `crates/qs-render/src/camera.rs:19` | `fov_x: f32` | filed | a `Degrees` (or `Seconds`) newtype: an editor-side angle or time with no C declaration to follow |
| `crates/qs-render/src/gpu.rs:1265` | `time: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-render/src/gpu.rs:1275` | `gamma: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-render/src/play.rs:278` | `gamma: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:108` | `upspeed: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:110` | `forwardspeed: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:112` | `backspeed: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:114` | `sidespeed: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:116` | `movespeedkey: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:118` | `yawspeed: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:120` | `pitchspeed: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:122` | `anglespeedkey: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:126` | `m_pitch: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/input.rs:128` | `m_yaw: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/scenario.rs:31` | `frametime: f64` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/scenario.rs:46` | `yaw: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sim/src/world/save.rs:49` | `time: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sound/src/mixer.rs:112` | `attenuation: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |
| `crates/qs-sound/src/mixer.rs:196` | `volume: f32` | accepted | a port of id's `float` field or cvar, kept as the C declares it so the spec statements read across; a newtype would wrap every arithmetic id writes |

### IDs

| Where | Field | Verdict | Why |
| --- | --- | --- | --- |
| `crates/qc-builtins/src/table.rs:36` | `number: u32` | filed | a builtin number, an ID: a `BuiltinNumber` newtype |
| `crates/qc-sema/src/lower/decl.rs:890` | `number: i32` | filed | a builtin's `binum`, an ID: the same `BuiltinNumber` |
| `crates/qs-agent/src/history.rs:29` | `number: u32` | filed | a thread's number, an ID: a newtype |
| `crates/qs-checkpoint/src/lib.rs:63` | `number: u32` | filed | a checkpoint's number, an ID: a `CheckpointNumber` newtype |
| `crates/qs-workbench/src/checkpoints.rs:34` | `number: u32` | filed | the same checkpoint number: the same newtype |
| `crates/qs-workbench/src/checkpoints.rs:79` | `number: u32` | filed | the same checkpoint number: the same newtype |
| `crates/qs-sim/src/world.rs:93` | `number: usize` | filed | an edict's number: qc-edict's `EdictNumber` |
| `crates/qs-bsp/src/lib.rs:671` | `planenum: usize` | accepted | a field of id's `dface_t` and its kin, kept as the BSP format names it (BSP-1 to BSP-17); the other `num*` fields beside it are counts, not IDs |
| `crates/qs-sound/src/mixer.rs:102` | `entnum: i32` | accepted | id's `channel_t.entnum`, kept as the C declares it |

### Single-use one-line helpers

| Where | Helper | Body | Verdict |
| --- | --- | --- | --- |
| `apps/desktop/src/entities.rs:129` | `format_live` | `format!("{count} {}", Text::Live.get())` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `apps/desktop/src/shell.rs:3577` | `preview_config` | `PreviewConfig::from_env(std::env::current_exe().unwrap_or_default())` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `apps/desktop/src/snapshots.rs:151` | `tasks_fixture` | `Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tasks")` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `apps/quakec-engine/src/bridge.rs:176` | `flush` | `(&*self.0).flush()` | accepted: a trait method the trait requires, not a helper |
| `crates/qc-builtins/src/dispatch.rs:1011` | `b_sprint` | `self.print_to_client(context)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-builtins/src/math.rs:208` | `format_5_1` | `format!("{:>5.1}", f64::from(f))` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-builtins/src/math.rs:236` | `u32_to_i32` | `i32::try_from(v).unwrap_or(0)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-codegen/src/compile.rs:892` | `set_line` | `self.program.origin = Some((self.file, span.start));` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-codegen/src/compile.rs:944` | `is_joined` | `self.joined.get(&value.ofs) == Some(&self.program.statements.len().saturating_sub(1))` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-ide/src/host.rs:232` | `stored_name` | `self.inner.stored_name(path)` | accepted: a trait method the trait requires, not a helper |
| `crates/qc-preproc/src/lib.rs:2581` | `frame_float` | `qc_lexer::int_to_f32(i64::from(value))` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-sema/src/check/vanilla.rs:366` | `set_name` | `self.name_span(expr.syntax().span(), name);` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-sema/src/db.rs:765` | `locate_diagnostic` | `FileDiagnostic::new(d.code, origins.locate_or(file, d.span), d.message.clone())` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-sema/src/entities.rs:713` | `is_flag_name` | `word.chars().any(\|c\| c.is_ascii_alphabetic()) && !word.eq_ignore_ascii_case("x")` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-sema/src/entities.rs:783` | `peek_is` | `self.0.trim_start().starts_with(c)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-sema/src/map_check.rs:345` | `is_target_key` | `key == "killtarget" \|\| (key.starts_with("target") && key != "targetname")` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-syntax/src/parser/mod.rs:225` | `latin1_byte` | `u8::try_from(u32::from(c)).unwrap_or(b'?')` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-syntax/src/parser/mod.rs:1072` | `recover_past` | `self.skip(scope, true);` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-trace/src/binary.rs:142` | `unzigzag` | `u.wrapping_shr(1).cast_signed() ^ (u & 1).cast_signed().wrapping_neg()` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-vm/src/ops.rs:802` | `goto` | `Ok(Control::Jump(machine.xstatement.wrapping_add(st.a)))` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qc-vm/src/ops.rs:966` | `state_think_time` | `(f64::from(time) + VM_STATE_THINK_INTERVAL) as f32` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-client/src/console.rs:742` | `int_of` | `height as i32` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-client/src/hud.rs:182` | `itoa` | `num.to_string().into_bytes()` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-commands/src/id.rs:74` | `borrow` | `&self.0` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-editor-view/src/view.rs:1177` | `digit_count` | `n.to_string().len().max(2)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-editor-view/src/vim.rs:681` | `is_blank_at` | `char_at(buffer, at).is_none_or(char::is_whitespace)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-map/src/document.rs:722` | `peek_same_line` | `self.lexer.clone().same_line_token().ok()?.ok()` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-map/src/rooms.rs:120` | `own_container` | `RoomKind::of(self).zip(self.value(b"_tb_id").and_then(id))` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-pak/src/lib.rs:429` | `read_array` | `bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-render/src/alias.rs:631` | `f64_of` | `u32::try_from(value).map_or(f64::MAX, f64::from)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-render/src/alias.rs:635` | `i64_of` | `i64::try_from(value).unwrap_or(i64::MAX)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-render/src/mesh.rs:1192` | `truncate_u32` | `value as u32` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-render/src/mesh.rs:1290` | `texture_split` | `fullbright_split(texture.mip(0)?, (texture.width, texture.height), palette)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-render/src/mesh.rs:1429` | `power_of_two` | `n.max(1).checked_next_power_of_two().unwrap_or(n)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-sim/src/spawn.rs:136` | `is_space` | `byte.cast_signed() <= b' '.cast_signed()` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-sim/src/spawn.rs:142` | `above_space` | `byte.cast_signed() > 32` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-sim/src/world/save.rs:400` | `float_field` | `self.field(n, name).as_f32()` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-wad/src/lib.rs:508` | `read_array` | `bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-workbench/src/staged.rs:125` | `is_decided` | `self.hunks.iter().all(\|hunk\| hunk.state != State::Waiting)` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |
| `crates/qs-workbench/src/vm_run.rs:330` | `num_edicts` | `self.machine.edicts().num_edicts()` | filed: inline it at its one call, or keep it with a doc comment saying what its name adds |

### Duplicated code

- `crates/qs-agent/src/lib.rs:1295` (14 lines, `fn collect_paths(value: &Value, out: &mut Vec<String>) {`) and `crates/qs-mcp/src/lib.rs:454` (14 lines, `fn collect_paths(value: &Value, out: &mut Vec<String>) {`): filed, one function both call.
- `crates/qs-agent/src/lib.rs:1319` (18 lines, `fn absolute_paths(arguments: &mut Value, project: &Path) {`) and `crates/qs-mcp/src/lib.rs:478` (18 lines, `fn absolute_paths(arguments: &mut Value, project: &Path) {`): filed, one function both call.
- `crates/qs-agent/src/lib.rs:1341` (11 lines, `fn lexical(path: &Path) -> PathBuf {`) and `crates/qs-mcp/src/lib.rs:501` (11 lines, `fn lexical(path: &Path) -> PathBuf {`): filed, one function both call.
- `crates/qs-workbench/src/commands.rs:816` (10 lines, `fn set<C: HasWorkbench>(&self, context: &mut C, before: bool) -> Result<(), BoxE`) and `crates/qs-workbench/src/map_check.rs:223` (10 lines, `fn set<C: HasWorkbench>(&self, context: &mut C, before: bool) -> Result<(), BoxE`): filed, one function both call.
- `crates/qc-vm/src/trace.rs:173` and `crates/qs-sim/src/spawn.rs:755` each repeat a function of `qs-differential`, a test-only crate outside this audit: not counted.

## Findings by issue

### P6-24: qs-client

- `crates/qs-client/src/console.rs:742` `int_of`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-client/src/hud.rs:182` `itoa`: inline it at its one call, or keep it with a doc comment saying what its name adds

| File:line | Rule | Item |
| --- | --- | --- |
| `crates/qs-client/src/client.rs:342` | bare-id | `SoundStart.entity` |
| `crates/qs-client/src/client.rs:598` | magic-number | `Client::with_limits:3` |
| `crates/qs-client/src/client.rs:605` | magic-number | `Client::with_limits:32` |
| `crates/qs-client/src/client.rs:638` | magic-number | `Client::with_limits:200` |
| `crates/qs-client/src/client.rs:638` | magic-number | `Client::with_limits:320` |
| `crates/qs-client/src/client.rs:700` | bool-argument | `Client::set_showscores` |
| `crates/qs-client/src/client.rs:880` | bare-id | `Client::model(index)` |
| `crates/qs-client/src/client.rs:886` | bare-id | `Client::model_name(index)` |
| `crates/qs-client/src/client.rs:1023` | magic-number | `Client::scene:255` |
| `crates/qs-client/src/client.rs:1099` | magic-number | `Client::read_message:128` |
| `crates/qs-client/src/client.rs:1100` | magic-number | `Client::read_message:127` |
| `crates/qs-client/src/client.rs:1160` | magic-number | `Client::read_message:3` |
| `crates/qs-client/src/client.rs:1161` | magic-number | `Client::read_message:7` |
| `crates/qs-client/src/client.rs:1182` | magic-number | `Client::read_message:16.0` |
| `crates/qs-client/src/client.rs:1185` | magic-number | `Client::read_message:1024` |
| `crates/qs-client/src/client.rs:1185` | magic-number | `Client::read_message:255` |
| `crates/qs-client/src/client.rs:1284` | magic-number | `Client::parse_serverinfo:30` |
| `crates/qs-client/src/client.rs:1306` | magic-number | `Client::parse_serverinfo:35` |
| `crates/qs-client/src/client.rs:1360` | magic-number | `Client::parse_update:32767` |
| `crates/qs-client/src/client.rs:1371` | magic-number | `Client::parse_update:8` |
| `crates/qs-client/src/client.rs:1484` | magic-number | `Client::parse_clientdata:3` |
| `crates/qs-client/src/client.rs:1484` | magic-number | `Client::parse_clientdata:4` |
| `crates/qs-client/src/client.rs:1503` | magic-number | `Client::parse_clientdata:16` |
| `crates/qs-client/src/client.rs:1576` | magic-number | `Client::parse_sound:64.0` |
| `crates/qs-client/src/client.rs:1582` | magic-number | `Client::parse_sound:3` |
| `crates/qs-client/src/client.rs:1583` | magic-number | `Client::parse_sound:7` |
| `crates/qs-client/src/client.rs:1593` | magic-number | `Client::parse_sound:255.0` |
| `crates/qs-client/src/client.rs:1607` | magic-number | `Client::parse_temp_entity:3` |
| `crates/qs-client/src/client.rs:1621` | magic-number | `Client::parse_temp_entity:20` |
| `crates/qs-client/src/client.rs:1621` | magic-number | `Client::parse_temp_entity:30` |
| `crates/qs-client/src/client.rs:1622` | magic-number | `Client::parse_temp_entity:226` |
| `crates/qs-client/src/client.rs:1623` | magic-number | `Client::parse_temp_entity:10` |
| `crates/qs-client/src/client.rs:1632` | magic-number | `Client::parse_temp_entity:5` |
| `crates/qs-client/src/client.rs:1658` | magic-number | `Client::parse_temp_entity:350.0` |
| `crates/qs-client/src/client.rs:1659` | magic-number | `Client::parse_temp_entity:0.5` |
| `crates/qs-client/src/client.rs:1660` | magic-number | `Client::parse_temp_entity:300.0` |
| `crates/qs-client/src/client.rs:1673` | magic-number | `Client::parse_temp_entity:4` |
| `crates/qs-client/src/client.rs:1768` | magic-number | `Client::lerp_point:0.01` |
| `crates/qs-client/src/client.rs:1773` | magic-number | `Client::lerp_point:1.01` |
| `crates/qs-client/src/client.rs:1810` | magic-number | `Client::relink_entities:5` |
| `crates/qs-client/src/client.rs:1824` | magic-number | `Client::relink_entities:3` |
| `crates/qs-client/src/client.rs:1862` | magic-number | `Client::relink_entities:180.0` |
| `crates/qs-client/src/client.rs:1863` | magic-number | `Client::relink_entities:360.0` |
| `crates/qs-client/src/client.rs:1884` | magic-number | `Client::relink_entities:200.0` |
| `crates/qs-client/src/client.rs:1884` | magic-number | `Client::relink_entities:31` |
| `crates/qs-client/src/client.rs:1887` | magic-number | `Client::relink_entities:18.0` |
| `crates/qs-client/src/client.rs:1889` | magic-number | `Client::relink_entities:16.0` |
| `crates/qs-client/src/client.rs:1892` | magic-number | `Client::relink_entities:32.0` |
| `crates/qs-client/src/client.rs:1893` | magic-number | `Client::relink_entities:0.1` |
| `crates/qs-client/src/client.rs:1904` | magic-number | `Client::relink_entities:400.0` |
| `crates/qs-client/src/client.rs:1908` | magic-number | `Client::relink_entities:0.001` |
| `crates/qs-client/src/client.rs:1920` | magic-number | `Client::relink_entities:4` |
| `crates/qs-client/src/client.rs:1930` | magic-number | `Client::relink_entities:0.01` |
| `crates/qs-client/src/client.rs:1934` | magic-number | `Client::relink_entities:6` |
| `crates/qs-client/src/client.rs:2011` | magic-number | `Client::update_temp_entities:270.0` |
| `crates/qs-client/src/client.rs:2011` | magic-number | `Client::update_temp_entities:90.0` |
| `crates/qs-client/src/client.rs:2014` | magic-number | `Client::update_temp_entities:180.0` |
| `crates/qs-client/src/client.rs:2017` | magic-number | `Client::update_temp_entities:360` |
| `crates/qs-client/src/client.rs:2017` | magic-number | `Client::update_temp_entities:360.0` |
| `crates/qs-client/src/client.rs:2078` | free-fn-on-type | `parse_baseline` |
| `crates/qs-client/src/client.rs:2083` | magic-number | `parse_baseline:3` |
| `crates/qs-client/src/client.rs:2101` | bool-argument | `setup_alias_frame` |
| `crates/qs-client/src/client.rs:2101` | free-fn-on-type | `setup_alias_frame` |
| `crates/qs-client/src/client.rs:2166` | bool-argument | `setup_entity_transform` |
| `crates/qs-client/src/client.rs:2166` | free-fn-on-type | `setup_entity_transform` |
| `crates/qs-client/src/client.rs:2166` | magic-number | `setup_entity_transform:3` |
| `crates/qs-client/src/client.rs:2205` | magic-number | `setup_entity_transform:180.0` |
| `crates/qs-client/src/client.rs:2206` | magic-number | `setup_entity_transform:360.0` |
| `crates/qs-client/src/client.rs:2225` | free-fn-on-type | `moved` |
| `crates/qs-client/src/console.rs:158` | magic-number | `Console::resize:3` |
| `crates/qs-client/src/console.rs:205` | magic-number | `Console::print:128` |
| `crates/qs-client/src/console.rs:297` | bool-argument | `Console::toggle` |
| `crates/qs-client/src/console.rs:313` | bool-argument | `Console::backquote` |
| `crates/qs-client/src/console.rs:323` | bool-argument | `Console::slide` |
| `crates/qs-client/src/console.rs:372` | magic-number | `Console::notify:8` |
| `crates/qs-client/src/console.rs:381` | bool-argument | `Console::draw` |
| `crates/qs-client/src/console.rs:399` | magic-number | `Console::draw:16` |
| `crates/qs-client/src/console.rs:399` | magic-number | `Console::draw:3` |
| `crates/qs-client/src/console.rs:407` | magic-number | `Console::draw:8` |
| `crates/qs-client/src/console.rs:421` | bool-argument | `Console::draw_quakespasm` |
| `crates/qs-client/src/console.rs:441` | magic-number | `Console::draw_quakespasm:3` |
| `crates/qs-client/src/console.rs:441` | magic-number | `Console::draw_quakespasm:7` |
| `crates/qs-client/src/console.rs:442` | magic-number | `Console::draw_quakespasm:8` |
| `crates/qs-client/src/console.rs:457` | magic-number | `Console::draw_quakespasm:4` |
| `crates/qs-client/src/console.rs:463` | magic-number | `Console::draw_quakespasm:16` |
| `crates/qs-client/src/console.rs:491` | magic-number | `Console::draw_quakespasm:11` |
| `crates/qs-client/src/console.rs:527` | magic-number | `Console::draw_input:10` |
| `crates/qs-client/src/console.rs:527` | magic-number | `Console::draw_input:11` |
| `crates/qs-client/src/console.rs:544` | magic-number | `Console::draw_input:16` |
| `crates/qs-client/src/console.rs:544` | magic-number | `Console::draw_input:8` |
| `crates/qs-client/src/console.rs:601` | magic-number | `Console::key:3` |
| `crates/qs-client/src/console.rs:665` | magic-number | `Console::key:127` |
| `crates/qs-client/src/console.rs:665` | magic-number | `Console::key:32` |
| `crates/qs-client/src/console.rs:702` | magic-number | `Console::most_back:3` |
| `crates/qs-client/src/console.rs:723` | magic-number | `chars:8` |
| `crates/qs-client/src/hud.rs:188` | bool-argument | `number` |
| `crates/qs-client/src/hud.rs:237` | magic-number | `bar_char:4` |
| `crates/qs-client/src/hud.rs:247` | magic-number | `bar_string:8` |
| `crates/qs-client/src/hud.rs:288` | magic-number | `color_for_map:8` |
| `crates/qs-client/src/hud.rs:292` | magic-number | `three:3` |
| `crates/qs-client/src/hud.rs:311` | free-fn-on-type | `status_bar` |
| `crates/qs-client/src/hud.rs:327` | magic-number | `status_bar:24` |
| `crates/qs-client/src/hud.rs:327` | magic-number | `status_bar:3` |
| `crates/qs-client/src/hud.rs:327` | magic-number | `status_bar:666` |
| `crates/qs-client/src/hud.rs:353` | magic-number | `status_bar:136` |
| `crates/qs-client/src/hud.rs:368` | magic-number | `status_bar:224` |
| `crates/qs-client/src/hud.rs:371` | magic-number | `status_bar:248` |
| `crates/qs-client/src/hud.rs:377` | magic-number | `inventory:17` |
| `crates/qs-client/src/hud.rs:377` | magic-number | `inventory:18` |
| `crates/qs-client/src/hud.rs:377` | magic-number | `inventory:192` |
| `crates/qs-client/src/hud.rs:377` | magic-number | `inventory:28` |
| `crates/qs-client/src/hud.rs:377` | magic-number | `inventory:32` |
| `crates/qs-client/src/hud.rs:377` | magic-number | `inventory:4` |
| `crates/qs-client/src/hud.rs:377` | magic-number | `inventory:6` |
| `crates/qs-client/src/hud.rs:378` | magic-number | `inventory:24` |
| `crates/qs-client/src/hud.rs:385` | magic-number | `inventory:10.0` |
| `crates/qs-client/src/hud.rs:389` | magic-number | `inventory:5` |
| `crates/qs-client/src/hud.rs:397` | magic-number | `inventory:16` |
| `crates/qs-client/src/hud.rs:411` | magic-number | `inventory:8` |
| `crates/qs-client/src/hud.rs:427` | magic-number | `inventory:320` |
| `crates/qs-client/src/hud.rs:440` | magic-number | `frags:240` |
| `crates/qs-client/src/hud.rs:445` | magic-number | `frags:23` |
| `crates/qs-client/src/hud.rs:446` | magic-number | `frags:4` |
| `crates/qs-client/src/hud.rs:451` | magic-number | `frags:15` |
| `crates/qs-client/src/hud.rs:452` | magic-number | `frags:10` |
| `crates/qs-client/src/hud.rs:452` | magic-number | `frags:8` |
| `crates/qs-client/src/hud.rs:458` | magic-number | `frags:28` |
| `crates/qs-client/src/hud.rs:470` | magic-number | `frags:3` |
| `crates/qs-client/src/hud.rs:479` | magic-number | `frags:24` |
| `crates/qs-client/src/hud.rs:482` | magic-number | `frags:16` |
| `crates/qs-client/src/hud.rs:487` | magic-number | `frags:17` |
| `crates/qs-client/src/hud.rs:495` | magic-number | `face:5` |
| `crates/qs-client/src/hud.rs:507` | magic-number | `face:100` |
| `crates/qs-client/src/hud.rs:508` | magic-number | `face:4` |
| `crates/qs-client/src/hud.rs:510` | magic-number | `face:20` |
| `crates/qs-client/src/hud.rs:519` | magic-number | `face:112` |
| `crates/qs-client/src/hud.rs:524` | magic-number | `scoreboard:232` |
| `crates/qs-client/src/hud.rs:531` | magic-number | `scoreboard:4` |
| `crates/qs-client/src/hud.rs:531` | magic-number | `scoreboard:8` |
| `crates/qs-client/src/hud.rs:537` | magic-number | `scoreboard:12` |
| `crates/qs-client/src/hud.rs:538` | magic-number | `scoreboard:60.0` |
| `crates/qs-client/src/hud.rs:540` | magic-number | `scoreboard:10` |
| `crates/qs-client/src/hud.rs:543` | magic-number | `scoreboard:184` |
| `crates/qs-client/src/hud.rs:557` | magic-number | `deathmatch_overlay:240` |
| `crates/qs-client/src/hud.rs:563` | magic-number | `deathmatch_overlay:8` |
| `crates/qs-client/src/hud.rs:567` | magic-number | `deathmatch_overlay:80` |
| `crates/qs-client/src/hud.rs:568` | magic-number | `deathmatch_overlay:40` |
| `crates/qs-client/src/hud.rs:574` | magic-number | `deathmatch_overlay:15` |
| `crates/qs-client/src/hud.rs:574` | magic-number | `deathmatch_overlay:4` |
| `crates/qs-client/src/hud.rs:613` | magic-number | `deathmatch_overlay:12` |
| `crates/qs-client/src/hud.rs:621` | magic-number | `deathmatch_overlay:64` |
| `crates/qs-client/src/hud.rs:628` | magic-number | `deathmatch_overlay:10` |
| `crates/qs-client/src/hud.rs:635` | free-fn-on-type | `intermission_overlay` |
| `crates/qs-client/src/hud.rs:643` | magic-number | `intermission_overlay:16` |
| `crates/qs-client/src/hud.rs:656` | magic-number | `intermission_overlay:64` |
| `crates/qs-client/src/hud.rs:657` | magic-number | `intermission_overlay:24` |
| `crates/qs-client/src/hud.rs:665` | magic-number | `intermission_overlay:56` |
| `crates/qs-client/src/hud.rs:668` | magic-number | `intermission_overlay:60` |
| `crates/qs-client/src/hud.rs:668` | magic-number | `intermission_overlay:60.0` |
| `crates/qs-client/src/hud.rs:669` | magic-number | `intermission_overlay:160` |
| `crates/qs-client/src/hud.rs:669` | magic-number | `intermission_overlay:3` |
| `crates/qs-client/src/hud.rs:671` | magic-number | `intermission_overlay:234` |
| `crates/qs-client/src/hud.rs:676` | magic-number | `intermission_overlay:246` |
| `crates/qs-client/src/hud.rs:683` | magic-number | `intermission_overlay:266` |
| `crates/qs-client/src/hud.rs:691` | magic-number | `intermission_overlay:104` |
| `crates/qs-client/src/hud.rs:696` | magic-number | `intermission_overlay:232` |
| `crates/qs-client/src/hud.rs:700` | magic-number | `intermission_overlay:240` |
| `crates/qs-client/src/hud.rs:710` | magic-number | `intermission_overlay:144` |
| `crates/qs-client/src/hud.rs:765` | bool-argument | `CenterPrint::draw` |
| `crates/qs-client/src/hud.rs:780` | magic-number | `CenterPrint::draw:9999` |
| `crates/qs-client/src/hud.rs:783` | magic-number | `CenterPrint::draw:4` |
| `crates/qs-client/src/hud.rs:784` | magic-number | `CenterPrint::draw:0.35` |
| `crates/qs-client/src/hud.rs:786` | magic-number | `CenterPrint::draw:48` |
| `crates/qs-client/src/hud.rs:794` | magic-number | `CenterPrint::draw:8` |
| `crates/qs-client/src/menu.rs:301` | magic-number | `save_comment:79` |
| `crates/qs-client/src/menu.rs:388` | magic-number | `Menu::quit:7` |
| `crates/qs-client/src/menu.rs:682` | magic-number | `Menu::draw_keys:4` |
| `crates/qs-client/src/menu.rs:684` | magic-number | `Menu::draw_keys:12` |
| `crates/qs-client/src/menu.rs:684` | magic-number | `Menu::draw_keys:32` |
| `crates/qs-client/src/menu.rs:686` | magic-number | `Menu::draw_keys:18` |
| `crates/qs-client/src/menu.rs:688` | magic-number | `Menu::draw_keys:48` |
| `crates/qs-client/src/menu.rs:690` | magic-number | `Menu::draw_keys:16` |
| `crates/qs-client/src/menu.rs:693` | magic-number | `Menu::draw_keys:140` |
| `crates/qs-client/src/menu.rs:696` | magic-number | `Menu::draw_keys:8` |
| `crates/qs-client/src/menu.rs:711` | magic-number | `Menu::draw_keys:130` |
| `crates/qs-client/src/menu.rs:717` | magic-number | `Menu::draw_keys:4.0` |
| `crates/qs-client/src/menu.rs:813` | magic-number | `Menu::draw_state:4` |
| `crates/qs-client/src/menu.rs:814` | magic-number | `Menu::draw_state:32` |
| `crates/qs-client/src/menu.rs:820` | magic-number | `Menu::draw_state:16` |
| `crates/qs-client/src/menu.rs:821` | magic-number | `Menu::draw_state:8` |
| `crates/qs-client/src/menu.rs:837` | magic-number | `Menu::draw_state:24` |
| `crates/qs-client/src/menu.rs:837` | magic-number | `Menu::draw_state:56` |
| `crates/qs-client/src/menu.rs:837` | magic-number | `Menu::draw_state:76` |
| `crates/qs-client/src/menu.rs:839` | magic-number | `Menu::draw_state:100` |
| `crates/qs-client/src/menu.rs:839` | magic-number | `Menu::draw_state:108` |
| `crates/qs-client/src/menu.rs:839` | magic-number | `Menu::draw_state:84` |
| `crates/qs-client/src/menu.rs:839` | magic-number | `Menu::draw_state:92` |
| `crates/qs-client/src/menu.rs:840` | magic-number | `Menu::draw_state:64` |
| `crates/qs-client/src/menu.rs:848` | free-fn-on-type | `new_game` |
| `crates/qs-client/src/menu.rs:876` | magic-number | `adjust:10.0` |
| `crates/qs-client/src/menu.rs:876` | magic-number | `adjust:120.0` |
| `crates/qs-client/src/menu.rs:876` | magic-number | `adjust:30.0` |
| `crates/qs-client/src/menu.rs:879` | magic-number | `adjust:0.05` |
| `crates/qs-client/src/menu.rs:879` | magic-number | `adjust:0.5` |
| `crates/qs-client/src/menu.rs:883` | magic-number | `adjust:11.0` |
| `crates/qs-client/src/menu.rs:885` | magic-number | `adjust:0.1` |
| `crates/qs-client/src/menu.rs:888` | magic-number | `adjust:200.0` |
| `crates/qs-client/src/menu.rs:891` | magic-number | `adjust:400.0` |
| `crates/qs-client/src/menu.rs:925` | magic-number | `draw_options:16` |
| `crates/qs-client/src/menu.rs:925` | magic-number | `draw_options:4` |
| `crates/qs-client/src/menu.rs:927` | magic-number | `draw_options:32` |
| `crates/qs-client/src/menu.rs:930` | magic-number | `draw_options:8` |
| `crates/qs-client/src/menu.rs:933` | magic-number | `draw_options:120` |
| `crates/qs-client/src/menu.rs:933` | magic-number | `draw_options:120.0` |
| `crates/qs-client/src/menu.rs:933` | magic-number | `draw_options:30.0` |
| `crates/qs-client/src/menu.rs:933` | magic-number | `draw_options:56` |
| `crates/qs-client/src/menu.rs:934` | magic-number | `draw_options:0.5` |
| `crates/qs-client/src/menu.rs:934` | magic-number | `draw_options:64` |
| `crates/qs-client/src/menu.rs:935` | magic-number | `draw_options:10.0` |
| `crates/qs-client/src/menu.rs:935` | magic-number | `draw_options:72` |
| `crates/qs-client/src/menu.rs:936` | magic-number | `draw_options:80` |
| `crates/qs-client/src/menu.rs:937` | magic-number | `draw_options:88` |
| `crates/qs-client/src/menu.rs:938` | magic-number | `draw_options:200` |
| `crates/qs-client/src/menu.rs:938` | magic-number | `draw_options:200.0` |
| `crates/qs-client/src/menu.rs:938` | magic-number | `draw_options:96` |
| `crates/qs-client/src/menu.rs:939` | magic-number | `draw_options:104` |
| `crates/qs-client/src/menu.rs:940` | magic-number | `draw_options:112` |
| `crates/qs-client/src/menu.rs:948` | magic-number | `slider:220` |
| `crates/qs-client/src/menu.rs:950` | magic-number | `slider:128` |
| `crates/qs-client/src/menu.rs:950` | magic-number | `slider:8` |
| `crates/qs-client/src/menu.rs:953` | magic-number | `slider:129` |
| `crates/qs-client/src/menu.rs:956` | magic-number | `slider:130` |
| `crates/qs-client/src/menu.rs:962` | magic-number | `slider:131` |
| `crates/qs-client/src/menu.rs:966` | bool-argument | `checkbox` |
| `crates/qs-client/src/menu.rs:967` | magic-number | `checkbox:220` |
| `crates/qs-client/src/menu.rs:972` | magic-number | `line_cursor:12` |
| `crates/qs-client/src/menu.rs:977` | magic-number | `line_cursor:4.0` |
| `crates/qs-client/src/menu.rs:980` | magic-number | `line_cursor:8` |
| `crates/qs-client/src/menu.rs:981` | magic-number | `line_cursor:32` |
| `crates/qs-client/src/menu.rs:995` | magic-number | `notify_string:320` |
| `crates/qs-client/src/menu.rs:996` | magic-number | `notify_string:70` |
| `crates/qs-client/src/menu.rs:1002` | magic-number | `notify_string:40` |
| `crates/qs-client/src/menu.rs:1003` | magic-number | `notify_string:8` |
| `crates/qs-client/src/menu.rs:1037` | magic-number | `plaque:16` |
| `crates/qs-client/src/menu.rs:1037` | magic-number | `plaque:4` |
| `crates/qs-client/src/menu.rs:1039` | magic-number | `plaque:32` |
| `crates/qs-client/src/menu.rs:1039` | magic-number | `plaque:72` |
| `crates/qs-client/src/menu.rs:1050` | magic-number | `cursor:10.0` |
| `crates/qs-client/src/menu.rs:1055` | magic-number | `cursor:20` |
| `crates/qs-client/src/menu.rs:1056` | magic-number | `cursor:32` |
| `crates/qs-client/src/menu.rs:1058` | magic-number | `cursor:54` |
| `crates/qs-client/src/menu.rs:1063` | bool-argument | `print` |
| `crates/qs-client/src/menu.rs:1071` | magic-number | `print:128` |
| `crates/qs-client/src/menu.rs:1074` | magic-number | `print:8` |
| `crates/qs-client/src/menu.rs:1089` | magic-number | `text_box:8` |
| `crates/qs-client/src/menu.rs:1119` | magic-number | `text_box:16` |
| `crates/qs-client/src/message.rs:84` | magic-number | `Reader::long:4` |
| `crates/qs-client/src/message.rs:92` | magic-number | `Reader::float:4` |
| `crates/qs-client/src/message.rs:111` | magic-number | `Reader::coord:8.0` |
| `crates/qs-client/src/message.rs:116` | magic-number | `Reader::angle:256.0` |
| `crates/qs-client/src/message.rs:116` | magic-number | `Reader::angle:360.0` |
| `crates/qs-client/src/particles.rs:79` | magic-number | `Particle::at:3` |
| `crates/qs-client/src/particles.rs:148` | magic-number | `Particles::explosion:1024` |
| `crates/qs-client/src/particles.rs:148` | magic-number | `Particles::explosion:3` |
| `crates/qs-client/src/particles.rs:150` | magic-number | `Particles::explosion:5.0` |
| `crates/qs-client/src/particles.rs:163` | magic-number | `Particles::explosion2:512` |
| `crates/qs-client/src/particles.rs:167` | magic-number | `Particles::explosion2:3` |
| `crates/qs-client/src/particles.rs:173` | magic-number | `Particles::explosion2:0.3` |
| `crates/qs-client/src/particles.rs:182` | magic-number | `Particles::blob_explosion:1024` |
| `crates/qs-client/src/particles.rs:182` | magic-number | `Particles::blob_explosion:3` |
| `crates/qs-client/src/particles.rs:184` | magic-number | `Particles::blob_explosion:0.05` |
| `crates/qs-client/src/particles.rs:184` | magic-number | `Particles::blob_explosion:8` |
| `crates/qs-client/src/particles.rs:186` | magic-number | `Particles::blob_explosion:66` |
| `crates/qs-client/src/particles.rs:188` | magic-number | `Particles::blob_explosion:150` |
| `crates/qs-client/src/particles.rs:190` | magic-number | `Particles::blob_explosion:6` |
| `crates/qs-client/src/particles.rs:204` | magic-number | `Particles::run_effect:3` |
| `crates/qs-client/src/particles.rs:209` | magic-number | `Particles::run_effect:1024` |
| `crates/qs-client/src/particles.rs:214` | magic-number | `Particles::run_effect:0.1` |
| `crates/qs-client/src/particles.rs:214` | magic-number | `Particles::run_effect:5` |
| `crates/qs-client/src/particles.rs:215` | magic-number | `Particles::run_effect:7` |
| `crates/qs-client/src/particles.rs:218` | magic-number | `Particles::run_effect:15` |
| `crates/qs-client/src/particles.rs:218` | magic-number | `Particles::run_effect:8` |
| `crates/qs-client/src/particles.rs:223` | magic-number | `Particles::run_effect:15.0` |
| `crates/qs-client/src/particles.rs:231` | magic-number | `Particles::lava_splash:16` |
| `crates/qs-client/src/particles.rs:231` | magic-number | `Particles::lava_splash:224` |
| `crates/qs-client/src/particles.rs:231` | magic-number | `Particles::lava_splash:50` |
| `crates/qs-client/src/particles.rs:234` | magic-number | `Particles::lava_splash:0.02` |
| `crates/qs-client/src/particles.rs:234` | magic-number | `Particles::lava_splash:31` |
| `crates/qs-client/src/particles.rs:235` | magic-number | `Particles::lava_splash:7` |
| `crates/qs-client/src/particles.rs:236` | magic-number | `Particles::lava_splash:8` |
| `crates/qs-client/src/particles.rs:238` | magic-number | `Particles::lava_splash:256.0` |
| `crates/qs-client/src/particles.rs:240` | magic-number | `Particles::lava_splash:63` |
| `crates/qs-client/src/particles.rs:253` | magic-number | `Particles::teleport_splash:16` |
| `crates/qs-client/src/particles.rs:253` | magic-number | `Particles::teleport_splash:24` |
| `crates/qs-client/src/particles.rs:253` | magic-number | `Particles::teleport_splash:3` |
| `crates/qs-client/src/particles.rs:253` | magic-number | `Particles::teleport_splash:32` |
| `crates/qs-client/src/particles.rs:253` | magic-number | `Particles::teleport_splash:50` |
| `crates/qs-client/src/particles.rs:254` | magic-number | `Particles::teleport_splash:4` |
| `crates/qs-client/src/particles.rs:257` | magic-number | `Particles::teleport_splash:0.02` |
| `crates/qs-client/src/particles.rs:257` | magic-number | `Particles::teleport_splash:0.2` |
| `crates/qs-client/src/particles.rs:257` | magic-number | `Particles::teleport_splash:7` |
| `crates/qs-client/src/particles.rs:260` | magic-number | `Particles::teleport_splash:8` |
| `crates/qs-client/src/particles.rs:271` | magic-number | `Particles::teleport_splash:63` |
| `crates/qs-client/src/particles.rs:283` | magic-number | `Particles::rocket_trail:152` |
| `crates/qs-client/src/particles.rs:283` | magic-number | `Particles::rocket_trail:67` |
| `crates/qs-client/src/particles.rs:287` | magic-number | `Particles::rocket_trail:3` |
| `crates/qs-client/src/particles.rs:294` | magic-number | `Particles::rocket_trail:128` |
| `crates/qs-client/src/particles.rs:295` | magic-number | `Particles::rocket_trail:3.0` |
| `crates/qs-client/src/particles.rs:305` | magic-number | `Particles::rocket_trail:6` |
| `crates/qs-client/src/particles.rs:315` | magic-number | `Particles::rocket_trail:4` |
| `crates/qs-client/src/particles.rs:325` | magic-number | `Particles::rocket_trail:0.5` |
| `crates/qs-client/src/particles.rs:327` | magic-number | `Particles::rocket_trail:230` |
| `crates/qs-client/src/particles.rs:327` | magic-number | `Particles::rocket_trail:52` |
| `crates/qs-client/src/particles.rs:332` | magic-number | `Particles::rocket_trail:30.0` |
| `crates/qs-client/src/particles.rs:340` | magic-number | `Particles::rocket_trail:0.3` |
| `crates/qs-client/src/particles.rs:342` | magic-number | `Particles::rocket_trail:15` |
| `crates/qs-client/src/particles.rs:342` | magic-number | `Particles::rocket_trail:8` |
| `crates/qs-client/src/particles.rs:358` | magic-number | `Particles::entity_field:111` |
| `crates/qs-client/src/particles.rs:362` | magic-number | `Particles::entity_field:0.01` |
| `crates/qs-client/src/particles.rs:362` | magic-number | `Particles::entity_field:255` |
| `crates/qs-client/src/particles.rs:402` | magic-number | `Particles::update:5.0` |
| `crates/qs-client/src/particles.rs:403` | magic-number | `Particles::update:10.0` |
| `crates/qs-client/src/particles.rs:404` | magic-number | `Particles::update:15.0` |
| `crates/qs-client/src/particles.rs:406` | magic-number | `Particles::update:4.0` |
| `crates/qs-client/src/particles.rs:419` | magic-number | `Particles::update:6.0` |
| `crates/qs-client/src/particles.rs:428` | magic-number | `Particles::update:8.0` |
| `crates/qs-client/src/particles.rs:477` | magic-number | `scatter:3` |
| `crates/qs-client/src/particles.rs:481` | magic-number | `scatter:16` |
| `crates/qs-client/src/particles.rs:481` | magic-number | `scatter:32` |
| `crates/qs-client/src/particles.rs:482` | magic-number | `scatter:256` |
| `crates/qs-client/src/particles.rs:482` | magic-number | `scatter:512` |
| `crates/qs-client/src/scene.rs:24` | bare-id | `SceneEntity.entity` |
| `crates/qs-client/src/view.rs:108` | magic-number | `ViewSettings::default:3` |
| `crates/qs-client/src/view.rs:241` | magic-number | `calc_bob:0.3` |
| `crates/qs-client/src/view.rs:241` | magic-number | `calc_bob:0.7` |
| `crates/qs-client/src/view.rs:242` | magic-number | `calc_bob:4.0` |
| `crates/qs-client/src/view.rs:242` | magic-number | `calc_bob:7.0` |
| `crates/qs-client/src/view.rs:249` | magic-number | `angledelta:180.0` |
| `crates/qs-client/src/view.rs:249` | magic-number | `angledelta:360.0` |
| `crates/qs-client/src/view.rs:254` | magic-number | `gamma_table:256` |
| `crates/qs-client/src/view.rs:265` | magic-number | `gamma_table:0.5` |
| `crates/qs-client/src/view.rs:265` | magic-number | `gamma_table:255` |
| `crates/qs-client/src/view.rs:265` | magic-number | `gamma_table:255.5` |
| `crates/qs-client/src/view.rs:266` | magic-number | `gamma_table:255.0` |
| `crates/qs-client/src/view.rs:298` | bool-argument | `ViewState::drift_pitch` |
| `crates/qs-client/src/view.rs:356` | magic-number | `ViewState::damage:0.5` |
| `crates/qs-client/src/view.rs:357` | magic-number | `ViewState::damage:10.0` |
| `crates/qs-client/src/view.rs:358` | magic-number | `ViewState::damage:0.2` |
| `crates/qs-client/src/view.rs:361` | magic-number | `ViewState::damage:150` |
| `crates/qs-client/src/view.rs:361` | magic-number | `ViewState::damage:3.0` |
| `crates/qs-client/src/view.rs:363` | magic-number | `ViewState::damage:100` |
| `crates/qs-client/src/view.rs:363` | magic-number | `ViewState::damage:200` |
| `crates/qs-client/src/view.rs:365` | magic-number | `ViewState::damage:220` |
| `crates/qs-client/src/view.rs:365` | magic-number | `ViewState::damage:50` |
| `crates/qs-client/src/view.rs:367` | magic-number | `ViewState::damage:255` |
| `crates/qs-client/src/view.rs:382` | magic-number | `ViewState::bonus_flash:186` |
| `crates/qs-client/src/view.rs:382` | magic-number | `ViewState::bonus_flash:215` |
| `crates/qs-client/src/view.rs:382` | magic-number | `ViewState::bonus_flash:50` |
| `crates/qs-client/src/view.rs:382` | magic-number | `ViewState::bonus_flash:69` |
| `crates/qs-client/src/view.rs:388` | magic-number | `ViewState::set_contents:130` |
| `crates/qs-client/src/view.rs:388` | magic-number | `ViewState::set_contents:50` |
| `crates/qs-client/src/view.rs:388` | magic-number | `ViewState::set_contents:80` |
| `crates/qs-client/src/view.rs:389` | magic-number | `ViewState::set_contents:150` |
| `crates/qs-client/src/view.rs:389` | magic-number | `ViewState::set_contents:255` |
| `crates/qs-client/src/view.rs:390` | magic-number | `ViewState::set_contents:25` |
| `crates/qs-client/src/view.rs:390` | magic-number | `ViewState::set_contents:5` |
| `crates/qs-client/src/view.rs:391` | magic-number | `ViewState::set_contents:128` |
| `crates/qs-client/src/view.rs:399` | magic-number | `ViewState::powerup_shift:255` |
| `crates/qs-client/src/view.rs:399` | magic-number | `ViewState::powerup_shift:30` |
| `crates/qs-client/src/view.rs:401` | magic-number | `ViewState::powerup_shift:20` |
| `crates/qs-client/src/view.rs:403` | magic-number | `ViewState::powerup_shift:100` |
| `crates/qs-client/src/view.rs:420` | magic-number | `ViewState::fade:150.0` |
| `crates/qs-client/src/view.rs:422` | magic-number | `ViewState::fade:100.0` |
| `crates/qs-client/src/view.rs:433` | magic-number | `ViewState::blend:100.0` |
| `crates/qs-client/src/view.rs:433` | magic-number | `ViewState::blend:255.0` |
| `crates/qs-client/src/view.rs:470` | magic-number | `ViewState::calc_refdef:3` |
| `crates/qs-client/src/view.rs:475` | magic-number | `ViewState::calc_refdef:32.0` |
| `crates/qs-client/src/view.rs:486` | magic-number | `ViewState::calc_refdef:80.0` |
| `crates/qs-client/src/view.rs:501` | magic-number | `ViewState::calc_refdef:14.0` |
| `crates/qs-client/src/view.rs:503` | magic-number | `ViewState::calc_refdef:22.0` |
| `crates/qs-client/src/view.rs:503` | magic-number | `ViewState::calc_refdef:30.0` |
| `crates/qs-client/src/view.rs:511` | magic-number | `ViewState::calc_refdef:0.4` |
| `crates/qs-client/src/view.rs:526` | magic-number | `ViewState::calc_refdef:12.0` |
| `crates/qs-client/src/view.rs:559` | magic-number | `ViewState::gun_angle:0.4` |
| `crates/qs-client/src/view.rs:559` | magic-number | `ViewState::gun_angle:10.0` |
| `crates/qs-client/src/view.rs:561` | magic-number | `ViewState::gun_angle:20.0` |
| `crates/qs-client/src/view.rs:605` | magic-number | `viewsize_lift:110.0` |
| `crates/qs-client/src/view.rs:607` | magic-number | `viewsize_lift:100.0` |
| `crates/qs-client/src/view.rs:609` | magic-number | `viewsize_lift:90.0` |
| `crates/qs-client/src/view.rs:611` | magic-number | `viewsize_lift:80.0` |
| `crates/qs-client/src/view.rs:612` | magic-number | `viewsize_lift:0.5` |

### P6-25: qs-sim

- `crates/qs-sim/src/world.rs:93` `number: usize`: an edict's number: qc-edict's `EdictNumber`
- `crates/qs-sim/src/spawn.rs:136` `is_space`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-sim/src/spawn.rs:142` `above_space`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-sim/src/world/save.rs:400` `float_field`: inline it at its one call, or keep it with a doc comment saying what its name adds

| File:line | Rule | Item |
| --- | --- | --- |
| `crates/qs-sim/src/builtins.rs:491` | bool-argument | `SimCall::link` |
| `crates/qs-sim/src/builtins.rs:667` | magic-number | `SimCall::traceline:3` |
| `crates/qs-sim/src/builtins.rs:710` | magic-number | `SimCall::checkclient:3` |
| `crates/qs-sim/src/builtins.rs:711` | magic-number | `SimCall::checkclient:7` |
| `crates/qs-sim/src/builtins.rs:727` | magic-number | `SimCall::aim:20.0` |
| `crates/qs-sim/src/builtins.rs:730` | magic-number | `SimCall::aim:2048.0` |
| `crates/qs-sim/src/builtins.rs:737` | magic-number | `SimCall::aim:3` |
| `crates/qs-sim/src/builtins.rs:762` | magic-number | `SimCall::aim:0.5` |
| `crates/qs-sim/src/builtins.rs:791` | magic-number | `SimCall::global_vector:3` |
| `crates/qs-sim/src/builtins.rs:994` | free-fn-on-type | `field` |
| `crates/qs-sim/src/builtins.rs:1001` | free-fn-on-type | `parm_raw` |
| `crates/qs-sim/src/builtins.rs:1005` | free-fn-on-type | `parm` |
| `crates/qs-sim/src/builtins.rs:1011` | free-fn-on-type | `parm_float` |
| `crates/qs-sim/src/builtins.rs:1015` | free-fn-on-type | `parm_vector` |
| `crates/qs-sim/src/builtins.rs:1026` | free-fn-on-type | `parm_edict` |
| `crates/qs-sim/src/client.rs:64` | magic-number | `wire_angle:255` |
| `crates/qs-sim/src/client.rs:64` | magic-number | `wire_angle:256` |
| `crates/qs-sim/src/client.rs:64` | magic-number | `wire_angle:360` |
| `crates/qs-sim/src/client.rs:66` | magic-number | `wire_angle:256.0` |
| `crates/qs-sim/src/client.rs:66` | magic-number | `wire_angle:360.0` |
| `crates/qs-sim/src/client.rs:77` | magic-number | `angle_vectors:360.0` |
| `crates/qs-sim/src/client.rs:160` | magic-number | `SimCall::client_think:4.0` |
| `crates/qs-sim/src/client.rs:162` | magic-number | `SimCall::client_think:3.0` |
| `crates/qs-sim/src/client.rs:191` | bool-argument | `SimCall::air_move` |
| `crates/qs-sim/src/client.rs:205` | magic-number | `SimCall::air_move:3` |
| `crates/qs-sim/src/client.rs:243` | magic-number | `SimCall::user_friction:3` |
| `crates/qs-sim/src/client.rs:316` | magic-number | `SimCall::water_move:3` |
| `crates/qs-sim/src/client.rs:440` | magic-number | `SimCall::check_water:3` |
| `crates/qs-sim/src/client.rs:443` | magic-number | `SimCall::check_water:0.5` |
| `crates/qs-sim/src/client.rs:447` | magic-number | `SimCall::check_water:3.0` |
| `crates/qs-sim/src/client.rs:455` | magic-number | `SimCall::check_stuck:18` |
| `crates/qs-sim/src/client.rs:491` | magic-number | `SimCall::wall_friction:0.5` |
| `crates/qs-sim/src/client.rs:507` | magic-number | `SimCall::try_unstick:3` |
| `crates/qs-sim/src/client.rs:523` | magic-number | `SimCall::try_unstick:0.1` |
| `crates/qs-sim/src/client.rs:525` | magic-number | `SimCall::try_unstick:4.0` |
| `crates/qs-sim/src/client.rs:532` | magic-number | `SimCall::try_unstick:7` |
| `crates/qs-sim/src/client.rs:536` | magic-number | `SimCall::walk_move:0.03125` |
| `crates/qs-sim/src/client.rs:593` | magic-number | `SimCall::walk_move:0.7` |
| `crates/qs-sim/src/collision/trace.rs:97` | free-fn-on-type | `recursive_hull_check` |
| `crates/qs-sim/src/collision/trace.rs:112` | free-fn-on-type | `check` |
| `crates/qs-sim/src/console.rs:454` | bool-argument | `World::forward` |
| `crates/qs-sim/src/console.rs:460` | bool-argument | `World::forward_args` |
| `crates/qs-sim/src/console.rs:484` | magic-number | `World::name:15` |
| `crates/qs-sim/src/console.rs:521` | magic-number | `World::color:15` |
| `crates/qs-sim/src/console.rs:522` | magic-number | `World::color:13` |
| `crates/qs-sim/src/console.rs:545` | magic-number | `World::color:16` |
| `crates/qs-sim/src/console.rs:609` | magic-number | `World::print_edict:3` |
| `crates/qs-sim/src/console.rs:620` | magic-number | `World::print_edict:15` |
| `crates/qs-sim/src/console.rs:714` | magic-number | `World::profile:10` |
| `crates/qs-sim/src/debug.rs:92` | bool-argument | `Debugger::set_breakpoint` |
| `crates/qs-sim/src/host_commands.rs:136` | magic-number | `atoi:10` |
| `crates/qs-sim/src/host_commands.rs:149` | bool-argument | `SimCall::client_command` |
| `crates/qs-sim/src/host_commands.rs:282` | magic-number | `SimCall::color:13` |
| `crates/qs-sim/src/host_commands.rs:282` | magic-number | `SimCall::color:15` |
| `crates/qs-sim/src/host_commands.rs:288` | magic-number | `SimCall::color:16` |
| `crates/qs-sim/src/input.rs:223` | magic-number | `Input::release:4` |
| `crates/qs-sim/src/input.rs:260` | magic-number | `Input::key_state:4` |
| `crates/qs-sim/src/input.rs:263` | magic-number | `Input::key_state:0.5` |
| `crates/qs-sim/src/input.rs:265` | magic-number | `Input::key_state:0.75` |
| `crates/qs-sim/src/input.rs:266` | magic-number | `Input::key_state:0.25` |
| `crates/qs-sim/src/input.rs:280` | magic-number | `Input::frame:3` |
| `crates/qs-sim/src/input.rs:350` | magic-number | `Input::adjust_angles:70.0` |
| `crates/qs-sim/src/input.rs:350` | magic-number | `Input::adjust_angles:80.0` |
| `crates/qs-sim/src/input.rs:351` | magic-number | `Input::adjust_angles:50.0` |
| `crates/qs-sim/src/input.rs:377` | magic-number | `Input::mouse_move:70.0` |
| `crates/qs-sim/src/input.rs:377` | magic-number | `Input::mouse_move:80.0` |
| `crates/qs-sim/src/input.rs:391` | magic-number | `anglemod:360.0` |
| `crates/qs-sim/src/input.rs:391` | magic-number | `anglemod:65535` |
| `crates/qs-sim/src/input.rs:391` | magic-number | `anglemod:65536.0` |
| `crates/qs-sim/src/keys.rs:110` | magic-number | `numbered:12` |
| `crates/qs-sim/src/keys.rs:110` | magic-number | `numbered:32` |
| `crates/qs-sim/src/keys.rs:110` | magic-number | `numbered:4` |
| `crates/qs-sim/src/keys.rs:136` | magic-number | `key_name:127` |
| `crates/qs-sim/src/keys.rs:136` | magic-number | `key_name:32` |
| `crates/qs-sim/src/keys.rs:142` | magic-number | `key_name:12` |
| `crates/qs-sim/src/keys.rs:142` | magic-number | `key_name:4` |
| `crates/qs-sim/src/keys.rs:256` | magic-number | `Bindings::bind:3` |
| `crates/qs-sim/src/keys.rs:328` | magic-number | `Bindings::unbound_notice:200` |
| `crates/qs-sim/src/messages.rs:136` | bool-argument | `Message::new` |
| `crates/qs-sim/src/messages.rs:241` | magic-number | `Message::write_coord:8.0` |
| `crates/qs-sim/src/messages.rs:246` | magic-number | `Message::write_angle:255` |
| `crates/qs-sim/src/messages.rs:246` | magic-number | `Message::write_angle:256` |
| `crates/qs-sim/src/messages.rs:246` | magic-number | `Message::write_angle:360` |
| `crates/qs-sim/src/messages.rs:267` | magic-number | `Vis::new:3` |
| `crates/qs-sim/src/messages.rs:267` | magic-number | `Vis::new:31` |
| `crates/qs-sim/src/messages.rs:492` | magic-number | `SimCall::centre:0.5` |
| `crates/qs-sim/src/messages.rs:566` | magic-number | `SimCall::local_sound:4` |
| `crates/qs-sim/src/messages.rs:609` | magic-number | `SimCall::mirror_sound:255.0` |
| `crates/qs-sim/src/messages.rs:609` | magic-number | `SimCall::mirror_sound:3` |
| `crates/qs-sim/src/messages.rs:610` | magic-number | `SimCall::mirror_sound:4` |
| `crates/qs-sim/src/messages.rs:642` | magic-number | `SimCall::mirror_sound:64.0` |
| `crates/qs-sim/src/messages.rs:656` | magic-number | `SimCall::mirror_ambientsound:3` |
| `crates/qs-sim/src/messages.rs:673` | magic-number | `SimCall::mirror_ambientsound:255.0` |
| `crates/qs-sim/src/messages.rs:674` | magic-number | `SimCall::mirror_ambientsound:64.0` |
| `crates/qs-sim/src/messages.rs:682` | magic-number | `SimCall::mirror_particle:3` |
| `crates/qs-sim/src/messages.rs:689` | magic-number | `SimCall::mirror_particle:127` |
| `crates/qs-sim/src/messages.rs:689` | magic-number | `SimCall::mirror_particle:128` |
| `crates/qs-sim/src/messages.rs:689` | magic-number | `SimCall::mirror_particle:16.0` |
| `crates/qs-sim/src/messages.rs:750` | magic-number | `SimCall::mirror_write:58` |
| `crates/qs-sim/src/messages.rs:751` | magic-number | `SimCall::mirror_write:59` |
| `crates/qs-sim/src/messages.rs:971` | magic-number | `SimCall::spawn_message:3` |
| `crates/qs-sim/src/messages.rs:1078` | magic-number | `SimCall::write_entities:3` |
| `crates/qs-sim/src/messages.rs:1079` | magic-number | `SimCall::write_entities:7` |
| `crates/qs-sim/src/messages.rs:1101` | magic-number | `SimCall::write_entities:0.1` |
| `crates/qs-sim/src/messages.rs:1139` | magic-number | `SimCall::write_entities:256` |
| `crates/qs-sim/src/messages.rs:1147` | magic-number | `SimCall::write_entities:8` |
| `crates/qs-sim/src/messages.rs:1182` | magic-number | `SimCall::write_clientdata:32` |
| `crates/qs-sim/src/messages.rs:1236` | magic-number | `SimCall::write_clientdata:23` |
| `crates/qs-sim/src/messages.rs:1237` | magic-number | `SimCall::write_clientdata:28` |
| `crates/qs-sim/src/messages.rs:1274` | magic-number | `SimCall::write_clientdata:16.0` |
| `crates/qs-sim/src/messages.rs:1315` | magic-number | `SimCall::set_ideal_pitch:360.0` |
| `crates/qs-sim/src/messages.rs:1321` | magic-number | `SimCall::set_ideal_pitch:3` |
| `crates/qs-sim/src/messages.rs:1324` | magic-number | `SimCall::set_ideal_pitch:12.0` |
| `crates/qs-sim/src/messages.rs:1328` | magic-number | `SimCall::set_ideal_pitch:160.0` |
| `crates/qs-sim/src/physics.rs:82` | magic-number | `MoveType::to_float:3.0` |
| `crates/qs-sim/src/physics.rs:83` | magic-number | `MoveType::to_float:4.0` |
| `crates/qs-sim/src/physics.rs:84` | magic-number | `MoveType::to_float:5.0` |
| `crates/qs-sim/src/physics.rs:85` | magic-number | `MoveType::to_float:6.0` |
| `crates/qs-sim/src/physics.rs:86` | magic-number | `MoveType::to_float:7.0` |
| `crates/qs-sim/src/physics.rs:87` | magic-number | `MoveType::to_float:8.0` |
| `crates/qs-sim/src/physics.rs:88` | magic-number | `MoveType::to_float:9.0` |
| `crates/qs-sim/src/physics.rs:89` | magic-number | `MoveType::to_float:10.0` |
| `crates/qs-sim/src/physics.rs:99` | bare-id | `Sound.entity` |
| `crates/qs-sim/src/physics.rs:171` | magic-number | `SimCall::physics:3` |
| `crates/qs-sim/src/physics.rs:302` | magic-number | `SimCall::push_move:4.0` |
| `crates/qs-sim/src/physics.rs:422` | magic-number | `SimCall::physics_toss:3` |
| `crates/qs-sim/src/physics.rs:537` | magic-number | `SimCall::fly_move:3` |
| `crates/qs-sim/src/physics.rs:565` | magic-number | `SimCall::fly_move:7` |
| `crates/qs-sim/src/physics.rs:635` | magic-number | `clip_velocity:3` |
| `crates/qs-sim/src/rerelease.rs:93` | magic-number | `patch_builtins:4` |
| `crates/qs-sim/src/scenario.rs:312` | string-error | `parse_expect` |
| `crates/qs-sim/src/scenario.rs:426` | free-fn-on-type | `play` |
| `crates/qs-sim/src/scenario.rs:541` | free-fn-on-type | `run` |
| `crates/qs-sim/src/scenario.rs:570` | free-fn-on-type | `check` |
| `crates/qs-sim/src/scenario.rs:589` | string-error | `read` |
| `crates/qs-sim/src/scenario.rs:589` | free-fn-on-type | `read` |
| `crates/qs-sim/src/scenario.rs:626` | free-fn-on-type | `compare` |
| `crates/qs-sim/src/scenario.rs:646` | free-fn-on-type | `shown` |
| `crates/qs-sim/src/scenario.rs:658` | free-fn-on-type | `shot` |
| `crates/qs-sim/src/spawn.rs:55` | bare-id | `FieldDef.slot` |
| `crates/qs-sim/src/spawn.rs:143` | magic-number | `above_space:32` |
| `crates/qs-sim/src/spawn.rs:258` | bool-argument | `SpawnHost::allocate` |
| `crates/qs-sim/src/spawn.rs:278` | bare-id | `SpawnHost::global(slot)` |
| `crates/qs-sim/src/spawn.rs:281` | bare-id | `SpawnHost::set_field(edict)` |
| `crates/qs-sim/src/spawn.rs:287` | bare-id | `SpawnHost::spawnflags(edict)` |
| `crates/qs-sim/src/spawn.rs:294` | bare-id | `SpawnHost::spawn(edict)` |
| `crates/qs-sim/src/spawn.rs:298` | bare-id | `SpawnHost::free(edict)` |
| `crates/qs-sim/src/spawn.rs:378` | bare-id | `apply_key(edict)` |
| `crates/qs-sim/src/spawn.rs:390` | magic-number | `apply_key:4` |
| `crates/qs-sim/src/spawn.rs:462` | bare-id | `parse_saved_edict(edict)` |
| `crates/qs-sim/src/spawn.rs:519` | bool-argument | `inhibited` |
| `crates/qs-sim/src/spawn.rs:551` | bool-argument | `load_from_file` |
| `crates/qs-sim/src/spawn.rs:636` | magic-number | `c_atoi:10` |
| `crates/qs-sim/src/spawn.rs:726` | magic-number | `parse_vector:3` |
| `crates/qs-sim/src/spawn.rs:755` | magic-number | `ftoi:2147483648.0` |
| `crates/qs-sim/src/spawn.rs:755` | magic-number | `ftoi:2147483649.0` |
| `crates/qs-sim/src/world.rs:374` | magic-number | `World::load:16` |
| `crates/qs-sim/src/world.rs:434` | bool-argument | `World::set_standard_quake` |
| `crates/qs-sim/src/world.rs:674` | magic-number | `World::signon_spawn:16` |
| `crates/qs-sim/src/world/changes.rs:39` | bool-argument | `World::track_changes` |
| `crates/qs-sim/src/world/edit.rs:276` | magic-number | `World::move_player:3` |
| `crates/qs-sim/src/world/edit.rs:293` | free-fn-on-type | `spawn_pairs` |
| `crates/qs-sim/src/world/edit.rs:319` | bare-id | `LineHit.edict` |
| `crates/qs-sim/src/world/edit.rs:334` | magic-number | `World::trace_line:3` |
| `crates/qs-sim/src/world/reload.rs:491` | free-fn-on-type | `change` |
| `crates/qs-sim/src/world/save.rs:127` | magic-number | `SaveGame::read:0.1` |
| `crates/qs-sim/src/world/save.rs:387` | bare-id | `World::slot_of(slot)` |
| `crates/qs-sim/src/world/state.rs:16` | bare-id | `RawState::global(slot)` |
| `crates/qs-sim/src/world/state.rs:18` | bare-id | `RawState::field(edict)` |
| `crates/qs-sim/src/world/state.rs:18` | bare-id | `RawState::field(slot)` |
| `crates/qs-sim/src/world/state.rs:104` | magic-number | `typed_value:3` |

### P6-26: the VM, edicts, builtins and traces

- `crates/qc-builtins/src/table.rs:36` `number: u32`: a builtin number, an ID: a `BuiltinNumber` newtype
- `crates/qc-builtins/src/dispatch.rs:1011` `b_sprint`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-builtins/src/math.rs:208` `format_5_1`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-builtins/src/math.rs:236` `u32_to_i32`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-trace/src/binary.rs:142` `unzigzag`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-vm/src/ops.rs:802` `goto`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-vm/src/ops.rs:966` `state_think_time`: inline it at its one call, or keep it with a doc comment saying what its name adds

| File:line | Rule | Item |
| --- | --- | --- |
| `crates/qc-builtins/src/atof.rs:29` | magic-number | `special:3` |
| `crates/qc-builtins/src/atof.rs:85` | magic-number | `hexadecimal:16` |
| `crates/qc-builtins/src/atof.rs:116` | magic-number | `hexadecimal:10` |
| `crates/qc-builtins/src/atof.rs:154` | magic-number | `Mantissa::push:16` |
| `crates/qc-builtins/src/atof.rs:157` | magic-number | `Mantissa::push:4` |
| `crates/qc-builtins/src/atof.rs:175` | magic-number | `Mantissa::rounded:1023` |
| `crates/qc-builtins/src/atof.rs:178` | magic-number | `Mantissa::rounded:1074` |
| `crates/qc-builtins/src/atof.rs:178` | magic-number | `Mantissa::rounded:52` |
| `crates/qc-builtins/src/atof.rs:204` | magic-number | `Mantissa::rounded:53` |
| `crates/qc-builtins/src/atof.rs:216` | magic-number | `Mantissa::rounded:2047` |
| `crates/qc-builtins/src/dispatch.rs:125` | magic-number | `MockBuiltins::dispatch:3` |
| `crates/qc-builtins/src/dispatch.rs:126` | magic-number | `MockBuiltins::dispatch:4` |
| `crates/qc-builtins/src/dispatch.rs:617` | magic-number | `read_field_vector:3` |
| `crates/qc-builtins/src/dispatch.rs:857` | magic-number | `radius_delta:0.5` |
| `crates/qc-builtins/src/dispatch.rs:874` | magic-number | `MockBuiltins::link_edict:15.0` |
| `crates/qc-builtins/src/dispatch.rs:957` | magic-number | `MockBuiltins::bad_precache_string:128` |
| `crates/qc-builtins/src/dispatch.rs:1143` | magic-number | `ParmArgs::word:3` |
| `crates/qc-builtins/src/dispatch.rs:1283` | magic-number | `MockBuiltins::vkquake_extension:3` |
| `crates/qc-builtins/src/dispatch.rs:1470` | magic-number | `MockBuiltins::b_strconv:3` |
| `crates/qc-builtins/src/dispatch.rs:1490` | magic-number | `MockBuiltins::b_clienttype:3.0` |
| `crates/qc-builtins/src/dispatch.rs:1545` | magic-number | `MockBuiltins::b_substring:1023` |
| `crates/qc-builtins/src/dispatch.rs:1564` | magic-number | `MockBuiltins::b_strncmp:3` |
| `crates/qc-builtins/src/dispatch.rs:1943` | magic-number | `MockBuiltins::b_vk_sound:255` |
| `crates/qc-builtins/src/dispatch.rs:1943` | magic-number | `MockBuiltins::b_vk_sound:255.0` |
| `crates/qc-builtins/src/dispatch.rs:1943` | magic-number | `MockBuiltins::b_vk_sound:3` |
| `crates/qc-builtins/src/dispatch.rs:1944` | magic-number | `MockBuiltins::b_vk_sound:4` |
| `crates/qc-builtins/src/dispatch.rs:1952` | magic-number | `MockBuiltins::b_vk_sound:4.0` |
| `crates/qc-builtins/src/dispatch.rs:1980` | magic-number | `MockBuiltins::b_vk_particle:3` |
| `crates/qc-builtins/src/dispatch.rs:1996` | magic-number | `MockBuiltins::b_vk_ambientsound:3` |
| `crates/qc-builtins/src/dispatch.rs:2033` | magic-number | `MockBuiltins::b_tracebox:3` |
| `crates/qc-builtins/src/dispatch.rs:2036` | magic-number | `MockBuiltins::b_tracebox:4` |
| `crates/qc-builtins/src/dispatch.rs:2037` | magic-number | `MockBuiltins::b_tracebox:5` |
| `crates/qc-builtins/src/dispatch.rs:2163` | magic-number | `MockBuiltins::b_sound:3` |
| `crates/qc-builtins/src/dispatch.rs:2164` | magic-number | `MockBuiltins::b_sound:4` |
| `crates/qc-builtins/src/dispatch.rs:2165` | magic-number | `MockBuiltins::b_sound:255` |
| `crates/qc-builtins/src/dispatch.rs:2165` | magic-number | `MockBuiltins::b_sound:255.0` |
| `crates/qc-builtins/src/dispatch.rs:2170` | magic-number | `MockBuiltins::b_sound:4.0` |
| `crates/qc-builtins/src/dispatch.rs:2173` | magic-number | `MockBuiltins::b_sound:7` |
| `crates/qc-builtins/src/dispatch.rs:2210` | magic-number | `MockBuiltins::b_ambientsound:3` |
| `crates/qc-builtins/src/dispatch.rs:2224` | magic-number | `MockBuiltins::b_particle:3` |
| `crates/qc-builtins/src/dispatch.rs:2361` | magic-number | `MockBuiltins::b_traceline:3` |
| `crates/qc-builtins/src/dispatch.rs:2500` | magic-number | `MockBuiltins::b_movetogoal:315.0` |
| `crates/qc-builtins/src/dispatch.rs:2500` | magic-number | `MockBuiltins::b_movetogoal:45.0` |
| `crates/qc-builtins/src/dispatch.rs:2520` | magic-number | `MockBuiltins::step_self:360.0` |
| `crates/qc-builtins/src/dispatch.rs:2549` | magic-number | `MockBuiltins::change_yaw:180.0` |
| `crates/qc-builtins/src/dispatch.rs:2550` | magic-number | `MockBuiltins::change_yaw:360.0` |
| `crates/qc-builtins/src/localization.rs:77` | magic-number | `Localization::loc_load_file:11` |
| `crates/qc-builtins/src/localization.rs:77` | magic-number | `Localization::loc_load_file:12` |
| `crates/qc-builtins/src/localization.rs:77` | magic-number | `Localization::loc_load_file:187` |
| `crates/qc-builtins/src/localization.rs:77` | magic-number | `Localization::loc_load_file:191` |
| `crates/qc-builtins/src/localization.rs:77` | magic-number | `Localization::loc_load_file:239` |
| `crates/qc-builtins/src/localization.rs:77` | magic-number | `Localization::loc_load_file:8` |
| `crates/qc-builtins/src/localization.rs:84` | magic-number | `Localization::loc_load_file:3` |
| `crates/qc-builtins/src/localization.rs:304` | magic-number | `loc_parse_arg:10` |
| `crates/qc-builtins/src/math.rs:39` | magic-number | `makevectors:360.0` |
| `crates/qc-builtins/src/math.rs:109` | magic-number | `vectoangles:270` |
| `crates/qc-builtins/src/math.rs:109` | magic-number | `vectoangles:90` |
| `crates/qc-builtins/src/math.rs:131` | magic-number | `rint:0.5` |
| `crates/qc-builtins/src/math.rs:159` | magic-number | `anglemod:360.0` |
| `crates/qc-builtins/src/math.rs:159` | magic-number | `anglemod:65536.0` |
| `crates/qc-builtins/src/math.rs:161` | magic-number | `anglemod:65535` |
| `crates/qc-builtins/src/math.rs:217` | magic-number | `whole_degrees:180.0` |
| `crates/qc-builtins/src/math.rs:219` | magic-number | `whole_degrees:360` |
| `crates/qc-builtins/src/math.rs:267` | magic-number | `q_atof:10` |
| `crates/qc-builtins/src/math.rs:271` | magic-number | `q_atof:16.0` |
| `crates/qc-builtins/src/math.rs:299` | magic-number | `q_atof:10.0` |
| `crates/qc-builtins/src/mock.rs:116` | magic-number | `Rng::next_rand:32767` |
| `crates/qc-builtins/src/mock.rs:121` | magic-number | `Rng::next_rand:16` |
| `crates/qc-builtins/src/mock.rs:229` | magic-number | `EngineProfile::coord_bytes:4` |
| `crates/qc-builtins/src/mock.rs:676` | magic-number | `MockEngine::model_bounds_for:3` |
| `crates/qc-builtins/src/mock.rs:685` | magic-number | `MockEngine::model_bounds_for:16.0` |
| `crates/qc-builtins/src/printf.rs:166` | magic-number | `Reader::long:10` |
| `crates/qc-builtins/src/printf.rs:267` | magic-number | `Reader::directive:8` |
| `crates/qc-builtins/src/printf.rs:321` | magic-number | `wide_bits:32` |
| `crates/qc-builtins/src/printf.rs:360` | magic-number | `to_u64:63` |
| `crates/qc-builtins/src/printf.rs:370` | free-fn-on-type | `signed_arg` |
| `crates/qc-builtins/src/printf.rs:380` | free-fn-on-type | `unsigned_arg` |
| `crates/qc-builtins/src/printf.rs:392` | free-fn-on-type | `double_arg` |
| `crates/qc-builtins/src/printf.rs:409` | free-fn-on-type | `convert` |
| `crates/qc-builtins/src/printf.rs:409` | magic-number | `convert:3` |
| `crates/qc-builtins/src/printf.rs:491` | free-fn-on-type | `string_arg` |
| `crates/qc-builtins/src/printf.rs:507` | magic-number | `quoted:4` |
| `crates/qc-builtins/src/printf.rs:543` | bool-argument | `integer` |
| `crates/qc-builtins/src/printf.rs:543` | free-fn-on-type | `integer` |
| `crates/qc-builtins/src/printf.rs:565` | bool-argument | `pad` |
| `crates/qc-builtins/src/printf.rs:565` | free-fn-on-type | `pad` |
| `crates/qc-builtins/src/printf.rs:587` | free-fn-on-type | `floating` |
| `crates/qc-builtins/src/printf.rs:604` | magic-number | `floating:6` |
| `crates/qc-builtins/src/printf.rs:626` | bool-argument | `fixed` |
| `crates/qc-builtins/src/printf.rs:635` | bool-argument | `exponent` |
| `crates/qc-builtins/src/printf.rs:659` | bool-argument | `general` |
| `crates/qc-builtins/src/printf.rs:664` | magic-number | `general:4` |
| `crates/qc-builtins/src/strbuf.rs:52` | bare-id | `StringBuffers::delete(slot)` |
| `crates/qc-builtins/src/strbuf.rs:63` | bare-id | `StringBuffers::size(slot)` |
| `crates/qc-builtins/src/strbuf.rs:69` | bare-id | `StringBuffers::get(index)` |
| `crates/qc-builtins/src/strbuf.rs:69` | bare-id | `StringBuffers::get(slot)` |
| `crates/qc-builtins/src/strbuf.rs:79` | bool-argument | `StringBuffers::add` |
| `crates/qc-builtins/src/strbuf.rs:79` | bare-id | `StringBuffers::add(slot)` |
| `crates/qc-builtins/src/strconv.rs:16` | magic-number | `number:128` |
| `crates/qc-builtins/src/strconv.rs:17` | magic-number | `number:30` |
| `crates/qc-builtins/src/strconv.rs:18` | magic-number | `number:98` |
| `crates/qc-builtins/src/strconv.rs:29` | magic-number | `punct:128` |
| `crates/qc-builtins/src/strconv.rs:42` | magic-number | `alpha:128` |
| `crates/qc-builtins/src/strconv.rs:43` | magic-number | `alpha:5` |
| `crates/qc-builtins/src/strconv.rs:59` | magic-number | `strconv:1023` |
| `crates/qc-builtins/src/strconv.rs:62` | magic-number | `strconv:128` |
| `crates/qc-builtins/src/strconv.rs:67` | magic-number | `strconv:98` |
| `crates/qc-builtins/src/strconv.rs:69` | magic-number | `strconv:30` |
| `crates/qc-builtins/src/strconv.rs:79` | magic-number | `strconv:127` |
| `crates/qc-builtins/src/strconv.rs:79` | magic-number | `strconv:16` |
| `crates/qc-builtins/src/tokenize.rs:129` | bare-id | `Tokens::argv(index)` |
| `crates/qc-builtins/src/tokenize.rs:132` | magic-number | `Tokens::argv:1023` |
| `crates/qc-builtins/src/tokenize.rs:140` | bare-id | `Tokens::start(index)` |
| `crates/qc-builtins/src/tokenize.rs:147` | bare-id | `Tokens::end(index)` |
| `crates/qc-builtins/src/tokenize.rs:166` | magic-number | `com_parse:32` |
| `crates/qc-builtins/src/userinfo.rs:105` | magic-number | `ClientInfo::decode:15` |
| `crates/qc-builtins/src/userinfo.rs:106` | magic-number | `ClientInfo::decode:13` |
| `crates/qc-builtins/src/userinfo.rs:110` | magic-number | `ClientInfo::decode:4` |
| `crates/qc-builtins/src/userinfo.rs:137` | bare-id | `Clients::get(slot)` |
| `crates/qc-builtins/src/userinfo.rs:142` | bare-id | `Clients::get_mut(slot)` |
| `crates/qc-builtins/src/userinfo.rs:174` | magic-number | `ClientCommand::parse:3` |
| `crates/qc-builtins/src/userinfo.rs:198` | magic-number | `tokenize_command:32` |
| `crates/qc-builtins/src/userinfo.rs:323` | magic-number | `atoi:10` |
| `crates/qc-builtins/src/vkstrings.rs:58` | magic-number | `float_to_unsigned:63` |
| `crates/qc-builtins/src/vkstrings.rs:75` | magic-number | `chr2str:57344` |
| `crates/qc-builtins/src/vkstrings.rs:75` | magic-number | `chr2str:57600` |
| `crates/qc-builtins/src/vkstrings.rs:78` | magic-number | `chr2str:6` |
| `crates/qc-builtins/src/vkstrings.rs:82` | magic-number | `chr2str:256` |
| `crates/qc-builtins/src/vkstrings.rs:155` | magic-number | `uri_escape:15` |
| `crates/qc-builtins/src/vkstrings.rs:159` | magic-number | `uri_escape:4` |
| `crates/qc-builtins/src/vkstrings.rs:179` | magic-number | `hex_digit:10` |
| `crates/qc-builtins/src/vkstrings.rs:198` | magic-number | `uri_unescape:4` |
| `crates/qc-builtins/src/vkstrings.rs:205` | magic-number | `uri_unescape:3` |
| `crates/qc-edict/src/layout.rs:117` | magic-number | `EdictLayout::new:4` |
| `crates/qc-edict/src/layout.rs:198` | magic-number | `EdictLayout::pointer_value:4` |
| `crates/qc-edict/src/layout.rs:215` | magic-number | `EdictLayout::pointer_slot:4` |
| `crates/qc-edict/src/store.rs:251` | bool-argument | `EdictStore::track` |
| `crates/qc-edict/src/system.rs:25` | bare-id | `SysVar.slot` |
| `crates/qc-trace/src/binary.rs:20` | free-fn-on-type | `encode` |
| `crates/qc-trace/src/binary.rs:30` | free-fn-on-type | `encode_header` |
| `crates/qc-trace/src/binary.rs:45` | free-fn-on-type | `encode_record` |
| `crates/qc-trace/src/binary.rs:124` | magic-number | `put_uvar:127` |
| `crates/qc-trace/src/binary.rs:124` | magic-number | `put_uvar:128` |
| `crates/qc-trace/src/binary.rs:128` | magic-number | `put_uvar:7` |
| `crates/qc-trace/src/binary.rs:139` | magic-number | `zigzag:31` |
| `crates/qc-trace/src/binary.rs:184` | magic-number | `Reader::header:4` |
| `crates/qc-trace/src/binary.rs:289` | magic-number | `Reader::word:4` |
| `crates/qc-trace/src/binary.rs:297` | magic-number | `Reader::uvar:127` |
| `crates/qc-trace/src/binary.rs:297` | magic-number | `Reader::uvar:128` |
| `crates/qc-trace/src/binary.rs:297` | magic-number | `Reader::uvar:15` |
| `crates/qc-trace/src/binary.rs:309` | magic-number | `Reader::uvar:7` |
| `crates/qc-trace/src/json.rs:19` | free-fn-on-type | `to_json` |
| `crates/qc-trace/src/json.rs:29` | free-fn-on-type | `header_to_json` |
| `crates/qc-trace/src/json.rs:43` | free-fn-on-type | `record_to_json` |
| `crates/qc-trace/src/json.rs:121` | free-fn-on-type | `json_text` |
| `crates/qc-trace/src/json.rs:121` | magic-number | `json_text:126` |
| `crates/qc-trace/src/json.rs:121` | magic-number | `json_text:32` |
| `crates/qc-trace/src/json.rs:184` | free-fn-on-type | `bad` |
| `crates/qc-trace/src/json.rs:290` | magic-number | `parse_bits:8` |
| `crates/qc-trace/src/json.rs:296` | magic-number | `parse_bits:16` |
| `crates/qc-trace/src/json.rs:301` | magic-number | `hex_digit:16` |
| `crates/qc-trace/src/json.rs:308` | magic-number | `unescape_text:126` |
| `crates/qc-trace/src/json.rs:308` | magic-number | `unescape_text:32` |
| `crates/qc-trace/src/json.rs:318` | magic-number | `unescape_text:16` |
| `crates/qc-trace/src/lib.rs:318` | magic-number | `Header::pointer_target:4` |
| `crates/qc-trace/src/lib.rs:330` | bare-id | `GlobalChange.slot` |
| `crates/qc-trace/src/lib.rs:339` | bare-id | `FieldChange.edict` |
| `crates/qc-vm/src/exec.rs:267` | free-fn-on-type | `run_inner` |
| `crates/qc-vm/src/exec.rs:394` | free-fn-on-type | `enter_function` |
| `crates/qc-vm/src/exec.rs:455` | free-fn-on-type | `copy_parameters` |
| `crates/qc-vm/src/exec.rs:483` | free-fn-on-type | `leave_function` |
| `crates/qc-vm/src/exec.rs:518` | bare-id | `function_at(index)` |
| `crates/qc-vm/src/exec.rs:556` | free-fn-on-type | `global_error` |
| `crates/qc-vm/src/exec.rs:584` | free-fn-on-type | `before_first_statement` |
| `crates/qc-vm/src/exec.rs:599` | free-fn-on-type | `stack_trace` |
| `crates/qc-vm/src/machine.rs:38` | bare-id | `GlobalAccessError.slot` |
| `crates/qc-vm/src/machine.rs:216` | bool-argument | `Machine::set_tracing` |
| `crates/qc-vm/src/machine.rs:229` | bare-id | `Machine::global(slot)` |
| `crates/qc-vm/src/machine.rs:246` | bare-id | `Machine::set_global(slot)` |
| `crates/qc-vm/src/machine.rs:266` | bare-id | `Machine::global3(slot)` |
| `crates/qc-vm/src/machine.rs:282` | bare-id | `Machine::set_global3(slot)` |
| `crates/qc-vm/src/machine.rs:300` | bare-id | `add_slot(slot)` |
| `crates/qc-vm/src/ops.rs:48` | free-fn-on-type | `execute_statement` |
| `crates/qc-vm/src/ops.rs:132` | magic-number | `execute_statement:3` |
| `crates/qc-vm/src/ops.rs:133` | magic-number | `execute_statement:4` |
| `crates/qc-vm/src/ops.rs:134` | magic-number | `execute_statement:5` |
| `crates/qc-vm/src/ops.rs:135` | magic-number | `execute_statement:6` |
| `crates/qc-vm/src/ops.rs:136` | magic-number | `execute_statement:7` |
| `crates/qc-vm/src/ops.rs:137` | magic-number | `execute_statement:8` |
| `crates/qc-vm/src/ops.rs:152` | free-fn-on-type | `read_slot` |
| `crates/qc-vm/src/ops.rs:152` | bare-id | `read_slot(slot)` |
| `crates/qc-vm/src/ops.rs:159` | free-fn-on-type | `read_float` |
| `crates/qc-vm/src/ops.rs:164` | free-fn-on-type | `read_vec` |
| `crates/qc-vm/src/ops.rs:172` | free-fn-on-type | `write_slot` |
| `crates/qc-vm/src/ops.rs:172` | bare-id | `write_slot(slot)` |
| `crates/qc-vm/src/ops.rs:184` | free-fn-on-type | `write_float` |
| `crates/qc-vm/src/ops.rs:194` | bool-argument | `bool_slot` |
| `crates/qc-vm/src/ops.rs:204` | free-fn-on-type | `float_binop` |
| `crates/qc-vm/src/ops.rs:224` | free-fn-on-type | `vector_binop` |
| `crates/qc-vm/src/ops.rs:224` | magic-number | `vector_binop:3` |
| `crates/qc-vm/src/ops.rs:241` | free-fn-on-type | `mul_v` |
| `crates/qc-vm/src/ops.rs:252` | free-fn-on-type | `mul_fv` |
| `crates/qc-vm/src/ops.rs:252` | magic-number | `mul_fv:3` |
| `crates/qc-vm/src/ops.rs:264` | free-fn-on-type | `mul_vf` |
| `crates/qc-vm/src/ops.rs:264` | magic-number | `mul_vf:3` |
| `crates/qc-vm/src/ops.rs:276` | free-fn-on-type | `add_operand` |
| `crates/qc-vm/src/ops.rs:302` | free-fn-on-type | `float_cmp` |
| `crates/qc-vm/src/ops.rs:348` | bool-argument | `Equality::holds` |
| `crates/qc-vm/src/ops.rs:358` | free-fn-on-type | `vector_cmp` |
| `crates/qc-vm/src/ops.rs:411` | free-fn-on-type | `string_cmp` |
| `crates/qc-vm/src/ops.rs:459` | free-fn-on-type | `bits_cmp` |
| `crates/qc-vm/src/ops.rs:479` | free-fn-on-type | `not_float` |
| `crates/qc-vm/src/ops.rs:492` | free-fn-on-type | `not_vector` |
| `crates/qc-vm/src/ops.rs:508` | free-fn-on-type | `not_string` |
| `crates/qc-vm/src/ops.rs:524` | free-fn-on-type | `not_bits` |
| `crates/qc-vm/src/ops.rs:539` | free-fn-on-type | `logical` |
| `crates/qc-vm/src/ops.rs:564` | free-fn-on-type | `bitwise` |
| `crates/qc-vm/src/ops.rs:587` | free-fn-on-type | `store_scalar` |
| `crates/qc-vm/src/ops.rs:600` | free-fn-on-type | `store_vector` |
| `crates/qc-vm/src/ops.rs:600` | magic-number | `store_vector:3` |
| `crates/qc-vm/src/ops.rs:624` | free-fn-on-type | `load_target` |
| `crates/qc-vm/src/ops.rs:645` | free-fn-on-type | `load_scalar` |
| `crates/qc-vm/src/ops.rs:660` | free-fn-on-type | `load_vector` |
| `crates/qc-vm/src/ops.rs:660` | magic-number | `load_vector:3` |
| `crates/qc-vm/src/ops.rs:693` | free-fn-on-type | `address` |
| `crates/qc-vm/src/ops.rs:729` | free-fn-on-type | `storep_target` |
| `crates/qc-vm/src/ops.rs:744` | free-fn-on-type | `pointer_target` |
| `crates/qc-vm/src/ops.rs:754` | free-fn-on-type | `storep_scalar` |
| `crates/qc-vm/src/ops.rs:767` | free-fn-on-type | `storep_vector` |
| `crates/qc-vm/src/ops.rs:767` | magic-number | `storep_vector:3` |
| `crates/qc-vm/src/ops.rs:802` | free-fn-on-type | `goto` |
| `crates/qc-vm/src/ops.rs:814` | bool-argument | `JumpWhen::jumps` |
| `crates/qc-vm/src/ops.rs:823` | free-fn-on-type | `branch` |
| `crates/qc-vm/src/ops.rs:844` | free-fn-on-type | `call` |
| `crates/qc-vm/src/ops.rs:907` | free-fn-on-type | `done` |
| `crates/qc-vm/src/ops.rs:907` | magic-number | `done:3` |
| `crates/qc-vm/src/ops.rs:921` | free-fn-on-type | `state` |
| `crates/qc-vm/src/trace.rs:128` | free-fn-on-type | `snapshot_fields` |

### P6-27: the lexer, preprocessor and syntax tree

- `crates/qc-preproc/src/lib.rs:2581` `frame_float`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-syntax/src/parser/mod.rs:225` `latin1_byte`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-syntax/src/parser/mod.rs:1072` `recover_past`: inline it at its one call, or keep it with a doc comment saying what its name adds

| File:line | Rule | Item |
| --- | --- | --- |
| `crates/qc-lexer/src/fte_string.rs:95` | bool-argument | `Lexer::fte_string_prefix` |
| `crates/qc-lexer/src/fte_string.rs:125` | bool-argument | `Lexer::fte_string` |
| `crates/qc-lexer/src/fte_string.rs:213` | magic-number | `Lexer::cooked_string:57372` |
| `crates/qc-lexer/src/fte_string.rs:335` | magic-number | `Lexer::character_in_string:128` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:11` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:12` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57360` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57361` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57362` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57372` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57373` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57374` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57375` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57472` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57473` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57474` |
| `crates/qc-lexer/src/fte_string.rs:386` | magic-number | `Lexer::fte_escape:57475` |
| `crates/qc-lexer/src/fte_string.rs:418` | magic-number | `Lexer::fte_escape:4` |
| `crates/qc-lexer/src/fte_string.rs:419` | magic-number | `Lexer::fte_escape:8` |
| `crates/qc-lexer/src/fte_string.rs:421` | magic-number | `Lexer::fte_escape:16` |
| `crates/qc-lexer/src/fte_string.rs:463` | magic-number | `Lexer::fte_octal:8` |
| `crates/qc-lexer/src/fte_string.rs:474` | magic-number | `Lexer::fte_escape:7` |
| `crates/qc-lexer/src/fte_string.rs:491` | magic-number | `Lexer::fte_braced:16` |
| `crates/qc-lexer/src/fte_string.rs:493` | magic-number | `Lexer::fte_braced:10` |
| `crates/qc-lexer/src/fte_string.rs:522` | magic-number | `Lexer::fte_hex:16` |
| `crates/qc-lexer/src/fte_string.rs:544` | magic-number | `Lexer::write_code:128` |
| `crates/qc-lexer/src/fte_string.rs:544` | magic-number | `Lexer::write_code:57344` |
| `crates/qc-lexer/src/fte_string.rs:544` | magic-number | `Lexer::write_code:57599` |
| `crates/qc-lexer/src/fte_string.rs:562` | magic-number | `Lexer::utf8_code:1114111` |
| `crates/qc-lexer/src/fte_string.rs:593` | magic-number | `utf8:128` |
| `crates/qc-lexer/src/fte_string.rs:593` | magic-number | `utf8:63` |
| `crates/qc-lexer/src/fte_string.rs:593` | magic-number | `utf8:65280` |
| `crates/qc-lexer/src/fte_string.rs:600` | magic-number | `utf8:4` |
| `crates/qc-lexer/src/fte_string.rs:600` | magic-number | `utf8:5` |
| `crates/qc-lexer/src/fte_string.rs:607` | magic-number | `utf8:6` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:1114111` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:128` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:15` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:192` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:2048` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:31` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:63` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:65536` |
| `crates/qc-lexer/src/fte_string.rs:623` | magic-number | `utf8_check:7` |
| `crates/qc-lexer/src/fte_string.rs:628` | magic-number | `utf8_check:3` |
| `crates/qc-lexer/src/fte_string.rs:629` | magic-number | `utf8_check:4` |
| `crates/qc-lexer/src/fte_string.rs:638` | magic-number | `utf8_check:64` |
| `crates/qc-lexer/src/lib.rs:92` | magic-number | `is_whitespace:128` |
| `crates/qc-lexer/src/lib.rs:538` | magic-number | `hex_float:16` |
| `crates/qc-lexer/src/lib.rs:539` | magic-number | `hex_float:16.0` |
| `crates/qc-lexer/src/lib.rs:623` | magic-number | `c_atoi:10` |
| `crates/qc-lexer/src/lib.rs:677` | bool-argument | `DollarCommand::from_word` |
| `crates/qc-lexer/src/lib.rs:777` | magic-number | `Lexer::run:128` |
| `crates/qc-lexer/src/lib.rs:875` | magic-number | `Lexer::fte_skip_trivia:3` |
| `crates/qc-lexer/src/lib.rs:1113` | magic-number | `Lexer::fteqcc_number:16` |
| `crates/qc-lexer/src/lib.rs:1122` | magic-number | `Lexer::fteqcc_number:8` |
| `crates/qc-lexer/src/lib.rs:1128` | magic-number | `Lexer::fteqcc_number:10` |
| `crates/qc-lexer/src/lib.rs:1258` | magic-number | `Lexer::check_suffix:128` |
| `crates/qc-lexer/src/lib.rs:1319` | magic-number | `Lexer::vector:3` |
| `crates/qc-lexer/src/lib.rs:1357` | magic-number | `Lexer::character:128` |
| `crates/qc-lexer/src/lib.rs:1357` | magic-number | `Lexer::character:65533` |
| `crates/qc-lexer/src/lib.rs:1391` | magic-number | `Lexer::character:3` |
| `crates/qc-lexer/src/lib.rs:1499` | magic-number | `Lexer::fte_name:128` |
| `crates/qc-lexer/src/lib.rs:1599` | magic-number | `Lexer::macro_name:128` |
| `crates/qc-lexer/src/lib.rs:1599` | magic-number | `Lexer::macro_name:33` |
| `crates/qc-preproc/src/cond.rs:55` | bool-argument | `CondStack::open` |
| `crates/qc-preproc/src/cond.rs:148` | magic-number | `atoi:10` |
| `crates/qc-preproc/src/cond.rs:182` | magic-number | `fte_atoi:10` |
| `crates/qc-preproc/src/cond.rs:589` | magic-number | `FteIf::value:7` |
| `crates/qc-preproc/src/fte_directive.rs:151` | magic-number | `join_end:3` |
| `crates/qc-preproc/src/fte_directive.rs:302` | magic-number | `skip_to_end_of_line:3` |
| `crates/qc-preproc/src/fte_directive.rs:805` | magic-number | `read_if:5` |
| `crates/qc-preproc/src/fte_directive.rs:807` | magic-number | `read_if:6` |
| `crates/qc-preproc/src/fte_directive.rs:853` | magic-number | `read_pack:3` |
| `crates/qc-preproc/src/fte_directive.rs:995` | magic-number | `false_side:4` |
| `crates/qc-preproc/src/fte_directive.rs:1002` | magic-number | `false_side:3` |
| `crates/qc-preproc/src/fte_directive.rs:1261` | magic-number | `Pragma::line:4` |
| `crates/qc-preproc/src/fte_directive.rs:1271` | magic-number | `strtoul:12` |
| `crates/qc-preproc/src/fte_directive.rs:1290` | magic-number | `strtoul:16` |
| `crates/qc-preproc/src/fte_directive.rs:1292` | magic-number | `strtoul:8` |
| `crates/qc-preproc/src/fte_directive.rs:1294` | magic-number | `strtoul:10` |
| `crates/qc-preproc/src/fte_directive/load.rs:64` | magic-number | `Wide::width:4` |
| `crates/qc-preproc/src/fte_directive/load.rs:72` | magic-number | `Wide::value:4` |
| `crates/qc-preproc/src/fte_directive/load.rs:100` | free-fn-on-type | `decode_utf` |
| `crates/qc-preproc/src/fte_directive/load.rs:100` | magic-number | `decode_utf:1023` |
| `crates/qc-preproc/src/fte_directive/load.rs:100` | magic-number | `decode_utf:1114111` |
| `crates/qc-preproc/src/fte_directive/load.rs:100` | magic-number | `decode_utf:55296` |
| `crates/qc-preproc/src/fte_directive/load.rs:100` | magic-number | `decode_utf:56320` |
| `crates/qc-preproc/src/fte_directive/load.rs:100` | magic-number | `decode_utf:57344` |
| `crates/qc-preproc/src/fte_directive/load.rs:100` | magic-number | `decode_utf:65536` |
| `crates/qc-preproc/src/fte_directive/load.rs:115` | magic-number | `decode_utf:10` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:127` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:128` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:15` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:192` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:2047` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:224` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:240` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:31` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:63` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:65535` |
| `crates/qc-preproc/src/fte_directive/load.rs:134` | magic-number | `encode:7` |
| `crates/qc-preproc/src/fte_directive/load.rs:144` | magic-number | `encode:6` |
| `crates/qc-preproc/src/fte_directive/load.rs:146` | magic-number | `encode:12` |
| `crates/qc-preproc/src/fte_directive/load.rs:148` | magic-number | `encode:18` |
| `crates/qc-preproc/src/fte_keywords.rs:324` | magic-number | `Keyword::bit:3` |
| `crates/qc-preproc/src/fte_keywords.rs:325` | magic-number | `Keyword::bit:4` |
| `crates/qc-preproc/src/fte_keywords.rs:326` | magic-number | `Keyword::bit:5` |
| `crates/qc-preproc/src/fte_keywords.rs:327` | magic-number | `Keyword::bit:6` |
| `crates/qc-preproc/src/fte_keywords.rs:328` | magic-number | `Keyword::bit:7` |
| `crates/qc-preproc/src/fte_keywords.rs:329` | magic-number | `Keyword::bit:8` |
| `crates/qc-preproc/src/fte_keywords.rs:330` | magic-number | `Keyword::bit:9` |
| `crates/qc-preproc/src/fte_keywords.rs:331` | magic-number | `Keyword::bit:10` |
| `crates/qc-preproc/src/fte_keywords.rs:332` | magic-number | `Keyword::bit:11` |
| `crates/qc-preproc/src/fte_keywords.rs:333` | magic-number | `Keyword::bit:12` |
| `crates/qc-preproc/src/fte_keywords.rs:334` | magic-number | `Keyword::bit:13` |
| `crates/qc-preproc/src/fte_keywords.rs:335` | magic-number | `Keyword::bit:14` |
| `crates/qc-preproc/src/fte_keywords.rs:336` | magic-number | `Keyword::bit:15` |
| `crates/qc-preproc/src/fte_keywords.rs:337` | magic-number | `Keyword::bit:16` |
| `crates/qc-preproc/src/fte_keywords.rs:338` | magic-number | `Keyword::bit:17` |
| `crates/qc-preproc/src/fte_keywords.rs:339` | magic-number | `Keyword::bit:18` |
| `crates/qc-preproc/src/fte_keywords.rs:340` | magic-number | `Keyword::bit:19` |
| `crates/qc-preproc/src/fte_keywords.rs:341` | magic-number | `Keyword::bit:20` |
| `crates/qc-preproc/src/fte_keywords.rs:342` | magic-number | `Keyword::bit:21` |
| `crates/qc-preproc/src/fte_keywords.rs:343` | magic-number | `Keyword::bit:22` |
| `crates/qc-preproc/src/fte_keywords.rs:344` | magic-number | `Keyword::bit:23` |
| `crates/qc-preproc/src/fte_keywords.rs:345` | magic-number | `Keyword::bit:24` |
| `crates/qc-preproc/src/fte_keywords.rs:346` | magic-number | `Keyword::bit:25` |
| `crates/qc-preproc/src/fte_keywords.rs:347` | magic-number | `Keyword::bit:26` |
| `crates/qc-preproc/src/fte_keywords.rs:348` | magic-number | `Keyword::bit:27` |
| `crates/qc-preproc/src/fte_keywords.rs:349` | magic-number | `Keyword::bit:28` |
| `crates/qc-preproc/src/fte_keywords.rs:350` | magic-number | `Keyword::bit:29` |
| `crates/qc-preproc/src/fte_keywords.rs:351` | magic-number | `Keyword::bit:30` |
| `crates/qc-preproc/src/fte_keywords.rs:352` | magic-number | `Keyword::bit:31` |
| `crates/qc-preproc/src/fte_keywords.rs:353` | magic-number | `Keyword::bit:32` |
| `crates/qc-preproc/src/fte_keywords.rs:354` | magic-number | `Keyword::bit:33` |
| `crates/qc-preproc/src/fte_keywords.rs:355` | magic-number | `Keyword::bit:34` |
| `crates/qc-preproc/src/fte_keywords.rs:356` | magic-number | `Keyword::bit:35` |
| `crates/qc-preproc/src/fte_keywords.rs:357` | magic-number | `Keyword::bit:36` |
| `crates/qc-preproc/src/fte_keywords.rs:358` | magic-number | `Keyword::bit:37` |
| `crates/qc-preproc/src/fte_keywords.rs:359` | magic-number | `Keyword::bit:38` |
| `crates/qc-preproc/src/fte_keywords.rs:360` | magic-number | `Keyword::bit:39` |
| `crates/qc-preproc/src/fte_keywords.rs:361` | magic-number | `Keyword::bit:40` |
| `crates/qc-preproc/src/fte_keywords.rs:362` | magic-number | `Keyword::bit:41` |
| `crates/qc-preproc/src/fte_keywords.rs:363` | magic-number | `Keyword::bit:42` |
| `crates/qc-preproc/src/fte_keywords.rs:364` | magic-number | `Keyword::bit:43` |
| `crates/qc-preproc/src/fte_keywords.rs:365` | magic-number | `Keyword::bit:44` |
| `crates/qc-preproc/src/fte_keywords.rs:366` | magic-number | `Keyword::bit:45` |
| `crates/qc-preproc/src/fte_keywords.rs:367` | magic-number | `Keyword::bit:46` |
| `crates/qc-preproc/src/fte_keywords.rs:368` | magic-number | `Keyword::bit:47` |
| `crates/qc-preproc/src/fte_keywords.rs:369` | magic-number | `Keyword::bit:48` |
| `crates/qc-preproc/src/fte_keywords.rs:370` | magic-number | `Keyword::bit:49` |
| `crates/qc-preproc/src/fte_keywords.rs:371` | magic-number | `Keyword::bit:50` |
| `crates/qc-preproc/src/fte_keywords.rs:372` | magic-number | `Keyword::bit:51` |
| `crates/qc-preproc/src/fte_keywords.rs:373` | magic-number | `Keyword::bit:52` |
| `crates/qc-preproc/src/fte_keywords.rs:374` | magic-number | `Keyword::bit:53` |
| `crates/qc-preproc/src/fte_keywords.rs:480` | bool-argument | `Keywords::with` |
| `crates/qc-preproc/src/fte_keywords.rs:506` | bool-argument | `Keywords::with_coexist` |
| `crates/qc-preproc/src/fte_keywords.rs:512` | bool-argument | `Keywords::with_case_insensitive` |
| `crates/qc-preproc/src/fte_keywords.rs:518` | bool-argument | `Keywords::with_hexen2_target` |
| `crates/qc-preproc/src/fte_keywords.rs:523` | bool-argument | `Keywords::set` |
| `crates/qc-preproc/src/fte_keywords.rs:581` | bool-argument | `Keywords::named_definition` |
| `crates/qc-preproc/src/fte_keywords.rs:650` | bool-argument | `KeywordEdit::hexen2_target` |
| `crates/qc-preproc/src/fte_macro.rs:276` | bool-argument | `macro_string_spelling` |
| `crates/qc-preproc/src/fte_macro.rs:292` | bool-argument | `ChunkReader::next_item` |
| `crates/qc-preproc/src/fte_macro.rs:721` | free-fn-on-type | `read_define` |
| `crates/qc-preproc/src/fte_macro.rs:767` | free-fn-on-type | `read_body` |
| `crates/qc-preproc/src/fte_pragma.rs:102` | bool-argument | `FlagSettings::set_default` |
| `crates/qc-preproc/src/fte_pragma.rs:164` | bool-argument | `FlagSettings::set_named` |
| `crates/qc-preproc/src/fte_pragma.rs:175` | bool-argument | `FlagSettings::set` |
| `crates/qc-preproc/src/fte_pragma.rs:240` | magic-number | `optimise_warnings:3` |
| `crates/qc-preproc/src/lib.rs:210` | bool-argument | `MacroTable::set_any_case` |
| `crates/qc-preproc/src/lib.rs:2003` | magic-number | `Preprocessor::define:3` |
| `crates/qc-preproc/src/lib.rs:2009` | magic-number | `Preprocessor::define:4` |
| `crates/qc-syntax/src/ast.rs:81` | free-fn-on-type | `child` |
| `crates/qc-syntax/src/ast.rs:85` | free-fn-on-type | `children` |
| `crates/qc-syntax/src/ast.rs:90` | free-fn-on-type | `around` |
| `crates/qc-syntax/src/ast.rs:487` | free-fn-on-type | `flatten_externs` |
| `crates/qc-syntax/src/ast.rs:533` | free-fn-on-type | `decl_type` |
| `crates/qc-syntax/src/ast.rs:595` | free-fn-on-type | `scoped_name` |
| `crates/qc-syntax/src/ast.rs:600` | free-fn-on-type | `scope_of` |
| `crates/qc-syntax/src/ast.rs:641` | free-fn-on-type | `pointer_form` |
| `crates/qc-syntax/src/ast.rs:667` | free-fn-on-type | `head_stars` |
| `crates/qc-syntax/src/ast.rs:675` | free-fn-on-type | `prefix_lists` |
| `crates/qc-syntax/src/ast.rs:684` | free-fn-on-type | `suffix_list` |
| `crates/qc-syntax/src/ast.rs:1339` | free-fn-on-type | `is_word` |
| `crates/qc-syntax/src/c_scalar.rs:287` | magic-number | `CWords::take:16` |
| `crates/qc-syntax/src/c_scalar.rs:289` | magic-number | `CWords::take:32` |
| `crates/qc-syntax/src/c_scalar.rs:293` | magic-number | `CWords::take:8` |
| `crates/qc-syntax/src/c_scalar.rs:299` | magic-number | `CWords::take:128` |
| `crates/qc-syntax/src/c_scalar.rs:299` | magic-number | `CWords::take:64` |
| `crates/qc-syntax/src/c_scalar.rs:320` | bool-argument | `CWords::made` |
| `crates/qc-syntax/src/c_scalar.rs:327` | magic-number | `CWords::made:32` |
| `crates/qc-syntax/src/c_scalar.rs:334` | magic-number | `CWords::made:64` |
| `crates/qc-syntax/src/c_scalar.rs:341` | magic-number | `CWords::made:8` |
| `crates/qc-syntax/src/c_scalar.rs:343` | magic-number | `CWords::made:16` |
| `crates/qc-syntax/src/lib.rs:312` | free-fn-on-type | `finish` |
| `crates/qc-syntax/src/parser/fte.rs:656` | magic-number | `Parser::at_parenthesised_name:3` |
| `crates/qc-syntax/src/parser/fte.rs:850` | bool-argument | `Parser::fte_param` |
| `crates/qc-syntax/src/parser/fte.rs:1364` | magic-number | `Parser::fte_at_cast:3` |
| `crates/qc-syntax/src/parser/fte.rs:1432` | magic-number | `Parser::vector_expr:3` |
| `crates/qc-syntax/src/parser/fte.rs:1471` | bool-argument | `Parser::sizeof_type_follows` |
| `crates/qc-syntax/src/parser/fte.rs:1759` | bool-argument | `Parser::scoped_name` |
| `crates/qc-syntax/src/parser/mod.rs:64` | magic-number | `Precedence::top:6` |
| `crates/qc-syntax/src/parser/mod.rs:65` | magic-number | `Precedence::top:13` |
| `crates/qc-syntax/src/parser/mod.rs:73` | magic-number | `Precedence::not:4` |
| `crates/qc-syntax/src/parser/mod.rs:82` | magic-number | `Precedence::assign:5` |
| `crates/qc-syntax/src/parser/mod.rs:83` | magic-number | `Precedence::assign:13` |
| `crates/qc-syntax/src/parser/mod.rs:94` | magic-number | `Precedence::ternary:5` |
| `crates/qc-syntax/src/parser/mod.rs:95` | magic-number | `Precedence::ternary:12` |
| `crates/qc-syntax/src/parser/mod.rs:129` | magic-number | `Precedence::binary:4` |
| `crates/qc-syntax/src/parser/mod.rs:132` | magic-number | `Precedence::binary:5` |
| `crates/qc-syntax/src/parser/mod.rs:133` | magic-number | `Precedence::binary:6` |
| `crates/qc-syntax/src/parser/mod.rs:143` | magic-number | `Precedence::binary:7` |
| `crates/qc-syntax/src/parser/mod.rs:144` | magic-number | `Precedence::binary:8` |
| `crates/qc-syntax/src/parser/mod.rs:145` | magic-number | `Precedence::binary:9` |
| `crates/qc-syntax/src/parser/mod.rs:146` | magic-number | `Precedence::binary:10` |
| `crates/qc-syntax/src/parser/mod.rs:147` | magic-number | `Precedence::binary:11` |
| `crates/qc-syntax/src/parser/mod.rs:148` | magic-number | `Precedence::binary:13` |
| `crates/qc-syntax/src/parser/mod.rs:708` | bool-argument | `Parser::define` |
| `crates/qc-syntax/src/parser/mod.rs:1076` | bool-argument | `Parser::skip` |
| `crates/qc-syntax/src/parser/mod.rs:1822` | bool-argument | `Parser::declarator` |
| `crates/qc-syntax/src/parser/mod.rs:1914` | bool-argument | `Parser::declarator_head` |
| `crates/qc-syntax/src/parser/mod.rs:2334` | bool-argument | `Parser::declare_type_shaped` |
| `crates/qc-syntax/src/parser/mod.rs:2632` | bool-argument | `Parser::for_local_decl` |
| `crates/qc-syntax/src/parser/mod.rs:2749` | bool-argument | `Parser::local_decl` |
| `crates/qc-syntax/src/parser/mod.rs:2924` | bool-argument | `Parser::operand` |
| `crates/qc-syntax/src/recovery.rs:101` | free-fn-on-type | `items` |
| `crates/qc-syntax/src/recovery.rs:127` | free-fn-on-type | `deletable` |

### P6-28: the semantic model, the IDE layer and the compiler

- `crates/qc-sema/src/lower/decl.rs:890` `number: i32`: a builtin's `binum`, an ID: the same `BuiltinNumber`
- `crates/qc-codegen/src/compile.rs:892` `set_line`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-codegen/src/compile.rs:944` `is_joined`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-sema/src/check/vanilla.rs:366` `set_name`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-sema/src/db.rs:765` `locate_diagnostic`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-sema/src/entities.rs:713` `is_flag_name`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-sema/src/entities.rs:783` `peek_is`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qc-sema/src/map_check.rs:345` `is_target_key`: inline it at its one call, or keep it with a doc comment saying what its name adds

| File:line | Rule | Item |
| --- | --- | --- |
| `crates/qc-codegen/src/compile.rs:681` | magic-number | `Compiler::function:8` |
| `crates/qc-codegen/src/compile.rs:684` | magic-number | `Compiler::function:3` |
| `crates/qc-codegen/src/compile.rs:755` | magic-number | `Compiler::parameter_slot:3` |
| `crates/qc-codegen/src/compile.rs:1049` | magic-number | `parm_slot:3` |
| `crates/qc-codegen/src/compile.rs:1094` | free-fn-on-type | `literal_of_token` |
| `crates/qc-codegen/src/compile/allocate.rs:30` | bare-id | `Slots::statement(index)` |
| `crates/qc-codegen/src/compile/allocate.rs:46` | bare-id | `Slots::block(index)` |
| `crates/qc-codegen/src/compile/allocate.rs:61` | bare-id | `Slots::dropped(index)` |
| `crates/qc-codegen/src/compile/allocate.rs:103` | bare-id | `CreationOrder::statement(index)` |
| `crates/qc-codegen/src/compile/allocate.rs:117` | bare-id | `CreationOrder::block(index)` |
| `crates/qc-codegen/src/compile/arrays.rs:83` | bool-argument | `Compiler::array_accessor` |
| `crates/qc-codegen/src/compile/arrays.rs:187` | magic-number | `Compiler::emit_getter:3` |
| `crates/qc-codegen/src/compile/arrays.rs:247` | magic-number | `Compiler::emit_setter:3` |
| `crates/qc-codegen/src/compile/arrays.rs:330` | magic-number | `Compiler::set_ladder:3` |
| `crates/qc-codegen/src/compile/arrays.rs:346` | magic-number | `Compiler::set_ladder:4` |
| `crates/qc-codegen/src/compile/arrays.rs:395` | magic-number | `Compiler::ladder:4` |
| `crates/qc-codegen/src/compile/arrays.rs:427` | magic-number | `Compiler::bounds_error:3` |
| `crates/qc-codegen/src/compile/body.rs:101` | magic-number | `Compiler::write_statements:3` |
| `crates/qc-codegen/src/compile/body.rs:465` | bool-argument | `writable` |
| `crates/qc-codegen/src/compile/body.rs:504` | magic-number | `call_count:3` |
| `crates/qc-codegen/src/compile/body.rs:505` | magic-number | `call_count:4` |
| `crates/qc-codegen/src/compile/body.rs:506` | magic-number | `call_count:5` |
| `crates/qc-codegen/src/compile/body.rs:507` | magic-number | `call_count:6` |
| `crates/qc-codegen/src/compile/body.rs:508` | magic-number | `call_count:7` |
| `crates/qc-codegen/src/compile/body.rs:509` | magic-number | `call_count:8` |
| `crates/qc-codegen/src/compile/body.rs:531` | bool-argument | `bare_return` |
| `crates/qc-codegen/src/ops.rs:74` | free-fn-on-type | `row` |
| `crates/qc-codegen/src/ops.rs:140` | free-fn-on-type | `select` |
| `crates/qc-codegen/src/program.rs:83` | magic-number | `ValueType::words:3` |
| `crates/qc-codegen/src/program.rs:946` | magic-number | `Program::emit_result:3` |
| `crates/qc-engines/src/check.rs:54` | free-fn-on-type | `missing_builtins` |
| `crates/qc-ide/src/actions.rs:362` | magic-number | `remove_local:3` |
| `crates/qc-ide/src/format.rs:72` | magic-number | `protected:4` |
| `crates/qc-ide/src/highlight.rs:69` | magic-number | `Modifier::bit:4` |
| `crates/qc-ide/src/highlight.rs:161` | bool-argument | `modifiers` |
| `crates/qc-ide/src/host.rs:602` | bare-id | `AnalysisHost::module(index)` |
| `crates/qc-project/src/graph.rs:216` | bool-argument | `com_parse_words` |
| `crates/qc-project/src/graph.rs:284` | bool-argument | `list_words` |
| `crates/qc-project/src/graph.rs:328` | bool-argument | `code_line` |
| `crates/qc-sema/src/cast.rs:58` | magic-number | `assumed_int:2147483647` |
| `crates/qc-sema/src/cast.rs:58` | magic-number | `assumed_int:4294967295` |
| `crates/qc-sema/src/cast.rs:58` | magic-number | `assumed_int:9223372036854775807` |
| `crates/qc-sema/src/cast.rs:89` | magic-number | `Literal::of_fteqcc_number:16` |
| `crates/qc-sema/src/cast.rs:91` | magic-number | `Literal::of_fteqcc_number:8` |
| `crates/qc-sema/src/cast.rs:93` | magic-number | `Literal::of_fteqcc_number:10` |
| `crates/qc-sema/src/cast.rs:402` | free-fn-on-type | `pointees_agree` |
| `crates/qc-sema/src/check/assign.rs:617` | magic-number | `Checker::convert_to:3` |
| `crates/qc-sema/src/check/assign.rs:729` | bool-argument | `Checker::step` |
| `crates/qc-sema/src/check/assign.rs:854` | magic-number | `Checker::vector_aggregate:3` |
| `crates/qc-sema/src/check/assign.rs:943` | bool-argument | `Checker::element_store` |
| `crates/qc-sema/src/check/assign.rs:1026` | magic-number | `Checker::copy_struct:3` |
| `crates/qc-sema/src/check/assign.rs:1135` | magic-number | `Checker::array_set:3` |
| `crates/qc-sema/src/check/assign.rs:1135` | magic-number | `Checker::array_set:3.0` |
| `crates/qc-sema/src/check/assign.rs:1394` | magic-number | `Checker::local_declarator:8` |
| `crates/qc-sema/src/check/call.rs:430` | magic-number | `Checker::call_statements:3` |
| `crates/qc-sema/src/check/call.rs:600` | magic-number | `Checker::generate_call:3` |
| `crates/qc-sema/src/check/call.rs:661` | magic-number | `Checker::generate_call:7` |
| `crates/qc-sema/src/check/call.rs:747` | magic-number | `Checker::intrinsic_value:3` |
| `crates/qc-sema/src/check/call.rs:748` | magic-number | `Checker::intrinsic_value:4` |
| `crates/qc-sema/src/check/call.rs:763` | magic-number | `Checker::intrinsic_value:3.0` |
| `crates/qc-sema/src/check/call.rs:1019` | free-fn-on-type | `type_params` |
| `crates/qc-sema/src/check/call.rs:1040` | free-fn-on-type | `variant_parameter` |
| `crates/qc-sema/src/check/constant.rs:93` | bool-argument | `Constant::string` |
| `crates/qc-sema/src/check/constant.rs:205` | magic-number | `Constant::of_number:16` |
| `crates/qc-sema/src/check/constant.rs:214` | magic-number | `Constant::of_number:8` |
| `crates/qc-sema/src/check/constant.rs:215` | magic-number | `Constant::of_number:10` |
| `crates/qc-sema/src/check/constant.rs:220` | magic-number | `Constant::of_number:36` |
| `crates/qc-sema/src/check/emulate.rs:246` | magic-number | `Checker::statement_temps:3` |
| `crates/qc-sema/src/check/emulate.rs:318` | magic-number | `Checker::clobber_return:3` |
| `crates/qc-sema/src/check/emulate.rs:644` | magic-number | `Checker::complement:3` |
| `crates/qc-sema/src/check/emulate.rs:656` | magic-number | `Checker::by_word:3` |
| `crates/qc-sema/src/check/emulate.rs:1027` | magic-number | `Checker::unsigned:2147483648` |
| `crates/qc-sema/src/check/emulate.rs:1027` | magic-number | `Checker::unsigned:255` |
| `crates/qc-sema/src/check/emulate.rs:1027` | magic-number | `Checker::unsigned:32` |
| `crates/qc-sema/src/check/emulate.rs:1067` | magic-number | `Checker::unsigned:8` |
| `crates/qc-sema/src/check/emulate.rs:1459` | free-fn-on-type | `conversion_opcodes` |
| `crates/qc-sema/src/check/expr.rs:390` | bool-argument | `Checker::def_value` |
| `crates/qc-sema/src/check/expr.rs:606` | magic-number | `Checker::convert:100` |
| `crates/qc-sema/src/check/flow.rs:245` | bool-argument | `Layout::finish` |
| `crates/qc-sema/src/check/flow.rs:264` | bool-argument | `Layout::dead_and_missing_returns` |
| `crates/qc-sema/src/check/fold.rs:60` | bool-argument | `builtin_call` |
| `crates/qc-sema/src/check/fold.rs:111` | bool-argument | `fold_builtin_call` |
| `crates/qc-sema/src/check/fold.rs:148` | bool-argument | `float_bool` |
| `crates/qc-sema/src/check/fold.rs:153` | bool-argument | `int_bool` |
| `crates/qc-sema/src/check/fold.rs:166` | magic-number | `both:255` |
| `crates/qc-sema/src/check/fold.rs:166` | magic-number | `both:32` |
| `crates/qc-sema/src/check/fold.rs:337` | magic-number | `both:8` |
| `crates/qc-sema/src/check/fold.rs:608` | magic-number | `overflow:2139095040` |
| `crates/qc-sema/src/check/fold.rs:630` | free-fn-on-type | `round` |
| `crates/qc-sema/src/check/fold.rs:660` | free-fn-on-type | `lanes` |
| `crates/qc-sema/src/check/fold.rs:670` | free-fn-on-type | `lanes_f` |
| `crates/qc-sema/src/check/fold.rs:815` | magic-number | `c_g:5` |
| `crates/qc-sema/src/check/fold.rs:815` | magic-number | `c_g:6` |
| `crates/qc-sema/src/check/fold.rs:832` | magic-number | `c_g:4` |
| `crates/qc-sema/src/check/member.rs:305` | bool-argument | `Checker::vector_component` |
| `crates/qc-sema/src/check/member.rs:441` | magic-number | `Checker::struct_member:8` |
| `crates/qc-sema/src/check/member.rs:442` | magic-number | `Checker::struct_member:16` |
| `crates/qc-sema/src/check/member.rs:450` | magic-number | `Checker::struct_member:31` |
| `crates/qc-sema/src/check/member.rs:454` | magic-number | `Checker::struct_member:5` |
| `crates/qc-sema/src/check/member.rs:505` | magic-number | `Checker::struct_value:5` |
| `crates/qc-sema/src/check/member.rs:600` | magic-number | `Checker::member_index:4` |
| `crates/qc-sema/src/check/member.rs:600` | magic-number | `Checker::member_index:8` |
| `crates/qc-sema/src/check/member.rs:731` | free-fn-on-type | `alignment` |
| `crates/qc-sema/src/check/member.rs:733` | magic-number | `alignment:8` |
| `crates/qc-sema/src/check/member.rs:734` | magic-number | `alignment:16` |
| `crates/qc-sema/src/check/member.rs:735` | magic-number | `alignment:32` |
| `crates/qc-sema/src/check/mod.rs:659` | bool-argument | `Reference::element` |
| `crates/qc-sema/src/check/mod.rs:822` | bare-id | `TypedBody::origin(index)` |
| `crates/qc-sema/src/check/mod.rs:833` | bare-id | `TypedBody::string(index)` |
| `crates/qc-sema/src/check/mod.rs:856` | free-fn-on-type | `converts` |
| `crates/qc-sema/src/check/reference.rs:71` | magic-number | `Checker::read:8` |
| `crates/qc-sema/src/check/reference.rs:189` | magic-number | `Checker::narrow_element:31` |
| `crates/qc-sema/src/check/reference.rs:229` | magic-number | `Checker::narrow_bitfield:31` |
| `crates/qc-sema/src/check/reference.rs:246` | magic-number | `Checker::narrow_local:31` |
| `crates/qc-sema/src/check/reference.rs:267` | magic-number | `Checker::extend_bits:8` |
| `crates/qc-sema/src/check/reference.rs:287` | magic-number | `Checker::field_vector_index:3` |
| `crates/qc-sema/src/check/reference.rs:317` | bool-argument | `Checker::load_from_array` |
| `crates/qc-sema/src/check/reference.rs:359` | magic-number | `Checker::load_from_array:3` |
| `crates/qc-sema/src/check/reference.rs:359` | magic-number | `Checker::load_from_array:3.0` |
| `crates/qc-sema/src/check/reference.rs:522` | magic-number | `Checker::load_from_pointer:8` |
| `crates/qc-sema/src/check/reference.rs:524` | magic-number | `Checker::load_from_pointer:16` |
| `crates/qc-sema/src/check/reference.rs:619` | magic-number | `Checker::load_words_from_pointer:3` |
| `crates/qc-sema/src/check/reference.rs:727` | magic-number | `Checker::index:3` |
| `crates/qc-sema/src/check/reference.rs:911` | magic-number | `Checker::address_of:4` |
| `crates/qc-sema/src/check/reference.rs:959` | magic-number | `Checker::address_of:8` |
| `crates/qc-sema/src/check/reference.rs:1070` | magic-number | `Checker::store_reference:3.0` |
| `crates/qc-sema/src/check/reference.rs:1229` | free-fn-on-type | `eval_int` |
| `crates/qc-sema/src/check/reference.rs:1241` | free-fn-on-type | `word_type` |
| `crates/qc-sema/src/check/reference.rs:1284` | free-fn-on-type | `narrow_bits` |
| `crates/qc-sema/src/check/reference.rs:1286` | magic-number | `narrow_bits:8` |
| `crates/qc-sema/src/check/reference.rs:1287` | magic-number | `narrow_bits:16` |
| `crates/qc-sema/src/check/statement.rs:66` | magic-number | `Checker::variadic_prologue:3` |
| `crates/qc-sema/src/check/statement.rs:195` | magic-number | `Checker::statement:3` |
| `crates/qc-sema/src/check/statement.rs:1202` | free-fn-on-type | `dropped_block` |
| `crates/qc-sema/src/check/term.rs:115` | magic-number | `Checker::negate:3` |
| `crates/qc-sema/src/check/term.rs:132` | bool-argument | `Checker::prefix_step` |
| `crates/qc-sema/src/check/term.rs:414` | magic-number | `Checker::vector_of:3` |
| `crates/qc-sema/src/check/term.rs:537` | magic-number | `Checker::size_of:31` |
| `crates/qc-sema/src/check/term.rs:540` | magic-number | `Checker::size_of:3` |
| `crates/qc-sema/src/check/term.rs:573` | magic-number | `Checker::length:3` |
| `crates/qc-sema/src/check/term.rs:583` | free-fn-on-type | `signed_arithmetic` |
| `crates/qc-sema/src/check/term.rs:597` | bool-argument | `step_f32` |
| `crates/qc-sema/src/check/truth.rs:56` | magic-number | `Test::opcode:3` |
| `crates/qc-sema/src/check/truth.rs:262` | bool-argument | `Checker::ternary_condition` |
| `crates/qc-sema/src/check/truth.rs:353` | magic-number | `Checker::condition_arm:3` |
| `crates/qc-sema/src/check/truth.rs:527` | bool-argument | `Checker::arms` |
| `crates/qc-sema/src/check/truth.rs:620` | free-fn-on-type | `words` |
| `crates/qc-sema/src/check/truth.rs:622` | magic-number | `words:3` |
| `crates/qc-sema/src/check/truth.rs:629` | free-fn-on-type | `is_truth` |
| `crates/qc-sema/src/check/vanilla.rs:669` | magic-number | `Vanilla::call:8` |
| `crates/qc-sema/src/db.rs:60` | bare-id | `QueryCounts.module_index` |
| `crates/qc-sema/src/db.rs:217` | bool-argument | `Db::set_autoproto` |
| `crates/qc-sema/src/db.rs:631` | bool-argument | `Db::expanded_file` |
| `crates/qc-sema/src/db.rs:765` | free-fn-on-type | `locate_diagnostic` |
| `crates/qc-sema/src/entities.rs:399` | bool-argument | `Classes::self_field` |
| `crates/qc-sema/src/entities.rs:616` | magic-number | `literal_after:256` |
| `crates/qc-sema/src/entities.rs:638` | free-fn-on-type | `is_spawn_type` |
| `crates/qc-sema/src/entities.rs:644` | free-fn-on-type | `key_type` |
| `crates/qc-sema/src/entities.rs:699` | magic-number | `flag_value:32` |
| `crates/qc-sema/src/entities.rs:866` | magic-number | `byte:255.0` |
| `crates/qc-sema/src/expand.rs:203` | bool-argument | `ProtoPlan::new` |
| `crates/qc-sema/src/expand.rs:305` | bool-argument | `preprocess` |
| `crates/qc-sema/src/expand.rs:305` | free-fn-on-type | `preprocess` |
| `crates/qc-sema/src/ir.rs:149` | magic-number | `Name::is_made_once:8` |
| `crates/qc-sema/src/ir.rs:194` | magic-number | `BodyStatement::uses:3` |
| `crates/qc-sema/src/ir.rs:212` | bare-id | `BodyStatement::temp_words(index)` |
| `crates/qc-sema/src/ir.rs:235` | magic-number | `BodyStatement::temp_words:3` |
| `crates/qc-sema/src/ir.rs:244` | bare-id | `BodyStatement::made_words(index)` |
| `crates/qc-sema/src/ir.rs:248` | magic-number | `BodyStatement::made_words:3` |
| `crates/qc-sema/src/lower/body.rs:233` | free-fn-on-type | `collect_params` |
| `crates/qc-sema/src/lower/decl.rs:544` | free-fn-on-type | `string_len` |
| `crates/qc-sema/src/lower/decl.rs:1118` | free-fn-on-type | `no_function` |
| `crates/qc-sema/src/lower/decl.rs:1135` | free-fn-on-type | `declarator_type` |
| `crates/qc-sema/src/lower/expr.rs:331` | free-fn-on-type | `type_uses` |
| `crates/qc-sema/src/lower/types.rs:290` | magic-number | `IntConst::of:9.0e15` |
| `crates/qc-sema/src/lower/types.rs:1159` | free-fn-on-type | `nested_type` |
| `crates/qc-sema/src/map_check.rs:72` | bare-id | `MapDiagnostic.entity` |
| `crates/qc-sema/src/map_check.rs:330` | magic-number | `EntitySchema::check_pair:3` |
| `crates/qc-sema/src/nav.rs:47` | bool-argument | `ModuleIndex::references` |
| `crates/qc-sema/src/resolve/body.rs:151` | bool-argument | `Resolver::resolve_body` |
| `crates/qc-sema/src/resolve/body.rs:475` | bool-argument | `Resolver::declare_alias_def` |
| `crates/qc-sema/src/resolve/mod.rs:811` | bool-argument | `Init::of` |
| `crates/qc-sema/src/resolve/mod.rs:878` | bool-argument | `Resolver::pass` |
| `crates/qc-sema/src/resolve/types/layout.rs:77` | magic-number | `Resolver::layout_at:8` |
| `crates/qc-sema/src/resolve/types/layout.rs:78` | magic-number | `Resolver::layout_at:16` |
| `crates/qc-sema/src/resolve/types/layout.rs:83` | magic-number | `Resolver::layout_at:3` |
| `crates/qc-sema/src/resolve/types/layout.rs:223` | magic-number | `Resolver::member_layout:32` |
| `crates/qc-sema/src/resolve/types/layout.rs:337` | free-fn-on-type | `array_count` |
| `crates/qc-sema/src/ty.rs:210` | magic-number | `Type::word_size:3` |
| `crates/qc-sema/src/ty.rs:470` | magic-number | `default_text:16` |
| `crates/qc-sema/src/ty.rs:498` | magic-number | `default_text:32` |
| `crates/qc-sema/src/ty.rs:500` | magic-number | `default_text:64` |

### P6-29: qs-render

- `crates/qs-render/src/camera.rs:14` `yaw: f32`: a `Degrees` (or `Seconds`) newtype: an editor-side angle or time with no C declaration to follow
- `crates/qs-render/src/camera.rs:16` `pitch: f32`: a `Degrees` (or `Seconds`) newtype: an editor-side angle or time with no C declaration to follow
- `crates/qs-render/src/camera.rs:19` `fov_x: f32`: a `Degrees` (or `Seconds`) newtype: an editor-side angle or time with no C declaration to follow
- `crates/qs-render/src/alias.rs:631` `f64_of`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-render/src/alias.rs:635` `i64_of`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-render/src/mesh.rs:1192` `truncate_u32`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-render/src/mesh.rs:1290` `texture_split`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-render/src/mesh.rs:1429` `power_of_two`: inline it at its one call, or keep it with a doc comment saying what its name adds

| File:line | Rule | Item |
| --- | --- | --- |
| `crates/qs-render/src/alias.rs:178` | magic-number | `AliasModel::read:3` |
| `crates/qs-render/src/alias.rs:221` | magic-number | `AliasModel::read:0.5` |
| `crates/qs-render/src/alias.rs:352` | bool-argument | `shade_light` |
| `crates/qs-render/src/alias.rs:404` | magic-number | `bob_rotate:65535` |
| `crates/qs-render/src/alias.rs:440` | magic-number | `translation:16` |
| `crates/qs-render/src/alias.rs:440` | magic-number | `translation:240` |
| `crates/qs-render/src/alias.rs:440` | magic-number | `translation:255` |
| `crates/qs-render/src/alias.rs:440` | magic-number | `translation:256` |
| `crates/qs-render/src/alias.rs:446` | magic-number | `translation:15` |
| `crates/qs-render/src/alias.rs:446` | magic-number | `translation:4` |
| `crates/qs-render/src/alias.rs:449` | magic-number | `translation:128` |
| `crates/qs-render/src/alias.rs:478` | magic-number | `skin_images:4` |
| `crates/qs-render/src/alias.rs:585` | magic-number | `word:4` |
| `crates/qs-render/src/camera.rs:63` | magic-number | `Camera::turn:360.0` |
| `crates/qs-render/src/camera.rs:64` | magic-number | `Camera::turn:89.0` |
| `crates/qs-render/src/camera.rs:145` | magic-number | `multiply:4` |
| `crates/qs-render/src/gpu.rs:189` | bare-id | `PlayerSkin.slot` |
| `crates/qs-render/src/gpu.rs:282` | free-fn-on-type | `pipeline_of` |
| `crates/qs-render/src/gpu.rs:359` | magic-number | `MapRenderer::new:3` |
| `crates/qs-render/src/gpu.rs:839` | magic-number | `MapRenderer::set_lightmap:4` |
| `crates/qs-render/src/gpu.rs:883` | magic-number | `MapRenderer::set_view:4` |
| `crates/qs-render/src/gpu.rs:1019` | magic-number | `model_bytes:256` |
| `crates/qs-render/src/gpu.rs:1038` | magic-number | `model_offset:256` |
| `crates/qs-render/src/gpu.rs:1104` | free-fn-on-type | `blend_of` |
| `crates/qs-render/src/gpu.rs:1117` | free-fn-on-type | `primitive` |
| `crates/qs-render/src/gpu.rs:1131` | free-fn-on-type | `world_sampler` |
| `crates/qs-render/src/gpu.rs:1165` | magic-number | `upload_skybox:255` |
| `crates/qs-render/src/gpu.rs:1248` | magic-number | `Fog::default:3` |
| `crates/qs-render/src/gpu.rs:1290` | magic-number | `View::new:3.0` |
| `crates/qs-render/src/gpu.rs:1290` | magic-number | `View::new:4.0` |
| `crates/qs-render/src/gpu.rs:1372` | magic-number | `MapCallback::paint:3` |
| `crates/qs-render/src/look.rs:42` | magic-number | `Look::light_shift:8` |
| `crates/qs-render/src/look.rs:43` | magic-number | `Look::light_shift:7` |
| `crates/qs-render/src/look.rs:80` | magic-number | `Look::fade_alpha:0.5` |
| `crates/qs-render/src/look.rs:81` | magic-number | `Look::fade_alpha:0.8` |
| `crates/qs-render/src/look.rs:90` | bool-argument | `Look::console_alpha` |
| `crates/qs-render/src/look.rs:93` | magic-number | `Look::console_alpha:0.5` |
| `crates/qs-render/src/look.rs:95` | magic-number | `Look::console_alpha:3` |
| `crates/qs-render/src/look.rs:99` | magic-number | `Look::console_alpha:1.2` |
| `crates/qs-render/src/look.rs:157` | magic-number | `adapt_fov_x:0.75` |
| `crates/qs-render/src/look.rs:163` | magic-number | `adapt_fov_x:360.0` |
| `crates/qs-render/src/md5.rs:105` | bool-argument | `Md5Model::read` |
| `crates/qs-render/src/md5.rs:251` | magic-number | `frame_joints:3` |
| `crates/qs-render/src/md5.rs:273` | magic-number | `surface_vertices:3` |
| `crates/qs-render/src/md5.rs:345` | magic-number | `surface_skins:4` |
| `crates/qs-render/src/md5.rs:352` | magic-number | `surface_skins:3` |
| `crates/qs-render/src/md5.rs:382` | magic-number | `quantise:360.0` |
| `crates/qs-render/src/mesh.rs:93` | magic-number | `Palette::read:256` |
| `crates/qs-render/src/mesh.rs:93` | magic-number | `Palette::read:3` |
| `crates/qs-render/src/mesh.rs:107` | magic-number | `Palette::with_gamma:256.0` |
| `crates/qs-render/src/mesh.rs:113` | magic-number | `Palette::with_gamma:0.5` |
| `crates/qs-render/src/mesh.rs:113` | magic-number | `Palette::with_gamma:255.0` |
| `crates/qs-render/src/mesh.rs:120` | bare-id | `Palette::rgba(index)` |
| `crates/qs-render/src/mesh.rs:122` | magic-number | `Palette::rgba:255` |
| `crates/qs-render/src/mesh.rs:129` | magic-number | `Palette::filled_colour:255` |
| `crates/qs-render/src/mesh.rs:324` | magic-number | `Placement::matrix:3` |
| `crates/qs-render/src/mesh.rs:616` | magic-number | `Polygon::emit:3` |
| `crates/qs-render/src/mesh.rs:633` | magic-number | `Polygon::emit:0.5` |
| `crates/qs-render/src/mesh.rs:681` | magic-number | `dot:3` |
| `crates/qs-render/src/mesh.rs:771` | magic-number | `Changes::patches:4` |
| `crates/qs-render/src/mesh.rs:818` | magic-number | `Tree::new:3` |
| `crates/qs-render/src/mesh.rs:968` | magic-number | `Lightmaps::place:3` |
| `crates/qs-render/src/mesh.rs:972` | magic-number | `Lightmaps::place:4` |
| `crates/qs-render/src/mesh.rs:997` | magic-number | `Lightmaps::image_with:4` |
| `crates/qs-render/src/mesh.rs:999` | magic-number | `Lightmaps::image_with:255` |
| `crates/qs-render/src/mesh.rs:1039` | magic-number | `PlacedLight::blit:4` |
| `crates/qs-render/src/mesh.rs:1041` | magic-number | `PlacedLight::blit:255` |
| `crates/qs-render/src/mesh.rs:1089` | magic-number | `PlacedLight::reaches:3` |
| `crates/qs-render/src/mesh.rs:1090` | magic-number | `PlacedLight::reaches:0.5` |
| `crates/qs-render/src/mesh.rs:1104` | magic-number | `PlacedLight::add_dynamic_light:3` |
| `crates/qs-render/src/mesh.rs:1212` | magic-number | `Colour::texel:255` |
| `crates/qs-render/src/mesh.rs:1212` | magic-number | `Colour::texel:256` |
| `crates/qs-render/src/mesh.rs:1260` | magic-number | `face_light:3` |
| `crates/qs-render/src/mesh.rs:1307` | magic-number | `fullbright_split:255` |
| `crates/qs-render/src/mesh.rs:1308` | magic-number | `fullbright_split:4` |
| `crates/qs-render/src/mesh.rs:1335` | magic-number | `sky_layers:128` |
| `crates/qs-render/src/mesh.rs:1335` | magic-number | `sky_layers:256` |
| `crates/qs-render/src/mesh.rs:1343` | magic-number | `sky_layers:4` |
| `crates/qs-render/src/mesh.rs:1344` | magic-number | `sky_layers:3` |
| `crates/qs-render/src/mesh.rs:1379` | magic-number | `frame:100` |
| `crates/qs-render/src/mesh.rs:1388` | magic-number | `frame:10.0` |
| `crates/qs-render/src/mesh.rs:1405` | free-fn-on-type | `mip_levels` |
| `crates/qs-render/src/mesh.rs:1417` | magic-number | `mip_levels:4` |
| `crates/qs-render/src/mesh.rs:1435` | free-fn-on-type | `resample` |
| `crates/qs-render/src/mesh.rs:1440` | magic-number | `resample:16` |
| `crates/qs-render/src/mesh.rs:1444` | magic-number | `resample:4` |
| `crates/qs-render/src/mesh.rs:1460` | magic-number | `resample:255` |
| `crates/qs-render/src/mesh.rs:1471` | magic-number | `mip_map:4` |
| `crates/qs-render/src/mesh.rs:1495` | magic-number | `mip_map:8` |
| `crates/qs-render/src/offscreen.rs:290` | magic-number | `Offscreen::read_back:4` |
| `crates/qs-render/src/offscreen.rs:347` | magic-number | `to_rgba:4` |
| `crates/qs-render/src/particles.rs:52` | magic-number | `particle_texture:4` |
| `crates/qs-render/src/particles.rs:60` | magic-number | `particle_texture:255` |
| `crates/qs-render/src/particles.rs:64` | magic-number | `particle_texture:8` |
| `crates/qs-render/src/particles.rs:80` | magic-number | `particle_triangles:3` |
| `crates/qs-render/src/particles.rs:93` | magic-number | `particle_triangles:255.0` |
| `crates/qs-render/src/picture.rs:96` | magic-number | `read_pcx:3` |
| `crates/qs-render/src/picture.rs:96` | magic-number | `read_pcx:65` |
| `crates/qs-render/src/picture.rs:97` | magic-number | `read_pcx:5` |
| `crates/qs-render/src/picture.rs:97` | magic-number | `read_pcx:8` |
| `crates/qs-render/src/picture.rs:100` | magic-number | `read_pcx:4` |
| `crates/qs-render/src/picture.rs:101` | magic-number | `read_pcx:10` |
| `crates/qs-render/src/picture.rs:101` | magic-number | `read_pcx:6` |
| `crates/qs-render/src/picture.rs:102` | magic-number | `read_pcx:66` |
| `crates/qs-render/src/picture.rs:126` | magic-number | `read_pcx:255` |
| `crates/qs-render/src/play.rs:202` | magic-number | `conchars_image:128` |
| `crates/qs-render/src/play.rs:209` | magic-number | `conchars_image:16` |
| `crates/qs-render/src/play.rs:212` | magic-number | `conchars_image:8` |
| `crates/qs-render/src/play.rs:331` | magic-number | `Play::new:480` |
| `crates/qs-render/src/play.rs:331` | magic-number | `Play::new:640` |
| `crates/qs-render/src/play.rs:449` | bare-id | `Play::add_instance(entity)` |
| `crates/qs-render/src/play.rs:607` | magic-number | `Play::frame:3` |
| `crates/qs-render/src/play.rs:690` | bool-argument | `Play::canvas` |
| `crates/qs-render/src/play.rs:753` | magic-number | `Play::draw_item:4` |
| `crates/qs-render/src/play.rs:754` | magic-number | `Play::draw_item:15` |
| `crates/qs-render/src/play.rs:800` | magic-number | `Play::draw_item:255.0` |
| `crates/qs-render/src/play.rs:823` | free-fn-on-type | `draw_alias` |
| `crates/qs-render/src/play.rs:891` | bool-argument | `quad` |
| `crates/qs-render/src/play.rs:891` | free-fn-on-type | `quad` |
| `crates/qs-render/src/play.rs:916` | magic-number | `quad:3` |
| `crates/qs-render/src/play.rs:920` | magic-number | `quad:6` |
| `crates/qs-render/src/play.rs:983` | magic-number | `FogState::at:255.0` |
| `crates/qs-render/src/skybox.rs:118` | magic-number | `Skybox::layers:255` |
| `crates/qs-render/src/skybox.rs:127` | free-fn-on-type | `nearest` |
| `crates/qs-render/src/skybox.rs:127` | magic-number | `nearest:4` |
| `crates/qs-render/src/sprite.rs:211` | magic-number | `SpriteModel::draw:3` |
| `crates/qs-render/src/sprite.rs:288` | free-fn-on-type | `image` |

### P6-30: the file formats, the file system and sound

- `crates/qs-checkpoint/src/lib.rs:63` `number: u32`: a checkpoint's number, an ID: a `CheckpointNumber` newtype
- `crates/qs-map/src/document.rs:722` `peek_same_line`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-map/src/rooms.rs:120` `own_container`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-pak/src/lib.rs:429` `read_array`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-wad/src/lib.rs:508` `read_array`: inline it at its one call, or keep it with a doc comment saying what its name adds

| File:line | Rule | Item |
| --- | --- | --- |
| `crates/qs-bsp/src/lib.rs:136` | magic-number | `LumpId::index:3` |
| `crates/qs-bsp/src/lib.rs:137` | magic-number | `LumpId::index:4` |
| `crates/qs-bsp/src/lib.rs:138` | magic-number | `LumpId::index:5` |
| `crates/qs-bsp/src/lib.rs:139` | magic-number | `LumpId::index:6` |
| `crates/qs-bsp/src/lib.rs:140` | magic-number | `LumpId::index:7` |
| `crates/qs-bsp/src/lib.rs:141` | magic-number | `LumpId::index:8` |
| `crates/qs-bsp/src/lib.rs:142` | magic-number | `LumpId::index:9` |
| `crates/qs-bsp/src/lib.rs:143` | magic-number | `LumpId::index:10` |
| `crates/qs-bsp/src/lib.rs:144` | magic-number | `LumpId::index:11` |
| `crates/qs-bsp/src/lib.rs:145` | magic-number | `LumpId::index:12` |
| `crates/qs-bsp/src/lib.rs:146` | magic-number | `LumpId::index:13` |
| `crates/qs-bsp/src/lib.rs:147` | magic-number | `LumpId::index:14` |
| `crates/qs-bsp/src/lib.rs:701` | magic-number | `Face::style_count:255` |
| `crates/qs-bsp/src/lib.rs:852` | magic-number | `vec3_at:4` |
| `crates/qs-bsp/src/lib.rs:852` | magic-number | `vec3_at:8` |
| `crates/qs-bsp/src/lib.rs:857` | magic-number | `short3_at:4` |
| `crates/qs-bsp/src/lib.rs:936` | magic-number | `fitz_clipnode_child:65536` |
| `crates/qs-bsp/src/lib.rs:1155` | magic-number | `Bsp::mod_load_edges:4` |
| `crates/qs-bsp/src/lib.rs:1188` | magic-number | `Bsp::mod_load_textures:4` |
| `crates/qs-bsp/src/lib.rs:1231` | magic-number | `Bsp::mod_load_planes:12` |
| `crates/qs-bsp/src/lib.rs:1232` | magic-number | `Bsp::mod_load_planes:16` |
| `crates/qs-bsp/src/lib.rs:1244` | magic-number | `Bsp::mod_load_texinfo:4` |
| `crates/qs-bsp/src/lib.rs:1249` | magic-number | `Bsp::mod_load_texinfo:16` |
| `crates/qs-bsp/src/lib.rs:1256` | magic-number | `Bsp::mod_load_texinfo:0.32` |
| `crates/qs-bsp/src/lib.rs:1258` | magic-number | `Bsp::mod_load_texinfo:0.49` |
| `crates/qs-bsp/src/lib.rs:1259` | magic-number | `Bsp::mod_load_texinfo:3` |
| `crates/qs-bsp/src/lib.rs:1260` | magic-number | `Bsp::mod_load_texinfo:0.99` |
| `crates/qs-bsp/src/lib.rs:1265` | magic-number | `Bsp::mod_load_texinfo:32` |
| `crates/qs-bsp/src/lib.rs:1266` | magic-number | `Bsp::mod_load_texinfo:36` |
| `crates/qs-bsp/src/lib.rs:1316` | magic-number | `Bsp::mod_load_faces:4` |
| `crates/qs-bsp/src/lib.rs:1317` | magic-number | `Bsp::mod_load_faces:8` |
| `crates/qs-bsp/src/lib.rs:1318` | magic-number | `Bsp::mod_load_faces:12` |
| `crates/qs-bsp/src/lib.rs:1319` | magic-number | `Bsp::mod_load_faces:16` |
| `crates/qs-bsp/src/lib.rs:1320` | magic-number | `Bsp::mod_load_faces:20` |
| `crates/qs-bsp/src/lib.rs:1328` | magic-number | `Bsp::mod_load_faces:10` |
| `crates/qs-bsp/src/lib.rs:1372` | magic-number | `Bsp::mod_load_faces:16384` |
| `crates/qs-bsp/src/lib.rs:1373` | magic-number | `Bsp::mod_load_faces:8192` |
| `crates/qs-bsp/src/lib.rs:1417` | magic-number | `Bsp::calc_surface_extents:99999.0` |
| `crates/qs-bsp/src/lib.rs:1417` | magic-number | `Bsp::calc_surface_extents:999999.0` |
| `crates/qs-bsp/src/lib.rs:1468` | magic-number | `Bsp::calc_surface_extents:16` |
| `crates/qs-bsp/src/lib.rs:1468` | magic-number | `Bsp::calc_surface_extents:16.0` |
| `crates/qs-bsp/src/lib.rs:1569` | magic-number | `Bsp::mod_load_leafs:14` |
| `crates/qs-bsp/src/lib.rs:1569` | magic-number | `Bsp::mod_load_leafs:20` |
| `crates/qs-bsp/src/lib.rs:1569` | magic-number | `Bsp::mod_load_leafs:8` |
| `crates/qs-bsp/src/lib.rs:1571` | magic-number | `Bsp::mod_load_leafs:32` |
| `crates/qs-bsp/src/lib.rs:1576` | magic-number | `Bsp::mod_load_leafs:4` |
| `crates/qs-bsp/src/lib.rs:1667` | magic-number | `Bsp::mod_load_nodes:4` |
| `crates/qs-bsp/src/lib.rs:1667` | magic-number | `Bsp::mod_load_nodes:6` |
| `crates/qs-bsp/src/lib.rs:1668` | magic-number | `Bsp::mod_load_nodes:8` |
| `crates/qs-bsp/src/lib.rs:1669` | magic-number | `Bsp::mod_load_nodes:14` |
| `crates/qs-bsp/src/lib.rs:1670` | magic-number | `Bsp::mod_load_nodes:20` |
| `crates/qs-bsp/src/lib.rs:1674` | magic-number | `Bsp::mod_load_nodes:12` |
| `crates/qs-bsp/src/lib.rs:1675` | magic-number | `Bsp::mod_load_nodes:18` |
| `crates/qs-bsp/src/lib.rs:1676` | magic-number | `Bsp::mod_load_nodes:24` |
| `crates/qs-bsp/src/lib.rs:1682` | magic-number | `Bsp::mod_load_nodes:36` |
| `crates/qs-bsp/src/lib.rs:1753` | magic-number | `Bsp::mod_load_clipnodes:4` |
| `crates/qs-bsp/src/lib.rs:1753` | magic-number | `Bsp::mod_load_clipnodes:8` |
| `crates/qs-bsp/src/lib.rs:1755` | magic-number | `Bsp::mod_load_clipnodes:6` |
| `crates/qs-bsp/src/lib.rs:1783` | magic-number | `Bsp::mod_load_submodels:12` |
| `crates/qs-bsp/src/lib.rs:1784` | magic-number | `Bsp::mod_load_submodels:36` |
| `crates/qs-bsp/src/lib.rs:1784` | magic-number | `Bsp::mod_load_submodels:40` |
| `crates/qs-bsp/src/lib.rs:1784` | magic-number | `Bsp::mod_load_submodels:44` |
| `crates/qs-bsp/src/lib.rs:1784` | magic-number | `Bsp::mod_load_submodels:48` |
| `crates/qs-bsp/src/lib.rs:1786` | magic-number | `Bsp::mod_load_submodels:56` |
| `crates/qs-bsp/src/lib.rs:1787` | magic-number | `Bsp::mod_load_submodels:60` |
| `crates/qs-bsp/src/lib.rs:1803` | magic-number | `Bsp::mod_load_submodels:3` |
| `crates/qs-bsp/src/lib.rs:1814` | magic-number | `Bsp::mod_load_submodels:24` |
| `crates/qs-bsp/src/lib.rs:1816` | magic-number | `Bsp::mod_load_submodels:52` |
| `crates/qs-bsp/src/lib.rs:1845` | magic-number | `Bsp::vis_row_size:3` |
| `crates/qs-bsp/src/lib.rs:1845` | magic-number | `Bsp::vis_row_size:7` |
| `crates/qs-bsp/src/lib.rs:1960` | magic-number | `hull_for_size:3.0` |
| `crates/qs-bsp/src/lib.rs:1962` | magic-number | `hull_for_size:32.0` |
| `crates/qs-bsp/src/lib.rs:1976` | magic-number | `length:3` |
| `crates/qs-bsp/src/lib.rs:2022` | magic-number | `load_texture:15` |
| `crates/qs-bsp/src/lib.rs:2176` | magic-number | `sequence_animations:10` |
| `crates/qs-bsp/src/lib.rs:2276` | magic-number | `fitz_sky:3` |
| `crates/qs-bsp/src/lib.rs:2323` | magic-number | `lightmap_size:4` |
| `crates/qs-bsp/src/lib.rs:2349` | magic-number | `replace_entities:4` |
| `crates/qs-checkpoint/src/lib.rs:288` | magic-number | `Store::write_object:6` |
| `crates/qs-checkpoint/src/lib.rs:346` | magic-number | `Store::read_tree:20` |
| `crates/qs-checkpoint/src/lib.rs:362` | magic-number | `Store::read_tree:21` |
| `crates/qs-checkpoint/src/lib.rs:455` | magic-number | `parse_hex:20` |
| `crates/qs-checkpoint/src/lib.rs:457` | magic-number | `parse_hex:40` |
| `crates/qs-checkpoint/src/lib.rs:462` | magic-number | `parse_hex:16` |
| `crates/qs-layout/src/lib.rs:124` | magic-number | `Layout::default:240.0` |
| `crates/qs-layout/src/lib.rs:128` | magic-number | `Layout::default:320.0` |
| `crates/qs-layout/src/lib.rs:132` | magic-number | `Layout::default:230.0` |
| `crates/qs-map/src/document.rs:174` | magic-number | `Face::set:3` |
| `crates/qs-map/src/document.rs:241` | magic-number | `Face::set:9` |
| `crates/qs-map/src/document.rs:691` | bare-id | `BrushModel.entity` |
| `crates/qs-map/src/document.rs:739` | magic-number | `number:1e15` |
| `crates/qs-map/src/document.rs:846` | magic-number | `Parser::face:3` |
| `crates/qs-map/src/document.rs:866` | magic-number | `Parser::face:4` |
| `crates/qs-map/src/geometry.rs:51` | magic-number | `f32::rotation:180.0` |
| `crates/qs-map/src/geometry.rs:51` | magic-number | `f64::rotation:180.0` |
| `crates/qs-map/src/geometry.rs:219` | magic-number | `sin_cos:90.0` |
| `crates/qs-map/src/geometry.rs:221` | magic-number | `sin_cos:180.0` |
| `crates/qs-map/src/geometry.rs:223` | magic-number | `sin_cos:270.0` |
| `crates/qs-map/src/geometry.rs:237` | magic-number | `texvecs:6` |
| `crates/qs-map/src/geometry.rs:259` | magic-number | `texvecs:4` |
| `crates/qs-map/src/geometry.rs:277` | magic-number | `valve_texvecs:3` |
| `crates/qs-mdl/src/mdl.rs:446` | magic-number | `TriVertX::read:3` |
| `crates/qs-mdl/src/mdl.rs:466` | bare-id | `MdlFrame::vertex(index)` |
| `crates/qs-mdl/src/mdl.rs:467` | magic-number | `MdlFrame::vertex:4` |
| `crates/qs-mdl/src/mdl.rs:476` | magic-number | `MdlFrame::vertex_count:4` |
| `crates/qs-mdl/src/mdl.rs:581` | magic-number | `Mdl::read:4` |
| `crates/qs-mdl/src/mdl.rs:588` | magic-number | `Mdl::read:3` |
| `crates/qs-mdl/src/mdl.rs:589` | magic-number | `Mdl::read:5` |
| `crates/qs-mdl/src/mdl.rs:589` | magic-number | `Mdl::read:6` |
| `crates/qs-mdl/src/mdl.rs:589` | magic-number | `Mdl::read:7` |
| `crates/qs-mdl/src/mdl.rs:590` | magic-number | `Mdl::read:8` |
| `crates/qs-mdl/src/mdl.rs:591` | magic-number | `Mdl::read:10` |
| `crates/qs-mdl/src/mdl.rs:591` | magic-number | `Mdl::read:11` |
| `crates/qs-mdl/src/mdl.rs:591` | magic-number | `Mdl::read:9` |
| `crates/qs-mdl/src/mdl.rs:592` | magic-number | `Mdl::read:12` |
| `crates/qs-mdl/src/mdl.rs:593` | magic-number | `Mdl::read:13` |
| `crates/qs-mdl/src/mdl.rs:594` | magic-number | `Mdl::read:14` |
| `crates/qs-mdl/src/mdl.rs:595` | magic-number | `Mdl::read:15` |
| `crates/qs-mdl/src/mdl.rs:596` | magic-number | `Mdl::read:16` |
| `crates/qs-mdl/src/mdl.rs:597` | magic-number | `Mdl::read:17` |
| `crates/qs-mdl/src/mdl.rs:598` | magic-number | `Mdl::read:18` |
| `crates/qs-mdl/src/mdl.rs:599` | magic-number | `Mdl::read:19` |
| `crates/qs-mdl/src/mdl.rs:600` | magic-number | `Mdl::read:20` |
| `crates/qs-mdl/src/mdl.rs:709` | magic-number | `Mdl::bounds:3` |
| `crates/qs-mdl/src/mdl.rs:710` | magic-number | `Mdl::bounds:16.0` |
| `crates/qs-mdl/src/mdl.rs:716` | magic-number | `Mdl::position:3` |
| `crates/qs-mdl/src/mdl.rs:795` | free-fn-on-type | `mod_load_alias_skin_group` |
| `crates/qs-mdl/src/mdl.rs:815` | free-fn-on-type | `mod_load_alias_frame` |
| `crates/qs-mdl/src/mdl.rs:815` | magic-number | `mod_load_alias_frame:24` |
| `crates/qs-mdl/src/mdl.rs:820` | magic-number | `mod_load_alias_frame:8` |
| `crates/qs-mdl/src/mdl.rs:821` | magic-number | `mod_load_alias_frame:16` |
| `crates/qs-mdl/src/mdl.rs:822` | magic-number | `mod_load_alias_frame:4` |
| `crates/qs-mdl/src/mdl.rs:832` | free-fn-on-type | `mod_load_alias_group` |
| `crates/qs-mdl/src/mdl.rs:847` | magic-number | `mod_load_alias_group:4` |
| `crates/qs-mdl/src/mdl.rs:848` | magic-number | `mod_load_alias_group:8` |
| `crates/qs-pak/src/lib.rs:170` | magic-number | `Header::parse:4` |
| `crates/qs-pak/src/lib.rs:171` | magic-number | `Header::parse:8` |
| `crates/qs-pak/src/lib.rs:327` | magic-number | `Pak::with_directory:4` |
| `crates/qs-paths/src/fonts.rs:26` | free-fn-on-type | `font_dirs` |
| `crates/qs-paths/src/lib.rs:160` | free-fn-on-type | `resolve` |
| `crates/qs-paths/src/lib.rs:207` | free-fn-on-type | `required` |
| `crates/qs-paths/src/lib.rs:236` | free-fn-on-type | `absolute_var` |
| `crates/qs-sound/src/mixer.rs:83` | magic-number | `Ambient::index:3` |
| `crates/qs-sound/src/mixer.rs:137` | magic-number | `Listener::default:3` |
| `crates/qs-sound/src/mixer.rs:336` | magic-number | `Mixer::raw_samples:256.0` |
| `crates/qs-sound/src/mixer.rs:530` | magic-number | `Mixer::start_sound:255.0` |
| `crates/qs-sound/src/mixer.rs:565` | magic-number | `Mixer::start_sound:0.1` |
| `crates/qs-sound/src/mixer.rs:613` | magic-number | `Mixer::local_sound:3` |
| `crates/qs-sound/src/mixer.rs:796` | magic-number | `Mixer::paint_channels:256.0` |
| `crates/qs-sound/src/mixer.rs:863` | magic-number | `Mixer::paint_channels:8` |
| `crates/qs-sound/src/mixer.rs:887` | free-fn-on-type | `paint_channel` |
| `crates/qs-sound/src/mixer.rs:898` | magic-number | `paint_channel:255` |
| `crates/qs-sound/src/mixer.rs:900` | magic-number | `paint_channel:3` |
| `crates/qs-sound/src/mixer.rs:900` | magic-number | `paint_channel:8` |
| `crates/qs-sound/src/mixer.rs:932` | magic-number | `transfer:8` |
| `crates/qs-sound/src/music.rs:95` | bool-argument | `Music::play` |
| `crates/qs-sound/src/music.rs:110` | bool-argument | `Music::set_paused` |
| `crates/qs-sound/src/music.rs:224` | magic-number | `CueSheet::read:4` |
| `crates/qs-sound/src/music.rs:293` | magic-number | `msf:60` |
| `crates/qs-sound/src/sfx.rs:72` | magic-number | `SoundData::resample:256.0` |
| `crates/qs-sound/src/sfx.rs:79` | magic-number | `SoundData::resample:8` |
| `crates/qs-sound/src/sfx.rs:132` | bare-id | `SoundData::sample_8(index)` |
| `crates/qs-sound/src/sfx.rs:141` | bare-id | `SoundData::sample_16(index)` |
| `crates/qs-templates/src/lib.rs:141` | string-error | `Fetch::get` |
| `crates/qs-templates/src/lib.rs:268` | free-fn-on-type | `create` |
| `crates/qs-templates/src/lib.rs:273` | free-fn-on-type | `make` |
| `crates/qs-templates/src/lib.rs:344` | free-fn-on-type | `notice` |
| `crates/qs-vfs/src/lib.rs:765` | magic-number | `com_file_extension:7` |
| `crates/qs-vfs/src/zip.rs:137` | magic-number | `Zip::open:65535` |
| `crates/qs-vfs/src/zip.rs:150` | magic-number | `Zip::open:4` |
| `crates/qs-vfs/src/zip.rs:151` | magic-number | `Zip::open:6` |
| `crates/qs-vfs/src/zip.rs:152` | magic-number | `Zip::open:8` |
| `crates/qs-vfs/src/zip.rs:153` | magic-number | `Zip::open:10` |
| `crates/qs-vfs/src/zip.rs:159` | magic-number | `Zip::open:12` |
| `crates/qs-vfs/src/zip.rs:160` | magic-number | `Zip::open:16` |
| `crates/qs-vfs/src/zip.rs:182` | magic-number | `Zip::open:20` |
| `crates/qs-vfs/src/zip.rs:183` | magic-number | `Zip::open:24` |
| `crates/qs-vfs/src/zip.rs:184` | magic-number | `Zip::open:28` |
| `crates/qs-vfs/src/zip.rs:185` | magic-number | `Zip::open:30` |
| `crates/qs-vfs/src/zip.rs:186` | magic-number | `Zip::open:32` |
| `crates/qs-vfs/src/zip.rs:187` | magic-number | `Zip::open:42` |
| `crates/qs-vfs/src/zip.rs:188` | magic-number | `Zip::open:46` |
| `crates/qs-vfs/src/zip.rs:245` | magic-number | `Zip::read:26` |
| `crates/qs-vfs/src/zip.rs:246` | magic-number | `Zip::read:28` |
| `crates/qs-vfs/src/zip.rs:280` | magic-number | `crc32:3988292384` |
| `crates/qs-vfs/src/zip.rs:280` | magic-number | `crc32:8` |
| `crates/qs-vfs/src/zip.rs:301` | magic-number | `u32_at:4` |
| `crates/qs-vfs/src/zip/write.rs:83` | magic-number | `write_zip:101010256` |
| `crates/qs-vfs/src/zip/write.rs:83` | magic-number | `write_zip:33639248` |
| `crates/qs-vfs/src/zip/write.rs:83` | magic-number | `write_zip:67324752` |
| `crates/qs-vfs/src/zip/write.rs:95` | magic-number | `write_zip:8` |
| `crates/qs-wad/src/lib.rs:330` | bare-id | `Wad::lump(index)` |
| `crates/qs-wad/src/lib.rs:380` | magic-number | `QPic::parse:8` |
| `crates/qs-wad/src/lib.rs:386` | magic-number | `QPic::parse:4` |
| `crates/qs-wad/src/lib.rs:437` | magic-number | `Conchars::glyph:64` |
| `crates/qs-wad/src/lib.rs:438` | magic-number | `Conchars::glyph:4` |
| `crates/qs-wad/src/lib.rs:439` | magic-number | `Conchars::glyph:15` |
| `crates/qs-wad/src/lib.rs:441` | magic-number | `Conchars::glyph:8` |
| `crates/qs-wad/src/lib.rs:489` | magic-number | `MipTexHeader::parse:4` |
| `crates/qs-wad/src/lib.rs:497` | magic-number | `MipTexHeader::parse:3` |
| `crates/qs-wad/src/lib.rs:497` | magic-number | `MipTexHeader::parse:5` |

### P6-31: the desktop app and the editor view

- `apps/desktop/src/map_scene.rs:65` `yaw: f32`: a `Degrees` (or `Seconds`) newtype: an editor-side angle or time with no C declaration to follow
- `apps/desktop/src/entities.rs:129` `format_live`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `apps/desktop/src/shell.rs:3577` `preview_config`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `apps/desktop/src/snapshots.rs:151` `tasks_fixture`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-editor-view/src/view.rs:1177` `digit_count`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-editor-view/src/vim.rs:681` `is_blank_at`: inline it at its one call, or keep it with a doc comment saying what its name adds

| File:line | Rule | Item |
| --- | --- | --- |
| `apps/desktop/src/agent.rs:456` | bare-id | `AgentPanel::permit(index)` |
| `apps/desktop/src/agent.rs:469` | bare-id | `AgentPanel::asking(index)` |
| `apps/desktop/src/agent.rs:518` | bare-id | `AgentPanel::report(index)` |
| `apps/desktop/src/agent.rs:694` | magic-number | `AgentPanel::ui:24` |
| `apps/desktop/src/agent.rs:879` | bool-argument | `field` |
| `apps/desktop/src/agent.rs:1014` | magic-number | `queue_box:200.0` |
| `apps/desktop/src/agent.rs:1053` | magic-number | `thread_view:320.0` |
| `apps/desktop/src/agent.rs:1121` | magic-number | `edit_messages:48` |
| `apps/desktop/src/agent.rs:1167` | magic-number | `changes:8` |
| `apps/desktop/src/debugger.rs:93` | magic-number | `debugger_ui:3` |
| `apps/desktop/src/editor.rs:518` | magic-number | `Editor::popup_ui:3` |
| `apps/desktop/src/finder.rs:170` | magic-number | `FileFinder::list_ui:200.0` |
| `apps/desktop/src/git_panel.rs:157` | magic-number | `git_ui:160.0` |
| `apps/desktop/src/git_panel.rs:305` | magic-number | `hunk_ui:3` |
| `apps/desktop/src/main.rs:63` | magic-number | `main:1440.0` |
| `apps/desktop/src/main.rs:63` | magic-number | `main:900.0` |
| `apps/desktop/src/main.rs:64` | magic-number | `main:600.0` |
| `apps/desktop/src/main.rs:64` | magic-number | `main:960.0` |
| `apps/desktop/src/main.rs:66` | magic-number | `main:32` |
| `apps/desktop/src/multibuffer.rs:192` | bool-argument | `Multibuffer::show` |
| `apps/desktop/src/multibuffer.rs:239` | bare-id | `Multibuffer::edited(index)` |
| `apps/desktop/src/multibuffer.rs:343` | magic-number | `form_ui:280.0` |
| `apps/desktop/src/palette.rs:191` | magic-number | `Palette::list_ui:200.0` |
| `apps/desktop/src/palette.rs:330` | bool-argument | `row` |
| `apps/desktop/src/palette.rs:367` | magic-number | `row:48.0` |
| `apps/desktop/src/palette.rs:404` | bool-argument | `task_row` |
| `apps/desktop/src/palette.rs:487` | bool-argument | `field_ui` |
| `apps/desktop/src/palette.rs:532` | magic-number | `field_ui:32.0` |
| `apps/desktop/src/picker.rs:157` | magic-number | `Picker::list_ui:40.0` |
| `apps/desktop/src/project.rs:226` | free-fn-on-type | `problem_label` |
| `apps/desktop/src/shell.rs:326` | magic-number | `Shell::new:3` |
| `apps/desktop/src/shell.rs:550` | bare-id | `Shell::agent_report(index)` |
| `apps/desktop/src/shell.rs:1502` | magic-number | `Shell::resize:0.5` |
| `apps/desktop/src/shell.rs:1512` | bool-argument | `Shell::save_if_changed` |
| `apps/desktop/src/shell.rs:2232` | magic-number | `Shell::status_bar:8` |
| `apps/desktop/src/shell.rs:2270` | bool-argument | `Shell::panel` |
| `apps/desktop/src/shell.rs:2350` | magic-number | `Shell::panel:3` |
| `apps/desktop/src/shell.rs:2466` | bool-argument | `Shell::show_places` |
| `apps/desktop/src/shell.rs:2823` | bool-argument | `Shell::centre` |
| `apps/desktop/src/shell.rs:2839` | magic-number | `Shell::centre:8` |
| `apps/desktop/src/shell.rs:2979` | bool-argument | `centre_tabs` |
| `apps/desktop/src/shell.rs:3010` | magic-number | `breadcrumb:4` |
| `apps/desktop/src/shell.rs:3053` | bool-argument | `file_tab` |
| `apps/desktop/src/shell.rs:3102` | bool-argument | `dragged_size` |
| `apps/desktop/src/shell.rs:3104` | magic-number | `dragged_size:0.5` |
| `apps/desktop/src/shell.rs:3128` | bool-argument | `right` |
| `apps/desktop/src/shell.rs:3151` | magic-number | `right:3` |
| `apps/desktop/src/shell.rs:3190` | bool-argument | `left` |
| `apps/desktop/src/shell.rs:3226` | magic-number | `left:3` |
| `apps/desktop/src/shell.rs:3301` | free-fn-on-type | `view_style` |
| `apps/desktop/src/shell.rs:3348` | bool-argument | `bottom` |
| `apps/desktop/src/shell.rs:3586` | magic-number | `status_dot:4.0` |
| `apps/desktop/src/shell.rs:3602` | bool-argument | `commanded_size` |
| `apps/desktop/src/shell.rs:3603` | magic-number | `commanded_size:0.5` |
| `apps/desktop/src/shell.rs:3606` | free-fn-on-type | `labelled` |
| `apps/desktop/src/shell.rs:3614` | free-fn-on-type | `count` |
| `apps/desktop/src/strings.rs:1033` | bool-argument | `search_count` |
| `apps/desktop/src/theme.rs:450` | free-fn-on-type | `parse_color` |
| `apps/desktop/src/theme.rs:457` | magic-number | `parse_color:6` |
| `apps/desktop/src/theme.rs:459` | magic-number | `parse_color:16` |
| `crates/qs-editor-view/src/layout.rs:237` | bare-id | `LayoutCache::begin(revision)` |
| `crates/qs-editor-view/src/selection.rs:304` | bool-argument | `EditState::add_caret` |
| `crates/qs-editor-view/src/selection.rs:502` | free-fn-on-type | `shifted` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:105` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:108` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:122` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:125` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:127` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:135` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:139` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:142` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:176` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:199` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:204` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:210` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:217` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:226` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:236` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:238` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:24` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:242` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:25` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:26` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:31` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:32` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:33` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:34` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:36` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:38` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:40` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:42` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:46` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:49` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:51` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:54` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:58` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:64` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:69` |
| `crates/qs-editor-view/src/view.rs:72` | magic-number | `ViewStyle::default:74` |
| `crates/qs-editor-view/src/view.rs:74` | magic-number | `ViewStyle::default:13.0` |
| `crates/qs-editor-view/src/view.rs:955` | bool-argument | `EditorView::paint` |
| `crates/qs-editor-view/src/view.rs:1097` | magic-number | `EditorView::paint:4` |
| `crates/qs-editor-view/src/view.rs:1228` | magic-number | `Marks::paint:3.0` |
| `crates/qs-editor-view/src/view.rs:1351` | magic-number | `paint_note:4.0` |
| `crates/qs-editor-view/src/view.rs:1500` | bool-argument | `fold_mark` |
| `crates/qs-editor-view/src/view.rs:1510` | magic-number | `fold_mark:4.0` |
| `crates/qs-editor-view/src/vim.rs:208` | magic-number | `Vim::command_key:10` |
| `crates/qs-editor-view/src/vim.rs:510` | bool-argument | `Vim::put` |
| `crates/qs-editor-view/src/vim.rs:560` | free-fn-on-type | `order` |
| `crates/qs-editor-view/src/vim.rs:565` | free-fn-on-type | `after_char` |
| `crates/qs-editor-view/src/vim.rs:575` | free-fn-on-type | `line_span` |
| `crates/qs-editor-view/src/vim.rs:604` | free-fn-on-type | `last_char` |
| `crates/qs-editor-view/src/vim.rs:614` | free-fn-on-type | `normal_clamp` |
| `crates/qs-editor-view/src/vim.rs:625` | free-fn-on-type | `step_left` |
| `crates/qs-editor-view/src/vim.rs:637` | bool-argument | `step_right` |
| `crates/qs-editor-view/src/vim.rs:637` | free-fn-on-type | `step_right` |
| `crates/qs-editor-view/src/vim.rs:654` | free-fn-on-type | `vertical` |
| `crates/qs-editor-view/src/vim.rs:673` | free-fn-on-type | `char_at` |
| `crates/qs-editor-view/src/vim.rs:681` | free-fn-on-type | `is_blank_at` |
| `crates/qs-editor-view/src/vim.rs:686` | free-fn-on-type | `forward` |
| `crates/qs-editor-view/src/vim.rs:697` | free-fn-on-type | `backward` |
| `crates/qs-editor-view/src/vim.rs:710` | free-fn-on-type | `class_at` |
| `crates/qs-editor-view/src/vim.rs:712` | magic-number | `class_at:3` |
| `crates/qs-editor-view/src/vim.rs:718` | free-fn-on-type | `word_forward` |
| `crates/qs-editor-view/src/vim.rs:733` | magic-number | `word_forward:3` |
| `crates/qs-editor-view/src/vim.rs:744` | free-fn-on-type | `word_backward` |
| `crates/qs-editor-view/src/vim.rs:753` | magic-number | `word_backward:3` |
| `crates/qs-editor-view/src/vim.rs:771` | free-fn-on-type | `word_end` |
| `crates/qs-editor-view/src/vim.rs:780` | magic-number | `word_end:3` |

### P6-32: the preview host, the engine bridge and its protocol

| File:line | Rule | Item |
| --- | --- | --- |
| `apps/quakec-engine/src/audio.rs:158` | magic-number | `CdTrackStream::read:3` |
| `apps/quakec-engine/src/audio.rs:453` | magic-number | `device:32768.0` |
| `apps/quakec-engine/src/audio.rs:485` | free-fn-on-type | `fill` |
| `apps/quakec-engine/src/bridge.rs:210` | free-fn-on-type | `serve` |
| `apps/quakec-engine/src/game.rs:1056` | magic-number | `Game::pick:0.5` |
| `apps/quakec-engine/src/game.rs:1130` | magic-number | `vector_param:3` |
| `apps/quakec-engine/src/game/debug.rs:20` | bare-id | `Breakpoint.id` |
| `apps/quakec-engine/src/headless.rs:40` | free-fn-on-type | `run` |
| `apps/quakec-engine/src/headless.rs:108` | free-fn-on-type | `timerefresh` |
| `apps/quakec-engine/src/headless.rs:137` | magic-number | `timerefresh:360.0` |
| `apps/quakec-engine/src/headless.rs:176` | free-fn-on-type | `view_script` |
| `apps/quakec-engine/src/headless.rs:268` | bool-argument | `Overlay::hud` |
| `apps/quakec-engine/src/headless.rs:306` | free-fn-on-type | `ppm` |
| `apps/quakec-engine/src/headless.rs:308` | magic-number | `ppm:4` |
| `apps/quakec-engine/src/keys.rs:135` | magic-number | `menu_bound:12` |
| `apps/quakec-engine/src/keys.rs:227` | bool-argument | `host_files` |
| `apps/quakec-engine/src/keys.rs:246` | bool-argument | `configuration` |
| `apps/quakec-engine/src/load.rs:135` | magic-number | `Rand::next:1103515245` |
| `apps/quakec-engine/src/load.rs:135` | magic-number | `Rand::next:2147483647` |
| `apps/quakec-engine/src/load.rs:136` | magic-number | `Rand::next:12345` |
| `apps/quakec-engine/src/load.rs:161` | free-fn-on-type | `load` |
| `apps/quakec-engine/src/load.rs:392` | magic-number | `load:30.0` |
| `apps/quakec-engine/src/load.rs:433` | magic-number | `facing:3` |
| `apps/quakec-engine/src/load.rs:448` | magic-number | `facing:160.0` |
| `apps/quakec-engine/src/load.rs:448` | magic-number | `facing:55.0` |
| `apps/quakec-engine/src/load.rs:449` | magic-number | `facing:96.0` |
| `apps/quakec-engine/src/load.rs:523` | free-fn-on-type | `game_dir` |
| `apps/quakec-engine/src/load.rs:530` | free-fn-on-type | `saves` |
| `apps/quakec-engine/src/load.rs:536` | free-fn-on-type | `save_path` |
| `apps/quakec-engine/src/load.rs:551` | free-fn-on-type | `act` |
| `apps/quakec-engine/src/main.rs:54` | free-fn-on-type | `run` |
| `apps/quakec-engine/src/screenshot.rs:112` | free-fn-on-type | `encode` |
| `apps/quakec-engine/src/screenshot.rs:131` | magic-number | `base64:3` |
| `apps/quakec-engine/src/screenshot.rs:131` | magic-number | `base64:4` |
| `apps/quakec-engine/src/screenshot.rs:134` | magic-number | `base64:63` |
| `apps/quakec-engine/src/screenshot.rs:139` | magic-number | `base64:16` |
| `apps/quakec-engine/src/screenshot.rs:139` | magic-number | `base64:8` |
| `apps/quakec-engine/src/screenshot.rs:140` | magic-number | `base64:18` |
| `apps/quakec-engine/src/screenshot.rs:141` | magic-number | `base64:12` |
| `apps/quakec-engine/src/screenshot.rs:143` | magic-number | `base64:6` |
| `apps/quakec-engine/src/values.rs:43` | magic-number | `from_json:3` |
| `apps/quakec-engine/src/window.rs:61` | free-fn-on-type | `run` |
| `apps/quakec-engine/src/window.rs:102` | magic-number | `run:1280.0` |
| `apps/quakec-engine/src/window.rs:102` | magic-number | `run:800.0` |
| `apps/quakec-engine/src/window.rs:116` | magic-number | `run:32` |
| `apps/quakec-engine/src/window.rs:120` | magic-number | `run:3` |
| `apps/quakec-engine/src/window.rs:281` | magic-number | `Viewer::ui:0.1` |
| `apps/quakec-engine/src/window.rs:348` | magic-number | `Viewer::ui:30.0` |
| `apps/quakec-engine/src/window.rs:531` | magic-number | `Viewer::change_level:3` |
| `apps/quakec-engine/src/window.rs:591` | bool-argument | `Viewer::hold_pointer` |
| `apps/quakec-engine/src/window.rs:1103` | magic-number | `screen_height:0.1` |
| `apps/quakec-engine/src/window.rs:1103` | magic-number | `screen_height:640` |
| `crates/qs-bridge/src/message.rs:71` | magic-number | `ErrorCode::code:32700` |
| `crates/qs-bridge/src/message.rs:72` | magic-number | `ErrorCode::code:32600` |
| `crates/qs-bridge/src/message.rs:73` | magic-number | `ErrorCode::code:32601` |
| `crates/qs-bridge/src/message.rs:74` | magic-number | `ErrorCode::code:32602` |
| `crates/qs-bridge/src/message.rs:75` | magic-number | `ErrorCode::code:32603` |
| `crates/qs-bridge/src/message.rs:76` | magic-number | `ErrorCode::code:32001` |
| `crates/qs-bridge/src/message.rs:77` | magic-number | `ErrorCode::code:32002` |
| `crates/qs-bridge/src/message.rs:78` | magic-number | `ErrorCode::code:32003` |
| `crates/qs-bridge/src/message.rs:79` | magic-number | `ErrorCode::code:32004` |
| `crates/qs-bridge/src/message.rs:80` | magic-number | `ErrorCode::code:32005` |
| `crates/qs-bridge/src/message.rs:81` | magic-number | `ErrorCode::code:32006` |
| `crates/qs-bridge/src/message.rs:82` | magic-number | `ErrorCode::code:32007` |
| `crates/qs-bridge/src/message.rs:83` | magic-number | `ErrorCode::code:32008` |
| `crates/qs-engine/src/client.rs:442` | bare-id | `Client::set_field(edict)` |
| `crates/qs-engine/src/client.rs:578` | bare-id | `Client::clear_breakpoint(id)` |
| `crates/qs-engine/src/client.rs:656` | magic-number | `base64_decode:3` |
| `crates/qs-engine/src/client.rs:660` | magic-number | `base64_decode:26` |
| `crates/qs-engine/src/client.rs:661` | magic-number | `base64_decode:52` |
| `crates/qs-engine/src/client.rs:662` | magic-number | `base64_decode:62` |
| `crates/qs-engine/src/client.rs:663` | magic-number | `base64_decode:63` |
| `crates/qs-engine/src/client.rs:669` | magic-number | `base64_decode:4` |
| `crates/qs-engine/src/client.rs:682` | magic-number | `base64_decode:6` |
| `crates/qs-engine/src/launch.rs:106` | free-fn-on-type | `host_command` |
| `crates/qs-engine/src/launch.rs:129` | free-fn-on-type | `launch` |

### P6-33: the workbench, the agent, models, MCP, commands, git, the CLI and the LSP server

- `crates/qs-agent/src/history.rs:29` `number: u32`: a thread's number, an ID: a newtype
- `crates/qs-workbench/src/checkpoints.rs:34` `number: u32`: the same checkpoint number: the same newtype
- `crates/qs-workbench/src/checkpoints.rs:79` `number: u32`: the same checkpoint number: the same newtype
- `crates/qs-commands/src/id.rs:74` `borrow`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-workbench/src/staged.rs:125` `is_decided`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-workbench/src/vm_run.rs:330` `num_edicts`: inline it at its one call, or keep it with a doc comment saying what its name adds
- `crates/qs-agent/src/lib.rs:1295` and `crates/qs-mcp/src/lib.rs:454`, the same body: one function both call, in the lower of the two crates or a shared module
- `crates/qs-agent/src/lib.rs:1319` and `crates/qs-mcp/src/lib.rs:478`, the same body: one function both call, in the lower of the two crates or a shared module
- `crates/qs-agent/src/lib.rs:1341` and `crates/qs-mcp/src/lib.rs:501`, the same body: one function both call, in the lower of the two crates or a shared module
- `crates/qs-workbench/src/commands.rs:816` and `crates/qs-workbench/src/map_check.rs:223`, the same body: one function both call, in the lower of the two crates or a shared module

| File:line | Rule | Item |
| --- | --- | --- |
| `apps/cli/src/doctor.rs:55` | free-fn-on-type | `run` |
| `apps/cli/src/doctor.rs:79` | free-fn-on-type | `name` |
| `apps/cli/src/doctor.rs:88` | bool-argument | `games` |
| `apps/cli/src/lib.rs:626` | free-fn-on-type | `serve_mcp` |
| `apps/lsp/src/server.rs:52` | magic-number | `ErrorCode::code:32700` |
| `apps/lsp/src/server.rs:53` | magic-number | `ErrorCode::code:32600` |
| `apps/lsp/src/server.rs:54` | magic-number | `ErrorCode::code:32601` |
| `apps/lsp/src/server.rs:55` | magic-number | `ErrorCode::code:32602` |
| `apps/lsp/src/server.rs:56` | magic-number | `ErrorCode::code:32002` |
| `apps/lsp/src/server.rs:57` | magic-number | `ErrorCode::code:32803` |
| `apps/lsp/src/server.rs:748` | magic-number | `Server::semantic_tokens:5` |
| `apps/lsp/src/server.rs:768` | magic-number | `Server::semantic_tokens:31` |
| `apps/lsp/src/server.rs:917` | magic-number | `completion_kind:3` |
| `apps/lsp/src/server.rs:918` | magic-number | `completion_kind:5` |
| `apps/lsp/src/server.rs:919` | magic-number | `completion_kind:6` |
| `apps/lsp/src/server.rs:929` | magic-number | `token_type:3` |
| `apps/lsp/src/uri.rs:66` | magic-number | `to_path_on:3` |
| `apps/lsp/src/uri.rs:141` | magic-number | `push_encoded:15` |
| `apps/lsp/src/uri.rs:148` | magic-number | `push_encoded:4` |
| `apps/lsp/src/uri.rs:149` | magic-number | `push_encoded:16` |
| `apps/lsp/src/uri.rs:168` | magic-number | `percent_decode:16` |
| `apps/lsp/src/uri.rs:174` | magic-number | `percent_decode:3` |
| `crates/qs-agent/src/lib.rs:80` | bare-id | `EntityState.edict` |
| `crates/qs-agent/src/lib.rs:127` | free-fn-on-type | `ask_about` |
| `crates/qs-agent/src/lib.rs:277` | free-fn-on-type | `inline_answer` |
| `crates/qs-agent/src/lib.rs:401` | magic-number | `base64:63` |
| `crates/qs-agent/src/lib.rs:404` | magic-number | `base64:3` |
| `crates/qs-agent/src/lib.rs:404` | magic-number | `base64:4` |
| `crates/qs-agent/src/lib.rs:407` | magic-number | `base64:16` |
| `crates/qs-agent/src/lib.rs:407` | magic-number | `base64:8` |
| `crates/qs-agent/src/lib.rs:408` | magic-number | `base64:18` |
| `crates/qs-agent/src/lib.rs:409` | magic-number | `base64:12` |
| `crates/qs-agent/src/lib.rs:411` | magic-number | `base64:6` |
| `crates/qs-agent/src/lib.rs:445` | string-error | `Model::reply` |
| `crates/qs-agent/src/lib.rs:459` | string-error | `Model::reply_streaming` |
| `crates/qs-agent/src/lib.rs:511` | magic-number | `RunConfig::new:20000` |
| `crates/qs-agent/src/lib.rs:511` | magic-number | `RunConfig::new:200000` |
| `crates/qs-agent/src/lib.rs:514` | magic-number | `RunConfig::new:40` |
| `crates/qs-agent/src/lib.rs:776` | magic-number | `Run::needs_compacting:100` |
| `crates/qs-agent/src/lib.rs:1168` | free-fn-on-type | `refusal` |
| `crates/qs-agent/src/permissions.rs:198` | free-fn-on-type | `subjects` |
| `crates/qs-commands/src/remote.rs:189` | free-fn-on-type | `run` |
| `crates/qs-commands/src/remote.rs:324` | free-fn-on-type | `unexpected` |
| `crates/qs-git/src/conflict.rs:71` | bare-id | `resolve(index)` |
| `crates/qs-git/src/diff.rs:61` | bare-id | `FileDiff::patch(index)` |
| `crates/qs-git/src/lib.rs:286` | magic-number | `Repo::status:3` |
| `crates/qs-git/src/lib.rs:333` | bare-id | `Repo::stage_hunk(index)` |
| `crates/qs-git/src/lib.rs:345` | bare-id | `Repo::unstage_hunk(index)` |
| `crates/qs-git/src/lib.rs:463` | magic-number | `Repo::log:4` |
| `crates/qs-mcp/src/lib.rs:145` | string-error | `Commands::run` |
| `crates/qs-mcp/src/lib.rs:318` | magic-number | `Server::serve_bytes:16` |
| `crates/qs-mcp/src/lib.rs:344` | string-error | `Server::check_roots` |
| `crates/qs-models/src/anthropic.rs:31` | free-fn-on-type | `request` |
| `crates/qs-models/src/anthropic.rs:79` | free-fn-on-type | `effort` |
| `crates/qs-models/src/anthropic.rs:161` | string-error | `reply` |
| `crates/qs-models/src/http.rs:130` | magic-number | `Http::send:300` |
| `crates/qs-models/src/http.rs:147` | magic-number | `Http::send:200` |
| `crates/qs-models/src/lib.rs:126` | magic-number | `default_max_tokens:16000` |
| `crates/qs-models/src/lib.rs:233` | magic-number | `Usage::cost:1000000.0` |
| `crates/qs-models/src/lib.rs:322` | string-error | `ProviderModel::take` |
| `crates/qs-models/src/openai.rs:19` | free-fn-on-type | `request` |
| `crates/qs-models/src/openai.rs:110` | string-error | `reply` |
| `crates/qs-models/src/stream.rs:123` | string-error | `AnthropicStream::answer` |
| `crates/qs-workbench/src/build.rs:104` | bool-argument | `gravity` |
| `crates/qs-workbench/src/build.rs:189` | free-fn-on-type | `unloadable_message` |
| `crates/qs-workbench/src/commands.rs:493` | bare-id | `CodeActionParams.index` |
| `crates/qs-workbench/src/commands.rs:816` | bool-argument | `RenameStep::set` |
| `crates/qs-workbench/src/map_check.rs:223` | bool-argument | `RenameKeyStep::set` |
| `crates/qs-workbench/src/preview.rs:361` | bare-id | `Preview::unwatch(index)` |
| `crates/qs-workbench/src/preview.rs:418` | bare-id | `Preview::pending(edict)` |
| `crates/qs-workbench/src/preview.rs:448` | bare-id | `Preview::select(edict)` |
| `crates/qs-workbench/src/preview.rs:721` | magic-number | `Preview::stop:20` |
| `crates/qs-workbench/src/preview.rs:976` | free-fn-on-type | `built_program` |
| `crates/qs-workbench/src/preview.rs:1151` | bare-id | `SelectParams.edict` |
| `crates/qs-workbench/src/preview.rs:1178` | bare-id | `SelectedEntity.edict` |
| `crates/qs-workbench/src/preview.rs:1289` | bare-id | `UnwatchParams.index` |
| `crates/qs-workbench/src/preview/debug.rs:22` | bare-id | `Breakpoint.id` |
| `crates/qs-workbench/src/preview/debug.rs:203` | bare-id | `Debugger::unwatch(index)` |
| `crates/qs-workbench/src/preview/engine.rs:58` | bare-id | `EngineInspectParams.edict` |
| `crates/qs-workbench/src/preview/entities.rs:40` | bare-id | `Selected.edict` |
| `crates/qs-workbench/src/preview/entities.rs:113` | bare-id | `Entities::classname(edict)` |
| `crates/qs-workbench/src/search.rs:382` | bool-argument | `FileStep::set` |
| `crates/qs-workbench/src/terminal.rs:301` | bare-id | `Terminals::get_mut(index)` |
| `crates/qs-workbench/src/tools.rs:176` | bool-argument | `Workbench::search` |
| `crates/qs-workbench/src/vm_run.rs:240` | magic-number | `slots:3` |
| `crates/qs-workbench/src/vm_run.rs:257` | free-fn-on-type | `changes` |
