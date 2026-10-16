# QC virtual machine execution

Prefix: `VM` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how the engine executes a loaded `progs.dat`: the statement loop, every vanilla
opcode, calls and returns, the parameter and return slots, the local stack, builtin dispatch,
the runaway loop guard and what happens on an error, as implemented by `WinQuake/pr_exec.c`
(`PR_ExecuteProgram`, `PR_EnterFunction`, `PR_LeaveFunction`, `PR_RunError`) with the
structures of `WinQuake/pr_comp.h` and `WinQuake/progs.h`, in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. Implemented by `qc-vm` (P1-10).

The file format, opcode numbers and value encodings are in [`progs.md`](progs.md) (PROGS-7,
PROGS-8, PROGS-17, PROGS-18); the call depth, local stack, runaway and builtin limits are in
[`limits.md`](limits.md) (LIMIT-2 to LIMIT-5). Entity memory, the system globals and fields
and what each builtin does are in `edicts.md` (P1-03) and `builtins.md` (P1-04). Every
statement in this file is in [`coverage-baseline.tsv`](coverage-baseline.tsv) until `qc-vm`
cites it.

Notation: `g[n]` is global slot `n`; `a`, `b` and `c` are a statement's operands (PROGS-7.1);
"the float in slot n" and "the 32-bit pattern in slot n" read the same 4 bytes as an f32 or
an i32 (PROGS-17.1). `1.0` and `0.0` are the f32 values 0x3F800000 and 0x00000000.

**Reference build.** Where id's C is undefined or depends on the compiler or the CPU, the
statements below fix one answer. To agree with them, the id VM oracle (P1-11) is built with
`-fno-strict-aliasing` (VM-5.1, VM-10.2) and `-ffp-contract=off` (VM-5.2), and the vectors
for VM-9.2 run on an x86 host. The test vectors were checked with exactly that build (see
[Test vectors](#test-vectors)).

## VM-1 Machine state and operands

**Source:** `WinQuake/pr_exec.c`, `pr_stack`, `pr_depth`, `localstack`, `localstack_used`,
`pr_xfunction`, `pr_xstatement`, `pr_argc`, `PR_ExecuteProgram`; `WinQuake/quakedef.h`,
`PARANOID`.

### Behaviour

- **VM-1.1** The VM state is the globals (PROGS-12), the edicts, a call stack of frames (each
  the statement to return to and the function it belongs to), the call depth, a local stack
  of `LIMIT_LOCALSTACK_SIZE` slots and the number of them in use, the current function, the
  current statement and the argument count of the last call (`pr_argc`). All of it persists
  from one `PR_ExecuteProgram` call to the next; only the runaway counter belongs to one call
  (VM-2.3).
- **VM-1.2** Execution reads the statements and functions tables and never writes them,
  except each function's `profile` counter (VM-3.1).
- **VM-1.3** Each opcode uses its operands as VM-3.6 says. An operand used as a global slot
  names one slot for a scalar and the three slots `n`, `n+1`, `n+2` for a vector; an opcode
  reads and writes only the slots its section names. An operand the opcode does not use is
  ignored, whatever its value.
- **VM-1.4** id's VM checks no operand: `PARANOID` is commented out in `quakedef.h`, so a
  global slot outside `0` to `numglobals - 1` is read or written outside the globals, which is
  undefined in C. This spec fixes the answer: a statement that would read or write a global
  slot outside that range fails with a runtime error (VM-19.4) before it changes any global or
  field. It is a deliberate divergence; qcc never writes such an operand.

## VM-2 Starting a run

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`.

The engine runs QuakeC by calling `PR_ExecuteProgram` with a function index: `main`,
`StartFrame`, a think or touch function, a spawn function and so on (`edicts.md`).

### Behaviour

- **VM-2.1** A function index of 0, or not less than `numfunctions`, fails with
  `PR_ExecuteProgram: NULL function`. This error goes straight to `Host_Error`, not through
  `PR_RunError`: no statement or stack trace is printed, the call depth is not reset, and when
  the global `self` is not 0 the engine prints that entity first.
- **VM-2.2** A negative function index passes id's test (`!fnum || fnum >= numfunctions`) and
  reads before the start of the functions table, which is undefined in C. This spec fixes the
  answer: it fails as VM-2.1 does.
- **VM-2.3** A run sets its own runaway counter to `LIMIT_RUNAWAY` (LIMIT-4.1), clears the
  statement trace flag (`pr_trace`, set by the `traceon` builtin), records the current call
  depth as its exit depth, and enters the function as VM-15 says. It returns to its caller
  when a return brings the call depth back to the exit depth (VM-16.4).
- **VM-2.4** A builtin (`first_statement` below 0) passed to `PR_ExecuteProgram` is entered as
  if it were a QC function, and execution starts at the negative statement index
  `first_statement`, which is undefined in C. This spec fixes the answer: it fails with a
  runtime error (VM-19.4) before any statement runs. The engine never does this with id's
  progs.

## VM-3 The statement loop

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`.

### Behaviour

- **VM-3.1** Each step of the loop moves to the next statement (the current one plus one),
  decrements the runaway counter and, if it reaches 0, fails with `runaway loop error`
  without executing the statement (LIMIT-4.1). Otherwise it adds one to the current
  function's `profile`, makes the statement current and executes it.
- **VM-3.2** Nothing marks the end of a function: execution runs on into the next statement in
  the table, whichever function it belongs to, until a `DONE` or `RETURN` (VM-16), a jump
  (VM-13) or a call (VM-14) moves it.
- **VM-3.3** Reaching a statement index outside `0` to `numstatements - 1` (by running past the
  last statement, by a jump, or through a function's `first_statement`) reads outside the
  statements table, which is undefined in C. This spec fixes the answer: it fails with a
  runtime error (VM-19.4) before the statement executes.
- **VM-3.4** An opcode above 65 fails with `Bad opcode <op>`, the opcode printed as an
  unsigned decimal number (PROGS-7.4). The step has already counted against the runaway
  counter and `profile`.
- **VM-3.5** Statement 0 is reserved and all zero (PROGS-1.5), so executing it is `DONE 0 0 0`:
  a jump to statement 0, or a function whose `first_statement` is 0 (LIMIT-5.2), returns at
  once.
- **VM-3.6** The operands of each opcode are:

  | Ops | Names | `a` | `b` | `c` | Section |
  | --- | --- | --- | --- | --- | --- |
  | 0, 43 | `DONE`, `RETURN` | Value to return (3 slots) | Unused | Unused | VM-16 |
  | 1, 5, 6, 8 | `MUL_F`, `DIV_F`, `ADD_F`, `SUB_F` | Float | Float | Float result | VM-4 |
  | 2 | `MUL_V` | Vector | Vector | Float result | VM-5 |
  | 3 | `MUL_FV` | Float | Vector | Vector result | VM-5 |
  | 4 | `MUL_VF` | Vector | Float | Vector result | VM-5 |
  | 7, 9 | `ADD_V`, `SUB_V` | Vector | Vector | Vector result | VM-5 |
  | 10, 15, 20 to 23 | `EQ_F`, `NE_F`, `LE`, `GE`, `LT`, `GT` | Float | Float | Float result | VM-6 |
  | 11, 16 | `EQ_V`, `NE_V` | Vector | Vector | Float result | VM-6 |
  | 12, 17 | `EQ_S`, `NE_S` | String | String | Float result | VM-7 |
  | 13, 18 | `EQ_E`, `NE_E` | Entity | Entity | Float result | VM-7 |
  | 14, 19 | `EQ_FNC`, `NE_FNC` | Function | Function | Float result | VM-7 |
  | 24 to 29 | `LOAD_F`, `LOAD_V`, `LOAD_S`, `LOAD_ENT`, `LOAD_FLD`, `LOAD_FNC` | Entity | Field | Result (3 slots for `LOAD_V`) | VM-11 |
  | 30 | `ADDRESS` | Entity | Field | Pointer result | VM-12 |
  | 31 to 36 | `STORE_F`, `STORE_V`, `STORE_S`, `STORE_ENT`, `STORE_FLD`, `STORE_FNC` | Source | Destination | Unused | VM-10 |
  | 37 to 42 | `STOREP_F`, `STOREP_V`, `STOREP_S`, `STOREP_ENT`, `STOREP_FLD`, `STOREP_FNC` | Source | Pointer | Unused | VM-12 |
  | 44 to 48 | `NOT_F`, `NOT_V`, `NOT_S`, `NOT_ENT`, `NOT_FNC` | Operand | Unused | Float result | VM-8 |
  | 49, 50 | `IF`, `IFNOT` | Condition | Jump offset, not a slot | Unused | VM-13 |
  | 51 to 59 | `CALL0` to `CALL8` | Function | Unused | Unused | VM-14 |
  | 60 | `STATE` | Frame (float) | Think function | Unused | VM-17 |
  | 61 | `GOTO` | Jump offset, not a slot | Unused | Unused | VM-13 |
  | 62, 63 | `AND`, `OR` | Float | Float | Float result | VM-8 |
  | 64, 65 | `BITAND`, `BITOR` | Float | Float | Float result | VM-9 |

## VM-4 Float arithmetic

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_ADD_F`, `OP_SUB_F`, `OP_MUL_F`,
`OP_DIV_F`.

`ADD_F`, `SUB_F`, `MUL_F` and `DIV_F` are C's `+`, `-`, `*` and `/` on two `float`s. id's
shipped x86 build ran them on the x87 FPU at a higher internal precision, but for a single
operation on two f32 values rounding that result to f32 gives the same bits as computing in
f32, so the only arithmetic where the precision shows is `MUL_V` (VM-5.2).

### Behaviour

- **VM-4.1** `c = a + b`, `a - b`, `a * b` or `a / b` in IEEE 754 single precision with
  round-to-nearest-even, the result rounded to f32 once. Subnormal operands and results are
  kept, not flushed to zero, and signed zeros follow IEEE 754 (`-0 - +0` is `-0`).
- **VM-4.2** No arithmetic opcode fails. Overflow gives an infinity; a non-zero finite value
  divided by zero gives an infinity whose sign is the exclusive or of the operands' signs (so
  `1 / -0` is `-inf`); `0 / 0`, `inf - inf`, `inf / inf` and `0 * inf` give a NaN; and any NaN
  operand gives a NaN.
- **VM-4.3** The sign and payload of a NaN that an operation produces are not specified: C
  leaves them to the CPU, and they differ between hosts (x86 produces 0xFFC00000 for `0 / 0`,
  ARM 0x7FC00000). An implementation may produce any NaN, and comparing a result with an
  expected value treats every NaN as equal to every other NaN. Copying a NaN never changes it
  (VM-10.3).
- **VM-4.4** `c` may be the same slot as `a` or `b`: both operands are read before `c` is
  written.

## VM-5 Vector arithmetic

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_ADD_V`, `OP_SUB_V`, `OP_MUL_V`,
`OP_MUL_FV`, `OP_MUL_VF`.

### Behaviour

- **VM-5.1** `ADD_V` and `SUB_V` compute `c[i] = a[i] + b[i]` (or `-`) for `i` = 0, 1, 2 in
  that order, each component as VM-4.1. Each component reads its operands after the previous
  component's result is written, so when `c` overlaps `a` or `b` shifted by one or two slots,
  later components see the values just written. The C writes the components in this order,
  but because the operands are then partially overlapping objects a compiler may reorder the
  reads (GCC at `-O2` does unless given `-fno-strict-aliasing`); this spec fixes the source
  order.
- **VM-5.2** `MUL_V` is the dot product: `c = (a[0]*b[0] + a[1]*b[1]) + a[2]*b[2]`, three
  products and two sums, each rounded to f32 as VM-4.1, added left to right, with no fused
  multiply-add. All six inputs are read before the one result slot is written. id's x87 build
  kept the intermediate values at a higher precision and could differ in the last bit; this
  spec fixes per-operation f32 rounding, which every SSE and ARM build gives when compiled
  without contraction (`-ffp-contract=off`).
- **VM-5.3** `MUL_FV` computes `c[i] = a * b[i]` for `i` = 0, 1, 2 in order, reading the float
  `a` again for each component, so when `c[0]` is `a`'s slot the later components use the new
  value.
- **VM-5.4** `MUL_VF` computes `c[i] = b * a[i]` for `i` = 0, 1, 2 in order, reading the float
  `b` again for each component in the same way.

## VM-6 Float and vector comparisons

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_EQ_F`, `OP_NE_F`, `OP_LE`, `OP_GE`,
`OP_LT`, `OP_GT`, `OP_EQ_V`, `OP_NE_V`.

### Behaviour

- **VM-6.1** `EQ_F`, `NE_F`, `LE`, `GE`, `LT` and `GT` write `1.0` to `c` when `a == b`,
  `a != b`, `a <= b`, `a >= b`, `a < b` or `a > b` holds for the floats, and `0.0` when it does
  not.
- **VM-6.2** The comparisons are IEEE 754's: `-0` equals `+0`, and a NaN is unordered with
  everything including itself, so `EQ_F`, `LE`, `GE`, `LT` and `GT` give `0.0` and `NE_F`
  gives `1.0` when either operand is a NaN.
- **VM-6.3** `EQ_V` gives `1.0` when all three components are equal as VM-6.2 says, and `NE_V`
  gives `1.0` when any component is unequal; a NaN in any component makes `EQ_V` `0.0` and
  `NE_V` `1.0`, even comparing a vector with itself.
- **VM-6.4** All inputs are read before `c` is written, so `c` may overlap `a` or `b`.

## VM-7 String, entity and function comparisons

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_EQ_S`, `OP_NE_S`, `OP_EQ_E`,
`OP_NE_E`, `OP_EQ_FNC`, `OP_NE_FNC`.

### Behaviour

- **VM-7.1** `EQ_S` compares the two strings by content with `strcmp`: the bytes from each
  string value's offset up to its NUL (PROGS-11.1), compared as unsigned bytes. It writes
  `1.0` when they are equal. Different offsets holding the same text are equal, and offset 0
  equals every empty string.
- **VM-7.2** `NE_S` writes `strcmp`'s result converted to float: `0.0` when the strings are
  equal, otherwise the first differing byte of `a` minus that of `b`, both as unsigned bytes,
  the NUL at the end of the shorter string counting as 0. C fixes only the sign of `strcmp`;
  this spec fixes the byte difference, which glibc and macOS's C library return. Code that
  only tests the result for zero is unaffected.
- **VM-7.3** A string value is an offset into the string table or a string the engine
  allocated (PROGS-11.6). id resolves any other value to memory outside both, which is
  undefined in C. This spec fixes the answer: `EQ_S`, `NE_S` and `NOT_S` with a string value
  that is neither fail with a runtime error (VM-19.4).
- **VM-7.4** `EQ_E` and `NE_E` compare the 32-bit patterns of `a` and `b`, and `EQ_FNC` and
  `NE_FNC` compare them as function indices, writing `1.0` or `0.0`. None of them checks that
  the value names an entity or function, and `EQ_E` compares bits, not floats: `-0.0` and
  `0.0` differ, and a NaN pattern equals itself.

## VM-8 Logical operators

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_NOT_F`, `OP_NOT_V`, `OP_NOT_S`,
`OP_NOT_ENT`, `OP_NOT_FNC`, `OP_AND`, `OP_OR`.

### Behaviour

- **VM-8.1** `NOT_F` writes `1.0` when the float `a` equals `0.0` (so for `+0` and `-0`) and
  `0.0` otherwise, including for a NaN.
- **VM-8.2** `NOT_V` writes `1.0` when all three components of `a` equal `0.0`.
- **VM-8.3** `NOT_S` writes `1.0` when the string value is 0 or names an empty string (its
  first byte is NUL), and `0.0` otherwise.
- **VM-8.4** `NOT_ENT` writes `1.0` when the entity value is 0, the world, and `0.0`
  otherwise. It compares the value only, without checking that it names an edict or whether
  that edict is free.
- **VM-8.5** `NOT_FNC` writes `1.0` when the function index is 0.
- **VM-8.6** `AND` writes `1.0` when the floats `a` and `b` are both non-zero, and `OR` when
  either is. A NaN is non-zero and `-0` is zero. Both operands are always read; C's
  short-circuit has no visible effect.
- **VM-8.7** These opcodes read their operands as floats, unlike `IF` and `IFNOT` (VM-13.3),
  which test the 32-bit pattern: a slot holding `-0.0` is false for `NOT_F`, `AND` and `OR`
  but true for `IF`. An integer value read as a float (a string offset, an entity value) is
  zero only when its pattern is 0x00000000 or 0x80000000.

## VM-9 Bitwise operators

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_BITAND`, `OP_BITOR`.

### Behaviour

- **VM-9.1** `BITAND` and `BITOR` convert each float operand to a 32-bit signed integer by
  truncation toward zero, take the two's complement AND or OR, and write the integer
  converted back to float, rounded to nearest even, so a result above 2^24 can lose its low
  bits.
- **VM-9.2** Converting a NaN, an infinity or a value whose truncation is outside -2^31 to
  2^31 - 1 is undefined in C. This spec fixes the result of the conversion as -2147483648
  (0x80000000), the value an x86 build produces: the "integer indefinite" result of SSE's
  `cvttss2si` and of the x87 `fist` instructions. An ARM build gives a different value (it saturates, and a NaN becomes 0),
  so an oracle run of these vectors is on x86.
- **VM-9.3** Negative operands work in two's complement: `-8 | 3` is `-5` and `-1 & 5` is `5`.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `VM_FLOAT_TO_INT_INVALID` | -2147483648 | The x86 "integer indefinite" value, the answer VM-9.2 fixes for `(int)a->_float` in `OP_BITAND` and `OP_BITOR` (`WinQuake/pr_exec.c`, `PR_ExecuteProgram`) |

## VM-10 Global stores

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_STORE_F`, `OP_STORE_V`,
`OP_STORE_S`, `OP_STORE_ENT`, `OP_STORE_FLD`, `OP_STORE_FNC`.

### Behaviour

- **VM-10.1** `STORE_F`, `STORE_S`, `STORE_ENT`, `STORE_FLD` and `STORE_FNC` copy the 32-bit
  pattern of slot `a` to slot `b`. They are the same operation: the type in the name is not
  checked.
- **VM-10.2** `STORE_V` copies slots `a`, `a+1`, `a+2` to `b`, `b+1`, `b+2` one at a time in
  that order. When `b` is `a+1` or `a+2` a later copy reads a slot already written, so
  `STORE_V 100 101` sets slots 101 to 103 to the old value of slot 100. This is the source
  order, fixed as in VM-5.1.
- **VM-10.3** Every copy is bit-exact, including `-0.0` and every NaN pattern, signalling NaNs
  among them. `STORE_V` copies through float assignments in C, which id's x87 build may have
  done by loading and storing through the FPU, quietening a signalling NaN; this spec fixes
  the bit-exact copy, which no QC arithmetic can tell apart because no arithmetic produces a
  signalling NaN.
- **VM-10.4** In a store, `b` is the destination and `c` is unused, unlike the other opcodes,
  which write their result to `c`.

## VM-11 Entity field loads

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_LOAD_F`, `OP_LOAD_V`, `OP_LOAD_S`,
`OP_LOAD_ENT`, `OP_LOAD_FLD`, `OP_LOAD_FNC`; `WinQuake/progs.h`, `PROG_TO_EDICT`.

### Behaviour

- **VM-11.1** `LOAD_F`, `LOAD_S`, `LOAD_ENT`, `LOAD_FLD` and `LOAD_FNC` take an entity value in
  `a` (PROGS-17.4) and a field index in `b` (PROGS-17.5), and copy the 32-bit pattern of that
  entity's field slot `b` to slot `c`. They are the same operation.
- **VM-11.2** `LOAD_V` copies the entity's field slots `b`, `b+1`, `b+2` to `c`, `c+1`, `c+2`.
- **VM-11.3** A load works on any edict: the world and free edicts are read like any other.
- **VM-11.4** id does not check the entity value or the field index (`PARANOID` is off), so a
  value that does not name an edict, or a field slot outside `0` to `entityfields - 1`, reads
  other memory, which is undefined in C. This spec fixes the answer: such a load fails with a
  runtime error (VM-19.4). Which entity values name an edict is in `edicts.md` (P1-03).

## VM-12 Field addresses and pointer stores

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_ADDRESS`, `OP_STOREP_F`,
`OP_STOREP_V`, `OP_STOREP_S`, `OP_STOREP_ENT`, `OP_STOREP_FLD`, `OP_STOREP_FNC`;
`WinQuake/server.h`, `server_state_t`.

qcc compiles an assignment to an entity field, `e.f = x`, as `ADDRESS` followed by one of the
`STOREP` opcodes.

### Behaviour

- **VM-12.1** `ADDRESS` takes an entity value in `a` and a field index in `b` and writes to `c`
  a pointer to that entity's field slot `b` (PROGS-17.7). In id's engine the pointer is the
  entity value plus the byte offset of the fields within an edict plus `4 * b`; the offset is
  engine-specific (`edicts.md`), so only `STOREP` gives a pointer meaning.
- **VM-12.2** `ADDRESS` on the world (entity value 0) fails with `assignment to world entity`
  while the server is active (`sv.state == ss_active`). While a map is loading
  (`ss_loading`, when spawn functions run) it succeeds.
- **VM-12.3** `ADDRESS` reads no field and checks neither the field index nor whether the edict
  is free.
- **VM-12.4** `STOREP_F`, `STOREP_S`, `STOREP_ENT`, `STOREP_FLD` and `STOREP_FNC` copy the
  32-bit pattern of slot `a` to the field slot that the pointer in slot `b` names; `STOREP_V`
  copies slots `a` to `a+2` to that field slot and the next two, in order.
- **VM-12.5** id writes wherever the pointer leads, which is undefined in C when it is not a
  field slot of an edict. This spec fixes the answer: a `STOREP` whose pointer does not name a
  field slot of an edict, or for `STOREP_V` three consecutive field slots of one edict, fails
  with a runtime error (VM-19.4) without writing.
- **VM-12.6** `STOREP` does not check for the world or a free edict: a pointer to a world field
  made while loading still writes to the world when the server is active.

## VM-13 Jumps

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_GOTO`, `OP_IF`, `OP_IFNOT`.

### Behaviour

- **VM-13.1** `GOTO` makes the next statement the current one plus `a`, a signed offset
  (PROGS-7.2).
- **VM-13.2** `IF` makes the next statement the current one plus `b` when the 32-bit pattern in
  slot `a` is not zero, and the current one plus one otherwise. `IFNOT` jumps by `b` when the
  pattern is zero.
- **VM-13.3** The test is on the 32-bit pattern, not on the float: `-0.0` (0x80000000) and every
  NaN are true, and a string value is true when its offset is not 0, even if the string it
  names is empty.
- **VM-13.4** An offset of 0 jumps to the same statement, so `GOTO 0` repeats until the runaway
  guard stops it (VM-3.1).
- **VM-13.5** A jump outside the statements table is VM-3.3's error.

## VM-14 Calls

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_CALL0` to `OP_CALL8`;
`WinQuake/pr_cmds.c`, `pr_builtin`, `pr_numbuiltins`.

### Behaviour

- **VM-14.1** `CALL0` to `CALL8` call the function whose index is in slot `a`. The call sets
  the argument count (`pr_argc`), which builtins read, to the number in the opcode name. The
  arguments are already in the parameter slots `OFS_PARM0` to `OFS_PARM7` (PROGS-18.1),
  written by earlier statements; the call itself moves nothing.
- **VM-14.2** A function index of 0 fails with `NULL function`.
- **VM-14.3** id reads `pr_functions[index]` without checking, so a negative index or one not
  less than `numfunctions` is undefined in C. This spec fixes the answer: it fails with a
  runtime error (VM-19.4).
- **VM-14.4** When the function is a builtin (`first_statement` below 0, PROGS-10.2), its
  number is `-first_statement`; a number of `LIMIT_NUM_BUILTINS` or more fails with
  `Bad builtin call number` (LIMIT-5.1, LIMIT-5.3). Otherwise the builtin is called directly:
  no frame is pushed, the call depth does not change (LIMIT-2.3), no locals are saved and no
  parameters are copied. A builtin returns a value by writing `OFS_RETURN`. When it returns,
  execution continues with the statement after the call.
- **VM-14.5** When the function is a QC function it is entered as VM-15 says, and execution
  continues at its `first_statement`. The statement after the call runs when it returns
  (VM-16.4).
- **VM-14.6** The number in the opcode is not checked against the callee's `numparms`: the
  callee copies `numparms` parameters from the parameter slots whatever they hold, including
  values left there by earlier calls.

## VM-15 Entering a function

**Source:** `WinQuake/pr_exec.c`, `PR_EnterFunction`.

### Behaviour

- **VM-15.1** Entering pushes a frame holding the current statement and the current function,
  and adds one to the call depth; when the new depth is `LIMIT_MAX_STACK_DEPTH` or more it
  fails with `stack overflow` (LIMIT-2.1). The depth is checked before the local stack.
- **VM-15.2** Next it saves the callee's `locals` slots, from `parm_start` up, onto the local
  stack above the slots in use. When the slots in use plus `locals` is more than
  `LIMIT_LOCALSTACK_SIZE` it fails with `PR_ExecuteProgram: locals stack overflow` before
  saving anything (LIMIT-3.1).
- **VM-15.3** Then it copies the parameters: with `o` starting at `parm_start`, for each
  parameter `i` from 0 to `numparms - 1` and each `j` from 0 to `parm_size[i] - 1`, slot
  `4 + 3*i + j` is copied to slot `o` and `o` goes up by one. Parameters are packed: a float
  parameter after a vector parameter lands at `parm_start + 3`. The copy comes after the save,
  so the saved values are the caller's.
- **VM-15.4** `parm_size` is not checked: a size above 3 reads on past the parameter's three
  slots into the next parameter's, and past slot 27 into ordinary globals. A `numparms` of 0
  or less copies nothing (PROGS-10.5).
- **VM-15.5** The callee becomes the current function and execution continues at its
  `first_statement`.
- **VM-15.6** A negative `locals` makes id move the local stack's top down, and a later entry
  or return writes outside it, which is undefined in C. This spec fixes the answer: entering
  a function whose `locals` is negative fails with a runtime error (VM-19.4). A save, restore
  or parameter copy that reaches a global slot outside the globals is VM-1.4's error.
- **VM-15.7** Entering does not clear the local slots: a local that is not a parameter starts
  with whatever value its global slot held, normally the value left by the caller or by the
  last function that used the slot.

## VM-16 Returning

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_DONE`, `OP_RETURN`;
`PR_LeaveFunction`.

### Behaviour

- **VM-16.1** `DONE` and `RETURN` are the same operation. They copy slots `a`, `a+1`, `a+2` to
  `OFS_RETURN` (slots 1, 2, 3), one slot at a time in that order. All three are copied, even
  for a function returning a float or nothing.
- **VM-16.2** qcc ends every function with `DONE 0 0 0`, so falling off the end copies slot 0
  to slot 1, then slot 1 (now slot 0's value) to slot 2, then slot 2 to slot 3: all three
  return slots get slot 0's value, normally 0. A function that returns nothing therefore
  clears the return slots.
- **VM-16.3** After the copy, the current function's `locals` slots are restored from the top
  of the local stack to `parm_start` up, the slots in use drop by `locals`, and the frame is
  popped: the current function and statement become the ones the frame saved. The return
  value is copied before the restore, so a function can return one of its own locals.
- **VM-16.4** When the call depth is then back at the exit depth of the current run
  (VM-2.3), `PR_ExecuteProgram` returns to its caller. Otherwise execution continues with the
  statement after the call in the restored function.
- **VM-16.5** The return copy is bit-exact, as VM-10.3 fixes for `STORE_V`.
- **VM-16.6** `PR_LeaveFunction`'s own checks, `prog stack underflow` (through `Sys_Error`) and
  `PR_ExecuteProgram: locals stack underflow`, cannot fail: every return matches an entry of
  the same function, and `locals` is never negative (VM-15.6).

## VM-17 STATE

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_STATE`; `WinQuake/progdefs.q1`,
`globalvars_t`, `entvars_t`.

qcc compiles a frame function, `void() name = [frame, think] { ... }`, to a `STATE` at the
start of the body. `FPS_20` is not defined anywhere in id's build, so the interval is 0.1.

### Behaviour

- **VM-17.1** `STATE` acts on the entity in the global `self`. It sets that entity's
  `nextthink` field to the global `time` plus 0.1, added in double precision:
  `(float)((double)time + 0.1)`, where 0.1 is the double nearest to 0.1, not the float.
- **VM-17.2** It sets the entity's `frame` field to the float in `a` only when that float is not
  equal to the current `frame` as VM-6.2 compares: when `a` is `-0.0` and `frame` is `+0.0`, or
  the reverse, `frame` keeps its bits, and when `a` is a NaN `frame` becomes that NaN.
- **VM-17.3** It sets the entity's `think` field to the 32-bit pattern in `b`, a function index
  that is not checked.
- **VM-17.4** `STATE` does not check for the world: with `self` 0 it writes the world's fields.
  A `self` that does not name an edict is VM-11.4's error. Where `self`, `time`, `nextthink`,
  `frame` and `think` are is in `edicts.md`; in id's `progdefs.q1` they are global slots 28
  and 31 and field slots 46, 30 and 44.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `VM_STATE_THINK_INTERVAL` | 0.1 (a double) | `WinQuake/pr_exec.c`, `OP_STATE`: `ed->v.nextthink = pr_global_struct->time + 0.1;` (the `#else` branch of `FPS_20`) |

## VM-18 Nested runs

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `PR_EnterFunction`, `PR_LeaveFunction`.

A builtin can call `PR_ExecuteProgram` itself, for example to run touch functions while
moving an entity (`builtins.md`). That run is nested inside the run that called the builtin.

### Behaviour

- **VM-18.1** A nested run shares the call stack, the call depth, the local stack and the
  globals with the outer run, and has its own runaway counter and exit depth (LIMIT-2.2,
  LIMIT-4.2).
- **VM-18.2** When the nested run returns, the current function and statement are again the
  outer function and its call statement, restored from the frame the nested entry pushed; the
  builtin then returns and the outer run continues with the statement after the call.
- **VM-18.3** A nested run clears the statement trace flag (VM-2.3), so tracing started before
  it is off after it.
- **VM-18.4** An error in a nested run ends every run, not only the nested one (VM-19.2).

## VM-19 Errors

**Source:** `WinQuake/pr_exec.c`, `PR_RunError`, `PR_StackTrace`, `PR_PrintStatement`;
`WinQuake/host.c`, `Host_Error`.

### Behaviour

- **VM-19.1** A runtime error from the VM (`stack overflow`,
  `PR_ExecuteProgram: locals stack overflow`, `runaway loop error`, `NULL function`,
  `Bad builtin call number`, `Bad opcode <op>`, `assignment to world entity`), like one from a
  builtin, goes through `PR_RunError`. It prints the current statement (the last one VM-3.1
  made current, so for `runaway loop error` the statement before the one that was not run),
  then the call stack from the innermost function out as `<file> : <function>`, then the
  message; it sets the call depth to 0 ("dump the stack so host_error can shutdown
  functions") and calls `Host_Error ("Program error")`.
- **VM-19.2** `Host_Error` ("This shuts down both the client and server") never returns: no
  further statement of any run executes, at any level of nesting, and `PR_ExecuteProgram` does
  not return to its caller.
- **VM-19.3** After an error the call depth is 0, but the local stack is not reset: the slots
  the aborted functions saved stay in use, are never restored and are never released, since
  nothing outside `pr_exec.c` touches `localstack_used`. The globals keep the values they had
  when the error happened, including the locals of the aborted functions.
- **VM-19.4** The errors this spec adds where id's C is undefined (VM-1.4, VM-2.2, VM-2.4,
  VM-3.3, VM-7.3, VM-11.4, VM-12.5, VM-14.3, VM-15.6) are runtime errors with the effects of
  VM-19.1 to VM-19.3. id has no message for them, so the message is the implementation's.
- **VM-19.5** An opcode that fails writes no global and no field. It may already have set the
  argument count (a failing call) or counted against `profile` (VM-3.4). A builtin that fails
  may have written before it failed (`builtins.md`).

## Test vectors

The vectors are small programs with the expected changes to the globals and fields after
every statement, in [`vectors/vm/`](vectors/vm/README.md): one file per group of sections,
142 vectors covering all 66 vanilla opcodes. The README gives the format, the fixed
environment every vector runs in and how they were checked.

Checked on 2026-10-16 by running the 128 vectors that have a defined answer in id's C through
`PR_ExecuteProgram`, copied unchanged from `WinQuake/pr_exec.c` at the pinned commit with
`pr_comp.h`, `progs.h` and `progdefs.q1`, in a local harness on x86-64 (GCC 14.4,
`-O2 -fno-strict-aliasing -ffp-contract=off`): every vector matched. The same harness built
for arm64 (Apple clang) matched every vector except the two for VM-9.2, as VM-9.2 says. The
other 14 vectors pin the divergences of VM-19.4 and expect only an error. The vectors join
the oracle suite when P1-11 lands.

| Group | File | Statements |
| --- | --- | --- |
| Float and vector arithmetic | [`arith.txt`](vectors/vm/arith.txt) | VM-4, VM-5 |
| Comparisons and logical operators | [`compare.txt`](vectors/vm/compare.txt) | VM-6 to VM-8 |
| Bitwise operators | [`bitwise.txt`](vectors/vm/bitwise.txt) | VM-9 |
| Stores, loads and pointers | [`store.txt`](vectors/vm/store.txt) | VM-10 to VM-12 |
| Control flow, calls, `STATE`, nested runs and errors | [`flow.txt`](vectors/vm/flow.txt) | VM-1 to VM-3, VM-13 to VM-19 |
