// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// Windows has no <sys/time.h>; id's cmdlib.c uses gettimeofday only to time the compile.

#ifndef QS_QCC_SYS_TIME_H
#define QS_QCC_SYS_TIME_H

#include <sys/timeb.h>

struct timeval {
    long tv_sec;
    long tv_usec;
};

struct timezone {
    int tz_minuteswest;
    int tz_dsttime;
};

static int gettimeofday(struct timeval *tv, struct timezone *tz) {
    struct _timeb now;
    _ftime(&now);
    tv->tv_sec = (long)now.time;
    tv->tv_usec = (long)now.millitm * 1000L;
    (void)tz;
    return 0;
}

#endif
