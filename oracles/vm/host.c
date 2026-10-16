// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The VM oracle's host: the command line, the memory arena, loading a progs, the run wrapper
// and statement hook that write the trace around id's PR_ExecuteProgram, and the error paths
// that stand in for id's Host_Error and Sys_Error (differential.md DIFF-1, DIFF-8).

#include <limits.h>
#include <signal.h>

#include "oracle.h"

// id's VM entry point, which QS_ENGINE renames to the run wrapper everywhere else.
#undef PR_ExecuteProgram
void PR_ExecuteProgram(func_t fnum);

// What id's pr_exec.c defines and the host reads.
extern int pr_depth;
extern int localstack_used;

// pr_exec.c's call stack (quakedef.h, QS_PR_EXEC): id's prstack_t, LIMIT_MAX_STACK_DEPTH
// frames and one spare for PR_StackTrace's write past the end.
typedef struct {
    int s;
    dfunction_t *f;
} qs_prstack_t;

#define QS_MAX_STACK_DEPTH 32
static qs_prstack_t pr_stack_storage[QS_MAX_STACK_DEPTH + 1];

qs_prstack_t (*qs_pr_stack(void))[QS_MAX_STACK_DEPTH];
qs_prstack_t (*qs_pr_stack(void))[QS_MAX_STACK_DEPTH] {
    return (qs_prstack_t(*)[QS_MAX_STACK_DEPTH])pr_stack_storage;
}

// pr_exec.c's strcmp (quakedef.h, QS_PR_EXEC): VM-7.2's byte difference, the bytes compared
// as unsigned.
int qs_strcmp(const char *a, const char *b);
int qs_strcmp(const char *a, const char *b) {
    const unsigned char *x = (const unsigned char *)a;
    const unsigned char *y = (const unsigned char *)b;
    while (*x && *x == *y) {
        x++;
        y++;
    }
    return (int)*x - (int)*y;
}

// What id's pr_edict.c and pr_cmds.c would define; the oracle sets them up instead.
dprograms_t *progs;
dfunction_t *pr_functions;
char *pr_strings;
ddef_t *pr_globaldefs;
ddef_t *pr_fielddefs;
dstatement_t *pr_statements;
globalvars_t *pr_global_struct;
float *pr_globals;
int pr_edict_size;
unsigned short pr_crc;
qboolean pr_trace;
builtin_t *pr_builtins;
int pr_numbuiltins;

server_t sv;
server_static_t svs;
client_t *host_client;
double host_frametime;
vec3_t vec3_origin;

jmp_buf *qs_catch;

static qboolean echo_console;

// --- The arena ---

static byte *arena;
static size_t arena_size;
static size_t arena_used;

#define QS_ARENA_SIZE ((size_t)512 << 20)

void *qs_alloc(size_t size) {
    size_t start = (arena_used + 7) & ~(size_t)7;
    if (!arena || size > arena_size - start)
        Sys_Error("vm-oracle: out of memory");
    arena_used = start + size;
    return arena + start;
}

// The arena starts all zero, which the edicts rely on (EDICT-1.8).
qboolean qs_arena_init(void) {
    arena_size = QS_ARENA_SIZE;
    arena = calloc(1, arena_size);
    return arena != NULL;
}

static int num_engine_strings;
static int num_dirty;

// Empties the arena, and with it the engine strings, so that a fuzz target can load file after
// file in one process (oracles/vm/fuzz.c).
void qs_arena_reset(void) {
    memset(arena, 0, arena_used);
    arena_used = 0;
    num_engine_strings = 0;
    num_dirty = 0;
}

// --- Engine strings (EDICT-13, TRACE-4.10) ---

typedef struct {
    int value;
    char *last;
    qboolean emitted;
    qboolean dirty;
} engine_string_t;

static engine_string_t *engine_strings;
static int max_engine_strings;
static int *dirty_strings;

static int find_engine_string(int value) {
    int lo = 0;
    int hi = num_engine_strings - 1;
    while (lo <= hi) {
        int mid = lo + (hi - lo) / 2;
        if (engine_strings[mid].value == value)
            return mid;
        if (engine_strings[mid].value < value)
            lo = mid + 1;
        else
            hi = mid - 1;
    }
    return -1;
}

static void mark_dirty(int index) {
    if (engine_strings[index].dirty)
        return;
    engine_strings[index].dirty = true;
    dirty_strings[num_dirty++] = index;
}

// Engine strings are allocated from the arena in increasing address order, so the table
// stays sorted by value.
void qs_engine_string_register(char *text) {
    if (num_engine_strings == max_engine_strings) {
        int max = max_engine_strings ? max_engine_strings * 2 : 1024;
        engine_string_t *strings = realloc(engine_strings, (size_t)max * sizeof(*strings));
        int *dirty = realloc(dirty_strings, (size_t)max * sizeof(*dirty));
        if (!strings || !dirty)
            Sys_Error("vm-oracle: out of memory");
        engine_strings = strings;
        dirty_strings = dirty;
        max_engine_strings = max;
    }
    engine_string_t *s = &engine_strings[num_engine_strings];
    s->value = (int)(text - pr_strings);
    s->last = NULL;
    s->emitted = false;
    s->dirty = false;
    mark_dirty(num_engine_strings);
    num_engine_strings++;
}

char *qs_engine_string(const char *text) {
    size_t length = strlen(text) + 1;
    char *p = qs_alloc(length);
    memcpy(p, text, length);
    qs_engine_string_register(p);
    return p;
}

void qs_engine_string_touched(char *text) {
    int index = find_engine_string((int)(text - pr_strings));
    if (index >= 0)
        mark_dirty(index);
}

qboolean qs_is_engine_string(int value) {
    return find_engine_string(value) >= 0;
}

void qs_emit_string_records(void) {
    for (int i = 0; i < num_dirty; i++) {
        engine_string_t *s = &engine_strings[dirty_strings[i]];
        const char *text = pr_strings + s->value;
        s->dirty = false;
        if (s->emitted && strcmp(s->last, text) == 0)
            continue;
        free(s->last);
        size_t length = strlen(text) + 1;
        s->last = malloc(length);
        if (!s->last) {
            fprintf(stderr, "vm-oracle: out of memory\n");
            exit(QS_EXIT_IO);
        }
        memcpy(s->last, text, length);
        s->emitted = true;
        qs_trace_string(s->value, text);
    }
    num_dirty = 0;
}

// id's ED_NewString allocates its strings here.
void *Hunk_Alloc(int size) {
    char *p = qs_alloc((size_t)size);
    qs_engine_string_register(p);
    return p;
}

// --- Console ---

static char *console;
static size_t console_size;

static void vformat(char **buffer, size_t *size, const char *fmt, va_list args) {
    for (;;) {
        va_list copy;
        va_copy(copy, args);
        int n = vsnprintf(*buffer, *size, fmt, copy);
        va_end(copy);
        if (n >= 0 && (size_t)n < *size)
            return;
        size_t want = n >= 0 ? (size_t)n + 1 : *size * 2 + 256;
        char *grown = realloc(*buffer, want);
        if (!grown) {
            fprintf(stderr, "vm-oracle: out of memory\n");
            exit(QS_EXIT_IO);
        }
        *buffer = grown;
        *size = want;
    }
}

// The console is not compared (DIFF-6.5); the last message is kept because PR_RunError prints
// its message with Con_Printf just before calling Host_Error ("Program error").
void Con_Printf(char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    vformat(&console, &console_size, fmt, args);
    va_end(args);
    if (echo_console)
        fputs(console, stderr);
}

void Con_DPrintf(char *fmt, ...) {
    (void)fmt;
}

char *va(char *format, ...) {
    static char *text;
    static size_t size;
    va_list args;
    va_start(args, format);
    vformat(&text, &size, format, args);
    va_end(args);
    return text;
}

// --- Runs and the statement hook (TRACE-4) ---

typedef struct {
    int function;
    qboolean starting;
    qboolean pending;
    int p_function;
    int p_statement;
    int p_opcode;
    int p_builtin;
} run_t;

#define QS_MAX_RUNS 256
static run_t runs[QS_MAX_RUNS];
static int run_depth;

static void flush_pending(run_t *r) {
    if (!r->pending)
        return;
    r->pending = false;
    qs_trace_step(r->p_function, r->p_statement, r->p_opcode, r->p_builtin);
}

// TRACE-4.3: the builtin a CALL statement is about to call, read before the statement runs;
// -1 when it calls no builtin or the call will fail.
static int builtin_of(const dstatement_t *st) {
    if (st->op < OP_CALL0 || st->op > OP_CALL8)
        return -1;
    if (st->a < 0 || st->a >= progs->numglobals)
        return -1;
    int fnum = ((int *)pr_globals)[st->a];
    if (fnum <= 0 || fnum >= progs->numfunctions)
        return -1;
    long long first = pr_functions[fnum].first_statement;
    if (first >= 0 || -first >= pr_numbuiltins)
        return -1;
    return (int)-first;
}

// pr_exec.c calls this where it tests pr_trace: once when a run clears the flag, before it
// enters its function, and then after each statement becomes current and before it executes.
// A statement's step record is written when the next one becomes current, when its run
// returns, or before the error record of a runaway loop, which stops before the next
// statement (TRACE-4.2, TRACE-4.7).
qboolean *qs_statement_hook(void) {
    static qboolean off;
    off = false;
    if (run_depth == 0)
        return &off;
    run_t *r = &runs[run_depth - 1];
    if (r->starting) {
        r->starting = false;
        return &off;
    }
    flush_pending(r);
    int s = pr_xstatement;
    r->p_function = pr_xfunction ? (int)(pr_xfunction - pr_functions) : 0;
    r->p_statement = s;
    r->p_opcode = 0;
    r->p_builtin = -1;
    if (s >= 0 && s < progs->numstatements) {
        r->p_opcode = pr_statements[s].op;
        r->p_builtin = builtin_of(&pr_statements[s]);
    }
    r->pending = true;
    return &off;
}

void qs_run(func_t fnum) {
    if (run_depth == QS_MAX_RUNS)
        Sys_Error("vm-oracle: runs nested too deeply");
    run_t *r = &runs[run_depth++];
    r->function = fnum;
    r->starting = true;
    r->pending = false;
    qs_trace_run(fnum);
    PR_ExecuteProgram(fnum);
    flush_pending(r);
    qs_trace_exit();
    run_depth--;
}

// --- Errors (TRACE-4.6 to TRACE-4.9, DIFF-8.1, DIFF-8.2) ---

static void strip_newlines(char *text) {
    size_t n = strlen(text);
    while (n > 0 && text[n - 1] == '\n')
        text[--n] = 0;
}

static void fail(int kind, char *message) {
    int function = 0;
    int statement = -1;
    strip_newlines(message);
    if (run_depth > 0) {
        run_t *r = &runs[run_depth - 1];
        if (r->starting) {
            function = r->function;
        } else {
            if (strcmp(message, "runaway loop error") == 0)
                flush_pending(r);
            function = pr_xfunction ? (int)(pr_xfunction - pr_functions) : 0;
            statement = pr_xstatement;
        }
    }
    qs_trace_error(function, statement, message);
    run_depth = 0;
    if (!qs_catch) {
        fprintf(stderr, "vm-oracle: %s\n", message);
        qs_trace_close();
        exit(QS_EXIT_IO);
    }
    longjmp(*qs_catch, kind);
}

// PR_RunError prints its message and then calls Host_Error ("Program error"); the error
// record carries the message (TRACE-4.8). Any other Host_Error carries its own text.
void Host_Error(char *error, ...) {
    static char *text;
    static size_t size;
    va_list args;
    va_start(args, error);
    vformat(&text, &size, error, args);
    va_end(args);
    if (strcmp(text, "Program error") == 0 && console)
        fail(QS_ERROR_HOST, console);
    fail(QS_ERROR_HOST, text);
}

void qs_host_error(const char *message) {
    static char *text;
    static size_t size;
    size_t n = strlen(message) + 1;
    if (n > size) {
        char *grown = realloc(text, n);
        if (!grown) {
            fprintf(stderr, "vm-oracle: out of memory\n");
            exit(QS_EXIT_IO);
        }
        text = grown;
        size = n;
    }
    memcpy(text, message, n);
    fail(QS_ERROR_HOST, text);
}

void Sys_Error(char *error, ...) {
    static char *text;
    static size_t size;
    va_list args;
    va_start(args, error);
    vformat(&text, &size, error, args);
    va_end(args);
    fail(QS_ERROR_FATAL, text);
}

// The parts of pr_edict.c that pr_exec.c's error printing calls. The console is not
// compared, so they print nothing.
char *PR_GlobalString(int ofs) {
    (void)ofs;
    return "";
}

char *PR_GlobalStringNoContents(int ofs) {
    (void)ofs;
    return "";
}

void ED_Print(edict_t *ed) {
    (void)ed;
}

// --- Loading (DIFF-3.1) ---

// PROGS-4 and PROGS-6: CRC-16 with polynomial 0x1021, initial value 0xffff, no final XOR.
unsigned qs_crc16(const byte *data, int size) {
    unsigned crc = 0xffff;
    for (int i = 0; i < size; i++) {
        crc ^= (unsigned)data[i] << 8;
        for (int bit = 0; bit < 8; bit++)
            crc = (crc & 0x8000) ? ((crc << 1) ^ 0x1021) & 0xffff : (crc << 1) & 0xffff;
    }
    return crc;
}

static qboolean table_fits(int size, int ofs, int count, int record) {
    if (ofs < 0 || count < 0)
        return false;
    long long end = (long long)ofs + (long long)count * record;
    return end <= size;
}

#define QS_MAX_PROGS_SIZE (64L << 20)

// EDICT-3.5: the most field slots whose LIMIT_MAX_EDICTS edicts of EDICT-1.1's size (a 96-byte
// header) fit in 2147483647 bytes, (2147483647 / 600 - 96) / 4.
#define QS_MAX_ENTITYFIELDS ((INT_MAX / MAX_EDICTS - 96) / 4)

// Loads a progs as id's PR_LoadProgs does, into the arena, on a little-endian host, and checks
// it with qs_check_progs. Returns NULL or why the progs is refused.
const char *qs_load_progs(const char *path, qboolean check_crc, qs_progs_file_t *out) {
    FILE *f = fopen(path, "rb");
    if (!f)
        return "cannot open the progs";
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return "cannot read the progs";
    }
    long size = ftell(f);
    if (size < (long)sizeof(dprograms_t) || size > QS_MAX_PROGS_SIZE) {
        fclose(f);
        return "the progs is too short or too large";
    }
    rewind(f);
    byte *file = qs_alloc((size_t)size);
    if (fread(file, 1, (size_t)size, f) != (size_t)size) {
        fclose(f);
        return "cannot read the progs";
    }
    fclose(f);
    return qs_check_progs(file, size, check_crc, out);
}

// Checks the progs of `size` bytes at `file`, which lies in the arena, and makes it the one
// the VM runs. id checks only the version and, when check_crc is set, the header CRC; this also
// refuses a file whose tables lie outside it, which id would read past (PROGS-15), and with
// check_crc a progs too small for the system globals and fields (EDICT-2.4, EDICT-3.3).
// Returns NULL or why the progs is refused.
const char *qs_check_progs(byte *file, long size, qboolean check_crc, qs_progs_file_t *out) {
    if (size < (long)sizeof(dprograms_t) || size > QS_MAX_PROGS_SIZE)
        return "the progs is too short or too large";
    out->file = file;
    out->size = (int)size;

    pr_crc = (unsigned short)qs_crc16(file, (int)size);
    progs = (dprograms_t *)file;
    if (progs->version != PROG_VERSION)
        return "progs.dat has wrong version number";
    if (check_crc && progs->crc != PROGHEADER_CRC)
        return "progs.dat system vars have been modified, progdefs.h is out of date";
    int s = (int)size;
    if (!table_fits(s, progs->ofs_statements, progs->numstatements, sizeof(dstatement_t)) ||
        !table_fits(s, progs->ofs_globaldefs, progs->numglobaldefs, sizeof(ddef_t)) ||
        !table_fits(s, progs->ofs_fielddefs, progs->numfielddefs, sizeof(ddef_t)) ||
        !table_fits(s, progs->ofs_functions, progs->numfunctions, sizeof(dfunction_t)) ||
        !table_fits(s, progs->ofs_strings, progs->numstrings, 1) ||
        !table_fits(s, progs->ofs_globals, progs->numglobals, 4) || progs->numstrings < 1 ||
        progs->ofs_globals % 4 != 0 || progs->ofs_statements % 2 != 0 ||
        progs->ofs_functions % 4 != 0 || progs->ofs_globaldefs % 4 != 0 ||
        progs->ofs_fielddefs % 4 != 0 || progs->entityfields < 0)
        return "a table of the progs lies outside the file";
    if (check_crc && progs->numglobals < 92)
        return "the progs has fewer than 92 global slots";
    if (check_crc && progs->entityfields < 105)
        return "the progs has fewer than 105 field slots";
    if (progs->entityfields > QS_MAX_ENTITYFIELDS)
        return "the progs has more than 894760 field slots";

    pr_functions = (dfunction_t *)(file + progs->ofs_functions);
    pr_strings = (char *)file + progs->ofs_strings;
    pr_globaldefs = (ddef_t *)(file + progs->ofs_globaldefs);
    pr_fielddefs = (ddef_t *)(file + progs->ofs_fielddefs);
    pr_statements = (dstatement_t *)(file + progs->ofs_statements);
    pr_global_struct = (globalvars_t *)(file + progs->ofs_globals);
    pr_globals = (float *)pr_global_struct;
    pr_edict_size = progs->entityfields * 4 + (int)(sizeof(edict_t) - sizeof(entvars_t));
    return NULL;
}

void qs_alloc_edicts(int max_edicts) {
    sv.max_edicts = max_edicts;
    sv.edicts = qs_alloc((size_t)max_edicts * (size_t)pr_edict_size);
}

// --- Argument checks for builtins (BUILTIN-2.5, BUILTIN-2.7 to BUILTIN-2.9, BUILTIN-8.3) ---

// EDICT-1.9: an entity value names an edict when it is a whole number of edicts from the start
// of the array, inside the LIMIT_MAX_EDICTS edicts the server allocates.
edict_t *qs_edict(int value) {
    if (value < 0 || value % pr_edict_size != 0 || value / pr_edict_size >= sv.max_edicts)
        PR_RunError("invalid entity");
    return (edict_t *)((byte *)sv.edicts + value);
}

// BUILTIN-2.8: a string value is an offset into the string table or exactly the value of an
// engine string (EDICT-13.1).
char *qs_string(int value) {
    if ((value < 0 || value >= progs->numstrings) && !qs_is_engine_string(value))
        PR_RunError("invalid string");
    return pr_strings + value;
}

char *qs_field_string(edict_t *ed, int field) {
    if (field < 0 || field >= progs->entityfields)
        PR_RunError("invalid field");
    return qs_string(((int *)&ed->v)[field]);
}

// Text id's code made as `pr_strings + value`: every value is a 32-bit offset in the arena
// (oracle.h), so the difference gives the value back.
char *qs_checked_text(char *text) {
    return qs_string((int)(text - pr_strings));
}

// --- Crashes ---

#ifdef QS_COVERAGE
// The coverage build's runtime (clang's profile library) writes its profile at exit, which
// _Exit skips; a crash writes it here instead (oracles/README.md, Readiness).
int __llvm_profile_write_file(void);
#endif

#ifndef QS_FUZZ
static void on_crash(int sig) {
    qs_trace_flush_on_crash();
    fprintf(stderr, "vm-oracle: crashed with signal %d (id's code hit undefined behaviour)\n",
            sig);
#ifdef QS_COVERAGE
    __llvm_profile_write_file();
#endif
    _Exit(4);
}
#endif

// --- Vector mode: the environment of docs/spec/vectors/vm/README.md ---

// The numbers of the init file (oracles/README.md): integers as differential.md DIFF-2.7
// writes them, in the range their line takes and in an int.
static qboolean parse_int(const char *text, long long min, long long max, int *out) {
    long long value;
    if (min < INT_MIN)
        min = INT_MIN;
    if (max > INT_MAX)
        max = INT_MAX;
    if (!qs_parse_integer(text, min, max, &value))
        return false;
    *out = (int)value;
    return true;
}

// `bits <hex>`: one to eight hexadecimal digits, the value's 32 bits.
static qboolean parse_bits(const char *text, int *out) {
    size_t n = strlen(text);
    if (n < 1 || n > 8)
        return false;
    unsigned bits = 0;
    for (size_t i = 0; i < n; i++) {
        char c = text[i];
        unsigned digit;
        if (c >= '0' && c <= '9')
            digit = (unsigned)(c - '0');
        else if (c >= 'a' && c <= 'f')
            digit = (unsigned)(c - 'a' + 10);
        else if (c >= 'A' && c <= 'F')
            digit = (unsigned)(c - 'A' + 10);
        else
            return false;
        bits = bits << 4 | digit;
    }
    memcpy(out, &bits, sizeof(*out));
    return true;
}

// An entity or a pointer: an edict number and a field slot, each at least 0, whose value
// fits in an int.
static qboolean parse_place(const char *edict, const char *field, int *out) {
    int e;
    int f = 0;
    if (!parse_int(edict, 0, INT_MAX, &e) || (field && !parse_int(field, 0, INT_MAX, &f)))
        return false;
    long long value = (long long)e * pr_edict_size;
    if (field)
        value += (long long)offsetof(edict_t, v) + 4LL * f;
    if (value > INT_MAX)
        return false;
    *out = (int)value;
    return true;
}

static qboolean parse_value(int argc, char **argv, int *out) {
    if (argc == 2 && strcmp(argv[0], "bits") == 0)
        return parse_bits(argv[1], out);
    if (argc == 2 && strcmp(argv[0], "entity") == 0)
        return parse_place(argv[1], NULL, out);
    if (argc == 3 && strcmp(argv[0], "pointer") == 0)
        return parse_place(argv[1], argv[2], out);
    return false;
}

// Splits a line into at most `max` words; -1 when it has more.
static int split(char *line, char **words, int max) {
    int n = 0;
    for (char *w = strtok(line, " \t\r"); w; w = strtok(NULL, " \t\r")) {
        if (n == max)
            return -1;
        words[n++] = w;
    }
    return n;
}

// Copies the next line of the init file into `text`; returns 0 at the end of the file and -1
// for a line too long for `text`.
static int next_line(const char **cursor, const char *stop, char *text, size_t size) {
    const char *start = *cursor;
    if (start >= stop)
        return 0;
    const char *end = memchr(start, '\n', (size_t)(stop - start));
    if (!end)
        end = stop;
    *cursor = end < stop ? end + 1 : stop;
    size_t n = (size_t)(end - start);
    if (n >= size)
        return -1;
    memcpy(text, start, n);
    text[n] = 0;
    return 1;
}

// What the host does, in order (the vectors' README, "Host actions"). Without any, it runs
// the entry function once.
typedef enum {
    HOST_CLOCK,
    HOST_RUN,
    HOST_THINK,
    HOST_PUSH,
    HOST_PHYSICS,
    HOST_FRAME,
    HOST_EDICT,
} host_kind_t;

typedef struct {
    host_kind_t kind;
    double seconds;
    int a;
    int b;
} host_action_t;

#define QS_MAX_HOST_ACTIONS 256
static host_action_t host_actions[QS_MAX_HOST_ACTIONS];
static int num_host_actions;

static edict_t *vector_edict(int n) {
    return (edict_t *)((byte *)sv.edicts + (size_t)n * (size_t)pr_edict_size);
}

static void run_host_action(const host_action_t *action) {
    switch (action->kind) {
    case HOST_CLOCK:
        sv.time = action->seconds;
        break;
    case HOST_RUN:
        qs_run(action->a);
        break;
    case HOST_THINK:
        host_frametime = action->seconds;
        SV_RunThink(vector_edict(action->a));
        break;
    case HOST_PUSH:
        host_frametime = action->seconds;
        SV_Physics_Pusher(vector_edict(action->a));
        break;
    case HOST_PHYSICS:
        for (int i = 0; i < action->a; i++)
            qs_physics(action->seconds);
        break;
    case HOST_FRAME:
        for (int i = 0; i < action->a; i++)
            qs_server_frame(action->seconds, 0);
        break;
    case HOST_EDICT:
        ((int *)pr_globals)[action->b] = (int)EDICT_TO_PROG(EDICT_NUM(action->a));
        break;
    }
}

// The actions are in a function of their own so that no local variable lives across setjmp.
static int next_host_action;

static void run_host_actions(void) {
    static jmp_buf catch_point;
    if (!setjmp(catch_point)) {
        qs_catch = &catch_point;
        for (next_host_action = 0; next_host_action < num_host_actions; next_host_action++)
            run_host_action(&host_actions[next_host_action]);
    }
    qs_catch = NULL;
}

// `<seconds>`: a number as differential.md DIFF-2.8 writes it, read as the nearest double.
static qboolean parse_seconds(const char *text, double *out) {
    if (!qs_decimal_syntax(text))
        return false;
    *out = strtod(text, NULL);
    return true;
}

static qboolean parse_host_action(char **w, int n, int edicts, host_action_t *out) {
    if (n == 3 && strcmp(w[1], "clock") == 0) {
        out->kind = HOST_CLOCK;
        return parse_seconds(w[2], &out->seconds);
    }
    if (n == 3 && strcmp(w[1], "run") == 0) {
        out->kind = HOST_RUN;
        return parse_int(w[2], INT_MIN, INT_MAX, &out->a);
    }
    if (n == 4 && (strcmp(w[1], "think") == 0 || strcmp(w[1], "push") == 0)) {
        out->kind = strcmp(w[1], "think") == 0 ? HOST_THINK : HOST_PUSH;
        return parse_int(w[2], 0, edicts - 1, &out->a) && parse_seconds(w[3], &out->seconds);
    }
    if (n == 4 && (strcmp(w[1], "physics") == 0 || strcmp(w[1], "frame") == 0)) {
        out->kind = strcmp(w[1], "physics") == 0 ? HOST_PHYSICS : HOST_FRAME;
        return parse_int(w[3], 1, INT_MAX, &out->a) && parse_seconds(w[2], &out->seconds);
    }
    if (n == 4 && strcmp(w[1], "edict") == 0) {
        out->kind = HOST_EDICT;
        return parse_int(w[2], INT_MIN, INT_MAX, &out->a) &&
               parse_int(w[3], 0, progs->numglobals - 1, &out->b);
    }
    return false;
}

// DIFF-2.2: maxclients is at most 16.
#define QS_MAX_VECTOR_CLIENTS 16
static client_t vector_clients[QS_MAX_VECTOR_CLIENTS];

// Reads the init file of vector mode (oracles/README.md) from the `size` bytes at `data`, for
// the progs already loaded: sets up the edicts and client slots, then applies each value and
// collects the host actions. Returns QS_EXIT_TRACED, or QS_EXIT_REFUSED after saying why.
int qs_parse_init(const char *init_path, const char *data, long size, qs_vector_setup_t *out) {
    // A NUL byte would end the C string of its line early and hide the rest of it.
    const char *nul = memchr(data, 0, (size_t)size);
    if (nul) {
        int nul_line = 1;
        for (const char *p = data; p < nul; p++)
            nul_line += *p == '\n';
        fprintf(stderr, "vm-oracle: %s:%d: a NUL byte\n", init_path, nul_line);
        return QS_EXIT_REFUSED;
    }
    const char *stop = data + size;
    const char *cursor = data;
    char text[512];
    char *w[8];
    int got;
    int line = 0;
    int edicts = 3;
    int maxclients = 0;
    out->entry = 1;
    out->builtins = qs_vector_builtins;
    num_host_actions = 0;
    sv.state = ss_active;
    // The edicts and client slots are set up first, then the values are applied.
    while ((got = next_line(&cursor, stop, text, sizeof(text))) != 0) {
        line++;
        int n = got < 0 ? -1 : split(text, w, 8);
        if (n < 0) {
            fprintf(stderr, "vm-oracle: %s:%d: line too long or more than eight words\n",
                    init_path, line);
            return QS_EXIT_REFUSED;
        }
        if (n > 0 && strcmp(w[0], "edicts") == 0) {
            if (n != 2 || !parse_int(w[1], 1, MAX_EDICTS, &edicts)) {
                fprintf(stderr, "vm-oracle: %s:%d: bad edict count\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
        } else if (n > 0 && strcmp(w[0], "maxclients") == 0) {
            if (n != 2 || !parse_int(w[1], 0, QS_MAX_VECTOR_CLIENTS, &maxclients)) {
                fprintf(stderr, "vm-oracle: %s:%d: bad client count\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
        }
    }
    // The array has all LIMIT_MAX_EDICTS edicts, as the server's does (EDICT-1.4); `edicts`
    // of them are in use. No client slot has a player.
    qs_alloc_edicts(MAX_EDICTS);
    sv.num_edicts = edicts;
    svs.maxclients = maxclients;
    svs.clients = vector_clients;
    for (int i = 0; i < maxclients; i++)
        svs.clients[i].edict = vector_edict(i + 1);
    qs_init_buffers();
    cursor = data;
    line = 0;
    while (next_line(&cursor, stop, text, sizeof(text)) != 0) {
        line++;
        int n = split(text, w, 8);
        if (n == 0 || strcmp(w[0], "edicts") == 0 || strcmp(w[0], "maxclients") == 0)
            continue;
        int slot;
        int e;
        int field;
        int value;
        long long seed;
        if (n == 1 && strcmp(w[0], "loading") == 0) {
            sv.state = ss_loading;
        } else if (n == 2 && strcmp(w[0], "builtins") == 0 && strcmp(w[1], "engine") == 0) {
            out->builtins = qs_scenario_builtins;
        } else if (n == 2 && strcmp(w[0], "entry") == 0) {
            if (!parse_int(w[1], INT_MIN, INT_MAX, &out->entry)) {
                fprintf(stderr, "vm-oracle: %s:%d: bad entry\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
        } else if (n == 2 && strcmp(w[0], "seed") == 0) {
            if (!qs_parse_integer(w[1], 0, 4294967295LL, &seed)) {
                fprintf(stderr, "vm-oracle: %s:%d: bad seed\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
            qs_set_random_state((unsigned)seed);
        } else if (n >= 2 && strcmp(w[0], "host") == 0) {
            if (num_host_actions == QS_MAX_HOST_ACTIONS ||
                !parse_host_action(w, n, edicts, &host_actions[num_host_actions])) {
                fprintf(stderr, "vm-oracle: %s:%d: bad host action\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
            num_host_actions++;
        } else if (n >= 4 && strcmp(w[0], "global") == 0) {
            if (!parse_int(w[1], 0, progs->numglobals - 1, &slot)) {
                fprintf(stderr, "vm-oracle: %s:%d: bad global\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
            if (!parse_value(n - 2, w + 2, &value)) {
                fprintf(stderr, "vm-oracle: %s:%d: bad value\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
            ((int *)pr_globals)[slot] = value;
        } else if (n >= 5 && strcmp(w[0], "field") == 0) {
            if (!parse_int(w[1], 0, edicts - 1, &e) ||
                !parse_int(w[2], 0, progs->entityfields - 1, &field)) {
                fprintf(stderr, "vm-oracle: %s:%d: bad field\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
            if (!parse_value(n - 3, w + 3, &value)) {
                fprintf(stderr, "vm-oracle: %s:%d: bad value\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
            ((int *)&vector_edict(e)->v)[field] = value;
        } else {
            fprintf(stderr, "vm-oracle: %s:%d: unknown line\n", init_path, line);
            return QS_EXIT_REFUSED;
        }
    }
    if (num_host_actions == 0) {
        host_actions[0].kind = HOST_RUN;
        host_actions[0].a = out->entry;
        num_host_actions = 1;
    }
    return QS_EXIT_TRACED;
}

int qs_vector_main(const char *progs_path, const char *init_path, const char *trace_path) {
    qs_progs_file_t file;
    const char *refused = qs_load_progs(progs_path, false, &file);
    if (refused) {
        fprintf(stderr, "vm-oracle: %s: %s\n", progs_path, refused);
        return QS_EXIT_REFUSED;
    }
    long size;
    char *data = qs_read_file(init_path, &size);
    if (!data) {
        fprintf(stderr, "vm-oracle: cannot open %s\n", init_path);
        return QS_EXIT_REFUSED;
    }
    qs_vector_setup_t setup;
    int parsed = qs_parse_init(init_path, data, size, &setup);
    free(data);
    if (parsed != QS_EXIT_TRACED)
        return parsed;

    pr_builtins = setup.builtins;
    pr_numbuiltins = QS_NUM_BUILTINS;
    if (!qs_trace_open(trace_path)) {
        fprintf(stderr, "vm-oracle: cannot write %s\n", trace_path);
        return QS_EXIT_IO;
    }
    qs_trace_header(pr_crc, progs->numstrings);
    run_host_actions();
    printf("depth=%d localstack=%d\n", pr_depth, localstack_used);
    printf("datagram=");
    for (int i = 0; i < sv.datagram.cursize; i++)
        printf("%02x", sv.datagram.data[i]);
    printf("\n");
    if (!qs_trace_close()) {
        fprintf(stderr, "vm-oracle: cannot write %s\n", trace_path);
        return QS_EXIT_IO;
    }
    return QS_EXIT_TRACED;
}

// A fuzz target (oracles/vm/fuzz.c) brings libFuzzer's main instead.
#ifndef QS_FUZZ
static const char usage[] =
    "usage: vm-oracle [--console] scenario <scenario> <trace>\n"
    "       vm-oracle [--console] vector <progs.dat> <init> <trace>\n";

int main(int argc, char **argv) {
    if (!qs_arena_init()) {
        fprintf(stderr, "vm-oracle: out of memory\n");
        return QS_EXIT_IO;
    }
    signal(SIGSEGV, on_crash);
    signal(SIGILL, on_crash);
    signal(SIGFPE, on_crash);
    int first = 1;
    if (argc > 1 && strcmp(argv[1], "--console") == 0) {
        echo_console = true;
        first = 2;
    }
    int rest = argc - first;
    if (rest == 3 && strcmp(argv[first], "scenario") == 0)
        return qs_scenario_main(argv[first + 1], argv[first + 2]);
    if (rest == 4 && strcmp(argv[first], "vector") == 0)
        return qs_vector_main(argv[first + 1], argv[first + 2], argv[first + 3]);
    fputs(usage, stderr);
    return QS_EXIT_REFUSED;
}
#endif
