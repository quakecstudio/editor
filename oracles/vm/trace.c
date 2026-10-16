// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The trace writer: docs/spec/trace.md's binary form (TRACE-5), written from the spec alone,
// independently of qc-trace's Rust encoder, so that decoding this program's output with
// qc-trace checks both. Change sets (TRACE-3) are computed by comparing the globals and the
// fields of every edict in use with a snapshot taken at the previous change-carrying record.

#include "oracle.h"

#define TRACE_VERSION 1
#define TRACE_TAG_RUN 0x01
#define TRACE_TAG_STEP 0x02
#define TRACE_TAG_EXIT 0x03
#define TRACE_TAG_ERROR 0x04
#define TRACE_TAG_STRING 0x05

typedef struct {
    byte *data;
    size_t size;
    size_t used;
} buffer_t;

static FILE *trace_file;
static buffer_t out;
static buffer_t entries;
static qboolean write_failed;

static int *snap_globals;
static int *snap_fields;
static int snap_numglobals;
static int snap_entityfields;

static void reserve(buffer_t *b, size_t more) {
    size_t need = b->used + more;
    if (need <= b->size)
        return;
    size_t size = b->size ? b->size : 65536;
    while (size < need)
        size *= 2;
    byte *data = realloc(b->data, size);
    if (!data) {
        fprintf(stderr, "vm-oracle: out of memory\n");
        exit(QS_EXIT_IO);
    }
    b->data = data;
    b->size = size;
}

static void put_byte(buffer_t *b, unsigned value) {
    reserve(b, 1);
    b->data[b->used++] = (byte)value;
}

// TRACE-5.1: a word is 4 bytes, little-endian.
static void put_word(buffer_t *b, unsigned value) {
    put_byte(b, value & 0xff);
    put_byte(b, (value >> 8) & 0xff);
    put_byte(b, (value >> 16) & 0xff);
    put_byte(b, (value >> 24) & 0xff);
}

// TRACE-5.1, TRACE-5.2: LEB128 in the fewest bytes.
static void put_uvar(buffer_t *b, unsigned value) {
    while (value >= 0x80) {
        put_byte(b, (value & 0x7f) | 0x80);
        value >>= 7;
    }
    put_byte(b, value);
}

// TRACE-5.1: the zigzag form (n << 1) ^ (n >> 31), with an arithmetic shift.
static void put_svar(buffer_t *b, int value) {
    unsigned u = (unsigned)value;
    unsigned sign = value < 0 ? 0xffffffffu : 0;
    put_uvar(b, (u << 1) ^ sign);
}

// TRACE-5.3: a uvar byte count, then the bytes; text never holds a NUL.
static void put_text(buffer_t *b, const char *text) {
    size_t length = strlen(text);
    put_uvar(b, (unsigned)length);
    reserve(b, length);
    memcpy(b->data + b->used, text, length);
    b->used += length;
}

static void flush_out(void) {
    if (!trace_file || out.used == 0)
        return;
    if (fwrite(out.data, 1, out.used, trace_file) != out.used)
        write_failed = true;
    out.used = 0;
}

static void end_record(void) {
    if (out.used >= (1u << 20))
        flush_out();
}

qboolean qs_trace_open(const char *path) {
    trace_file = fopen(path, "wb");
    return trace_file != NULL;
}

static int *field_row(int edict) {
    byte *base = (byte *)sv.edicts + (size_t)edict * (size_t)pr_edict_size;
    return (int *)&((edict_t *)base)->v;
}

// TRACE-2, TRACE-5.4. Also takes the snapshot the first change set compares against
// (TRACE-3.1): the globals as loaded and every field slot of every edict.
void qs_trace_header(unsigned progs_crc, int numstrings) {
    put_byte(&out, 'Q');
    put_byte(&out, 'C');
    put_byte(&out, 'T');
    put_byte(&out, 'R');
    put_word(&out, TRACE_VERSION);
    put_word(&out, progs_crc);
    put_word(&out, (unsigned)pr_edict_size);
    put_word(&out, (unsigned)offsetof(edict_t, v));
    put_word(&out, (unsigned)progs->entityfields);
    put_word(&out, (unsigned)numstrings);
    end_record();

    snap_numglobals = progs->numglobals;
    snap_entityfields = progs->entityfields;
    snap_globals = calloc((size_t)snap_numglobals + 1, sizeof(int));
    snap_fields = calloc((size_t)sv.max_edicts * (size_t)snap_entityfields + 1, sizeof(int));
    if (!snap_globals || !snap_fields) {
        fprintf(stderr, "vm-oracle: out of memory\n");
        exit(QS_EXIT_IO);
    }
    memcpy(snap_globals, pr_globals, (size_t)snap_numglobals * sizeof(int));
    for (int n = 0; n < sv.max_edicts; n++)
        memcpy(snap_fields + (size_t)n * (size_t)snap_entityfields, field_row(n),
               (size_t)snap_entityfields * sizeof(int));
}

// TRACE-3, TRACE-5.5. Slots are compared as 32-bit patterns. The fields compared are those of
// edicts 0 to num_edicts - 1 (TRACE-3.4); a row of the snapshot beyond the edicts in use is
// never updated, so it still holds the zeros the edicts were allocated with, and an edict that
// comes into use is compared against all-zero fields.
static void put_changes(void) {
    const int *globals = (const int *)pr_globals;
    unsigned count = 0;
    entries.used = 0;
    for (int slot = 0; slot < snap_numglobals; slot++) {
        if (globals[slot] != snap_globals[slot]) {
            put_uvar(&entries, (unsigned)slot);
            put_word(&entries, (unsigned)globals[slot]);
            snap_globals[slot] = globals[slot];
            count++;
        }
    }
    put_uvar(&out, count);
    reserve(&out, entries.used);
    memcpy(out.data + out.used, entries.data, entries.used);
    out.used += entries.used;

    count = 0;
    entries.used = 0;
    size_t row_bytes = (size_t)snap_entityfields * sizeof(int);
    for (int n = 0; n < sv.num_edicts && n < sv.max_edicts; n++) {
        const int *row = field_row(n);
        int *snap = snap_fields + (size_t)n * (size_t)snap_entityfields;
        if (memcmp(row, snap, row_bytes) == 0)
            continue;
        for (int field = 0; field < snap_entityfields; field++) {
            if (row[field] != snap[field]) {
                put_uvar(&entries, (unsigned)n);
                put_uvar(&entries, (unsigned)field);
                put_word(&entries, (unsigned)row[field]);
                snap[field] = row[field];
                count++;
            }
        }
    }
    put_uvar(&out, count);
    reserve(&out, entries.used);
    memcpy(out.data + out.used, entries.data, entries.used);
    out.used += entries.used;
}

// TRACE-4.10: string records come before the change-carrying record that may first show the
// value.
static void begin_change_record(unsigned tag) {
    qs_emit_string_records();
    put_byte(&out, tag);
}

void qs_trace_run(int function) {
    begin_change_record(TRACE_TAG_RUN);
    put_svar(&out, function);
    put_changes();
    end_record();
}

void qs_trace_step(int function, int statement, int opcode, int builtin) {
    begin_change_record(TRACE_TAG_STEP);
    put_svar(&out, function);
    put_svar(&out, statement);
    put_uvar(&out, (unsigned)opcode);
    if (builtin >= 0) {
        put_byte(&out, 0x01);
        put_uvar(&out, (unsigned)builtin);
    } else {
        put_byte(&out, 0x00);
    }
    put_changes();
    end_record();
}

void qs_trace_exit(void) {
    begin_change_record(TRACE_TAG_EXIT);
    put_changes();
    end_record();
}

void qs_trace_error(int function, int statement, const char *message) {
    begin_change_record(TRACE_TAG_ERROR);
    put_svar(&out, function);
    put_svar(&out, statement);
    put_text(&out, message);
    put_changes();
    end_record();
}

void qs_trace_string(int value, const char *text) {
    put_byte(&out, TRACE_TAG_STRING);
    put_word(&out, (unsigned)value);
    put_text(&out, text);
    end_record();
}

qboolean qs_trace_close(void) {
    flush_out();
    if (trace_file && fclose(trace_file) != 0)
        write_failed = true;
    trace_file = NULL;
    return !write_failed;
}

// Called when the oracle crashes on id's undefined behaviour (differential.md DIFF-10.1):
// the records written so far still reach the file, so the comparison can run up to them.
void qs_trace_flush_on_crash(void) {
    flush_out();
    if (trace_file)
        fflush(trace_file);
}
