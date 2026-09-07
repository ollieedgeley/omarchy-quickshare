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
  property string lastPeerId: ""
  property bool duplicateCompletionObserved: false
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
      || journey.providedEmpty || journey.explicitFailure
      || journey.peerProjection || journey.duplicates
      || (journey.preference && journey.automatic)
      ? Quickshell.env("CLIPBOARD_STARTED") : ""
    printErrors: false
    onLoaded: {
      if (root.step !== 2 || text() !== "started" || root.panel.actionBusy) {
        return
      }
      if (journey.duplicates) {
        root.panel.choosePeer(journey.peer)
        root.panel.choosePeer(journey.peer)
        widget.open()
        widget.open()
        releaseClipboard.running = true
        root.step = 3
        return
      }
      if (journey.peerProjection) {
        root.lastPeerId = root.panel.peers[root.panel.peers.length - 1].id
        activateProjection.running = true
        root.step = 14
        return
      }
      if (journey.preference) {
        if (journey.preference === "disable-queued") {
          widget.readClipboard("preview", "")
        }
        changePreference.running = true
        root.step = 12
        return
      }
      if (journey.queuedReplacement) widget.readClipboard("preview", "")
      if (journey.providedEmpty) {
        if (widget.paste("") !== "empty") {
          throw new Error("Provided empty value did not report failure")
        }
      } else if (journey.invalidate) root.invalidate()
      else if (journey.peerChange) root.panel.choosePeer(journey.peer)
      else if (!journey.explicitPending && !journey.explicitFailure) {
        widget.readClipboard("preview", "")
      }
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
    path: journey.failSubmission || journey.peerProjection
      ? Quickshell.env("SUBMISSION_ATTEMPTS") : ""
    printErrors: false
    onLoaded: {
      if (root.step === 16) {
        if (text() !== "") {
          console.error("Vanished recipient triggered submission")
          Qt.exit(3)
          return
        }
        root.panel.choosePeer(journey.peer)
        root.step = 17
        return
      }
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

  Process {
    id: changePreference
    command: [
      "env", "omarchy-quickshare", "config", "set", "read_clipboard_on_select",
      String(!journey.automatic),
    ]
    onExited: function(code) {
      if (code !== 0) {
        console.error("Live preference change failed")
        Qt.exit(3)
      }
    }
  }

  Process {
    id: activateProjection
    command: ["touch", Quickshell.env("PROJECTION_ACTIVE")]
  }

  Process {
    id: configurePreferred
    command: ["env", "omarchy-quickshare", "peer", "pin", "pixel-8"]
    onExited: function(code) {
      if (code !== 0) {
        console.error("Preferred peer configuration failed")
        Qt.exit(3)
        return
      }
      activateProjection.running = true
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
        console.error("CAPTURE_STATE", JSON.stringify({
          selected: widget.selectedPeerId,
          armed: widget.selectionArmed,
          capture: widget.clipboardPreview,
          clipboardBusy: widget.clipboardBusy,
          currentRead: widget.clipboardAction,
          queuedRead: widget.pendingClipboardAction,
          active: root.panel ? root.panel.activeShare : null,
          error: root.panel ? root.panel.actionError : "",
          actionBusy: root.panel ? root.panel.actionBusy : false,
        }))
        Qt.exit(2)
        return
      }
      if (!widget.protocolReady) return
      if (journey.explicitFailure && root.step >= 2 && root.step < 11
          && root.panel.activeShareId.length > 0) {
        console.error("Prior capture dispatched during explicit replacement")
        Qt.exit(3)
        return
      }
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
        if (journey.explicitPending || journey.explicitFailure) {
          widget.readClipboard("preview", "")
        }
        root.step = 1
        if (journey.invalidate && !journey.automatic) {
          root.paste()
          widget.readClipboard("preview", "")
          root.step = 2
        }
      } else if (root.step === 1 && !root.panel.actionBusy) {
        if (journey.peerProjection === "appear") {
          if (root.panel.peers.some(function(peer) {
            return peer.id === "pixel-8"
          })) {
            console.error("Preferred arrival fixture did not hide P")
            Qt.exit(3)
            return
          }
          configurePreferred.running = true
          root.step = 18
          return
        }
        root.panel.choosePeer("pixel-8")
        root.step = 2
      } else if (root.step === 18) {
        var preferred = root.panel.peers.find(function(peer) {
          return peer.id === "pixel-8"
        })
        if (!preferred || !preferred.pinned) return
        if (widget.clipboardBusy || root.panel.activeShareId.length > 0
            || widget.selectedPeerId.length > 0 || !widget.showPasteBadge
            || widget.clipboardPreview !== journey.value) {
          console.error("Preferred peer appearance captured or dispatched")
          Qt.exit(3)
          return
        }
        if (!root.failureObserved) {
          root.failureObserved = true
          root.failureSnapshots = 0
        }
        if (root.failureSnapshots < 2) return
        root.panel.choosePeer(journey.peer)
        root.step = 4
      } else if (root.step === 2 && journey.preference === "enable") {
        changePreference.running = true
        root.step = 12
      } else if (root.step === 14) {
        var peers = root.panel.peers
        var hasP = peers.some(function(peer) { return peer.id === "pixel-8" })
        var replacement = peers.some(function(peer) {
          return peer.id === "replacement-for-pixel-8"
        })
        if (journey.peerProjection === "reorder") {
          if (peers[0].id !== root.lastPeerId) return
          releaseClipboard.running = true
          root.step = 4
          return
        }
        if (hasP || (journey.peerProjection === "replace" && !replacement)) {
          return
        }
        releaseClipboard.running = true
        root.step = 15
      } else if (root.step === 15 && !widget.clipboardBusy) {
        if (root.panel.activeShareId.length > 0 || !widget.showPasteBadge
            || widget.clipboardPreview !== journey.value) {
          console.error("Lost recipient dispatched or discarded capture")
          Qt.exit(3)
          return
        }
        if (!root.failureObserved) {
          root.failureObserved = true
          root.failureSnapshots = 0
        }
        if (root.failureSnapshots < 2) return
        root.step = 16
        submissionAttempts.reload()
      } else if (root.step === 12) {
        if (widget.readClipboardOnSelect !== !journey.automatic) return
        if (journey.automatic) {
          releaseClipboard.running = true
          root.step = journey.preference === "disable" ? 13 : 4
          return
        }
        root.step = 13
      } else if (root.step === 13) {
        if (journey.automatic && widget.clipboardBusy) return
        if (widget.clipboardBusy || root.panel.activeShareId.length > 0
            || widget.showPasteBadge) {
          console.error("Preference change triggered capture or share")
          Qt.exit(3)
          return
        }
        if (!root.failureObserved) {
          root.failureObserved = true
          root.failureSnapshots = 0
        }
        if (root.failureSnapshots < 2) return
        root.paste()
        root.step = 4
      } else if (root.step === 2 && journey.submissionMode) {
        submissionStarted.reload()
      } else if (root.step === 2
          && (journey.replacement || journey.invalidate
            || journey.peerChange || journey.explicitPending
            || journey.timeoutReplacement || journey.providedEmpty
            || journey.explicitFailure
            || journey.peerProjection || journey.duplicates
            || (journey.preference && journey.automatic))) {
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
      } else if (root.step === 3 && journey.explicitFailure
          && !widget.clipboardBusy && root.panel.actionError.length > 0) {
        if (!widget.showPasteBadge
            || widget.clipboardPreview !== journey.value) {
          throw new Error("Failed explicit replacement discarded prior capture")
        }
        if (!root.failureObserved) {
          root.failureObserved = true
          root.failureSnapshots = 0
        }
        if (root.failureSnapshots < 2) return
        root.panel.choosePeer(journey.peer)
        root.step = 11
      } else if (root.step === 3 && journey.providedEmpty
          && !widget.clipboardBusy && root.panel.actionError.length > 0) {
        if (root.panel.activeShareId.length > 0 || widget.showPasteBadge) {
          throw new Error("Provided empty value restored superseded capture")
        }
        if (!root.failureObserved) {
          root.failureObserved = true
          root.failureSnapshots = 0
        }
        if (root.failureSnapshots < 2) return
        root.paste()
        root.step = 4
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
      } else if (root.step === 2 && journey.invalidRead
          && !widget.clipboardBusy && root.panel.actionError.length > 0) {
        if (root.panel.activeShareId.length > 0 || widget.showPasteBadge) {
          throw new Error("Invalid clipboard capture admitted content")
        }
        if (!root.failureObserved) {
          root.failureObserved = true
          root.failureSnapshots = 0
        }
        if (root.failureSnapshots < 2) return
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
        if (journey.duplicates) {
          if (root.panel.actionBusy) return
          if (!root.duplicateCompletionObserved) {
            widget.paste(journey.value)
            widget.paste(journey.value)
            root.panel.choosePeer(journey.peer)
            root.panel.choosePeer(journey.peer)
            widget.open()
            widget.open()
            root.failureObserved = true
            root.failureSnapshots = 0
            root.duplicateCompletionObserved = true
            return
          }
          if (root.failureSnapshots < 2) return
          if (!widget.showPasteBadge
              || widget.clipboardPreview !== journey.value) {
            console.error("Duplicate events consumed independent next capture")
            Qt.exit(3)
            return
          }
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
