// C ABI for "Meeting with screen": recording the screen, and one video of it
// with the meeting's sound. Implemented in video_capture.swift. Rust side:
// src/video/bridge.rs. Every string in and out is UTF-8 JSON; strings
// returned here are malloc'd and freed with sokki_video_free.

#ifndef SOKKI_VIDEO_CAPTURE_H
#define SOKKI_VIDEO_CAPTURE_H

#include <stdbool.h>

// Whether the screen may be recorded, and the displays, without asking:
// {"screen":"allowed|not_allowed","displays":[{"id","width","height","main"}]}
char *sokki_video_sources(void);

// Shows macOS's Screen Recording prompt (or opens System Settings when it was
// declined before). Returns true when access is already allowed.
bool sokki_video_request_screen(void);

// Starts recording. config: {"screen":{"display":<id>|null,"path":"…"}}.
// Returns a handle, or NULL with *out_error set (free with sokki_video_free).
void *sokki_video_start(const char *config, char **out_error);

// Stops, finishes the file and frees the handle. Blocks until it is written.
// Returns {"screen":{"path","first_us","frames"}|null,"errors":[…]}; first_us
// is wall-clock microseconds of the first frame, the same clock as the
// meeting's audio tracks. The writer also keeps it in "<path>.first_us".
char *sokki_video_stop(void *handle);

// Renders the screen video with the sound into one .mp4. config:
// {"screen":{"path","first_us"},"audio":[{"path","first_us"}],"out":"…"}.
// Blocks. False with *out_error set on failure.
bool sokki_video_render(const char *config, char **out_error);

void sokki_video_free(char *s);

#endif
