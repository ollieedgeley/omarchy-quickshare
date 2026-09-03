import QtQuick
import Quickshell

ShellRoot {
  id: root

  property int acceptedShare: 0
  property int cancelledShare: 0
  property int dismissedShare: 0
  property int discoveryRequests: 0
  property string pinnedPeer: ""
  property string selectedPeer: ""
  property int selectedShare: 0
  property bool visibilityRequested: false

  function verifyOutbound() {
    var outbound = panel.viewState === "choose_peer"
      && panel.peers.length === 1
      && panel.progressPercent === 0
    panel.choosePeer("pixel-8")
    panel.pinPeer("pixel-8")
    var selected = root.selectedShare === 7
      && root.selectedPeer === "pixel-8"
      && root.pinnedPeer === "pixel-8"
    return outbound && selected
  }

  function verifyDiscovery() {
    panel.snapshot = {
      "active_share": {
        "attachment": {"type": "text", "value": "hello"},
        "direction": "outbound",
        "id": 7,
        "phase": "waiting_for_peer",
        "total_bytes": 5,
        "transferred_bytes": 0,
      },
      "discovery": "timed_out",
      "peers": [],
    }
    var expired = panel.discoveryMessage === "No devices found"
    panel.retryDiscovery()
    return expired && root.discoveryRequests === 1
  }

  function verifyIncoming() {
    panel.snapshot = incomingSnapshot("awaiting_local_consent", 0)
    var incoming = panel.viewState === "consent"
      && panel.attachmentLabel === "photo.jpg"
    panel.accept()
    panel.snapshot = incomingSnapshot("transferring", 5)
    var transfer = panel.viewState === "transfer"
      && panel.progressPercent === 50
    panel.cancel()
    var actions = root.acceptedShare === 8
      && root.cancelledShare === 8
    return incoming && transfer && actions
  }

  function verifyTerminalAndVisibility() {
    panel.snapshot = incomingSnapshot("failed", 5)
    panel.dismiss()
    var terminal = panel.viewState === "terminal"
      && root.dismissedShare === 8
    panel.snapshot = {"visibility": "closed"}
    panel.toggleVisibility()
    return terminal && root.visibilityRequested
  }

  function verifyPanel() {
    var valid = verifyOutbound()
      && verifyDiscovery()
      && verifyIncoming()
      && verifyTerminalAndVisibility()
    if (valid) {
      console.log("HARNESS_OK")
      Qt.quit()
      return
    }
    console.error("HARNESS_FAIL", panel.viewState, panel.progressPercent)
    Qt.exit(1)
  }

  function incomingSnapshot(phase, transferred) {
    return {
      "active_share": {
        "attachment": {"type": "file", "name": "photo.jpg",
          "size_bytes": 10},
        "direction": "inbound",
        "id": 8,
        "peer": {"id": "pixel-8", "name": "Ollie's Pixel",
          "pinned": false},
        "phase": phase,
        "total_bytes": 10,
        "transferred_bytes": transferred,
      },
      "peers": [],
    }
  }

  SharePanel {
    id: panel
    snapshot: ({
      "active_share": {
        "attachment": {"type": "text", "value": "hello"},
        "direction": "outbound",
        "id": 7,
        "phase": "waiting_for_peer",
        "total_bytes": 5,
        "transferred_bytes": 0,
      },
      "peers": [
        {"id": "pixel-8", "name": "Ollie's Pixel", "pinned": false},
      ],
    })
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
    onPinRequested: function(peerId) {
      root.pinnedPeer = peerId
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
