# Scenario test vectors

Vectors for the scenario file of [`../../differential.md`](../../differential.md) (DIFF-2).

| File | Statements | Vectors |
| --- | --- | --- |
| [`numbers.txt`](numbers.txt) | DIFF-2.7, DIFF-2.8, DIFF-2.9: which integer and number texts a scenario accepts, and the value each is read as | 101 |

## Format of `numbers.txt`

One vector per line, `#` to the end of the line a comment:

```text
<directive> <text> <result>
```

- `<directive>` is where the text goes: `frames` (an integer, 0 to 1000000), `frametime` (a
  number, greater than 0 and at most 1), `model` (the first of the six numbers) or `v_angle`
  (the first of the three numbers of an `input` line).
- `<text>` is the argument exactly as the scenario holds it. It cannot hold a space, a tab or a
  `#` (DIFF-2.1).
- `<result>` is `refused` when the scenario is refused (DIFF-2.9), or the value read: the
  integer in decimal for `frames`, the 16 hexadecimal digits of the `double` for `frametime`,
  and the 8 hexadecimal digits of the `f32` for `model` and `v_angle`.

## Who runs them

- `cargo xtask oracles` puts each text in a scenario of its own and runs it through the VM
  oracle (`xtask/src/oracles/numbers.rs`), in CI on Linux, macOS and Windows. A `refused`
  vector must exit with status 2 and name the vector's line; any other must exit with status
  0. The oracle does not report the value it read.
- `scenario_numbers` in `crates/qs-tests` reads each text as the Rust runner of P1-13 does,
  the syntax of DIFF-2.7 and DIFF-2.8 checked first and Rust's `i64`, `f64` or `f32` parsing
  after it, and checks both the verdict and the value.

An integer of more than 18 digits, such as `0000000000000000001`, is an integer by DIFF-2.7,
but the oracle refuses it today; that is
[#296](https://github.com/quakecstudio/editor/issues/296) (P1-36), which adds its vector.
