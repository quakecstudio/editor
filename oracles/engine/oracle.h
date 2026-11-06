// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The engine oracle's own interfaces, over id's own headers (WinQuake/quakedef.h, fetched by
// checksum): the exit codes, the engine strings the trace writer reports, and the trace writer
// it shares with the VM oracle (oracles/trace/trace.c, trace.md TRACE-5).

#ifndef QS_ORACLE_H
#define QS_ORACLE_H

#include <stddef.h>

#include "quakedef.h"

// The exit codes, as the VM oracle's: 0 when the run was traced (a QuakeC error included), 2
// for a refused command line or script, 3 for an I/O failure, 4 when id's code stopped the
// engine (Sys_Error).
#define QS_EXIT_TRACED 0
#define QS_EXIT_REFUSED 2
#define QS_EXIT_IO 3
#define QS_EXIT_ENGINE 4

// Writes a string record for every string value outside the string table whose text is new
// or has changed since its last record (TRACE-4.10).
void qs_emit_string_records(void);

// The trace writer (oracles/trace/trace.c).
qboolean qs_trace_open(const char *path);
void qs_trace_header(unsigned progs_crc, int numstrings);
void qs_trace_error(int function, int statement, const char *message);
void qs_trace_string(int value, const char *text);
void qs_trace_frame(unsigned frame);
void qs_trace_state(unsigned frame);
qboolean qs_trace_close(void);

#endif
