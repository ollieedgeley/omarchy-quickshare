import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
  id: root
  property int checks: 0
  property int step: 0
  property var panel: null
  property int waitingChecks: 0
  property bool failureObserved: false
  property int failureSnapshots: 0
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

  function invalidate() {
    if (journey.invalidate === "close") widget.close()
    else if (journey.invalidate === "clear") widget.clearPasteBadge()
    else root.panel.cancel()
  }

  FileView {
    id: clipboardStarted
    path: journey.replacement || journey.invalidate || journey.peerChange
      || journey.explicitPending || journey.timeoutReplacement
      ? Quickshell.env("CLIPBOARD_STARTED") : ""
    printErrors: false
    onLoaded: {
      if (root.step !== 2 || text() !== "started" || root.panel.actionBusy) {
        return
      }
      if (journey.queuedReplacement) widget.readClipboard("preview", "")
      if (journey.invalidate) root.invalidate()
      else if (journey.peerChange) root.panel.choosePeer(journey.peer)
      else if (!journey.explicitPending) widget.readClipboard("preview", "")
      if (!journey.timeoutReplacement) releaseClipboard.running = true
      root.step = 3
    }
  }
  Process {
    id: releaseClipboard
    command: ["touch", Quickshell.env("CLIPBOARD_RELEASE")]
  }
  BarWidget { id: widget }

  FileView {
    id: submissionStarted
    path: journey.submissionMode ? Quickshell.env("SUBMISSION_STARTED") : ""
    printErrors: false
    onLoaded: {
      if (root.step !== 2 || text() !== "started") return
      if (journey.failSubmission) {
        removeCapture.running = true
        root.step = 6
        return
      }
      var next = journey.newer === "same"
        ? journey.value : "next independent B"
      widget.paste(next)
      releaseSubmission.running = true
      root.step = 3
    }
  }
  Process {
    id: releaseSubmission
    command: ["touch", Quickshell.env("SUBMISSION_RELEASE")]
  }

  Process {
    id: removeCapture
    command: ["rm", Quickshell.env("CAPTURE_FILE")]
    onExited: function(code) {
      if (code !== 0) throw new Error("Fixture file removal failed")
      releaseSubmission.running = true
      root.step = 7
    }
  }
  Connections {
    target: root.panel
    function onSnapshotChanged() {
      if (root.failureObserved) root.failureSnapshots += 1
    }
  }
  FileView {
    id: submissionAttempts
    path: journey.failSubmission ? Quickshell.env("SUBMISSION_ATTEMPTS") : ""
    printErrors: false
    onLoaded: {
      if (root.step !== 8) return
      if (text() !== "send\n") throw new Error("Failed dispatch retried itself")
      restoreCapture.running = true
      root.step = 9
    }
  }
  Process {
    id: restoreCapture
    command: [
      "cp", Quickshell.env("CAPTURE_RESTORE_FILE"),
      Quickshell.env("CAPTURE_FILE"),
    ]
    onExited: function(code) {
      if (code !== 0) throw new Error("Fixture file restore failed")
      root.panel.choosePeer(journey.peer)
      root.step = 10
    }
  }

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
        if (journey.closedPaste) {
          root.panel = root.findPanel(widget)
          widget.paste(journey.value)
          root.step = 5
          return
        }
        widget.open()
        root.panel = root.findPanel(widget)
        if (journey.captureFirst) root.paste()
        if (journey.explicitPending) widget.readClipboard("preview", "")
        root.step = 1
        if (journey.invalidate && !journey.automatic) {
          root.paste()
          widget.readClipboard("preview", "")
          root.step = 2
        }
      } else if (root.step === 1 && !root.panel.actionBusy) {
        root.panel.choosePeer("pixel-8")
        root.step = 2
      } else if (root.step === 2 && journey.submissionMode) {
        submissionStarted.reload()
      } else if (root.step === 2
          && (journey.replacement || journey.invalidate
            || journey.peerChange || journey.explicitPending
            || journey.timeoutReplacement)) {
        clipboardStarted.reload()
      } else if (root.step === 5 && !root.panel.actionBusy) {
        if (root.panel.activeShareId.length > 0 || !widget.opened
            || !widget.showPasteBadge || widget.selectedPeerId.length > 0
            || widget.clipboardPreview !== journey.value) {
          console.error("CLOSED_PASTE_HIDDEN_SUBMISSION")
          Qt.exit(3)
          return
        }
        console.log("HARNESS_OK closed Paste captures without selection")
        Qt.quit()
      } else if (root.step === 2 && journey.contentCase === "flag"
          && !root.panel.actionBusy && root.panel.activeShareId.length === 0) {
        console.error("LITERAL_NOT_ADMITTED")
        Qt.exit(3)
      } else if (root.step === 3 && journey.timeoutReplacement
          && root.panel.actionError.length > 0) {
        throw new Error("Expired old read overrode newer explicit capture")
      } else if (root.step === 2 && journey.readTimeout
          && !widget.clipboardBusy && root.panel.actionError.length > 0) {
        if (root.panel.activeShareId.length > 0 || widget.showPasteBadge) {
          throw new Error("Timed-out clipboard capture admitted content")
        }
        root.paste()
        root.step = 4
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
      } else if (root.step === 3 && journey.invalidate
          && !widget.clipboardBusy) {
        if (!widget.opened) widget.open()
        if (widget.showPasteBadge || widget.clipboardPreview.length > 0
            || root.panel.activeShareId.length > 0) {
          console.error("CLOSED_CAPTURE_RESTORED")
          Qt.exit(3)
          return
        }
        if (journey.recover) {
          if (root.panel.actionBusy) return
          root.paste()
          root.panel.choosePeer("galaxy-tab")
          root.step = 4
          return
        }
        console.log("HARNESS_OK closed capture cannot replay")
        Qt.quit()
      } else if (root.step === 3 && journey.failReplacement
          && !widget.clipboardBusy && root.panel.actionError.length > 0) {
        if (root.panel.activeShareId.length > 0 || widget.showPasteBadge) {
          throw new Error("Failed replacement restored superseded content")
        }
        console.log("HARNESS_OK failed replacement waits without fallback")
        Qt.quit()
      } else if (root.step === 7 && !root.panel.actionBusy
          && root.panel.actionError.length > 0) {
        if (!widget.showPasteBadge || widget.clipboardPreview !== journey.value
            || root.panel.activeShareId.length > 0) {
          throw new Error("Failed admission did not retain independent capture")
        }
        if (!root.failureObserved) {
          root.failureObserved = true
          root.failureSnapshots = 0
        }
        if (root.failureSnapshots < 2) return
        root.step = 8
        submissionAttempts.reload()
      } else if (root.step >= 2 && root.panel.activeShareId.length > 0) {
        var share = root.panel.activeShare
        var expected = journey.attachment
        var actual = share.attachment
        if (actual.type !== expected.type || actual.value !== expected.value
            || actual.name !== expected.name
            || actual.size_bytes !== expected.size_bytes
            || share.peer.id !== journey.peer) {
          console.error("CAPTURE_MISMATCH", JSON.stringify(share))
          Qt.exit(3)
          return
        }
        if (journey.newer) {
          if (root.panel.actionBusy) return
          var next = journey.newer === "same"
            ? journey.value : "next independent B"
          if (!widget.showPasteBadge || widget.clipboardPreview !== next) {
            console.error("INDEPENDENT_CAPTURE_CONSUMED")
            Qt.exit(3)
            return
          }
        }
        if (journey.consume) {
          if (root.panel.actionBusy) return
          if (widget.showPasteBadge || widget.clipboardPreview.length > 0) {
            console.error("ADMITTED_CAPTURE_RETAINED")
            Qt.exit(3)
            return
          }
        }
        console.log("HARNESS_OK captured A admitted to", journey.peer)
        Qt.quit()
      }
    }
  }
}
