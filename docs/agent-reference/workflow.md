# Working in QuakeC Studio

Sources: [`docs/spec/qc-tests.md`](../spec/qc-tests.md) (QCT), [`docs/spec/scenario.md`](../spec/scenario.md)
(SCN), [`docs/commands.md`](../commands.md).

## Finding the code

`project.info` gives each module's dialect and engine, `project.files` the source files,
`project.search` the lines holding a text, `project.symbols` the definitions, and
`symbol.definition`, `symbol.references` and `symbol.hover` where a name is defined and used.
Read a file with `file.read` before changing it.

## Changing it

`edit.apply` stages each file's whole new text as hunks for the person to review. Nothing is
written to disk by you. Then `compile` must report `"clean": true`.

## Proving it

- A unit test is a function whose name starts with `test_`, taking no arguments. It passes when
  it returns and fails when it calls `error` or `objerror`, or stops with any other run error
  (QCT-2.2). Each runs in a fresh VM over a mock engine (QCT-2.1). `vm.run` calls one function.
- A scenario test is a `tests/*.scenario` file played in the server simulation on the
  project's program over the game's own files (QCT-3.1). Its lines (SCN-1):
  `map <name>`, `seed <n>`, `frames <count>`, `move <pitch> <yaw> <roll> <forward> <side>
  <up> <buttons> <impulse>`, `start <x> <y> <z> <yaw>`, `cmd <text>`, `console <text>`, and
  `expect <target> <op> <value>` with target `global <name>` or
  `field <player|n|classname=name> <field>`, op `==`, `!=`, `<`, `<=`, `>` or `>=`, and value
  a number, a vector `'x y z'`, a string in quotes or an edict `#n`. For example:

  ```text
  map start
  frames 4
  expect field player health == 100
  ```

- `project.test` runs every test; `scenario.run` plays one file. The run is done only when
  `compile` is clean and `project.test` passes.
