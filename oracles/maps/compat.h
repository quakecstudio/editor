// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// Included before every file of id's qbsp, vis and light (the compiler's force-include
// option), which are built unchanged. cmdlib.c calls the POSIX file functions without including
// their header, which C99 compilers refuse.

#ifndef QS_MAPS_COMPAT_H
#define QS_MAPS_COMPAT_H

#include <unistd.h>

#endif
