# QuakeC tests

Prefix: `QCT` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes a QuakeC project's tests and the command that runs them, `project.test` (P4-26,
QA-13): unit tests, functions of the program called alone in the VM over the mock engine as
`vm.run` calls one, and scenario tests, scenario files (`scenario.md`) played in `qs-sim`. A
run of the agent is done only when the compile is clean and these pass (AGT-5). It is this
project's design, not engine behaviour: what a call or a frame does is the VM's and `qs-sim`'s.
Implemented by `qs-workbench` (`testing`, the discovery, the runs and the command) and
`quakec-ide` (`test`, the command line's verb).

## Prior art

The model is **Rust's and Go's built-in tests**: a test is found by its name (`#[test]`,
`func TestX`), each runs on its own, a failure is the panic or `t.Error` that stopped it, and
the runner answers each test's verdict and a summary, the same from the editor and the command
line. QuakeC has no attributes and no exceptions, so the name is a prefix, as Go's is, and the
failure is id's own way of stopping on a broken invariant, the `error` builtin.

## QCT-1 Discovery

**Source:** this design.

### Behaviour

- **QCT-1.1** A unit test is a function of the program the project builds whose name starts
  with `test_` and that takes no parameters; a builtin is not one. Unit tests run in the order
  of the program's function table, the order the compiler defined them in.
- **QCT-1.2** A scenario test is a file whose name ends in `.scenario` under the project's
  `tests` directory, at any depth. Its name is its path from the project directory, its parts
  joined by `/`. Scenario tests run after the unit tests, in the order of their names compared
  byte by byte.
- **QCT-1.3** A filter, when given, keeps only the tests whose name contains it.

## QCT-2 Unit tests

**Source:** this design; `vm.run` (P4-02).

### Behaviour

- **QCT-2.1** Each unit test is called as `vm.run` calls a function: in a fresh VM over the
  mock engine with its fixed seed, on the program's initial globals, `self` the world. Nothing
  one test sets is seen by another; a fixture is a QuakeC function a test calls first.
- **QCT-2.2** A unit test passes when the call returns. It fails when the call stops with a run
  error, its message the run error's: an assertion is a call to `error` or `objerror` saying
  what was expected, as id's code stops on a broken invariant, and any other run error, such
  as a runaway loop or a call through a null function, fails the test the same way.

## QCT-3 Scenario tests

**Source:** this design; `scenario.run` (SCN-2).

### Behaviour

- **QCT-3.1** A scenario test plays its file as `scenario.run` does (SCN-2.1 to SCN-2.5), on
  the project's built program over the original game's files: the game folder given, else the
  first install found, as Play finds it. No screenshot is taken.
- **QCT-3.2** A scenario test passes when the scenario passed (SCN-2.4). It fails when an
  assertion failed, its message naming each failed assertion's line, text and value read; when
  a frame failed, its message the frame's error; and when the file does not read (SCN-1.1) or
  the game's files or map do not load, its message saying why.
- **QCT-3.3** With no game found, or a program not written to `build/<mod>/` in the project,
  every scenario test is skipped, its message saying why. A skipped test neither passes nor
  fails.

## QCT-4 The result

**Source:** this design.

### Behaviour

- **QCT-4.1** `project.test` builds the project once. When no module builds, it fails with the
  build's problems instead of answering tests. Otherwise it answers each test with its kind
  (`unit` or `scenario`), its name, its verdict (`passed`, `failed` or `skipped`) and its
  message; how many tests had each verdict; and `passed`, true when no test failed, so a
  project with no tests passes.
- **QCT-4.2** The same project and parameters give the same result from the command line
  (`quakec-ide test`, or `quakec-ide run project.test`), the palette and the agent: each runs
  the one command through the registry.
- **QCT-4.3** `quakec-ide test [<project>] [--filter <text>] [--game <folder>]` prints each
  test's verdict and name, the message of each that failed or was skipped, and the counts. It
  exits 0 when no test failed, 1 when a test failed, and 2 when the project did not open or
  build.
