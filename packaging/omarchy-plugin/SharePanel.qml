import QtQuick
import QtQuick.Controls as Controls
Item {
  id: root
  required property color accentColor
  required property color dangerColor
  required property color foregroundColor
  required property color mutedColor
  required property color surfaceColor
  required property int radius
  required property int controlHeight
  required property int gap
  required property int smallGap
  required property int bodyFontSize
  required property int borderWidth
  required property int focusBorderWidth
  required property real hoverFillAlpha
  required property real pressedFillAlpha
  required property int smallFontSize
  required property string fontFamily
  property string actionError: ""
  property var snapshot: ({})
  readonly property var activeShare: snapshot.active_share || ({})
  readonly property string activeShareId: String(activeShare.id_string || "")
  readonly property bool hasActiveShare: activeShareId.length > 0
  readonly property var attachment: activeShare.attachment || ({})
  readonly property var peers: snapshot.peers || []
  readonly property string phase: String(activeShare.phase || "")
  readonly property string viewState: {
    if (!hasActiveShare) return "idle"
    if (phase === "waiting_for_peer") return "peer_choice"
    if (phase === "awaiting_local_consent") return "consent"
    if (phase === "awaiting_peer_consent") return "waiting"
    if (phase === "transferring") return "transfer"
    return "terminal"
  }
  readonly property bool visibilityOpen: snapshot.visibility === "open"
  readonly property string previewText: {
    if (attachment.type === "file") {
      return String(attachment.name || attachment.value || "File")
    }
    return String(attachment.value || attachment.name || "Unknown attachment")
  }
  readonly property string previewIcon: attachmentIcon(attachment)
  readonly property string peerName: {
    var peer = activeShare.peer || ({})
    return String(peer.name || "")
  }
  readonly property string consentPin: String(
    activeShare.verification_code || "",
  )
  readonly property string discoveryMessage: {
    if (snapshot.discovery === "timed_out") return "No peers found"
    if (snapshot.discovery === "searching") {
      return peers.length > 0
        ? "Searching for more peers…"
        : "Searching for peers…"
    }
    if (viewState === "peer_choice") return "Search stopped"
    return ""
  }
  readonly property int progressPercent: {
    var total = Number(activeShare.total_bytes || 0)
    var transferred = Number(activeShare.transferred_bytes || 0)
    if (total <= 0) return phase === "completed" ? 100 : 0
    return Math.min(100, Math.max(0, Math.round(100 * transferred / total)))
  }
  readonly property string progressText:
    formatBytes(activeShare.transferred_bytes) + " / "
      + formatBytes(activeShare.total_bytes)
  readonly property string etaText: {
    var seconds = Number(activeShare.remaining_seconds)
    if (!isFinite(seconds) || seconds < 0) return "Estimating time remaining…"
    seconds = Math.ceil(seconds)
    if (seconds < 60) return seconds + "s remaining"
    return Math.floor(seconds / 60) + "m " + (seconds % 60) + "s remaining"
  }
  readonly property string terminalTitle: {
    var inbound = activeShare.direction === "inbound"
    if (phase === "completed") return inbound ? "Received" : "Sent"
    if (phase === "cancelled") return "Transfer cancelled"
    if (phase === "rejected") {
      return inbound ? "Share rejected" : "Peer declined"
    }
    return inbound ? "Could not receive" : "Could not send"
  }
  readonly property string terminalDetail: {
    var reason = String(activeShare.terminal_reason || "")
    var guidance = String(activeShare.recovery_guidance || "")
    if (reason.length > 0 && guidance.length > 0) {
      return reason + "\n" + guidance
    }
    return reason || guidance
  }
  signal acceptRequested(string shareId)
  signal cancelRequested(string shareId)
  signal dismissRequested(string shareId)
  signal discoverRequested()
  signal stopDiscoveryRequested()
  signal peerSelected(string shareId, string peerId)
  signal pinRequested(string peerId, bool shouldPin)
  signal rejectRequested(string shareId)
  signal visibilityRequested(bool shouldOpen)
  function accept() {
    if (viewState === "consent") acceptRequested(activeShareId)
  }
  function attachmentIcon(value) {
    var type = String(value.type || "").toLowerCase()
    var mime = String(value.mime_type || value.mime || "").toLowerCase()
    var name = String(value.name || value.value || "").toLowerCase()
    if (type === "text") return "󰊄"
    if (type === "url") return ""
    if (type === "android_app" || type === "apk" || name.endsWith(".apk")) {
      return ""
    }
    if (mime.startsWith("audio/")
        || /\.(flac|m4a|mp3|ogg|opus|wav)$/.test(name)) return "󰝚"
    if (mime.startsWith("video/")
        || /\.(avi|m4v|mkv|mov|mp4|webm)$/.test(name)) return ""
    if (type === "file") return ""
    return ""
  }
  function cancel() {
    if (hasActiveShare && viewState !== "terminal") {
      cancelRequested(activeShareId)
    }
  }
  function choosePeer(peerId) {
    if (viewState === "peer_choice") peerSelected(activeShareId, peerId)
  }
  function dismiss() {
    if (viewState === "terminal") dismissRequested(activeShareId)
  }
  function formatBytes(value) {
    var bytes = Math.max(0, Number(value || 0))
    if (!isFinite(bytes) || bytes < 1024) return Math.round(bytes) + " B"
    var units = ["KB", "MB", "GB", "TB"]
    var amount = bytes / 1024
    var index = 0
    while (amount >= 1024 && index < units.length - 1) {
      amount /= 1024
      index += 1
    }
    var digits = amount < 10 && amount % 1 !== 0 ? 1 : 0
    return amount.toFixed(digits) + " " + units[index]
  }
  function reject() {
    if (viewState === "consent") rejectRequested(activeShareId)
  }
  function toggleDiscovery() {
    if (viewState !== "peer_choice") return
    if (snapshot.discovery === "searching") stopDiscoveryRequested()
    else discoverRequested()
  }
  function togglePin(peerId, pinned) {
    if (viewState === "peer_choice") pinRequested(peerId, !pinned)
  }
  function toggleVisibility() {
    if (viewState === "idle") visibilityRequested(!visibilityOpen)
  }
  implicitHeight: content.implicitHeight
  component ActionButton: Controls.Button {
    id: action
    property bool dangerous: false
    activeFocusOnTab: true
    implicitHeight: root.controlHeight
    leftPadding: root.gap
    rightPadding: root.gap
    font.family: root.fontFamily
    font.pixelSize: root.bodyFontSize
    contentItem: Text {
      color: action.dangerous ? root.dangerColor : root.accentColor
      elide: Text.ElideRight
      font: action.font
      horizontalAlignment: Text.AlignHCenter
      text: action.text
      textFormat: Text.PlainText
      verticalAlignment: Text.AlignVCenter
    }
    background: Rectangle {
      color: action.down || action.hovered
        ? Qt.rgba(
            (action.dangerous ? root.dangerColor : root.accentColor).r,
            (action.dangerous ? root.dangerColor : root.accentColor).g,
            (action.dangerous ? root.dangerColor : root.accentColor).b,
            action.down ? root.pressedFillAlpha : root.hoverFillAlpha)
        : "transparent"
      border.color: action.activeFocus
        ? (action.dangerous ? root.dangerColor : root.accentColor)
        : root.mutedColor
      border.width: action.activeFocus
        ? root.focusBorderWidth : root.borderWidth
      radius: root.radius
    }
  }
  Column {
    id: content
    width: parent.width
    spacing: root.gap
    Text {
      width: parent.width
      color: root.foregroundColor
      font.bold: true
      font.family: root.fontFamily
      font.pixelSize: root.bodyFontSize
      text: {
        if (root.viewState === "peer_choice") return "Choose a peer"
        if (root.viewState === "consent") return "Incoming share"
        if (root.viewState === "waiting") return "Waiting for peer"
        if (root.viewState === "transfer") {
          return root.activeShare.direction === "inbound"
            ? "Receiving"
            : "Sending"
        }
        if (root.viewState === "terminal") return root.terminalTitle
        return root.visibilityOpen ? "Ready to receive" : "Quick Share"
      }
      textFormat: Text.PlainText
    }
    Rectangle {
      width: parent.width
      height: root.controlHeight
      color: root.surfaceColor
      radius: root.radius
      visible: root.hasActiveShare
      Row {
        anchors.fill: parent
        anchors.leftMargin: root.gap
        anchors.rightMargin: root.gap
        spacing: root.gap
        Text {
          anchors.verticalCenter: parent.verticalCenter
          color: root.accentColor
          font.family: root.fontFamily
          font.pixelSize: root.bodyFontSize
          text: root.previewIcon
          textFormat: Text.PlainText
        }
        Text {
          anchors.verticalCenter: parent.verticalCenter
          width: parent.width - x
          color: root.foregroundColor
          elide: Text.ElideRight
          font.family: root.fontFamily
          font.pixelSize: root.smallFontSize
          text: root.previewText
          textFormat: Text.PlainText
        }
      }
    }
    Column {
      width: parent.width
      spacing: root.smallGap
      visible: root.viewState === "peer_choice"
      Repeater {
        model: root.peers
        Rectangle {
          id: peerRow
          required property var modelData
          width: content.width
          height: root.controlHeight
          color: peerHover.hovered || modelData.pinned
            ? Qt.rgba(
                root.accentColor.r,
                root.accentColor.g,
                root.accentColor.b,
                root.hoverFillAlpha)
            : "transparent"
          border.color: activeFocus ? root.accentColor : root.mutedColor
          border.width: activeFocus
            ? root.focusBorderWidth : root.borderWidth
          activeFocusOnTab: true
          Accessible.role: Accessible.Button
          Accessible.name: String(modelData.name || "Unnamed peer")
          Keys.onReturnPressed: root.choosePeer(String(modelData.id))
          Keys.onEnterPressed: root.choosePeer(String(modelData.id))
          Keys.onSpacePressed: root.choosePeer(String(modelData.id))
          Row {
            anchors.fill: parent
            anchors.leftMargin: root.gap
            anchors.rightMargin: root.gap
            spacing: root.gap
            Text {
              anchors.verticalCenter: parent.verticalCenter
              width: parent.width - pinnedLabel.width - parent.spacing
              color: root.foregroundColor
              elide: Text.ElideRight
              font.family: root.fontFamily
              font.pixelSize: root.bodyFontSize
              text: String(peerRow.modelData.name || "Unnamed peer")
              textFormat: Text.PlainText
            }
            Text {
              id: pinnedLabel
              anchors.verticalCenter: parent.verticalCenter
              color: root.accentColor
              font.family: root.fontFamily
              font.pixelSize: root.smallFontSize
              text: peerRow.modelData.pinned ? "Pinned" : ""
              textFormat: Text.PlainText
            }
          }
          HoverHandler { id: peerHover }
          TapHandler {
            acceptedButtons: Qt.LeftButton
            onTapped: root.choosePeer(String(peerRow.modelData.id))
          }
          TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: root.togglePin(
              String(peerRow.modelData.id),
              Boolean(peerRow.modelData.pinned),
            )
          }
        }
      }
      Text {
        width: parent.width
        color: root.mutedColor
        font.family: root.fontFamily
        font.pixelSize: root.smallFontSize
        text: root.discoveryMessage
        textFormat: Text.PlainText
        visible: text.length > 0
      }
      Text {
        width: parent.width
        color: root.mutedColor
        font.family: root.fontFamily
        font.pixelSize: root.smallFontSize
        text: "Right-click a peer to pin or unpin it"
        textFormat: Text.PlainText
        visible: root.peers.length > 0
      }
      ActionButton {
        text: root.snapshot.discovery === "searching"
          ? "Stop searching" : "Search again"
        onClicked: root.toggleDiscovery()
      }
    }
    Column {
      width: parent.width
      spacing: root.smallGap
      visible: root.viewState === "idle"
      Text {
        width: parent.width
        color: root.mutedColor
        font.family: root.fontFamily
        font.pixelSize: root.smallFontSize
        text: root.visibilityOpen
          ? "Visible to nearby peers"
          : "Receiving is off"
        textFormat: Text.PlainText
      }
      ActionButton {
        text: root.visibilityOpen ? "Stop receiving" : "Receive"
        onClicked: root.toggleVisibility()
      }
    }
    Column {
      width: parent.width
      spacing: root.smallGap
      visible: root.viewState === "consent"
      Text {
        width: parent.width
        color: root.foregroundColor
        font.family: root.fontFamily
        font.pixelSize: root.bodyFontSize
        text: root.peerName
        textFormat: Text.PlainText
        visible: text.length > 0
      }
      Text {
        width: parent.width
        color: root.accentColor
        font.bold: true
        font.family: root.fontFamily
        font.pixelSize: root.bodyFontSize
        text: root.consentPin.length > 0
          ? "Confirm PIN " + root.consentPin
          : "Confirm this share on both devices"
        textFormat: Text.PlainText
      }
      Row {
        spacing: root.gap
        ActionButton {
          text: "Accept"
          onClicked: root.accept()
        }
        ActionButton {
          dangerous: true
          text: "Reject"
          onClicked: root.reject()
        }
      }
    }
    Column {
      width: parent.width
      spacing: root.smallGap
      visible: root.viewState === "waiting"
      Text {
        width: parent.width
        color: root.mutedColor
        font.family: root.fontFamily
        font.pixelSize: root.smallFontSize
        text: root.peerName.length > 0
          ? "Waiting for " + root.peerName + " to accept"
          : "Waiting for the peer to accept"
        textFormat: Text.PlainText
      }
      Text {
        width: parent.width
        color: root.accentColor
        font.bold: true
        font.family: root.fontFamily
        font.pixelSize: root.bodyFontSize
        text: "PIN " + root.consentPin
        textFormat: Text.PlainText
        visible: root.consentPin.length > 0
      }
    }
    Column {
      width: parent.width
      spacing: root.smallGap
      visible: root.viewState === "transfer"
      Rectangle {
        width: parent.width
        height: root.smallGap
        color: root.surfaceColor
        radius: root.radius
        Rectangle {
          width: parent.width * root.progressPercent / 100
          height: parent.height
          color: root.accentColor
          radius: root.radius
        }
      }
      Row {
        width: parent.width
        Text {
          color: root.foregroundColor
          font.family: root.fontFamily
          font.pixelSize: root.smallFontSize
          text: root.progressPercent + "% · " + root.progressText
          textFormat: Text.PlainText
        }
        Item {
          width: Math.max(
            0,
            parent.width
              - parent.children[0].width
              - parent.children[2].width,
          )
        }
        Text {
          color: root.mutedColor
          font.family: root.fontFamily
          font.pixelSize: root.smallFontSize
          text: root.etaText
          textFormat: Text.PlainText
        }
      }
      ActionButton {
        dangerous: true
        text: "Cancel"
        onClicked: root.cancel()
      }
    }
    Column {
      width: parent.width
      spacing: root.smallGap
      visible: root.viewState === "terminal"
      Text {
        width: parent.width
        color: root.mutedColor
        font.family: root.fontFamily
        font.pixelSize: root.smallFontSize
        text: root.terminalDetail
        textFormat: Text.PlainText
        visible: text.length > 0
        wrapMode: Text.WordWrap
      }
      ActionButton {
        text: "Done"
        onClicked: root.dismiss()
      }
    }
    Text {
      width: parent.width
      color: root.dangerColor
      font.family: root.fontFamily
      font.pixelSize: root.smallFontSize
      text: root.actionError
      textFormat: Text.PlainText
      visible: text.length > 0
      wrapMode: Text.WordWrap
    }
  }
}
