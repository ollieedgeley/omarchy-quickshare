import QtQuick
import Quickshell

ShellRoot {
  id: root

  readonly property string exactShareId: "18446744073709551615"
  property string acceptedShare: ""
  property string cancelledShare: ""
  property string dismissedShare: ""
  property int discoveryRequests: 0
  property int stopDiscoveryRequests: 0
  property string pinnedPeer: ""
  property bool pinValue: false
  property string rejectedShare: ""
  property string selectedPeer: ""
  property string selectedShare: ""
  property bool visibilityRequested: false

  function rendersPlainText(item, expected) {
    if (item.visible === false) return false
    if (item.text !== undefined && String(item.text) === expected) {
      return item.textFormat === Text.PlainText
    }
    var children = item.children || []
    for (var index = 0; index < children.length; index += 1) {
      if (rendersPlainText(children[index], expected)) return true
    }
    return false
  }

  function outboundSnapshot(phase, attachment, peers) {
    return {
      "active_share": {
        "attachment": attachment,
        "direction": "outbound",
        "id": 7,
        "id_string": root.exactShareId,
        "phase": phase,
        "total_bytes": 2048,
        "transferred_bytes": 512,
      },
      "discovery": "searching",
      "peers": peers || [],
      "visibility": "closed",
    }
  }

  function incomingSnapshot(phase, transferred) {
    return {
      "active_share": {
        "attachment": {
          "type": "file",
          "mime_type": "image/jpeg",
          "name": "<b>photo.jpg</b>",
          "size_bytes": 4096,
        },
        "direction": "inbound",
        "id": 8,
        "id_string": root.exactShareId,
        "peer": {
          "id": "pixel-8",
          "name": "<img src=x onerror=alert(1)>",
          "pinned": false,
        },
        "phase": phase,
        "verification_code": "0427",
        "total_bytes": 4096,
        "transferred_bytes": transferred,
      },
      "peers": [],
      "visibility": "open",
    }
  }

  function verifyAttachmentsAndPeerChoice() {
    panel.snapshot = outboundSnapshot(
      "waiting_for_peer",
      {"type": "url", "value": "https://example.test/<b>unsafe</b>"},
      [],
    )
    var preview = panel.viewState === "peer_choice"
      && panel.previewIcon === ""
      && panel.previewText === "https://example.test/<b>unsafe</b>"
      && rendersPlainText(panel, "https://example.test/<b>unsafe</b>")

    panel.snapshot = outboundSnapshot(
      "waiting_for_peer",
      {"type": "android_app", "name": "Quick Share"},
      [],
    )
    var apkType = panel.previewIcon === ""
    panel.snapshot = outboundSnapshot(
      "waiting_for_peer",
      {"type": "file", "name": "quick-share.apk"},
      [],
    )
    var apkName = panel.previewIcon === ""

    panel.snapshot = outboundSnapshot(
      "waiting_for_peer",
      {"type": "file", "name": "song.ogg"},
      [
        {"id": "pixel-8", "name": "<b>Ollie's Pixel</b>", "pinned": false},
        {"id": "tablet", "name": "Tablet", "pinned": true},
      ],
    )
    var typed = panel.previewIcon === "󰝚"
      && panel.peers.length === 2
      && panel.viewState === "peer_choice"
      && rendersPlainText(panel, "<b>Ollie's Pixel</b>")
    panel.choosePeer("pixel-8")
    panel.togglePin("pixel-8", false)
    var exactAction = root.selectedShare === root.exactShareId
      && root.selectedPeer === "pixel-8"
      && root.pinnedPeer === "pixel-8"
      && root.pinValue

    panel.togglePin("tablet", true)
    return preview && apkType && apkName && typed && exactAction
      && !root.pinValue
  }

  function verifyDiscovery() {
    var snapshot = outboundSnapshot(
      "waiting_for_peer",
      {"type": "text", "value": "hello"},
      [],
    )
    panel.snapshot = snapshot
    var searching = panel.discoveryMessage === "Searching for peers…"
    panel.toggleDiscovery()
    var expiredSnapshot = outboundSnapshot(
      "waiting_for_peer",
      {"type": "text", "value": "hello"},
      [],
    )
    expiredSnapshot.discovery = "timed_out"
    panel.snapshot = expiredSnapshot
    var expired = panel.discoveryMessage === "No peers found"
    panel.toggleDiscovery()
    return searching && expired && root.stopDiscoveryRequests === 1
      && root.discoveryRequests === 1
  }

  function verifyConsentAndWaiting() {
    panel.snapshot = incomingSnapshot("awaiting_local_consent", 0)
    var consent = panel.viewState === "consent"
      && panel.previewText === "<b>photo.jpg</b>"
      && panel.consentPin === "0427"
      && rendersPlainText(panel, "<b>photo.jpg</b>")
      && rendersPlainText(panel, "<img src=x onerror=alert(1)>")
      && panel.peerName === "<img src=x onerror=alert(1)>"
    panel.accept()
    panel.reject()

    var waiting = outboundSnapshot(
      "awaiting_peer_consent",
      {"type": "file", "mime_type": "video/mp4", "name": "clip.mp4"},
      [],
    )
    waiting.active_share.verification_code = "7391"
    panel.snapshot = waiting
    return consent
      && root.acceptedShare === root.exactShareId
      && root.rejectedShare === root.exactShareId
      && panel.viewState === "waiting"
      && panel.previewIcon === ""
      && panel.consentPin === "7391"
      && rendersPlainText(panel, "PIN 7391")
    }

  function verifyTransfer() {
    var current = incomingSnapshot("transferring", 1024)
    current.active_share.remaining_seconds = 65
    panel.snapshot = current
    var transfer = panel.viewState === "transfer"
      && panel.progressPercent === 25
      && panel.progressText === "1 KB / 4 KB"
      && panel.etaText === "1m 5s remaining"

    panel.snapshot = incomingSnapshot("transferring", 1024)
    var optional = panel.etaText === "Estimating time remaining…"
    panel.cancel()
    return transfer && optional
      && root.cancelledShare === root.exactShareId
  }

  function verifyTerminalAndIdle() {
    panel.snapshot = incomingSnapshot("completed", 4096)
    var complete = panel.viewState === "terminal"
      && panel.terminalTitle === "Received"
      && panel.terminalDetail === ""
    panel.dismiss()

    var failure = incomingSnapshot("failed", 1024)
    failure.active_share.terminal_reason = "<b>timed_out</b>"
    failure.active_share.recovery_guidance
      = "<img src=x onerror=alert(1)>"
    panel.snapshot = failure
    var detail = "<b>timed_out</b>\n<img src=x onerror=alert(1)>"
    var failed = panel.viewState === "terminal"
      && panel.terminalTitle === "Could not receive"
      && panel.terminalDetail === detail
      && rendersPlainText(panel, detail)
    panel.snapshot = {"visibility": "closed"}
    panel.actionError = "<b>Native command failed</b>"
    var nativeError = rendersPlainText(panel, "<b>Native command failed</b>")
    panel.toggleVisibility()
    return complete && failed && nativeError
      && root.dismissedShare === root.exactShareId
      && panel.viewState === "idle" && root.visibilityRequested
  }

  function verifyPanel() {
    var valid = verifyAttachmentsAndPeerChoice()
      && verifyDiscovery()
      && verifyConsentAndWaiting()
      && verifyTransfer()
      && verifyTerminalAndIdle()
    if (valid) {
      console.log("HARNESS_OK")
      Qt.quit()
      return
    }
    console.error("HARNESS_FAIL", panel.viewState, panel.progressPercent)
    Qt.exit(1)
  }

  SharePanel {
    id: panel
    width: 300
    accentColor: "#7aa2f7"
    dangerColor: "#f7768e"
    borderWidth: 1
    focusBorderWidth: 2
    hoverFillAlpha: 0.12
    pressedFillAlpha: 0.22
    foregroundColor: "#d8dee9"
    mutedColor: "#8f98a8"
    surfaceColor: "#24283b"
    radius: 6
    controlHeight: 34
    gap: 8
    smallGap: 4
    bodyFontSize: 12
    smallFontSize: 11
    fontFamily: "monospace"
    snapshot: ({})
    onStopDiscoveryRequested: function() {
      root.stopDiscoveryRequests += 1
    }
    onAcceptRequested: function(shareId) {
      root.acceptedShare = shareId
    }
    onCancelRequested: function(shareId) {
      root.cancelledShare = shareId
    }
    onDismissRequested: function(shareId) {
      root.dismissedShare = shareId
    }
    onDiscoverRequested: function() {
      root.discoveryRequests += 1
    }
    onPeerSelected: function(shareId, peerId) {
      root.selectedShare = shareId
      root.selectedPeer = peerId
    }
    onPinRequested: function(peerId, shouldPin) {
      root.pinnedPeer = peerId
      root.pinValue = shouldPin
    }
    onRejectRequested: function(shareId) {
      root.rejectedShare = shareId
    }
    onVisibilityRequested: function(open) {
      root.visibilityRequested = open
    }
  }

  Timer {
    interval: 25
    running: true
    onTriggered: root.verifyPanel()
  }
}
