# Readiness: P2 apps (P2-RA)

- **Issue:** #88 (`P2-RA`)
- **Reviewer:** an independent reviewer agent session (TEST-9); did not write `apps/cli`,
  `apps/lsp` or `apps/desktop`
- **Date:** 2026-11-04
- **Commit measured:** `4144a658599d8298355998123ca722c1063bacb1` (`test/88-apps-readiness`:
  `main` at `cf169599` with this review's property and CLI tests)
- **Verdict:** Not ready. The Readiness workflow cannot measure `quakec-studio` (P2-167), so
  no crate of the layer has a coverage number and the desktop app has no mutation score and
  fails 20 of 20 Linux flake runs for want of a GPU adapter; and the review found five
  defects in the code (P2-161 to P2-164) and two gaps against the bar (P2-165, TEST-5;
  P2-166, TEST-8). Each blocks readiness until it is fixed and re-reviewed (TEST-9).

## 1. Crates and spec sections

| Crate | Directory | Layer | Spec sections (crate README) |
| --- | --- | --- | --- |
| `quakec-ide` | `apps/cli` | apps | none of `docs/spec/`; the build is `qs-workbench`'s |
| `quakec-lsp` | `apps/lsp` | apps | none of `docs/spec/`; the protocol is LSP 3.18, cached in `specs/lsp/3.18/` |
| `quakec-studio` | `apps/desktop` | apps | none; the README's line names plan requirements UI-2, UI-14, UI-15 and CONV-16, which the spec check reads as sections (P2-167) |

The plan issues covered: P2-13 (#69, the CLI, merged in #706), P2-14 (#70, the LSP server) and
P2-126 (#613, the server's crash log). All are closed. No defect against the apps has been
reported from a higher layer (the apps are the top of the ranking), and no issue labelled
`apps` and `type:bug` is open.

## 2. Coverage, mutation score and spec citations

Readiness workflow run:
[41906749573](https://github.com/quakecstudio/editor/actions/runs/41906749573)
(`crates="quakec-ide quakec-lsp quakec-studio"`, on the commit above). It failed: the ratchet
step did not run.

| Crate | Lines (bar 90%) | Branches (bar 80%) | Mutation (bar 90%) | Spec citations (bar 100%) |
| --- | --- | --- | --- | --- |
| `quakec-ide` | not measured (P2-167) | not measured (P2-167) | 90.9% (10 of 11) | 100.0% (0 of 0) |
| `quakec-lsp` | not measured (P2-167) | not measured (P2-167) | 93.4% (155 of 166) | 100.0% (0 of 0) |
| `quakec-studio` | not measured (P2-167) | not measured (P2-167) | not measured: the unmutated baseline fails (P2-167) | not measured: the check stops on "section UI-2 has no active statements" (P2-167) |

The `Readiness coverage` job measures every crate in one `cargo llvm-cov` step, and
`quakec-studio`'s wgpu tests fail on the Linux runner with
`Failed to create render state: CustomNativeAdapterSelectionError("No adapter found")`,
because the Readiness workflow installs no Vulkan driver (pull-request CI installs
`mesa-vulkan-drivers`); that fails the step, so the two other crates have no coverage number
either. A number that was not measured is a failure (the skill and ADR-0009).

`cargo mutants` builds only 11 of `quakec-ide`'s 19 mutants; the other 8 do not build and do
not count (docs/testing.md, "Measuring a layer").

Surviving mutants:

| Crate | Mutant | Outcome |
| --- | --- | --- |
| `quakec-ide` | `apps/cli/src/main.rs: replace main -> ExitCode with Default::default()` | equivalent: as for xtask's `main` in `ratchets.toml`, cargo mutants builds the tests with `cargo test --no-run`, which never runs a binary's `main`, and the one line of logic (`ExitCode::from(code.number())`) is covered by the tests of `run` and `Exit::number` |
| `quakec-lsp` | `apps/lsp/src/main.rs: replace main -> ExitCode with Default::default()` | equivalent, for the same reason |
| `quakec-lsp` | `apps/lsp/src/render.rs: replace Markup::line_break -> &'static str with ""` and `with "xyzzy"` | killed by `a_builtins_hover_lists_the_engines_with_and_without_it` (`apps/lsp/src/tests/review.rs`) |
| `quakec-lsp` | `apps/lsp/src/render.rs: delete ! in engines` (three) and `replace == with != in engines` | killed by `a_builtins_hover_lists_the_engines_with_and_without_it` |
| `quakec-lsp` | `apps/lsp/src/server.rs: delete match arm "textDocument/didSave" in Server<L, W>::notification` | killed by `a_save_reads_the_project_again` |
| `quakec-lsp` | `apps/lsp/src/server.rs: replace == with != in Server<L, W>::initialize` (line 304, the UTF-8 offer) | killed by `the_position_encoding_is_the_one_offered` |
| `quakec-lsp` | `apps/lsp/src/transport.rs: replace > with >= in read_message` | killed by `a_message_of_the_longest_length_is_read` |
| `quakec-lsp` | `apps/lsp/src/server.rs: replace \| with ^ in Server<L, W>::semantic_tokens` | equivalent: the fold sets bit `index` once for each distinct `index` of `Modifier::ALL`, starting from 0, so no bit is set twice and `\|` and `^` give the same number for every input |

The kills were checked with a local `cargo mutants -p quakec-lsp --test-tool nextest` over
those mutants only (section 5). They are not in a Readiness measurement: the next one, after
P2-167, includes them.

## 3. Property, fuzz and differential results

| Crate | Property tests (TEST-4) | Fuzzing (TEST-5) | Differential (TEST-6) |
| --- | --- | --- | --- |
| `quakec-ide` | none needed: it decodes no format of its own (clap reads the command line, serde_json the parameters) | no parser of files or network input of its own | no oracle |
| `quakec-lsp` | none before this review; the review adds six (section 5): the framing round trip, the reader on arbitrary bytes, offsets to positions and back, positions in text order, paths to URIs and back, one answer per request. Two leave out cases that are defects (P2-161) | **gap (P2-165)**: no target reads the client's stream (`transport::read_message`, `Server::handle`); the property tests run 256 cases each | no oracle; the `lsp-corpus` CI job compares every request on the rerelease with `docs/spec/vectors/lsp/rerelease.txt` |
| `quakec-studio` | none listed in the README | reads `layout.toml` through `qs-layout` (L5, fuzzed as `qs_settings`) and system font files through `skrifa`; no target of its own | no oracle |

## 4. Flakes

20 consecutive runs of each crate's tests (TEST-7), from the same workflow run:

| Crate | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `quakec-ide` | 20 of 20 | 20 of 20 | 20 of 20 |
| `quakec-lsp` | 20 of 20 | 20 of 20 | 20 of 20 |
| `quakec-studio` | **0 of 20** (no GPU adapter on the runner, P2-167) | 20 of 20 | 20 of 20 |

No test waits on the wall clock: `rg 'sleep|Instant::now|SystemTime::now' apps` finds only
`apps/desktop/examples/editor_bench.rs`, the benchmark, which is not a test; the editor's
pause before `file.update` is driven by the harness's stepped time in the tests.

Performance (TEST-8): the desktop app's budgets, `open_project_and_type` and `keystroke` in
`docs/perf-budgets.md`, are still `pending` placeholders, so no Bench run checks them; the one
measurement, `docs/spikes/data/ref-mac/ref-mac-editor.md`, meets open (223 ms of 1000 ms) and
typing (p99 1.61 ms of 8.33 ms) on ref-mac only, and records one frame of 85.63 ms when the
project is checked after typing stops (P2-166). The CLI and the server have no budget of their
own; `id1_recompile` (active) covers the compiler they call.

## 5. Independent review (TEST-9)

- **Reviewer and time spent:** an independent agent session, about four hours: the three
  crates' sources and READMEs against LSP 3.18 (`specs/lsp/3.18/`), egui 0.36.2's
  `consume_shortcut`, the CLI's documented contract and PRJ-2.8; a probe of each suspicion
  with a test or the built binary.
- **Tests added** (all pass at the commit above, or on this branch after it):
  - `apps/lsp/src/tests/properties.rs` (new, proptest added as a dev-dependency):
    `written_messages_read_back`, `arbitrary_input_never_panics_the_reader`,
    `offsets_convert_to_positions_and_back`, `positions_follow_the_text`,
    `paths_convert_to_uris_and_back`, `every_request_is_answered_once`. Breaking
    `LineIndex::offset` by hand (`>` to `>=` on its width check) fails
    `offsets_convert_to_positions_and_back`.
  - `apps/lsp/src/tests/review.rs` (new, after the measurement, to kill its survivors):
    `a_builtins_hover_lists_the_engines_with_and_without_it`,
    `the_position_encoding_is_the_one_offered`, `a_save_reads_the_project_again`,
    `a_message_of_the_longest_length_is_read`.
  - `apps/cli/src/tests.rs`: `output_that_cannot_be_written_is_a_failure`,
    `warnings_are_printed_and_counted_but_do_not_fail`,
    `a_project_that_does_not_open_is_a_failure`,
    `run_refuses_parameters_that_are_not_an_object`.
- **Defects found**, each an issue this review now depends on:
  - P2-161 `quakec-lsp`: `uri::to_path` reads `/c:/x.qc` as the Windows path `c:/x.qc`
    and `uri::from_path` turns `\` into `/` on every host, so on Linux and macOS
    `/c:/x.qc` reads back as a relative path and `/p/a\b.qc` names another file; and
    `LineIndex::position` of an offset inside a multi-byte character is past the character
    (in UTF-8 positions, `é` at offset 1 is character 3, at offset 2 character 2). Found by
    the property tests, which leave both out until the fix.
  - P2-162 `quakec-ide`: `std::env::args()` panics on an argument that is not UTF-8;
    `quakec-ide check "$(printf 'a\377')"` exits 101 with
    "called `Result::unwrap()` on an `Err` value: \"a\\xFF\"" (the built binary, macOS).
  - P2-163 `quakec-studio`: Shift+Cmd+Z outside the editor runs `edit.undo`, not
    `edit.redo`: egui's `consume_shortcut` ignores an extra Shift ("you should match most
    specific shortcuts first"), and `keymap::BINDINGS` lists Cmd+Z first. The failing test
    is below.
  - P2-164 `quakec-ide` and `quakec-lsp`: `--dialect` and `initializationOptions` apply
    over a `quakec.toml`'s dialect (and target) for every module, though both READMEs and
    `--help` say they apply to a project with none; `check --dialect fteqcc` on a project
    whose `quakec.toml` names no dialect exits 0 instead of 1. The failing test is below.
  - P2-165 `quakec-lsp` (TEST-5): no fuzz target reads the client's stream, and a
    header line is read with no bound.
  - P2-166 `quakec-studio` (TEST-8): the editor's budgets are not checked in CI, and the
    check after typing stalls one frame for 85.63 ms on ref-mac.
  - P2-167 the Readiness workflow (and `apps/desktop/README.md`): no Vulkan driver on its
    Linux runners, and a `**Spec sections:**` line that names plan requirements.

The two tests that fail on `main` today and pass once their defect is fixed, for the fixes to
commit (a failing test cannot land before them):

```rust
// apps/desktop/src/tests.rs, P2-163
// PRIN-3: each binding's keys run its own command and no other, so Shift+Cmd+Z redoes rather
// than undoing again, outside the editor as in it.
#[test]
fn each_shortcut_runs_only_its_own_command() {
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, None).unwrap();
    let ember = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember");
    app.shell_mut().open_project(&ember).unwrap();
    frame(&ctx, &mut app);
    let defs = ember.join("defs.qc");
    let offset = fs::read_to_string(&defs).unwrap().find("bounces").unwrap();
    app.shell_mut()
        .execute(
            "symbol.rename",
            json!({ "path": defs, "offset": offset, "name": "hops" }),
        )
        .unwrap();
    let depths = |app: &App| {
        let session = app.shell().session();
        (session.undo_depth(), session.redo_depth())
    };
    assert_eq!(depths(&app), (1, 0));
    for (command, after) in [("edit.undo", (0, 1)), ("edit.redo", (1, 0))] {
        let shortcut = keymap::shortcut(command).unwrap();
        frame_with(
            &ctx,
            &mut app,
            vec![key(shortcut.logical_key, shortcut.modifiers)],
        );
        assert_eq!(depths(&app), after, "{command}");
    }
}
```

On `main` at `cf169599` it fails at `edit.redo` with left `(0, 1)`, right `(1, 0)`.

```rust
// apps/cli/src/tests.rs, P2-164
// PRJ-2.8 and the README: `--dialect` reads a project with no `quakec.toml` in that dialect,
// so a project file's dialect, vanilla when it names none, wins over the option.
#[test]
fn the_project_file_dialect_wins_over_the_option() {
    let dir = project(
        "toml-dialect",
        &ProjectSpec {
            world: "void() worldspawn = { float x = 1; x += 2; };\n",
        },
    );
    std::fs::write(
        dir.join("quakec.toml"),
        "[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
    )
    .unwrap();
    assert_eq!(
        cli(&["check", text(&dir), "--dialect", "fteqcc"]).0,
        Exit::Problems
    );
    std::fs::write(
        dir.join("quakec.toml"),
        "[project]\ndialect = \"fteqcc\"\n\n[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
    )
    .unwrap();
    assert_eq!(
        cli(&["check", text(&dir), "--dialect", "vanilla"]).0,
        Exit::Success
    );
}
```

On `main` at `cf169599` its first assertion fails with left `Success`, right `Problems`.

Looked at and found sound: the lifecycle (`initialize` once, `-32002` before it,
`InvalidRequest` after `shutdown`, `exit` codes), the framing's errors, the incremental
`didChange` order, the rename's change annotations (ADR-0019), the CLI's exit statuses and
its refusals of a wrong command line, the editor's own F12 and Shift+F12 order.

## 6. Ratchets

None: the layer did not pass, so no minimums are committed to `ratchets.toml`. The re-review
records `quakec-ide`, `quakec-lsp` and `quakec-studio` with `readiness = "P2-RA"` and the
equivalent mutants of section 2 from its passing run.

## 7. Known gaps

None that does not block: every gap above is an issue this review depends on (P2-161 to
P2-167). No crate README states an exception to the bar.
