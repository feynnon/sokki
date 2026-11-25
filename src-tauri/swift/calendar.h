// C ABI for the Mac calendar (EventKit): the events around a meeting with
// their attendees, for "send the notes to everyone" (board card
// notes-to-all) and the mail before a meeting (pre-meeting-mail).
// Implemented in calendar.swift. Rust side: src/meeting/calendar.rs.
// Strings returned here are malloc'd UTF-8 and freed with
// sokki_calendar_free.

#ifndef SOKKI_CALENDAR_H
#define SOKKI_CALENDAR_H

#include <stdbool.h>

// "allowed", "not_determined", "denied", "restricted" or "write_only".
// Never asks.
char *sokki_calendar_status(void);

// Shows macOS's calendar prompt once (full access to events). Blocks until
// the person answers: never call it on the main thread. True when allowed.
bool sokki_calendar_request(void);

// The events from `from` to `to` (unix seconds) in every calendar, as a
// JSON array in meeting_mail::calendar::CalendarEvent's shape. "[]" when
// access is not allowed.
char *sokki_calendar_events(double from, double to);

void sokki_calendar_free(char *s);

#endif
