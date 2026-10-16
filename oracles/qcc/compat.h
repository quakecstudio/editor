// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// Included before every file of id's qcc (the compiler's force-include option), which is
// built unchanged. qcc calls lseek, close and the other POSIX file functions without including
// their header, which C99 compilers refuse; on Windows it also needs the C runtime's names for
// them and binary file access.

#ifndef QS_QCC_COMPAT_H
#define QS_QCC_COMPAT_H

#ifdef _WIN32
#include <direct.h>
#include <fcntl.h>
#include <io.h>
#include <stdarg.h>
#include <string.h>
#include <sys/stat.h>

// cmdlib.c defines these itself, with types the C runtime's declarations disagree with.
#define filelength qs_qcc_filelength
#define tell qs_qcc_tell
#define strupr qs_qcc_strupr

// The C runtime refuses permission bits other than read and write, and opens files as text
// unless told otherwise.
static int qs_qcc_open(const char *path, int flags, ...) {
    int mode = 0;
    if (flags & _O_CREAT) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, int);
        va_end(args);
    }
    return _open(path, flags | _O_BINARY, mode & (_S_IREAD | _S_IWRITE));
}
#define open qs_qcc_open
#define mkdir(path, mode) _mkdir(path)
#define umask _umask
#define strcasecmp _stricmp
#else
#include <unistd.h>
#endif

#endif
