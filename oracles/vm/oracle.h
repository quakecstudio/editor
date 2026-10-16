// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The oracle's own interfaces: the memory arena, engine strings, the trace writer, the run
// wrapper and the error paths. Everything here is this project's code, written from
// docs/spec/trace.md and docs/spec/differential.md.

#ifndef QS_ORACLE_H
#define QS_ORACLE_H

#include "quakedef.h"

// The host's exit codes: 0 when the run was traced (QuakeC errors included), 2 for a bad
// command line, scenario or progs (nothing traced), 3 for an I/O failure.
#define QS_EXIT_TRACED 0
#define QS_EXIT_REFUSED 2
#define QS_EXIT_IO 3

// One arena holds the progs, the edicts and every engine string, so every string value is a
// 32-bit offset from pr_strings on a 64-bit host too (EDICT-13.1).
void *qs_alloc(size_t size);

// Engine strings (EDICT-13): text outside the string table. The trace writer emits a string
// record for each before the next change-carrying record, and again whenever its text
// changes (TRACE-4.10).
char *qs_engine_string(const char *text);
void qs_engine_string_register(char *text);
void qs_engine_string_touched(char *text);
qboolean qs_is_engine_string(int value);
void qs_emit_string_records(void);

// The trace writer (trace.md TRACE-5): the binary form, with change sets computed against a
// snapshot of the globals and the fields of every edict in use.
qboolean qs_trace_open(const char *path);
void qs_trace_header(unsigned progs_crc, int numstrings);
void qs_trace_run(int function);
void qs_trace_step(int function, int statement, int opcode, int builtin);
void qs_trace_exit(void);
void qs_trace_error(int function, int statement, const char *message);
void qs_trace_string(int value, const char *text);
qboolean qs_trace_close(void);
void qs_trace_flush_on_crash(void);

// The run wrapper (host.c): every call into QuakeC, from the host or a builtin.
void qs_run(func_t fnum);

// Errors. A runtime error is id's PR_RunError; a host error (BUILTIN-3.2) and a fatal error
// (BUILTIN-3.3) end every run. Each writes its error record, then returns to the host's
// innermost qs_catch point with the kind.
#define QS_ERROR_HOST 1
#define QS_ERROR_FATAL 2
void qs_host_error(const char *message);
extern jmp_buf *qs_catch;

// The builtin tables (builtins.c).
extern builtin_t qs_scenario_builtins[];
extern builtin_t qs_vector_builtins[];
#define QS_NUM_BUILTINS 79

// C's float-to-int conversion as builtins.md BUILTIN-2.1 and BUILTIN-2.2 fix it: truncation
// toward zero, and -2147483648 for a NaN or a value outside the int range, which is what x86
// gives. The oracle's own code uses it so that it agrees on every host; id's code keeps C's
// conversion (DIFF-1.3).
static inline int qs_ftoi(float f) {
    double d = f;
    if (d != d || d >= 2147483648.0 || d <= -2147483649.0)
        return (int)(-2147483647 - 1);
    return (int)d;
}

// Reading the oracle's input files (scenario.c): a whole file, and the integer and number
// syntax of differential.md DIFF-2.7 and DIFF-2.8, which vector mode's init file shares.
char *qs_read_file(const char *path, long *size_out);
qboolean qs_parse_integer(const char *text, long long min, long long max, long long *out);
qboolean qs_decimal_syntax(const char *text);

// The mock engine (engine.c): settings, the random state and model bounds of
// differential.md DIFF-2.4, DIFF-6.2 and DIFF-6.3.
void qs_set_cvar(const char *name, const char *text);
void qs_set_random_state(unsigned state);
void qs_add_model_bounds(const char *name, const float *mins, const float *maxs);
void qs_init_buffers(void);

// Loading (host.c).
typedef struct {
    byte *file;
    int size;
} qs_progs_file_t;
const char *qs_load_progs(const char *path, qboolean check_crc, qs_progs_file_t *out);
unsigned qs_crc16(const byte *data, int size);
void qs_alloc_edicts(int max_edicts);

// Server frames (scenario.c): a physics frame (DIFF-5.3) and a server frame (DIFF-5.1).
void qs_physics(double frametime);
void qs_server_frame(double frametime, long frame);

// Entry points.
int qs_scenario_main(const char *scenario_path, const char *trace_path);
int qs_vector_main(const char *progs_path, const char *init_path, const char *trace_path);

#endif
