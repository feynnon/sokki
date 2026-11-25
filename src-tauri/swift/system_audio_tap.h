// C ABI for system-audio capture via Core Audio process taps (macOS 14.2+).
// Implemented in system_audio_tap.swift (or system_audio_tap_stub.swift when the
// SDK lacks the API). Rust side: src/meeting/system_tap.rs.

#ifndef SOKKI_SYSTEM_AUDIO_TAP_H
#define SOKKI_SYSTEM_AUDIO_TAP_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Called on Core Audio's IO thread. `samples` is interleaved f32 with `channels`
// channels at `sample_rate` Hz, `frames` frames long. Must not block or allocate.
typedef void (*sokki_tap_callback)(void *user,
                                   const float *samples,
                                   size_t frames,
                                   uint32_t channels,
                                   double sample_rate);

// True when the running macOS supports process taps (14.2+).
bool sokki_tap_supported(void);

// Starts a global tap that excludes this process. Returns an opaque handle, or
// NULL with *out_error set to a malloc'd message (free with sokki_tap_free_error).
void *sokki_tap_start(sokki_tap_callback callback, void *user, char **out_error);

// Stops and releases everything created by sokki_tap_start. Safe with NULL.
void sokki_tap_stop(void *handle);

// Tears down and rebuilds the tap and its aggregate device, for the same
// callback (the tap went dead, or the default output changed). Returns false
// with *out_error set when building failed; the handle stays valid and may be
// rebuilt again or stopped.
bool sokki_tap_rebuild(void *handle, char **out_error);

// Number of other processes playing audio now, -1 when unknown. Their bundle
// ids, comma-separated, go into `buffer` (NUL-terminated, cut to `capacity`).
int32_t sokki_tap_other_apps_playing(char *buffer, size_t capacity);

// The default output device's id (0 when none), its name into `buffer`.
uint32_t sokki_tap_default_output(char *buffer, size_t capacity);

void sokki_tap_free_error(char *error);

#endif
