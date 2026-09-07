import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
  id: root
  property int checks: 0
  property int step: 0
  property var panel: null
  property int waitingChecks: 0
  readonly property var journey:
    JSON.parse(Quickshell.env("CAPTURE_JOURNEY"))

  function findPanel(item) {
    if (item.choosePeer !== undefined) return item
    var children = item.children || []
    for (var i = 0; i < children.length; i++) {
      var found = findPanel(children[i])
      if (found) return found
    }
    return null
  }


  function paste() {
    if (widget.paste(journey.value) !== "ok"
        || widget.clipboardPreview !== journey.value
        || !widget.showPasteBadge) {
      throw new Error("Paste did not display captured A")
    }
  }

  FileView {
    id: clipboardStarted
    path: journey.replacement ? Quickshell.env("CLIPBOARD_STARTED") : ""
    printErrors: false
    onLoaded: {
      if (root.step !== 2 || text() !== "started") return
      widget.readClipboard("preview", "")
      releaseClipboard.running = true
      root.step = 3
    }
  }
  Process {
    id: releaseClipboard
    command: ["touch", Quickshell.env("CLIPBOARD_RELEASE")]
  }
  BarWidget { id: widget }

  Timer {
    interval: 25
    repeat: true
    running: true
    onTriggered: {
      root.checks += 1
      if (root.checks > 240) {
        console.error("CAPTURE_TIMEOUT", root.step)
        Qt.exit(2)
        return
      }
      if (!widget.protocolReady) return
      if (root.step === 0) {
        widget.open()
        root.panel = root.findPanel(widget)
        if (journey.captureFirst) root.paste()
        root.step = 1
      } else if (root.step === 1 && !root.panel.actionBusy) {
        root.panel.choosePeer("pixel-8")
        root.step = 2
      } else if (root.step === 2 && journey.replacement) {
        clipboardStarted.reload()
      } else if (root.step === 2 && !journey.captureFirst
          && !journey.automatic) {
        if (root.panel.activeShareId.length > 0 || widget.clipboardBusy) {
          console.error("OFF_SELECTION_READ_OR_SENT")
          Qt.exit(3)
          return
        }
        root.waitingChecks += 1
        if (root.waitingChecks < 8) return
        root.paste()
        root.step = 3
      } else if (root.step === 3 && journey.failReplacement
          && !widget.clipboardBusy && root.panel.actionError.length > 0) {
        if (root.panel.activeShareId.length > 0 || widget.showPasteBadge) {
          throw new Error("Failed replacement restored superseded content")
        }
        console.log("HARNESS_OK failed replacement waits without fallback")
        Qt.quit()
      } else if (root.step >= 2 && root.panel.activeShareId.length > 0) {
        var share = root.panel.activeShare
        var expected = journey.attachment
        var actual = share.attachment
        if (actual.type !== expected.type || actual.value !== expected.value
            || actual.name !== expected.name
            || actual.size_bytes !== expected.size_bytes
            || share.peer.id !== "pixel-8") {
          console.error("CAPTURE_MISMATCH", JSON.stringify(share))
          Qt.exit(3)
          return
        }
        console.log("HARNESS_OK captured A admitted to pixel-8")
        Qt.quit()
      }
    }
  }
}
