#include "doomgeneric.h"
#include "doomkeys.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <time.h>
#include <termios.h>
#include <stdint.h>

/* Embedded doom1.wad via ld -r -b binary */
extern char _binary_doom1_wad_start[];
extern char _binary_doom1_wad_end[];

static int fb_fd = -1;
static int kb_fd = -1;
static int log_fd = -1;
static int web_input_fd = -1;
static struct termios orig_termios;

#define FB_WIDTH  640
#define FB_HEIGHT 480
#define WAD_PATH  "/tmp/doom1.wad"

/* Linux input event constants */
#define EV_KEY 1

/* Linux keycodes for keys we care about */
#define KEY_ESC_LC       1
#define KEY_1_LC         2
#define KEY_2_LC         3
#define KEY_3_LC         4
#define KEY_4_LC         5
#define KEY_5_LC         6
#define KEY_6_LC         7
#define KEY_7_LC         8
#define KEY_8_LC         9
#define KEY_9_LC        10
#define KEY_0_LC        11
#define KEY_MINUS_LC    12
#define KEY_EQUAL_LC    13
#define KEY_TAB_LC      15
#define KEY_Q_LC        16
#define KEY_W_LC        17
#define KEY_E_LC        18
#define KEY_R_LC        19
#define KEY_T_LC        20
#define KEY_Y_LC        21
#define KEY_ENTER_LC    28
#define KEY_A_LC        30
#define KEY_S_LC        31
#define KEY_D_LC        32
#define KEY_F_LC        33
#define KEY_LSHIFT_LC   42
#define KEY_COMMA_LC    51
#define KEY_DOT_LC      52
#define KEY_RSHIFT_LC   54
#define KEY_SPACE_LC    57
#define KEY_UP_LC      103
#define KEY_LEFT_LC    105
#define KEY_RIGHT_LC   106
#define KEY_DOWN_LC    108

struct virtio_input_event {
    uint16_t type;
    uint16_t code;
    uint32_t value;
};

static void extract_wad(void)
{
    size_t size = _binary_doom1_wad_end - _binary_doom1_wad_start;
    int fd = open(WAD_PATH, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) {
        fprintf(stderr, "Failed to create %s\n", WAD_PATH);
        exit(1);
    }
    size_t written = 0;
    while (written < size) {
        ssize_t n = write(fd, _binary_doom1_wad_start + written, size - written);
        if (n <= 0) {
            fprintf(stderr, "Failed to write WAD data\n");
            exit(1);
        }
        written += n;
    }
    close(fd);
    fprintf(stderr, "Extracted doom1.wad (%zu bytes)\n", size);
}

void DG_Init(void)
{
    log_fd = open("/tmp/doom_fps.log", O_WRONLY | O_CREAT | O_TRUNC, 0644);

    const char *fb_env = getenv("DOOM_FB_PATH");
    const char *fb_path = fb_env ? fb_env : "/dev/fb0";
    fb_fd = open(fb_path, O_WRONLY);
    if (fb_fd < 0) {
        fprintf(stderr, "Failed to open %s\n", fb_path);
        exit(1);
    }

    /* Clear top and bottom borders once */
    {
        int border = (FB_HEIGHT - DOOMGENERIC_RESY) / 2;
        size_t border_bytes = border * FB_WIDTH * sizeof(uint32_t);
        void *black = calloc(border * FB_WIDTH, sizeof(uint32_t));
        lseek(fb_fd, 0, SEEK_SET);
        write(fb_fd, black, border_bytes);
        lseek(fb_fd, (border + DOOMGENERIC_RESY) * FB_WIDTH * sizeof(uint32_t), SEEK_SET);
        write(fb_fd, black, border_bytes);
        free(black);
    }

    const char *input_env = getenv("DOOM_INPUT_PATH");
    if (input_env) {
        web_input_fd = open(input_env, O_RDONLY | O_NONBLOCK);
        if (web_input_fd >= 0)
            fprintf(stderr, "Using web input from %s\n", input_env);
    }

    kb_fd = open("/dev/keyboard0", O_RDONLY | O_NONBLOCK);
    if (kb_fd >= 0) {
        fprintf(stderr, "Using VirtIO keyboard input\n");
    } else {
        fprintf(stderr, "No VirtIO keyboard, falling back to stdin\n");
        struct termios raw;
        tcgetattr(STDIN_FILENO, &orig_termios);
        raw = orig_termios;
        raw.c_lflag &= ~(ICANON | ECHO);
        raw.c_cc[VMIN] = 0;
        raw.c_cc[VTIME] = 0;
        tcsetattr(STDIN_FILENO, TCSANOW, &raw);

        int flags = fcntl(STDIN_FILENO, F_GETFL, 0);
        fcntl(STDIN_FILENO, F_SETFL, flags | O_NONBLOCK);
    }
}

static uint32_t frame_count = 0;
static uint32_t fps_last_time = 0;

void DG_DrawFrame(void)
{
    int y_offset = (FB_HEIGHT - DOOMGENERIC_RESY) / 2;
    lseek(fb_fd, y_offset * FB_WIDTH * sizeof(uint32_t), SEEK_SET);
    write(fb_fd, DG_ScreenBuffer,
          DOOMGENERIC_RESX * DOOMGENERIC_RESY * sizeof(uint32_t));

    frame_count++;
    uint32_t now = DG_GetTicksMs();
    if (fps_last_time == 0) fps_last_time = now;
    uint32_t elapsed = now - fps_last_time;
    if (elapsed >= 2000) {
        char buf[64];
        int len = snprintf(buf, sizeof(buf), "FPS: %u.%u (frames=%u elapsed=%u)\n",
                (frame_count * 1000) / elapsed,
                ((frame_count * 10000) / elapsed) % 10,
                frame_count, elapsed);
        if (log_fd >= 0 && len > 0) {
            if (len > (int)sizeof(buf)) len = (int)sizeof(buf) - 1;
            write(log_fd, buf, len);
        }
        frame_count = 0;
        fps_last_time = now;
    }
}

void DG_SleepMs(uint32_t ms)
{
    struct timespec ts;
    ts.tv_sec = ms / 1000;
    ts.tv_nsec = (ms % 1000) * 1000000L;
    nanosleep(&ts, NULL);
}

uint32_t DG_GetTicksMs(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)(ts.tv_sec * 1000 + ts.tv_nsec / 1000000);
}

static unsigned char linux_keycode_to_doom(uint16_t code)
{
    switch (code) {
        case KEY_UP_LC:     return KEY_UPARROW;
        case KEY_DOWN_LC:   return KEY_DOWNARROW;
        case KEY_LEFT_LC:   return KEY_LEFTARROW;
        case KEY_RIGHT_LC:  return KEY_RIGHTARROW;
        case KEY_W_LC:      return KEY_UPARROW;
        case KEY_S_LC:      return KEY_DOWNARROW;
        case KEY_A_LC:      return KEY_LEFTARROW;
        case KEY_D_LC:      return KEY_RIGHTARROW;
        case KEY_SPACE_LC:  return KEY_FIRE;
        case KEY_ENTER_LC:  return KEY_ENTER;
        case KEY_ESC_LC:    return KEY_ESCAPE;
        case KEY_TAB_LC:    return KEY_TAB;
        case KEY_COMMA_LC:  return KEY_STRAFE_L;
        case KEY_DOT_LC:    return KEY_STRAFE_R;
        case KEY_E_LC:      return KEY_USE;
        case KEY_LSHIFT_LC: return KEY_RSHIFT;
        case KEY_RSHIFT_LC: return KEY_RSHIFT;
        case KEY_1_LC:      return '1';
        case KEY_2_LC:      return '2';
        case KEY_3_LC:      return '3';
        case KEY_4_LC:      return '4';
        case KEY_5_LC:      return '5';
        case KEY_6_LC:      return '6';
        case KEY_7_LC:      return '7';
        case KEY_8_LC:      return '8';
        case KEY_9_LC:      return '9';
        case KEY_0_LC:      return '0';
        case KEY_MINUS_LC:  return KEY_MINUS;
        case KEY_EQUAL_LC:  return KEY_EQUALS;
        case KEY_Y_LC:      return 'y';
        case KEY_Q_LC:      return 'q';
        case KEY_F_LC:      return 'f';
        case KEY_R_LC:      return 'r';
        case KEY_T_LC:      return 't';
        default:            return 0;
    }
}

static unsigned char convert_key(unsigned char c)
{
    switch (c) {
        case 'w': return KEY_UPARROW;
        case 's': return KEY_DOWNARROW;
        case 'a': return KEY_LEFTARROW;
        case 'd': return KEY_RIGHTARROW;
        case ' ': return KEY_FIRE;
        case '\n': return KEY_ENTER;
        case 27:  return KEY_ESCAPE;
        case '\t': return KEY_TAB;
        case ',': return KEY_STRAFE_L;
        case '.': return KEY_STRAFE_R;
        case 'e': return KEY_USE;
        default:  return c;
    }
}

/* Buffer for VirtIO keyboard events */
#define MAX_EVENTS 16
static struct virtio_input_event event_buf[MAX_EVENTS];
static int event_count = 0;
static int event_index = 0;

/* Buffer for web input events: [keycode, pressed] pairs */
static uint8_t web_buf[64];
static int web_count = 0;
static int web_idx = 0;

static int pending_release = 0;
static unsigned char pending_release_key = 0;

int DG_GetKey(int *pressed, unsigned char *doomKey)
{
    /* Web input path: 2-byte events [linux_keycode, is_pressed] */
    if (web_input_fd >= 0) {
        if (web_idx >= web_count) {
            ssize_t n = read(web_input_fd, web_buf, sizeof(web_buf));
            if (n > 0) {
                web_count = (n / 2) * 2;
                web_idx = 0;
            }
        }
        if (web_idx < web_count) {
            unsigned char key = linux_keycode_to_doom(web_buf[web_idx]);
            uint8_t is_pressed = web_buf[web_idx + 1];
            web_idx += 2;
            if (key) {
                *doomKey = key;
                *pressed = is_pressed ? 1 : 0;
                return 1;
            }
        }
    }

    if (kb_fd >= 0) {
        /* VirtIO keyboard path: real press/release events */
        if (event_index >= event_count) {
            ssize_t n = read(kb_fd, event_buf, sizeof(event_buf));
            if (n <= 0) return 0;
            event_count = n / sizeof(struct virtio_input_event);
            event_index = 0;
        }

        while (event_index < event_count) {
            struct virtio_input_event *ev = &event_buf[event_index++];
            if (ev->type != EV_KEY) continue;
            unsigned char key = linux_keycode_to_doom(ev->code);
            if (key == 0) continue;
            *pressed = (ev->value != 0) ? 1 : 0;
            *doomKey = key;
            return 1;
        }
        return 0;
    }

    /* Fallback: stdin with synthetic key-release */
    if (pending_release) {
        pending_release = 0;
        *pressed = 0;
        *doomKey = pending_release_key;
        return 1;
    }

    unsigned char c;
    int n = read(STDIN_FILENO, &c, 1);
    if (n <= 0) return 0;

    *pressed = 1;
    *doomKey = convert_key(c);
    pending_release = 1;
    pending_release_key = *doomKey;
    return 1;
}

void DG_SetWindowTitle(const char *title)
{
    (void)title;
}

int main(int argc, char **argv)
{
    extract_wad();

    /* Build argv for doomgeneric: doom -iwad /tmp/doom1.wad [user args...] */
    int new_argc = argc + 2;
    char **new_argv = malloc(sizeof(char *) * (new_argc + 1));
    new_argv[0] = argv[0];
    new_argv[1] = "-iwad";
    new_argv[2] = WAD_PATH;
    for (int i = 1; i < argc; i++)
        new_argv[i + 2] = argv[i];
    new_argv[new_argc] = NULL;

    doomgeneric_Create(new_argc, new_argv);
    for (;;)
        doomgeneric_Tick();
    return 0;
}
