import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Model.js" as Model

// Sokki in the bar. Hidden while nothing happens. A call that starts asks
// whether to record (the icon pulses and the panel opens by itself, without
// taking the keyboard); a recording shows a dot and the time; a note being
// written shows how far it is. Clicking opens the panel: the question, the
// recording's Stop, progress, and the last few notes.
//
// State comes from Service.qml (one `sokki events` stream for the whole
// shell); this file only draws it.
Panel {
  id: root
  moduleName: "sokki.meetings"
  // The service answers the plugin's IPC target.
  manageIpc: false

  readonly property var service: bar && bar.shell ? bar.shell.serviceFor("sokki.meetings") : null
  readonly property var current: service ? service.current : Model.emptyState()
  readonly property string phase: current.phase
  readonly property bool connected: !!service && service.connected
  readonly property double now: service ? service.now : Date.now()
  readonly property bool quiet: !!service && service.quietPanel

  // What the bar shows, most urgent first.
  readonly property string mode: {
    if (!service || !service.connected) return "idle"
    if (phase === "asking") return "asking"
    if (phase === "recording") return "recording"
    if (current.dictation === "listening") return "listening"
    if (current.processing) return "processing"
    if (current.download) return "downloading"
    if (current.dictation === "transcribing") return "transcribing"
    return "idle"
  }

  readonly property bool vertical: bar ? bar.vertical : false
  readonly property color foreground: Commons.Color.popups.text
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property color urgent: bar ? bar.urgent : Commons.Color.urgent
  readonly property string fontFamily: bar && bar.fontFamily ? bar.fontFamily : Style.font.family

  readonly property string elapsed: phase === "recording" ? Model.formatElapsed(now - current.sinceMs, Commons.I18n.formatNumber) : ""
  readonly property int secondsLeft: phase === "asking" ? Model.secondsLeft(current.expiresMs, now) : 0
  readonly property var notes: service ? service.notes : []
  // Speakers of the newest meeting still called "Speaker 2" (not while a
  // question or a recording needs the panel).
  readonly property var unnamed: service && phase === "idle" ? service.unnamedSpeakers : []
  // A name field has the keyboard: keys go to it, not the panel's cursor.
  property bool editing: false
  signal nameFocusRequested(int index)
  readonly property bool needsModels: connected && !current.modelsReady && !current.download

  // Keyboard cursor over the panel's actions and note rows.
  readonly property var targets: {
    var list = []
    if (phase === "asking") list.push("record", "dismiss")
    else if (phase === "recording") list.push("stop")
    else if (connected) list.push("start")
    if (needsModels) list.push("models")
    for (var j = 0; j < unnamed.length; j++) list.push("name:" + j)
    for (var i = 0; i < notes.length; i++) list.push("note:" + i)
    return list
  }
  property int cursor: 0
  property bool cursorActive: false
  readonly property string cursorTarget: cursorActive && cursor < targets.length ? targets[cursor] : ""

  visible: mode !== "idle" || opened
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  Commons.Translations { id: strings; directory: Qt.resolvedUrl("i18n") }

  function activate(target) {
    if (!service || !target) return
    if (target === "record" || target === "start") {
      service.record()
      close()
    } else if (target === "dismiss") {
      service.dismiss()
      close()
    } else if (target === "stop") {
      service.stop()
    } else if (target === "models") {
      service.installModels()
    } else if (target.indexOf("name:") === 0) {
      // The fields live in the panel's own scope; they listen for this.
      nameFocusRequested(Number(target.slice(5)))
    } else if (target.indexOf("note:") === 0) {
      var note = notes[Number(target.slice(5))]
      if (note) {
        service.openNote(note.id)
        close()
      }
    }
  }

  // Back from a name field to the panel's own keys.
  function endEditing() {
    editing = false
    Qt.callLater(function() { if (panel.loadedContent) panel.loadedContent.forceActiveFocus() })
  }

  function moveCursor(step) {
    if (!cursorActive) {
      cursorActive = true
      return
    }
    cursor = Math.max(0, Math.min(targets.length - 1, cursor + step))
  }

  function hover(target) {
    // The question pops up under wherever the pointer rests; letting that
    // move the selection would turn Enter into "Not now" (seen in the VM).
    // While asking, Enter records and Esc says not now; clicks still work.
    if (phase === "asking") return
    var index = targets.indexOf(target)
    if (index === -1) return
    cursor = index
    cursorActive = true
  }

  function barGlyph() {
    switch (mode) {
    case "recording": return Model.GLYPH.record
    case "asking":
    case "listening":
    case "transcribing": return Model.GLYPH.microphone
    case "processing": return Model.progressGlyph(current.processing.progress)
    case "downloading": return Model.progressGlyph(Model.downloadFraction(current.download))
    }
    return Model.GLYPH.microphoneIdle
  }

  function tooltip() {
    switch (mode) {
    case "asking": return strings.t("prompt.title")
    case "recording": return strings.t("tooltip.recording")
    case "listening": return strings.t("tooltip.listening")
    case "transcribing": return strings.t("tooltip.transcribing")
    case "processing": return strings.t("processing.title")
    case "downloading": return strings.t("models.downloading")
    }
    return strings.t("idle.title")
  }

  function noteLabel(note) {
    return note.title || note.app || strings.t("notes.untitled")
  }

  function noteMeta(note) {
    var date = Model.noteDate(note)
    var when = date ? date.toLocaleString(Qt.locale(Commons.I18n.formatLocale), Locale.ShortFormat) : ""
    if (note.status !== "done") return strings.t("notes.status." + note.status) + (when ? " · " + when : "")
    return strings.t("notes.meta").arg(when).arg(Commons.I18n.formatNumber(Model.durationMinutes(note)))
  }

  onOpenedChanged: {
    // Typing a name never outlives the panel: closed and reopened, the keys
    // belong to the panel again (a stuck field swallowed Enter on the next
    // record question in the VM).
    editing = false
    if (service) service.panelOpen = opened
    if (opened) {
      cursor = 0
      // A question is answered with Enter: the cursor starts on Record,
      // once the panel has the keyboard.
      cursorActive = phase === "asking" && !quiet
    }
  }
  onPhaseChanged: {
    editing = false
    cursor = 0
    cursorActive = opened && phase === "asking" && !quiet
  }
  onQuietChanged: if (opened && !quiet && phase === "asking") cursorActive = true
  onTargetsChanged: if (cursor >= targets.length) cursor = Math.max(0, targets.length - 1)

  Item {
    id: pulse
    anchors.fill: parent

    // A call is asking: the icon breathes until the question is answered.
    SequentialAnimation on opacity {
      id: breathing
      running: root.mode === "asking" && root.visible && !Style.reduceMotion
      loops: Animation.Infinite
      onRunningChanged: if (!running) pulse.opacity = 1
      NumberAnimation { from: 1.0; to: 0.35; duration: Style.duration(900); easing.type: Easing.InOutSine }
      NumberAnimation { from: 0.35; to: 1.0; duration: Style.duration(900); easing.type: Easing.InOutSine }
    }

    BarIconButton {
      id: button
      anchors.fill: parent
      bar: root.bar
      text: root.barGlyph()
      // The time beside the dot, the way the battery shows its percentage.
      labelText: root.mode === "recording" && !root.vertical ? root.elapsed : ""
      slotSize: labelText !== "" ? Style.bar.iconSlot + labelMetrics.advanceWidth + Style.space(4) : Style.bar.iconSlot
      active: root.mode === "recording" || root.mode === "listening"
      dimmed: root.mode === "processing" || root.mode === "downloading" || root.mode === "transcribing"
      tooltipText: root.tooltip()
      onPressed: function(mouseButton) {
        if (mouseButton !== Qt.LeftButton || !root.service) return
        if (root.opened && root.quiet) root.service.quietPanel = false
        else {
          root.service.quietPanel = false
          root.toggle()
        }
      }
    }

    TextMetrics {
      id: labelMetrics
      font.family: root.fontFamily
      font.pixelSize: Style.bar.iconFont
      text: button.labelText
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    // Opened by a call rather than by the person: shown, but it neither
    // takes the keyboard (a stray Enter in the call's chat must not start a
    // recording) nor closes on a click elsewhere. `yielding` arrived in
    // Omarchy 4.x after 4.0; older shells reject the property outright, so
    // it is bound only where it exists.
    Component.onCompleted: {
      if ("yielding" in panel)
        panel.yielding = Qt.binding(function() { return root.opened && root.quiet })
    }
    focusTarget: panel.loadedContent
    contentWidth: panel.fittedContentWidth(Style.space(340))
    contentHeight: panel.fittedContentHeight(panel.loadedContentHeight)

    content: PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: root.editing
      implicitHeight: column.implicitHeight
      onMoveRequested: function(dx, dy) { root.moveCursor(dy !== 0 ? dy : dx) }
      onActivateRequested: {
        if (root.cursorActive) root.activate(root.targets[root.cursor])
      }
      onCloseRequested: {
        // Escape on the question is "Not now".
        if (root.phase === "asking") root.activate("dismiss")
        else root.close()
      }
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Column {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        spacing: Style.spacing.panelGap

        // ---------- The question ----------
        Column {
          visible: root.phase === "asking"
          width: parent.width
          spacing: Style.space(10)

          PanelHero {
            title: strings.t("prompt.title")
            detail: strings.t("prompt.secondsLeft").arg(Commons.I18n.formatNumber(root.secondsLeft))
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconComponent: Component {
              Text {
                textFormat: Text.PlainText
                text: Model.GLYPH.microphone
                color: root.urgent
                font.family: root.fontFamily
                font.pixelSize: Style.font.display
              }
            }
          }

          PlainText {
            width: parent.width
            text: strings.t("prompt.body").arg(root.current.app || strings.t("prompt.anApp"))
            wrapMode: Text.WordWrap
          }

          Track {
            width: parent.width
            fraction: Model.askFraction(root.current, root.now)
            draining: true
          }

          RowLayout {
            width: parent.width
            spacing: Style.space(8)

            Button {
              Layout.fillWidth: true
              Layout.preferredWidth: 1
              text: strings.t("prompt.record")
              iconText: Model.GLYPH.record
              bordered: true
              foreground: root.foreground
              fontFamily: root.fontFamily
              hasCursor: root.cursorTarget === "record"
              onHovered: function(on) { if (on) root.hover("record") }
              onClicked: root.activate("record")
            }

            Button {
              Layout.fillWidth: true
              Layout.preferredWidth: 1
              text: strings.t("prompt.notNow")
              bordered: true
              foreground: root.foreground
              fontFamily: root.fontFamily
              hasCursor: root.cursorTarget === "dismiss"
              onHovered: function(on) { if (on) root.hover("dismiss") }
              onClicked: root.activate("dismiss")
            }
          }
        }

        // ---------- Recording ----------
        Column {
          visible: root.phase === "recording"
          width: parent.width
          spacing: Style.space(10)

          PanelHero {
            title: strings.t("recording.title")
            meta: root.current.callAudio ? root.current.app : strings.t("recording.micOnly")
            detail: root.elapsed
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconComponent: Component {
              Text {
                textFormat: Text.PlainText
                text: Model.GLYPH.record
                color: root.urgent
                font.family: root.fontFamily
                font.pixelSize: Style.font.display
              }
            }
          }

          Button {
            width: parent.width
            text: strings.t("recording.stop")
            iconText: Model.GLYPH.stop
            bordered: true
            foreground: root.foreground
            fontFamily: root.fontFamily
            hasCursor: root.cursorTarget === "stop"
            onHovered: function(on) { if (on) root.hover("stop") }
            onClicked: root.activate("stop")
          }
        }

        // ---------- Idle ----------
        Column {
          visible: root.phase === "idle"
          width: parent.width
          spacing: Style.space(10)

          PanelHero {
            title: strings.t("idle.title")
            meta: !root.connected ? strings.t("idle.offline")
              : (root.current.dictation === "listening" ? strings.t("tooltip.listening") : "")
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconComponent: Component {
              Text {
                textFormat: Text.PlainText
                text: Model.GLYPH.microphoneIdle
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.display
              }
            }
          }

          Button {
            visible: root.connected
            width: parent.width
            text: strings.t("idle.record")
            iconText: Model.GLYPH.record
            bordered: true
            foreground: root.foreground
            fontFamily: root.fontFamily
            hasCursor: root.cursorTarget === "start"
            onHovered: function(on) { if (on) root.hover("start") }
            onClicked: root.activate("start")
          }
        }

        // ---------- A note being written ----------
        Column {
          visible: !!root.current.processing
          width: parent.width
          spacing: Style.space(6)

          RowLayout {
            width: parent.width
            spacing: Style.space(12)

            PlainText {
              Layout.fillWidth: true
              text: root.current.processing ? strings.t(Model.stageKey(root.current.processing.stage)) : ""
              elide: Text.ElideRight
            }

            PlainText {
              visible: root.current.processing && root.current.processing.progress !== null
              text: root.current.processing && root.current.processing.progress !== null
                ? Commons.I18n.formatNumber(Math.round(root.current.processing.progress * 100)) + "%"
                : ""
              color: root.dim
            }
          }

          Track {
            width: parent.width
            fraction: root.current.processing ? root.current.processing.progress : null
          }

          PlainText {
            visible: root.current.queued > 0
            text: strings.tn("processing.queued", root.current.queued).arg(Commons.I18n.formatNumber(root.current.queued))
            color: root.dim
            font.pixelSize: Style.font.caption
          }
        }

        // ---------- Models ----------
        Column {
          visible: !!root.current.download
          width: parent.width
          spacing: Style.space(6)

          PlainText {
            width: parent.width
            text: strings.t("models.downloading")
            elide: Text.ElideRight
          }

          Track {
            width: parent.width
            fraction: Model.downloadFraction(root.current.download)
          }
        }

        Button {
          visible: root.needsModels
          width: parent.width
          text: strings.t("models.install")
          iconText: Model.GLYPH.download
          bordered: true
          foreground: root.foreground
          fontFamily: root.fontFamily
          hasCursor: root.cursorTarget === "models"
          onHovered: function(on) { if (on) root.hover("models") }
          onClicked: root.activate("models")
        }

        PlainText {
          visible: !!root.service && root.service.lastError !== "" && root.service.now - root.service.lastErrorAt < 60000
          width: parent.width
          text: root.service ? root.service.lastError : ""
          color: root.urgent
          font.pixelSize: Style.font.bodySmall
          wrapMode: Text.WordWrap
        }

        // ---------- Who was speaking? ----------
        PanelSeparator {
          visible: root.unnamed.length > 0
          foreground: root.foreground
        }

        Column {
          visible: root.unnamed.length > 0
          width: parent.width
          spacing: Style.space(6)

          PanelSectionHeader {
            text: strings.t("speakers.header")
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          PlainText {
            width: parent.width
            text: strings.t("speakers.hint")
            color: root.dim
            font.pixelSize: Style.font.bodySmall
            wrapMode: Text.WordWrap
          }

          Repeater {
            id: nameFields
            model: root.unnamed

            RowLayout {
              id: nameRow
              required property var modelData
              required property int index
              width: parent.width
              spacing: Style.space(10)

              // As Omarchy's clock panel does: hand the keys to the field
              // first, then focus it on the next tick.
              Connections {
                target: root
                function onNameFocusRequested(index) { if (index === nameRow.index) nameRow.focusField() }
              }

              function focusField() {
                root.editing = true
                Qt.callLater(function() { field.selectAll(); field.forceActiveFocus() })
              }


              PlainText {
                text: nameRow.modelData
                color: root.dim
                Layout.preferredWidth: Style.space(90)
              }

              TextField {
                id: field
                Layout.fillWidth: true
                placeholderText: strings.t("speakers.placeholder")
                foreground: root.foreground
                hasCursor: root.cursorTarget === "name:" + nameRow.index
                // A click into the field hands it the keys too.
                onActiveFocusChanged: if (activeFocus) root.editing = true
                Keys.onPressed: function(event) {
                  if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                    // Naming removes this row, so give the keys back first
                    // and save on the next tick (calling into a removed row
                    // left the panel stuck in typing mode).
                    var who = nameRow.modelData
                    var name = text
                    root.endEditing()
                    Qt.callLater(function() { root.service.nameSpeaker(who, name) })
                    event.accepted = true
                  } else if (event.key === Qt.Key_Escape) {
                    root.endEditing()
                    event.accepted = true
                  }
                }
              }
            }
          }
        }

        // ---------- Recent notes ----------
        PanelSeparator {
          visible: root.connected
          foreground: root.foreground
        }

        Column {
          visible: root.connected
          width: parent.width
          spacing: Style.space(6)

          PanelSectionHeader {
            text: strings.t("notes.header")
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          PlainText {
            visible: root.notes.length === 0
            width: parent.width
            text: strings.t("notes.empty")
            color: root.dim
            horizontalAlignment: Text.AlignHCenter
          }

          Repeater {
            model: root.notes

            NoteRow {
              required property var modelData
              required property int index
              width: column.width
              note: modelData
              target: "note:" + index
            }
          }
        }
      }
    }
  }

  component PlainText: Text {
    textFormat: Text.PlainText
    color: root.foreground
    font.family: root.fontFamily
    font.pixelSize: Style.font.body
  }

  // A thin bar: how far something is (fraction 0–1), or a slow breath when
  // that is not known (fraction null).
  component Track: Item {
    id: track
    property var fraction: null
    property bool draining: false
    readonly property bool known: fraction !== null && fraction !== undefined

    implicitHeight: Math.max(Style.space(4), Math.round(Style.spacing.controlHeight * 0.14))

    Rectangle {
      anchors.fill: parent
      radius: height / 2
      color: Qt.alpha(root.foreground, 0.15)
    }

    Rectangle {
      id: fill
      height: parent.height
      radius: height / 2
      color: root.foreground
      width: track.known ? Math.max(parent.height, parent.width * track.fraction) : parent.width
      opacity: track.known ? 1 : 0.35

      // A countdown runs down evenly; progress settles quickly.
      Behavior on width {
        NumberAnimation {
          duration: Style.duration(track.draining ? 1000 : 160)
          easing.type: track.draining ? Easing.Linear : Easing.Bezier
          easing.bezierCurve: Style.easeOut
        }
      }

      SequentialAnimation on opacity {
        running: !track.known && root.opened && !Style.reduceMotion
        loops: Animation.Infinite
        onRunningChanged: if (!running) fill.opacity = Qt.binding(function() { return track.known ? 1 : 0.35 })
        NumberAnimation { from: 0.35; to: 0.12; duration: Style.duration(900); easing.type: Easing.InOutSine }
        NumberAnimation { from: 0.12; to: 0.35; duration: Style.duration(900); easing.type: Easing.InOutSine }
      }
    }
  }

  component NoteRow: CursorSurface {
    id: row
    property var note: null
    property string target: ""

    hasCursor: root.cursorTarget === target
    foreground: root.foreground
    implicitHeight: rowContent.implicitHeight + Style.spacing.rowPaddingX

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onEntered: root.hover(row.target)
      onClicked: root.activate(row.target)
    }

    RowLayout {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(8)

      PlainText {
        text: Model.noteGlyph(row.note)
        color: row.note && row.note.status === "failed" ? root.urgent : root.foreground
        font.pixelSize: Style.font.icon
        Layout.alignment: Qt.AlignVCenter
      }

      ColumnLayout {
        id: rowContent
        Layout.fillWidth: true
        spacing: Style.space(1)

        PlainText {
          Layout.fillWidth: true
          text: row.note ? root.noteLabel(row.note) : ""
          elide: Text.ElideRight
        }

        PlainText {
          Layout.fillWidth: true
          text: row.note ? root.noteMeta(row.note) : ""
          color: root.dim
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
      }
    }
  }
}
