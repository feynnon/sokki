// C ABI for screen and camera recording and for rendering a finished look.
// Implemented in video_capture.swift. Rust side: src/video/bridge.rs.
// Every string in and out is UTF-8 JSON; strings returned here are malloc'd
// and freed with sokki_video_free.

#ifndef SOKKI_VIDEO_CAPTURE_H
#define SOKKI_VIDEO_CAPTURE_H

#include <stdbool.h>

// What can be recorded, without asking for anything:
// {"screen":"allowed|not_allowed","camera":"allowed|denied|not_asked|restricted",
//  "displays":[{"id","name","width","height"}],"cameras":[{"id","name"}]}
char *sokki_video_sources(void);

// Shows macOS's Screen Recording prompt (or opens System Settings when it was
// declined before). Returns true when access is already allowed.
bool sokki_video_request_screen(void);

// Shows macOS's Camera prompt and waits for the answer. True when allowed.
bool sokki_video_request_camera(void);

// Starts recording. config: {"screen":{"display":<id>,"path":"…"}|null,
// "camera":{"device":"<id>","path":"…"}|null}. Returns a handle, or NULL with
// *out_error set (free with sokki_video_free).
void *sokki_video_start(const char *config, char **out_error);

// Stops, finishes the files and frees the handle. Blocks until the files are
// written. Returns {"screen":{"path","first_us","frames"}|null,"camera":…,
// "errors":[…]}; first_us is wall-clock microseconds of the first frame, the
// same clock as the meeting's audio tracks.
char *sokki_video_stop(void *handle);

// Renders a look into one video. config: {"look":"bubble|side|vertical",
// "screen":{"path","first_us"}|null,"camera":{"path","first_us"}|null,
// "audio":[{"path","first_us"}],"out":"…"}. Blocks. False with *out_error set
// on failure.
bool sokki_video_render(const char *config, char **out_error);

// Shows (or hides) the live camera bubble: a round panel at the bottom left
// that floats over every app. device_id NULL: the default camera.
void sokki_video_bubble(bool show, const char *device_id);

void sokki_video_free(char *s);

#endif
