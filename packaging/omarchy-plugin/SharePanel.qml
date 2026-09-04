import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui

Item {
  id: root

  property var snapshot: ({})
  property string actionError: ""
  property bool actionBusy: false
  property bool showPasteBadge: false

  readonly property var activeShare: snapshot.active_share || ({})
  readonly property string activeShareId: String(activeShare.id_string || "")
  readonly property bool hasActiveShare: activeShareId.length > 0
  readonly property var attachment: activeShare.attachment || ({})
  readonly property var peers: snapshot.peers || []
  readonly property var orderedPeers: {
    var decorated = []
    for (var i = 0; i < peers.length; i++) {
      decorated.push({ peer: peers[i], index: i })
    }
    decorated.sort(function(a, b) {
      var pins = Number(Boolean(b.peer && b.peer.pinned))
        - Number(Boolean(a.peer && a.peer.pinned))
      return pins !== 0 ? pins : a.index - b.index
    })
    var result = []
    for (var j = 0; j < decorated.length; j++) {
      result.push(decorated[j].peer)
    }
    return result
  }
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
  readonly property string previewText: attachment.type === "file"
    ? String(attachment.name || attachment.value || "File")
    : String(attachment.value || attachment.name || "Unknown attachment")
  readonly property string previewIcon: attachmentIcon(attachment)
  readonly property string peerName: String((activeShare.peer || {}).name || "")
  readonly property string consentPin:
    String(activeShare.verification_code || "")
  readonly property string discoveryMessage: {
    if (snapshot.discovery === "timed_out") {
      return orderedPeers.length > 0
        ? "Search complete."
        : "No devices found. Turn on Bluetooth and make Quick Share visible."
    }
    if (snapshot.discovery === "searching") {
      return orderedPeers.length > 0
        ? "Searching for more devices…"
        : "Searching for devices…"
    }
    return viewState === "peer_choice" ? "Search stopped." : ""
  }
  readonly property bool totalKnown: Number(activeShare.total_bytes || 0) > 0
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
    if (!isFinite(seconds) || seconds < 0) {
      return "Estimating time remaining…"
    }
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
    if (reason && guidance) return reason + "\n" + guidance
    return reason || guidance
  }
  readonly property color terminalColor: phase === "completed"
    ? Color.accent
    : (phase === "failed" ? Color.urgent : Color.muted)
  readonly property string terminalSummary:
    terminalDetail || (phase === "completed"
    ? "The transfer finished successfully."
    : (phase === "cancelled"
      ? "No more data will be transferred."
      : (phase === "rejected"
        ? "The transfer did not start."
        : "Check both devices and try again.")))

  property bool cursorActive: false
  property string selectedTarget: ""
  readonly property var cursorTargets: {
    var targets = []
    if (viewState === "idle") return ["visibility"]
    if (viewState === "peer_choice") {
      for (var i = 0; i < orderedPeers.length; i++) {
        targets.push("peer:" + String(orderedPeers[i].id || ""))
      }
      targets.push("search", "cancel")
      return targets
    }
    if (viewState === "consent") return ["accept", "reject"]
    if (viewState === "waiting" || viewState === "transfer") {
      return ["cancel"]
    }
    return viewState === "terminal" ? ["done"] : targets
  }
  readonly property int selectedIndex: cursorTargets.indexOf(selectedTarget)

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
    if (!actionBusy && viewState === "consent") {
      acceptRequested(activeShareId)
    }
  }
  function cancel() {
    if (!actionBusy && hasActiveShare && viewState !== "terminal") {
      cancelRequested(activeShareId)
    }
  }
  function choosePeer(peerId) {
    if (!actionBusy && viewState === "peer_choice") {
      peerSelected(activeShareId, peerId)
    }
  }
  function dismiss() {
    if (!actionBusy && viewState === "terminal") {
      dismissRequested(activeShareId)
    }
  }
  function reject() {
    if (!actionBusy && viewState === "consent") {
      rejectRequested(activeShareId)
    }
  }
  function toggleDiscovery() {
    if (actionBusy || viewState !== "peer_choice") return
    if (snapshot.discovery === "searching") stopDiscoveryRequested()
    else discoverRequested()
  }
  function togglePin(peerId, pinned) {
    if (!actionBusy && viewState === "peer_choice") {
      pinRequested(peerId, !pinned)
    }
  }
  function toggleVisibility() {
    if (!actionBusy && viewState === "idle") {
      visibilityRequested(!visibilityOpen)
    }
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
    return type === "file" ? "" : ""
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
  function setCursor(target) {
    if (cursorTargets.indexOf(target) < 0) return
    cursorActive = true
    selectedTarget = target
  }
  function moveCursor(dx, dy) {
    if (cursorTargets.length === 0) return
    var index = selectedIndex
    if (!cursorActive || index < 0) {
      setCursor(cursorTargets[0])
      return
    }
    var delta = Number(dy) !== 0
      ? Math.sign(Number(dy))
      : Math.sign(Number(dx))
    if (delta === 0) return
    index = Math.max(0, Math.min(cursorTargets.length - 1, index + delta))
    setCursor(cursorTargets[index])
    if (selectedTarget.startsWith("peer:")) {
      peerChoice.showPeer(index)
    }
  }
  function activateTarget(target) {
    if (actionBusy) return
    if (target.startsWith("peer:")) choosePeer(target.slice(5))
    else if (target === "search") toggleDiscovery()
    else if (target === "cancel") cancel()
    else if (target === "accept") accept()
    else if (target === "reject") reject()
    else if (target === "visibility") toggleVisibility()
    else if (target === "done") dismiss()
  }
  function activateCursor() {
    if (cursorActive && selectedIndex >= 0) activateTarget(selectedTarget)
  }
  function toggleSelectedPin() {
    if (actionBusy || viewState !== "peer_choice"
        || !selectedTarget.startsWith("peer:")) return
    var peerId = selectedTarget.slice(5)
    for (var i = 0; i < orderedPeers.length; i++) {
      var peer = orderedPeers[i]
      if (String(peer.id || "") === peerId) {
        togglePin(peerId, Boolean(peer.pinned))
        return
      }
    }
  }
  function targetHasCursor(target) {
    return cursorActive && selectedTarget === target
  }

  onViewStateChanged: {
    cursorActive = false
    selectedTarget = ""
  }
  onCursorTargetsChanged: {
    if (cursorTargets.indexOf(selectedTarget) < 0) {
      selectedTarget = cursorTargets.length > 0 ? cursorTargets[0] : ""
    }
  }

  implicitHeight: content.implicitHeight

  Column {
    id: content
    width: parent.width
    spacing: Style.spacing.panelGap

    Column {
      visible: root.viewState === "idle"
      width: parent.width
      spacing: Style.spacing.panelGap

      PanelHero {
        width: parent.width
        title: "Quick Share"
        meta: root.visibilityOpen
          ? "Ready to receive"
          : "Receive visibility closed"
        foreground: Color.foreground
        fontFamily: Style.font.family
        iconComponent: Component {
          Text {
            text: ""
            color: Color.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.display
            textFormat: Text.PlainText
          }
        }
      }
      Text {
        width: parent.width
        text: "Paste while this panel is open to choose a nearby device."
        color: Color.muted
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        wrapMode: Text.WordWrap
        textFormat: Text.PlainText
      }
      PanelSeparator {
        foreground: Color.foreground
      }
      PanelSectionHeader {
        text: "RECEIVE"
        foreground: Color.foreground
        fontFamily: Style.font.family
        textFormat: Text.PlainText
      }
      CursorSurface {
        width: parent.width
        implicitHeight: receiveRow.implicitHeight + Style.spacing.rowPaddingX
        hasCursor: root.targetHasCursor("visibility")
        foreground: Color.foreground
        enabled: !root.actionBusy
        opacity: enabled ? 1 : 0.5
        Accessible.role: Accessible.CheckBox
        Accessible.name: root.visibilityOpen
          ? "Close Quick Share visibility"
          : "Open Quick Share visibility"
        Accessible.checked: root.visibilityOpen
        Accessible.onPressAction: root.toggleVisibility()

        RowLayout {
          id: receiveRow
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          anchors.leftMargin: Style.spacing.controlPaddingX
          anchors.rightMargin: Style.spacing.controlPaddingX
          spacing: Style.spacing.controlGap

          Text {
            Layout.fillWidth: true
            text: root.visibilityOpen
              ? "Visible to nearby devices"
              : "Not visible to nearby devices"
            color: Color.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.body
            elide: Text.ElideRight
            textFormat: Text.PlainText
          }
          ToggleSwitch {
            checked: root.visibilityOpen
            busy: root.actionBusy
            interactive: false
            foreground: Color.foreground
          }
        }
        MouseArea {
          anchors.fill: parent
          enabled: !root.actionBusy
          hoverEnabled: true
          cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
          onEntered: root.setCursor("visibility")
          onClicked: root.toggleVisibility()
        }
      }
    }

    PeerChoiceView {
      id: peerChoice
      visible: root.viewState === "peer_choice"
      width: parent.width
      actionBusy: root.actionBusy
      cursorActive: root.cursorActive
      discoveryMessage: root.discoveryMessage
      discoveryState: String(root.snapshot.discovery || "")
      orderedPeers: root.orderedPeers
      previewIcon: root.previewIcon
      previewText: root.previewText
      selectedTarget: root.selectedTarget
      showPasteBadge: root.showPasteBadge
      onCancelRequested: root.cancel()
      onCursorRequested: function(target) { root.setCursor(target) }
      onPeerRequested: function(peerId) { root.choosePeer(peerId) }
      onPinRequested: function(peerId, pinned) {
        root.togglePin(peerId, pinned)
      }
      onSearchRequested: root.toggleDiscovery()
    }

    ConsentView {
      visible: root.viewState === "consent" || root.viewState === "waiting"
      width: parent.width
      actionBusy: root.actionBusy
      attachmentName: root.previewText
      cursorActive: root.cursorActive
      peerName: root.peerName
      selectedTarget: root.selectedTarget
      verificationCode: root.consentPin
      waiting: root.viewState === "waiting"
      onAcceptRequested: root.accept()
      onCancelRequested: root.cancel()
      onCursorRequested: function(target) { root.setCursor(target) }
      onRejectRequested: root.reject()
    }

    TransferView {
      visible: root.viewState === "transfer"
      width: parent.width
      actionBusy: root.actionBusy
      cursorActive: root.cursorActive
      direction: String(root.activeShare.direction || "")
      etaText: root.etaText
      progressPercent: root.progressPercent
      progressText: root.totalKnown
        ? root.progressText
        : root.formatBytes(root.activeShare.transferred_bytes) + " transferred"
      selectedTarget: root.selectedTarget
      totalKnown: root.totalKnown
      onCancelRequested: root.cancel()
      onCursorRequested: function(target) { root.setCursor(target) }
    }

    TerminalView {
      visible: root.viewState === "terminal"
      width: parent.width
      actionBusy: root.actionBusy
      cursorActive: root.cursorActive
      phase: root.phase
      selectedTarget: root.selectedTarget
      summary: root.terminalSummary
      title: root.terminalTitle
      tone: root.terminalColor
      onCursorRequested: function(target) { root.setCursor(target) }
      onDoneRequested: root.dismiss()
    }

    Column {
      visible: root.actionError.length > 0
      width: parent.width
      spacing: Style.spacing.labelGap

      PanelSeparator {
        foreground: Color.urgent
      }
      PanelSectionHeader {
        text: "ACTION NEEDED"
        foreground: Color.urgent
        fontFamily: Style.font.family
        textFormat: Text.PlainText
      }
      Text {
        width: parent.width
        text: root.actionError
        color: Color.urgent
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        wrapMode: Text.WordWrap
        textFormat: Text.PlainText
      }
    }
  }
}
