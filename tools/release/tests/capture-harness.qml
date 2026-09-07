import QtQuick
import Quickshell

ShellRoot {
  id: root
  property int checks: 0
  property int step: 0
  property var panel: null
  property int waitingChecks: 0
  readonly property bool captureFirst:
    Quickshell.env("CAPTURE_FIRST") === "true"

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
    if (widget.paste("captured A\nexact bytes") !== "ok"
        || widget.clipboardPreview !== "captured A\nexact bytes"
        || !widget.showPasteBadge) {
      throw new Error("Paste did not display captured A")
    }
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
        if (root.captureFirst) root.paste()
        root.step = 1
      } else if (root.step === 1 && !root.panel.actionBusy) {
        root.panel.choosePeer("pixel-8")
        root.step = 2
      } else if (root.step === 2 && !root.captureFirst) {
        if (root.panel.activeShareId.length > 0 || widget.clipboardBusy) {
          console.error("OFF_SELECTION_READ_OR_SENT")
          Qt.exit(3)
          return
        }
        root.waitingChecks += 1
        if (root.waitingChecks < 8) return
        root.paste()
        root.step = 3
      } else if (root.step >= 2 && root.panel.activeShareId.length > 0) {
        var share = root.panel.activeShare
        if (share.attachment.value !== "captured A\nexact bytes"
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
