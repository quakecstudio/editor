// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// Included before each of id's files the engine oracle compiles: the C library's rand differs
// between Windows, macOS and Linux, so id's calls go to the VM oracle's generator instead
// (DIFF-6.2), defined in oracles/engine/rand.c. stdlib.h comes first so that its own
// declaration of rand is not renamed.

#ifndef QS_RAND_H
#define QS_RAND_H

#include <stdlib.h>

int qs_rand(void);
int qs_client_rand(void);
// The view oracle compiles id's client files with QS_CLIENT_RAND: their draws come from a
// generator of their own, so the server's are the server oracle's (VIEWDIFF-2.3).
#ifdef QS_CLIENT_RAND
#define rand qs_client_rand
#else
#define rand qs_rand
#endif

#endif
