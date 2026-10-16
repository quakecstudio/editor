// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// libFuzzer targets for the VM oracle's readers of files (oracles/README.md, Fuzzing): built by
// `cargo xtask oracles fuzz` with one of QS_FUZZ_SCENARIO (the scenario file, differential.md
// DIFF-2), QS_FUZZ_INIT (the init file of vector mode) or QS_FUZZ_PROGS (the progs loader,
// DIFF-3.1) defined, and with the rest of the oracle, whose main QS_FUZZ leaves out. Each input
// is read in this one process, so the readers set their state back to its defaults first.

#include <stdint.h>

#include "oracle.h"

int LLVMFuzzerInitialize(int *argc, char ***argv);
int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

int LLVMFuzzerInitialize(int *argc, char ***argv) {
    (void)argc;
    (void)argv;
    if (!qs_arena_init()) {
        fprintf(stderr, "vm-oracle fuzz: out of memory\n");
        abort();
    }
    return 0;
}

#if defined(QS_FUZZ_SCENARIO)

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    char *text = malloc(size + 1);
    if (!text)
        return 0;
    memcpy(text, data, size);
    text[size] = 0;
    qs_parse_scenario("fuzz/case.scn", text, (long)size);
    free(text);
    return 0;
}

#elif defined(QS_FUZZ_INIT)

// The progs every init file is read against: 128 globals, 110 fields, the null function and
// function 1, whose one statement is DONE, and a string table of the empty string.
static void load_progs(void) {
    long header = (long)sizeof(dprograms_t);
    long statements = header;
    long functions = statements + 2 * (long)sizeof(dstatement_t);
    long strings = functions + 2 * (long)sizeof(dfunction_t);
    long globals = (strings + 1 + 3) / 4 * 4;
    long size = globals + 128 * 4;
    byte *file = qs_alloc((size_t)size);
    dprograms_t *header_of = (dprograms_t *)file;
    header_of->version = PROG_VERSION;
    header_of->crc = PROGHEADER_CRC;
    header_of->ofs_statements = (int)statements;
    header_of->numstatements = 2;
    header_of->ofs_globaldefs = (int)globals;
    header_of->ofs_fielddefs = (int)globals;
    header_of->ofs_functions = (int)functions;
    header_of->numfunctions = 2;
    header_of->ofs_strings = (int)strings;
    header_of->numstrings = 1;
    header_of->ofs_globals = (int)globals;
    header_of->numglobals = 128;
    header_of->entityfields = 110;
    ((dfunction_t *)(file + functions))[1].first_statement = 1;
    qs_progs_file_t out;
    if (qs_check_progs(file, size, false, &out)) {
        fprintf(stderr, "vm-oracle fuzz: the init target's progs is refused\n");
        abort();
    }
}

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    qs_arena_reset();
    load_progs();
    qs_vector_setup_t setup;
    qs_parse_init("fuzz/case.init", (const char *)data, (long)size, &setup);
    return 0;
}

#elif defined(QS_FUZZ_PROGS)

// The loader with the header CRC checked, as scenario mode loads, and without, as vector mode
// does.
int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    for (int check_crc = 0; check_crc < 2; check_crc++) {
        qs_arena_reset();
        byte *file = qs_alloc(size + 1);
        memcpy(file, data, size);
        qs_progs_file_t out;
        qs_check_progs(file, (long)size, check_crc ? true : false, &out);
    }
    return 0;
}

#else
#error "define QS_FUZZ_SCENARIO, QS_FUZZ_INIT or QS_FUZZ_PROGS"
#endif
