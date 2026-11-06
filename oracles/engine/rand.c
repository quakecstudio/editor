// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// rand, as the VM oracle's (DIFF-6.2): a 32-bit state from 1, so a run's random numbers are
// the same on every OS. id's files call it through oracles/engine/rand.h; the engine and view
// oracles both link it.

#include "rand.h"

static unsigned random_state = 1;
static unsigned client_state = 1;

// The generator's next number from `state`.
static int next(unsigned *state) {
    *state = *state * 1103515245u + 12345u;
    return (int)((*state >> 16) & 0x7fff);
}

int qs_rand(void) { return next(&random_state); }

// The view oracle's client's own, the same generator from 1 (VIEWDIFF-2.3).
int qs_client_rand(void) { return next(&client_state); }
