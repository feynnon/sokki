import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import "Model.js" as Model

// The one place that talks to the Sokki daemon. It runs a single
// `sokki events` process for as long as the shell runs: the daemon pushes a
// whole snapshot of its state on every change, so nothing here polls. The
// bar widget on each monitor reads this object; it never starts processes
// of its own while idle.
//
// No `sokki` on PATH, or a daemon that is down, means a quiet state: the
// widget shows nothing.
Item {
  id: root

  // Injected by the shell.
  property var shell: null
  property var manifest: null
  property string omarchyPath: ""

  readonly property string pluginId: "sokki.meetings"

  // A state line has arrived and the daemon has not gone offline since.
  property bool connected: false
  // Not named `state`: every Item already has one.
  property var snapshot: Model.emptyState()
  readonly property var current: connected ? snapshot : Model.emptyState()
  readonly property string phase: current.phase
  readonly property bool processing: current.processing !== null
  readonly property bool downloading: current.download !== null
  readonly property bool dictating: current.dictation !== "idle"
  // Whether the bar has anything to show.
  readonly property bool active: phase !== "idle" || processing || downloading || dictating

  property var notes: []
  property bool notesLoaded: false
  // Speakers of the newest finished meeting who still have a placeholder
  // name and a voice sample: the panel asks who they were, once.
  property string speakersMeeting: ""
  property var unnamedSpeakers: []
  property string lastError: ""
  // When it came: the panel shows an error for a minute, not forever (an
  // old dictation error was still there long after dictation worked).
  property real lastErrorAt: 0

  // Wall clock for the recording time and the prompt's countdown. Ticks only
  // while one of them is on screen.
  property double now: Date.now()

  // Set by the widget whose panel is open (one at a time across monitors).
  property bool panelOpen: false
  // The panel is showing because a call started, not because the person
  // opened it: it stays out of the way (no keyboard, no click-away) until
  // they reach for it.
  property bool quietPanel: false
  // The panel opened itself for the current question.
  property bool promptedThisAsk: false

  property var _notified: ({})
  property double _streamStartedAt: 0
  property int _quickStops: 0

  Commons.Translations { id: strings; directory: Qt.resolvedUrl("i18n") }

  // ------------------------------------------------------------ actions

  function run(args) {
    Quickshell.execDetached(args)
  }

  function record() { run(["sokki", "meeting", "record"]) }
  function dismiss() { run(["sokki", "meeting", "dismiss"]) }
  function stop() { run(["sokki", "meeting", "stop"]) }
  function installModels() { run(["sokki", "models", "install"]) }

  // Names a speaker: the note is rewritten and the voice remembered, so the
  // next meeting names them by itself.
  function nameSpeaker(from, name) {
    var clean = String(name || "").trim()
    if (!speakersMeeting || !clean) return
    var rest = []
    for (var i = 0; i < unnamedSpeakers.length; i++)
      if (unnamedSpeakers[i] !== from) rest.push(unnamedSpeakers[i])
    unnamedSpeakers = rest
    run(["sokki", "name", speakersMeeting, from, clean])
  }

  function refreshSpeakers() {
    var newest = null
    for (var i = 0; i < notes.length; i++) {
      if (notes[i].status === "done") { newest = notes[i]; break }
    }
    if (!newest) {
      speakersMeeting = ""
      unnamedSpeakers = []
      return
    }
    if (speakersProcess.running) return
    speakersProcess.meeting = newest.id
    speakersProcess.command = ["sokki", "speakers", newest.id, "--json"]
    speakersProcess.running = true
  }

  function openNote(id) {
    if (id) run(["sokki", "open", String(id)])
  }

  function refreshNotes() {
    if (!connected || notesProcess.running) return
    notesProcess.running = true
  }

  // Open the panel, or bring it forward if it is showing quietly. Bound to
  // a key, this is how the prompt is answered from the keyboard.
  function togglePanel() {
    if (panelOpen && quietPanel) {
      quietPanel = false
      return
    }
    quietPanel = false
    if (shell) shell.toggle(pluginId, "")
  }

  function openPanel(quiet) {
    if (!shell) return false
    quietPanel = quiet === true
    var shown = shell.summon(pluginId, "")
    if (!shown) quietPanel = false
    return shown
  }

  function closePanel() {
    if (shell && panelOpen) shell.hide(pluginId)
  }

  function notify(args) {
    run(["omarchy-notification-send"].concat(args))
  }

  // ------------------------------------------------------------ events

  function handleLine(line) {
    var event = Model.parseEvent(line)
    if (!event) return
    if (event.kind === "state") {
      _quickStops = 0
      var was = connected ? snapshot : Model.emptyState()
      snapshot = event.state
      if (!connected) {
        connected = true
        refreshNotes()
      }
      transition(was, event.state)
    } else if (event.kind === "offline") {
      var before = connected ? snapshot : Model.emptyState()
      connected = false
      transition(before, Model.emptyState())
    } else if (event.kind === "note") {
      notes = Model.mergeNote(notes, event.note)
      noteWritten(event.note)
    } else if (event.kind === "error") {
      lastError = event.message
      lastErrorAt = Date.now()
      if (event.message) notify(["-u", "normal", "-g", Model.GLYPH.failed, "Sokki", event.message])
    }
  }

  function transition(was, next) {
    if (next.phase === "asking" && was.phase !== "asking") {
      lastError = ""
      promptedThisAsk = false
      // After the widget on the focused monitor has become visible.
      Qt.callLater(root.ask)
    } else if (next.phase !== "asking" && was.phase === "asking") {
      // Answered or timed out: a panel that opened only to ask goes away.
      if (promptedThisAsk && quietPanel) closePanel()
      promptedThisAsk = false
    }
    if (was.processing && !next.processing) refreshNotes()
  }

  function ask() {
    if (phase !== "asking") return
    if (panelOpen) {
      promptedThisAsk = false
      return
    }
    if (openPanel(true)) {
      promptedThisAsk = true
      return
    }
    // No bar widget to open (a replacement bar, say): ask in a notification.
    var seconds = Math.max(5, Model.secondsLeft(current.expiresMs, Date.now()))
    notify(["-u", "normal", "-g", Model.GLYPH.microphone, "-t", String(seconds * 1000),
      strings.t("prompt.title"), strings.t("prompt.body").arg(current.app || strings.t("prompt.anApp")),
      "--action", strings.t("prompt.record"), "sokki", "meeting", "record",
      "--action", strings.t("prompt.notNow"), "sokki", "meeting", "dismiss"])
  }

  function noteWritten(note) {
    if (note.status !== "done" && note.status !== "failed") return
    var key = note.id + ":" + note.status
    if (_notified[key]) return
    var next = ({})
    for (var k in _notified) next[k] = true
    next[key] = true
    _notified = next

    if (note.status === "done") {
      notify(["-g", Model.GLYPH.note, strings.t("notify.noteReady"),
        note.title || note.app || strings.t("notes.untitled"),
        "--action", strings.t("notify.open"), "sokki", "open", "--with", note.id])
    } else {
      notify(["-u", "normal", "-g", Model.GLYPH.failed, strings.t("notify.noteFailed"),
        note.title || note.app || strings.t("notes.untitled")])
    }
  }

  function streamStopped() {
    var quick = Date.now() - _streamStartedAt < 2000
    _quickStops = quick ? _quickStops + 1 : 0
    var before = connected ? snapshot : Model.emptyState()
    connected = false
    transition(before, Model.emptyState())
    // `sokki events` reconnects to the daemon by itself, so it only ends when
    // it crashes or is missing. Five quick ends in a row: give up quietly
    // until `omarchy-shell sokki.meetings reconnect` (the setup runs it).
    if (_quickStops < 5) restartTimer.start()
  }

  function reconnect() {
    _quickStops = 0
    restartTimer.stop()
    if (!stream.running) stream.running = true
  }

  Process {
    id: stream
    command: ["sokki", "events"]
    running: true
    onRunningChanged: {
      if (running) root._streamStartedAt = Date.now()
      else root.streamStopped()
    }
    stdout: SplitParser {
      onRead: function(line) { root.handleLine(line) }
    }
  }

  Timer {
    id: restartTimer
    interval: 3000
    onTriggered: if (!stream.running) stream.running = true
  }

  Process {
    id: notesProcess
    command: ["sokki", "notes", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var parsed = Model.parseNotes(text)
        if (parsed !== null) {
          root.notes = parsed
          root.notesLoaded = true
          root.refreshSpeakers()
        }
      }
    }
  }

  Process {
    id: speakersProcess
    property string meeting: ""
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var names = Model.parseUnnamedSpeakers(text)
        if (names === null) return
        root.speakersMeeting = speakersProcess.meeting
        root.unnamedSpeakers = names
      }
    }
  }

  Timer {
    interval: 1000
    repeat: true
    triggeredOnStart: true
    running: root.phase === "recording" || (root.phase === "asking" && root.panelOpen)
    onTriggered: root.now = Date.now()
  }

  onPanelOpenChanged: {
    if (panelOpen) {
      now = Date.now()
      refreshNotes()
    } else {
      quietPanel = false
      lastError = ""
    }
  }

  ShellIpc {
    target: root.pluginId

    function toggle(): void { root.togglePanel() }
    function open(): void { if (!root.panelOpen) root.openPanel(false); else root.quietPanel = false }
    function close(): void { root.closePanel() }
    function reconnect(): string { root.reconnect(); return "ok" }
    // English on purpose: an answer to a script, not text on screen.
    function status(): string { return root.connected ? root.phase : "offline" }
  }
}
