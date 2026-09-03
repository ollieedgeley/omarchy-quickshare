import QtQuick

Item {
  id: root

  property color accentColor: "#7aa2f7"
  property color dangerColor: "#f7768e"
  property color foregroundColor: "#ffffff"
  property color mutedColor: "#a9b1d6"
  property var snapshot: ({})
  property color surfaceColor: "#24283b"

  readonly property var activeShare: snapshot.active_share || ({})
  readonly property string attachmentLabel: {
    var attachment = activeShare.attachment || ({})
    if (attachment.type === "file") return attachment.name || "File"
    return attachment.value || "Nothing selected"
  }
  readonly property var peers: snapshot.peers || []
  readonly property string discoveryMessage: {
    if (snapshot.discovery === "timed_out") return "No devices found"
    if (snapshot.discovery === "searching") {
      return peers.length > 0
        ? "Searching for more devices…"
        : "Searching for devices…"
    }
    if (viewState === "choose_peer") return "Search stopped"
    return ""
  }
  readonly property int progressPercent: {
    var total = Number(activeShare.total_bytes || 0)
    var transferred = Number(activeShare.transferred_bytes || 0)
    if (total <= 0) return activeShare.phase === "completed" ? 100 : 0
    return Math.min(100, Math.round(100 * transferred / total))
  }
  readonly property string viewState: {
    if (!activeShare.id) return "idle"
    if (activeShare.phase === "waiting_for_peer") return "choose_peer"
    if (activeShare.phase === "awaiting_local_consent") return "consent"
    if (activeShare.phase === "awaiting_peer_consent") return "waiting"
    if (activeShare.phase === "transferring") return "transfer"
    return "terminal"
  }
  readonly property bool visibilityOpen: snapshot.visibility === "open"

  signal acceptRequested(int shareId)
  signal cancelRequested(int shareId)
  signal dismissRequested(int shareId)
  signal discoverRequested()
  signal peerSelected(int shareId, string peerId)
  signal pinRequested(string peerId)
  signal rejectRequested(int shareId)
  signal visibilityRequested(bool shouldOpen)

  function accept() {
    if (viewState === "consent") acceptRequested(activeShare.id)
  }

  function cancel() {
    if (viewState === "transfer") cancelRequested(activeShare.id)
  }

  function choosePeer(peerId) {
    if (viewState === "choose_peer") peerSelected(activeShare.id, peerId)
  }

  function dismiss() {
    if (viewState === "terminal") dismissRequested(activeShare.id)
  }

  function pinPeer(peerId) {
    if (viewState === "choose_peer") pinRequested(peerId)
  }

  function reject() {
    if (viewState === "consent") rejectRequested(activeShare.id)
  }

  function retryDiscovery() {
    if (viewState === "choose_peer"
        && snapshot.discovery === "timed_out") {
      discoverRequested()
    }
  }

  function toggleVisibility() {
    if (viewState === "idle") visibilityRequested(!visibilityOpen)
  }

  implicitHeight: content.implicitHeight
  implicitWidth: 300

  Column {
    id: content
    width: parent.width
    spacing: 8

    Text {
      width: parent.width
      color: root.foregroundColor
      font.bold: true
      text: {
        if (root.viewState === "choose_peer") return "Choose a device"
        if (root.viewState === "consent") return "Incoming share"
        if (root.viewState === "waiting") return "Waiting for device"
        if (root.viewState === "transfer") return "Transferring"
        if (root.viewState === "terminal") return root.activeShare.phase
        return "No share queued"
      }
      textFormat: Text.PlainText
    }

    Text {
      width: parent.width
      color: root.mutedColor
      elide: Text.ElideMiddle
      text: root.attachmentLabel
      textFormat: Text.PlainText
      visible: root.viewState !== "idle"
    }

    Column {
      width: parent.width
      spacing: 6
      visible: root.viewState === "choose_peer"

      Repeater {
        model: root.peers

        Rectangle {
          required property var modelData
          width: content.width
          height: 36
          color: root.surfaceColor
          radius: 6

          Text {
            anchors.centerIn: parent
            color: root.foregroundColor
            text: modelData.name + (modelData.pinned ? " · pinned" : "")
            textFormat: Text.PlainText
          }

          MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            onClicked: function(mouse) {
              if (mouse.button === Qt.RightButton) {
                root.pinPeer(modelData.id)
              } else {
                root.choosePeer(modelData.id)
              }
            }
          }
        }
      }

      Text {
        color: root.mutedColor
        text: "Right-click a device to pin it"
        visible: root.peers.length > 0
      }

      Text {
        color: root.mutedColor
        text: root.discoveryMessage
        visible: text.length > 0
      }

      Rectangle {
        width: 92
        height: 34
        color: root.accentColor
        radius: 6
        visible: root.snapshot.discovery === "timed_out"
        Text {
          anchors.centerIn: parent
          color: root.surfaceColor
          text: "Retry"
        }
        MouseArea {
          anchors.fill: parent
          onClicked: root.retryDiscovery()
        }
      }
    }

    Column {
      width: parent.width
      spacing: 6
      visible: root.viewState === "idle"

      Text {
        color: root.mutedColor
        text: root.visibilityOpen
          ? "Visible to nearby devices"
          : "Receiving is off"
      }

      Rectangle {
        width: 128
        height: 34
        color: root.accentColor
        radius: 6
        Text {
          anchors.centerIn: parent
          color: root.surfaceColor
          text: root.visibilityOpen ? "Stop receiving" : "Receive"
        }
        MouseArea {
          anchors.fill: parent
          onClicked: root.toggleVisibility()
        }
      }
    }

    Row {
      spacing: 8
      visible: root.viewState === "consent"

      Rectangle {
        width: 92
        height: 34
        color: root.accentColor
        radius: 6
        Text {
          anchors.centerIn: parent
          color: root.surfaceColor
          text: "Accept"
        }
        MouseArea {
          anchors.fill: parent
          onClicked: root.accept()
        }
      }

      Rectangle {
        width: 92
        height: 34
        color: root.dangerColor
        radius: 6
        Text {
          anchors.centerIn: parent
          color: root.surfaceColor
          text: "Reject"
        }
        MouseArea {
          anchors.fill: parent
          onClicked: root.reject()
        }
      }
    }

    Column {
      width: parent.width
      spacing: 6
      visible: root.viewState === "transfer"

      Rectangle {
        width: parent.width
        height: 8
        color: root.surfaceColor
        radius: 4
        Rectangle {
          width: parent.width * root.progressPercent / 100
          height: parent.height
          color: root.accentColor
          radius: 4
        }
      }

      Text {
        color: root.foregroundColor
        text: root.progressPercent + "% — "
          + Number(root.activeShare.transferred_bytes || 0) + " / "
          + Number(root.activeShare.total_bytes || 0) + " bytes"
      }

      Rectangle {
        width: 92
        height: 34
        color: root.dangerColor
        radius: 6
        Text {
          anchors.centerIn: parent
          color: root.surfaceColor
          text: "Cancel"
        }
        MouseArea {
          anchors.fill: parent
          onClicked: root.cancel()
        }
      }
    }

    Rectangle {
      width: 92
      height: 34
      color: root.accentColor
      radius: 6
      visible: root.viewState === "terminal"
      Text {
        anchors.centerIn: parent
        color: root.surfaceColor
        text: "Done"
      }
      MouseArea {
        anchors.fill: parent
        onClicked: root.dismiss()
      }
    }
  }
}
