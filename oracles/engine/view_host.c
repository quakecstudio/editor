// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The view oracle's harness: id's whole engine (WinQuake, the software renderer, the client,
// the server and the mixer, compiled from the pinned commit) run headless. It plays the system
// (sys_null.c's role), the video card (vid_null.c's, at 320 by 240) and the sound card (a DMA
// buffer that advances by the clock and is copied out as a device would play it).
//
//   view-oracle <script> <audio> [<engine word>...]
//
// The script is VIEWDIFF-1's (docs/spec/engine/view-diff.md), the one the preview host's
// --view-script plays: `map <name>` first, then `frames <n>`, `place <x> <y> <z> <pitch>
// <yaw>` and `shot <file>`, blank lines and `#` comments skipped. The view is drawn as the
// host draws it for the comparison: `viewsize 120` (no status bar), `crosshair 0`,
// `r_drawviewmodel 0` and `con_notifytime 0` (VIEWDIFF-2.2).

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "quakedef.h"
#include "d_local.h"

#define QS_EXIT_DONE 0
#define QS_EXIT_REFUSED 2
#define QS_EXIT_IO 3
#define QS_EXIT_ENGINE 4

#define QS_MEMORY (64 * 1024 * 1024)
static byte qs_memory[QS_MEMORY];

// The clock: frames of 1/72 s, exactly as host_framerate gives them.
#define QS_FRAME (1.0 / 72.0)
static double qs_time;

// ---- sys_null.c's role ----

#define MAX_HANDLES 10
static FILE *sys_handles[MAX_HANDLES];

static int find_handle(void) {
    for (int i = 1; i < MAX_HANDLES; i++)
        if (!sys_handles[i])
            return i;
    Sys_Error("out of handles");
    return -1;
}

int Sys_FileOpenRead(char *path, int *hndl) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        *hndl = -1;
        return -1;
    }
    int i = find_handle();
    sys_handles[i] = f;
    *hndl = i;
    fseek(f, 0, SEEK_END);
    int length = (int)ftell(f);
    fseek(f, 0, SEEK_SET);
    return length;
}

int Sys_FileOpenWrite(char *path) {
    (void)path;
    return -1;
}

void Sys_FileClose(int handle) {
    fclose(sys_handles[handle]);
    sys_handles[handle] = NULL;
}

void Sys_FileSeek(int handle, int position) { fseek(sys_handles[handle], position, SEEK_SET); }

int Sys_FileRead(int handle, void *dest, int count) {
    return (int)fread(dest, 1, (size_t)count, sys_handles[handle]);
}

int Sys_FileWrite(int handle, void *data, int count) {
    (void)handle;
    (void)data;
    return count;
}

int Sys_FileTime(char *path) {
    struct stat st;
    return stat(path, &st) == 0 ? 1 : -1;
}

void Sys_mkdir(char *path) { (void)path; }
void Sys_MakeCodeWriteable(unsigned long startaddr, unsigned long length) {
    (void)startaddr;
    (void)length;
}

void Sys_Error(char *error, ...) {
    va_list args;
    va_start(args, error);
    fputs("view-oracle: Sys_Error: ", stderr);
    vfprintf(stderr, error, args);
    fputc('\n', stderr);
    va_end(args);
    exit(QS_EXIT_ENGINE);
}

qboolean isDedicated = false;
static qboolean qs_console;

void Sys_Printf(char *fmt, ...) {
    if (!qs_console)
        return;
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
}

void Sys_Quit(void) { exit(QS_EXIT_DONE); }
double Sys_FloatTime(void) { return qs_time; }
char *Sys_ConsoleInput(void) { return NULL; }
void Sys_Sleep(void) {}
void Sys_SendKeyEvents(void) {}
void Sys_HighFPPrecision(void) {}
void Sys_LowFPPrecision(void) {}

// ---- vid_null.c's role, at 320 by 240: square pixels at 4:3, so vid.aspect is 1 ----

#define QS_WIDTH 320
#define QS_HEIGHT 240

viddef_t vid;
static byte vid_buffer[QS_WIDTH * QS_HEIGHT];
static short zbuffer[QS_WIDTH * QS_HEIGHT];
static byte surfcache[1024 * 1024];
unsigned short d_8to16table[256];
unsigned d_8to24table[256];
static byte qs_palette[768];

void VID_SetPalette(unsigned char *palette) { memcpy(qs_palette, palette, sizeof(qs_palette)); }
void VID_ShiftPalette(unsigned char *palette) { VID_SetPalette(palette); }

void VID_Init(unsigned char *palette) {
    vid.maxwarpwidth = vid.width = vid.conwidth = QS_WIDTH;
    vid.maxwarpheight = vid.height = vid.conheight = QS_HEIGHT;
    vid.aspect = 1.0;
    vid.numpages = 1;
    vid.colormap = host_colormap;
    vid.fullbright = 256 - LittleLong(*((int *)vid.colormap + 2048));
    vid.buffer = vid.conbuffer = vid_buffer;
    vid.rowbytes = vid.conrowbytes = QS_WIDTH;
    d_pzbuffer = zbuffer;
    D_InitCaches(surfcache, sizeof(surfcache));
    VID_SetPalette(palette);
}

void VID_Shutdown(void) {}
void VID_Update(vrect_t *rects) { (void)rects; }
void D_BeginDirectRect(int x, int y, byte *pbitmap, int width, int height) {
    (void)x, (void)y, (void)pbitmap, (void)width, (void)height;
}
void D_EndDirectRect(int x, int y, int width, int height) {
    (void)x, (void)y, (void)width, (void)height;
}

// Writes the screen through the palette VID_ShiftPalette last set: what the card shows.
static qboolean write_shot(const char *path) {
    FILE *f = fopen(path, "wb");
    if (!f)
        return false;
    fprintf(f, "P6\n%d %d\n255\n", QS_WIDTH, QS_HEIGHT);
    for (int i = 0; i < QS_WIDTH * QS_HEIGHT; i++)
        fwrite(&qs_palette[vid_buffer[i] * 3], 1, 3, f);
    return fclose(f) == 0;
}

// ---- the sound card: 11025 Hz, 16-bit stereo, a buffer of 2^14 samples ----

#define QS_SAMPLES (1 << 14)
static short qs_dma[QS_SAMPLES];
static FILE *qs_audio;
static long qs_played; // stereo frames played since the start

qboolean SNDDMA_Init(void) {
    shm = &sn;
    shm->splitbuffer = 0;
    shm->samplebits = 16;
    shm->speed = 11025;
    shm->channels = 2;
    shm->samples = QS_SAMPLES;
    shm->samplepos = 0;
    shm->submission_chunk = 1;
    shm->buffer = (unsigned char *)qs_dma;
    return true;
}

// The position the card has played to: the clock's stereo frames, in mono samples.
int SNDDMA_GetDMAPos(void) {
    long frames = (long)(qs_time * 11025.0);
    return (int)((frames * 2) % QS_SAMPLES);
}

void SNDDMA_Shutdown(void) {}
void SNDDMA_Submit(void) {}

// Copies out what the card played since the last call, up to the clock.
static void play_audio(void) {
    long until = (long)(qs_time * 11025.0);
    for (; qs_played < until; qs_played++) {
        long at = (qs_played * 2) % QS_SAMPLES;
        fwrite(&qs_dma[at], sizeof(short), 2, qs_audio);
    }
}

// ---- the script ----

// The player at the origin `v` gives, looking at its pitch and yaw, at rest: id's engine has
// no setpos, so this sets the server's edict, which the next frame sends to the client.
static void place(const float v[5]) {
    edict_t *player = svs.clients[0].edict;
    player->v.origin[0] = v[0];
    player->v.origin[1] = v[1];
    player->v.origin[2] = v[2];
    player->v.angles[0] = v[3];
    player->v.angles[1] = v[4];
    player->v.angles[2] = 0;
    VectorCopy(player->v.angles, player->v.v_angle);
    player->v.velocity[0] = player->v.velocity[1] = player->v.velocity[2] = 0;
    player->v.fixangle = 1;
    SV_LinkEdict(player, false);
}

static void run_frames(long count) {
    for (long i = 0; i < count; i++) {
        qs_time += QS_FRAME;
        Host_Frame(QS_FRAME);
        play_audio();
    }
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fputs("usage: view-oracle <script> <audio> [<engine word>...]\n", stderr);
        return QS_EXIT_REFUSED;
    }
    FILE *script = fopen(argv[1], "r");
    if (!script) {
        fprintf(stderr, "view-oracle: %s: cannot open\n", argv[1]);
        return QS_EXIT_REFUSED;
    }
    qs_audio = fopen(argv[2], "wb");
    if (!qs_audio) {
        fprintf(stderr, "view-oracle: %s: cannot write\n", argv[2]);
        return QS_EXIT_IO;
    }
    qs_console = getenv("QS_VIEW_CONSOLE") != NULL;
    static char *words[MAX_NUM_ARGVS];
    int count = 0;
    words[count++] = "view-oracle";
    for (int i = 3; i < argc && count < MAX_NUM_ARGVS; i++)
        words[count++] = argv[i];
    quakeparms_t parms = {0};
    parms.memsize = QS_MEMORY;
    parms.membase = qs_memory;
    parms.basedir = ".";
    COM_InitArgv(count, words);
    parms.argc = com_argc;
    parms.argv = com_argv;
    Host_Init(&parms);
    Cbuf_AddText("viewsize 120\ncrosshair 0\nr_drawviewmodel 0\ncon_notifytime 0\n");
    char line[1024];
    // VIEWDIFF-2.4: nothing is drawn but from a place to its shot.
    block_drawing = true;
    qboolean mapped = false;
    while (fgets(line, sizeof(line), script)) {
        line[strcspn(line, "\r\n")] = 0;
        char *text = line;
        while (*text == ' ' || *text == '\t')
            text++;
        if (!*text || *text == '#')
            continue;
        char *end;
        if (!mapped) {
            if (strncmp(text, "map ", 4) || !text[4] || strchr(text + 4, ' ')) {
                fprintf(stderr, "view-oracle: a view script starts with `map <name>`\n");
                return QS_EXIT_REFUSED;
            }
            Cbuf_AddText(text);
            Cbuf_AddText("\n");
            mapped = true;
        } else if (!strncmp(text, "frames ", 7)) {
            long count = strtol(text + 7, &end, 10);
            if (end == text + 7 || *end || count < 0) {
                fprintf(stderr, "view-oracle: refused: %s\n", text);
                return QS_EXIT_REFUSED;
            }
            run_frames(count);
        } else if (!strncmp(text, "place ", 6)) {
            float v[5];
            int used = 0;
            if (sscanf(text + 6, "%f %f %f %f %f%n", &v[0], &v[1], &v[2], &v[3], &v[4], &used) != 5 ||
                text[6 + used] || !sv.active) {
                fprintf(stderr, "view-oracle: refused: %s\n", text);
                return QS_EXIT_REFUSED;
            }
            place(v);
            // VIEWDIFF-2.4: drawing again, the console up, where SCR_SetUpToDrawConsole's
            // slide would have left it in the frames not drawn.
            scr_con_current = 0;
            block_drawing = false;
        } else if (!strncmp(text, "shot ", 5) && text[5]) {
            if (!write_shot(text + 5))
                return QS_EXIT_IO;
            block_drawing = true;
        } else {
            fprintf(stderr, "view-oracle: refused: %s\n", text);
            return QS_EXIT_REFUSED;
        }
    }
    fclose(script);
    return fclose(qs_audio) == 0 ? QS_EXIT_DONE : QS_EXIT_IO;
}
