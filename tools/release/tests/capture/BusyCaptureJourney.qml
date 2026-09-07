import QtQuick

QtObject {
  required property var harness
  required property var captureWidget
  required property var journey
  readonly property var panel: harness.panel

  function begin() {
    if (captureWidget.selectedPeerId !== "pixel-8"
        || !captureWidget.clipboardBusy) {
      throw new Error("Busy interruption did not begin with pending B/P")
    }
    var command = journey.busyPhase === "awaiting_local_consent"
      ? ["simulate", "incoming-text", "active frozen A"]
      : ["send", "--clipboard", "--peer", "galaxy-tab", "--",
        "active frozen A"]
    harness.runNativeAction(command, 60)
  }

  function requireInterruptedPreparation() {
    var expectedPeer = journey.busyPhase === "awaiting_local_consent"
      ? "pixel-8" : "galaxy-tab"
    if (panel.activeShare.attachment.value !== "active frozen A"
        || panel.activeShare.peer.id !== expectedPeer
        || captureWidget.selectedPeerId !== "pixel-8") {
      throw new Error("Visible busy share changed frozen A or displayed P")
    }
    panel.choosePeer("galaxy-tab")
    if (captureWidget.selectedPeerId !== "pixel-8") {
      throw new Error("Busy recipient gesture changed displayed P")
    }
  }

  function finishCapture() {
    if (captureWidget.clipboardBusy) return
    if (!harness.failureObserved) {
      harness.failureObserved = true
      harness.failureSnapshots = 0
    }
    if (harness.failureSnapshots < 2) return
    if (!captureWidget.showPasteBadge
        || captureWidget.clipboardPreview !== journey.value
        || captureWidget.selectedPeerId !== "pixel-8"
        || panel.activeShareId.length > 0) {
      throw new Error("Finished active share replayed or discarded pending B/P")
    }
    panel.choosePeer(journey.peer)
    harness.step = 4
  }

  function advance() {
    if (harness.step < 60 || harness.step > 65) return false
    if (harness.step === 60 && harness.nativeActionDone) {
      if (journey.busyPhase === "transferring") {
        harness.runNativeAction(["status", "--json"], 65)
      } else harness.step = 61
    } else if (harness.step === 65 && harness.nativeActionDone) {
      var response = JSON.parse(harness.nativeOutput).response
      var share = response.snapshot.active_share
      harness.runNativeAction([
        "simulate", "peer-accept", String(share.id_string),
      ], 61)
    } else if (harness.step === 61 && harness.nativeActionDone
        && panel.phase === journey.busyPhase) {
      requireInterruptedPreparation()
      harness.externalShareId = panel.activeShareId
      var action = journey.busyPhase === "awaiting_local_consent"
        ? "reject" : "cancel"
      harness.runNativeAction(["share", action, harness.externalShareId], 62)
    } else if (harness.step === 62 && harness.nativeActionDone) {
      harness.runNativeAction([
        "share", "dismiss", harness.externalShareId,
      ], 63)
    } else if (harness.step === 63 && harness.nativeActionDone
        && panel.activeShareId.length === 0) {
      harness.releaseRead()
      harness.step = 64
    } else if (harness.step === 64) {
      finishCapture()
    }
    return true
  }
}
