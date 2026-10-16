// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The VM oracle's host: the command line, the memory arena, loading a progs, the run wrapper
// and statement hook that write the trace around id's PR_ExecuteProgram, and the error paths
// that stand in for id's Host_Error and Sys_Error (differential.md DIFF-1, DIFF-8).

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

// --- Engine strings (EDICT-13, TRACE-4.10) ---

typedef struct {
    int value;
    char *last;
    qboolean emitted;
    qboolean dirty;
} engine_string_t;

static engine_string_t *engine_strings;
static int num_engine_strings;
static int max_engine_strings;
static int *dirty_strings;
static int num_dirty;

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

// Loads a progs as id's PR_LoadProgs does, into the arena, on a little-endian host. id checks
// only the version and, when check_crc is set, the header CRC; this loader also refuses a file
// whose tables lie outside it, which id would read past (PROGS-15), and with check_crc a progs
// too small for the system globals and fields (EDICT-2.4, EDICT-3.3). Returns NULL or why the
// progs is refused.
const char *qs_load_progs(const char *path, qboolean check_crc, qs_progs_file_t *out) {
    FILE *f = fopen(path, "rb");
    if (!f)
        return "cannot open the progs";
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return "cannot read the progs";
    }
    long size = ftell(f);
    if (size < (long)sizeof(dprograms_t) || size > (64L << 20)) {
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

// --- Argument checks for builtins (BUILTIN-2.3, BUILTIN-2.5, BUILTIN-8.3) ---

// An entity value names an edict when it is a whole number of edicts from the start of the
// array, inside the LIMIT_MAX_EDICTS edicts the server allocates (EDICT-1.4, EDICT-1.5).
edict_t *qs_edict(int value) {
    if (value < 0 || value % pr_edict_size != 0 || value / pr_edict_size >= sv.max_edicts)
        PR_RunError("invalid entity");
    return (edict_t *)((byte *)sv.edicts + value);
}

// A string value is an offset into the string table or an engine string (EDICT-13.1).
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

// --- Crashes ---

static void on_crash(int sig) {
    qs_trace_flush_on_crash();
    fprintf(stderr, "vm-oracle: crashed with signal %d (id's code hit undefined behaviour)\n",
            sig);
    _Exit(4);
}

// --- Vector mode: the environment of docs/spec/vectors/vm/README.md ---

static void parse_value(const char *path, int line, int argc, char **argv, int *out) {
    if (argc == 2 && strcmp(argv[0], "bits") == 0) {
        *out = (int)strtoul(argv[1], NULL, 16);
    } else if (argc == 2 && strcmp(argv[0], "entity") == 0) {
        *out = atoi(argv[1]) * pr_edict_size;
    } else if (argc == 3 && strcmp(argv[0], "pointer") == 0) {
        *out = atoi(argv[1]) * pr_edict_size + (int)offsetof(edict_t, v) + 4 * atoi(argv[2]);
    } else {
        fprintf(stderr, "vm-oracle: %s:%d: bad value\n", path, line);
        exit(QS_EXIT_REFUSED);
    }
}

static int split(char *line, char **words, int max) {
    int n = 0;
    for (char *w = strtok(line, " \t\r\n"); w && n < max; w = strtok(NULL, " \t\r\n"))
        words[n++] = w;
    return n;
}

int qs_vector_main(const char *progs_path, const char *init_path, const char *trace_path) {
    qs_progs_file_t file;
    const char *refused = qs_load_progs(progs_path, false, &file);
    if (refused) {
        fprintf(stderr, "vm-oracle: %s: %s\n", progs_path, refused);
        return QS_EXIT_REFUSED;
    }
    FILE *init = fopen(init_path, "r");
    if (!init) {
        fprintf(stderr, "vm-oracle: cannot open %s\n", init_path);
        return QS_EXIT_REFUSED;
    }
    char text[512];
    char *w[8];
    int line = 0;
    int entry = 1;
    int edicts = 3;
    sv.state = ss_active;
    // The edicts are allocated first, then the values are applied.
    long start = ftell(init);
    while (fgets(text, sizeof(text), init)) {
        int n = split(text, w, 8);
        if (n == 2 && strcmp(w[0], "edicts") == 0)
            edicts = atoi(w[1]);
    }
    if (edicts < 1 || edicts > MAX_EDICTS) {
        fprintf(stderr, "vm-oracle: %s: bad edict count\n", init_path);
        return QS_EXIT_REFUSED;
    }
    qs_alloc_edicts(edicts);
    sv.num_edicts = edicts;
    fseek(init, start, SEEK_SET);
    while (fgets(text, sizeof(text), init)) {
        line++;
        int n = split(text, w, 8);
        if (n == 0 || strcmp(w[0], "edicts") == 0)
            continue;
        if (n == 1 && strcmp(w[0], "loading") == 0) {
            sv.state = ss_loading;
        } else if (n == 2 && strcmp(w[0], "entry") == 0) {
            entry = atoi(w[1]);
        } else if (n >= 4 && strcmp(w[0], "global") == 0) {
            int slot = atoi(w[1]);
            if (slot < 0 || slot >= progs->numglobals) {
                fprintf(stderr, "vm-oracle: %s:%d: bad global\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
            parse_value(init_path, line, n - 2, w + 2, &((int *)pr_globals)[slot]);
        } else if (n >= 5 && strcmp(w[0], "field") == 0) {
            int e = atoi(w[1]);
            int field = atoi(w[2]);
            if (e < 0 || e >= edicts || field < 0 || field >= progs->entityfields) {
                fprintf(stderr, "vm-oracle: %s:%d: bad field\n", init_path, line);
                return QS_EXIT_REFUSED;
            }
            edict_t *ed = (edict_t *)((byte *)sv.edicts + (size_t)e * (size_t)pr_edict_size);
            parse_value(init_path, line, n - 3, w + 3, &((int *)&ed->v)[field]);
        } else {
            fprintf(stderr, "vm-oracle: %s:%d: unknown line\n", init_path, line);
            return QS_EXIT_REFUSED;
        }
    }
    fclose(init);

    pr_builtins = qs_vector_builtins;
    pr_numbuiltins = QS_NUM_BUILTINS;
    if (!qs_trace_open(trace_path)) {
        fprintf(stderr, "vm-oracle: cannot write %s\n", trace_path);
        return QS_EXIT_IO;
    }
    qs_trace_header(pr_crc, progs->numstrings);
    jmp_buf catch_point;
    if (!setjmp(catch_point)) {
        qs_catch = &catch_point;
        qs_run(entry);
    }
    qs_catch = NULL;
    printf("depth=%d localstack=%d\n", pr_depth, localstack_used);
    if (!qs_trace_close()) {
        fprintf(stderr, "vm-oracle: cannot write %s\n", trace_path);
        return QS_EXIT_IO;
    }
    return QS_EXIT_TRACED;
}

static const char usage[] =
    "usage: vm-oracle [--console] scenario <scenario> <trace>\n"
    "       vm-oracle [--console] vector <progs.dat> <init> <trace>\n";

int main(int argc, char **argv) {
    arena_size = QS_ARENA_SIZE;
    arena = calloc(1, arena_size);
    if (!arena) {
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
