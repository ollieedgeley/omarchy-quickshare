import QtQuick
import Quickshell

ShellRoot {
  id: root

  property int acceptedShare: 0
  property int cancelledShare: 0
  property string selectedPeer: ""
  property int selectedShare: 0

  function verifyPanel() {
    var outbound = panel.viewState === "choose_peer"
      && panel.peers.length === 1
      && panel.progressPercent === 0
    panel.choosePeer("pixel-8")
    var selected = root.selectedShare === 7
      && root.selectedPeer === "pixel-8"
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
    if (outbound && selected && incoming && transfer && actions) {
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
    onPeerSelected: function(shareId, peerId) {
      root.selectedShare = shareId
      root.selectedPeer = peerId
    }
  }

  Timer {
    interval: 25
    running: true
    onTriggered: root.verifyPanel()
  }
}
