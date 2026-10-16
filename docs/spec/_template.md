# <Title>

Prefix: `<PREFIX>` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes <what>, as implemented by <id file(s)> in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit `<sha>`.
Implemented by `<crate>`.

## <PREFIX>-1 <Section title>

**Source:** `WinQuake/<file>.c`, `<Function>` (id-Software/Quake@`<sha>`).

<One or two sentences of context: where this behaviour is used and why it matters. Not
normative.>

### Behaviour

- **<PREFIX>-1.1** <One observable fact a test can check.>
- **<PREFIX>-1.2** <An edge case: empty input, a limit, malformed data.>
- **<PREFIX>-1.3** <Undefined or platform-specific behaviour in the C, the answer this spec
  fixes and why.>

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `<PREFIX>_<NAME>` | <value> | `WinQuake/<file>.h`, `<IDENTIFIER>` used by `<Function>` |

### Test vectors

Checked against id's code on <date> with <how: the oracle, or id's function built locally>.

| Input | Output | Statements |
| --- | --- | --- |
| <input> | <output> | <PREFIX>-1.1 |

Larger vectors: [`vectors/<spec>/`](vectors/).

<!-- A withdrawn statement stays in place:
- **<PREFIX>-1.4** **Withdrawn** <date>: <reason>; replaced by <PREFIX>-1.5.
-->
