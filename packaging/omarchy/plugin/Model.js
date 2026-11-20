// Pure logic for the Sokki plugin: parsing the daemon's JSON lines and
// formatting what the bar and the panel show. No QML here, so Node can load
// it too. The daemon's API is specified in crates/sokki/src/api.rs.
//
// Everything read from the daemon is treated as untrusted: strings are
// capped, numbers checked, unknown values dropped.

var MAX_TEXT = 200
var MAX_NOTES = 5

// Nerd Font (Material Design) glyphs, written as code points so no editor
// can mangle them.
var GLYPH = {
  microphone: String.fromCodePoint(0xf036c),     // md-microphone
  microphoneIdle: String.fromCodePoint(0xf036e), // md-microphone_outline
  record: String.fromCodePoint(0xf044a),         // md-record
  stop: String.fromCodePoint(0xf04db),           // md-stop
  waiting: String.fromCodePoint(0xf051f),        // md-timer_sand
  note: String.fromCodePoint(0xf09ed),           // md-text_box_outline
  failed: String.fromCodePoint(0xf05d6),         // md-alert_circle_outline
  download: String.fromCodePoint(0xf01da)        // md-download
}

// md-circle_slice_1 … md-circle_slice_8: a pie that fills with progress.
var SLICES = [0xf0a9e, 0xf0a9f, 0xf0aa0, 0xf0aa1, 0xf0aa2, 0xf0aa3, 0xf0aa4, 0xf0aa5]

var STAGES = ["preparing", "diarizing", "transcribing", "summarizing", "writing"]
var NOTE_STATUSES = ["recording", "queued", "processing", "done", "failed"]

function text(value) {
  if (value === undefined || value === null) return ""
  var s = String(value).replace(/[\u0000-\u001f\u007f]/g, " ").trim()
  return s.length > MAX_TEXT ? s.substring(0, MAX_TEXT - 1) + "…" : s
}

function number(value, fallback) {
  var n = Number(value)
  return isFinite(n) ? n : fallback
}

function emptyState() {
  return {
    phase: "idle",
    app: "",
    id: "",
    sinceMs: 0,
    expiresMs: 0,
    callAudio: true,
    processing: null,
    queued: 0,
    dictation: "idle",
    modelsReady: true,
    download: null
  }
}

function normalizeState(raw) {
  var out = emptyState()
  if (!raw || typeof raw !== "object") return out

  var meeting = raw.meeting && typeof raw.meeting === "object" ? raw.meeting : {}
  var phase = String(meeting.phase || "idle")
  if (phase === "asking") {
    out.phase = "asking"
    out.app = text(meeting.app)
    out.sinceMs = number(meeting.since_ms, 0)
    out.expiresMs = number(meeting.expires_ms, 0)
  } else if (phase === "recording") {
    out.phase = "recording"
    out.id = text(meeting.id)
    out.app = text(meeting.app)
    out.sinceMs = number(meeting.since_ms, 0)
    out.callAudio = meeting.call_audio !== false
  }

  var p = raw.processing
  if (p && typeof p === "object") {
    var progress = p.progress === undefined || p.progress === null ? null : number(p.progress, null)
    if (progress !== null) progress = Math.max(0, Math.min(1, progress))
    out.processing = {
      id: text(p.id),
      stage: STAGES.indexOf(String(p.stage)) !== -1 ? String(p.stage) : "",
      progress: progress
    }
  }
  out.queued = Math.max(0, Math.floor(number(raw.queued, 0)))

  var dictation = String(raw.dictation || "idle")
  out.dictation = dictation === "listening" || dictation === "transcribing" ? dictation : "idle"

  var models = raw.models && typeof raw.models === "object" ? raw.models : {}
  out.modelsReady = models.ready !== false
  var d = models.downloading
  if (d && typeof d === "object") {
    var total = Math.max(0, number(d.total, 0))
    out.download = {
      file: text(d.file),
      done: Math.max(0, number(d.done, 0)),
      total: total
    }
  }
  return out
}

function normalizeNote(raw) {
  if (!raw || typeof raw !== "object") return null
  var id = text(raw.id)
  if (!id) return null
  var status = String(raw.status || "")
  return {
    id: id,
    title: text(raw.title),
    app: text(raw.app),
    startedAt: text(raw.started_at),
    durationS: Math.max(0, number(raw.duration_s, 0)),
    status: NOTE_STATUSES.indexOf(status) !== -1 ? status : "done"
  }
}

// One line of `sokki events` → { kind, ... } or null for anything else.
function parseEvent(line) {
  var raw
  try {
    raw = JSON.parse(String(line || ""))
  } catch (e) {
    return null
  }
  if (!raw || typeof raw !== "object") return null
  switch (raw.event) {
  case "state":
    return { kind: "state", state: normalizeState(raw) }
  case "offline":
    return { kind: "offline" }
  case "note":
    var note = normalizeNote(raw)
    return note ? { kind: "note", note: note } : null
  case "error":
    return { kind: "error", message: text(raw.message) }
  }
  return null
}

// The reply of `sokki notes --json`.
// The reply of `sokki speakers <id> --json`: names still to be given, for
// speakers whose voice can be remembered. null when unreadable.
function parseUnnamedSpeakers(reply) {
  var raw
  try {
    raw = JSON.parse(String(reply || ""))
  } catch (e) {
    return null
  }
  if (!raw || !Array.isArray(raw.speakers)) return null
  var out = []
  for (var i = 0; i < raw.speakers.length; i++) {
    var sp = raw.speakers[i]
    if (sp && sp.unnamed && sp.has_voice && typeof sp.name === "string") out.push(sp.name)
  }
  return out
}

function parseNotes(reply) {
  var raw
  try {
    raw = JSON.parse(String(reply || ""))
  } catch (e) {
    return null
  }
  if (!raw || raw.ok !== true || !Array.isArray(raw.notes)) return null
  var out = []
  for (var i = 0; i < raw.notes.length && out.length < MAX_NOTES; i++) {
    var note = normalizeNote(raw.notes[i])
    if (note) out.push(note)
  }
  return out
}

// A note event updates the list in place, or puts a new meeting first.
function mergeNote(notes, note) {
  var out = [note]
  for (var i = 0; i < (notes || []).length && out.length < MAX_NOTES; i++) {
    if (notes[i] && notes[i].id !== note.id) out.push(notes[i])
  }
  return out
}

function identity(n) {
  return String(n)
}

// "4:07" or "1:04:07", with the language's digits.
function formatElapsed(ms, formatNumber) {
  var fmt = typeof formatNumber === "function" ? formatNumber : identity
  var total = Math.max(0, Math.floor(number(ms, 0) / 1000))
  var h = Math.floor(total / 3600)
  var m = Math.floor((total % 3600) / 60)
  var s = total % 60
  function pad(n) { return n < 10 ? fmt(0) + fmt(n) : fmt(n) }
  return h > 0 ? fmt(h) + ":" + pad(m) + ":" + pad(s) : fmt(m) + ":" + pad(s)
}

function secondsLeft(expiresMs, nowMs) {
  return Math.max(0, Math.ceil((number(expiresMs, 0) - number(nowMs, 0)) / 1000))
}

// 1 → 0 as the question runs out.
function askFraction(state, nowMs) {
  var span = state.expiresMs - state.sinceMs
  if (span <= 0) return 0
  return Math.max(0, Math.min(1, (state.expiresMs - nowMs) / span))
}

function progressGlyph(progress) {
  if (progress === null || progress === undefined) return GLYPH.waiting
  var i = Math.max(0, Math.min(SLICES.length - 1, Math.floor(progress * SLICES.length)))
  return String.fromCodePoint(SLICES[i])
}

function downloadFraction(download) {
  if (!download || !(download.total > 0)) return null
  return Math.max(0, Math.min(1, download.done / download.total))
}

function stageKey(stage) {
  return STAGES.indexOf(String(stage)) !== -1 ? "stage." + stage : "stage.preparing"
}

function noteGlyph(note) {
  if (!note) return GLYPH.note
  if (note.status === "failed") return GLYPH.failed
  if (note.status === "recording") return GLYPH.record
  if (note.status === "queued" || note.status === "processing") return GLYPH.waiting
  return GLYPH.note
}

function noteDate(note) {
  var d = new Date(note ? note.startedAt : "")
  return isNaN(d.getTime()) ? null : d
}

function durationMinutes(note) {
  return Math.max(1, Math.round((note ? note.durationS : 0) / 60))
}

if (typeof module !== "undefined") {
  module.exports = {
    GLYPH: GLYPH,
    emptyState: emptyState,
    normalizeState: normalizeState,
    normalizeNote: normalizeNote,
    parseEvent: parseEvent,
    parseNotes: parseNotes,
    parseUnnamedSpeakers: parseUnnamedSpeakers,
    mergeNote: mergeNote,
    formatElapsed: formatElapsed,
    secondsLeft: secondsLeft,
    askFraction: askFraction,
    progressGlyph: progressGlyph,
    downloadFraction: downloadFraction,
    stageKey: stageKey,
    noteGlyph: noteGlyph,
    noteDate: noteDate,
    durationMinutes: durationMinutes
  }
}
