import QtQuick
import Quickshell.Io
import qs.Commons
import qs.Ui

BarWidget {
  id: root

  moduleName: "io.github.ollieedgeley.omarchy-quickshare"

  property bool popupOpen: false
  property bool popoutSwitchClosing: false
  property var capturedContent: null
  property var submittedCapture: null
  readonly property bool pasteLatch: capturedContent !== null
  property string clipboardAction: ""
  property string pendingClipboardAction: ""
  property int captureGeneration: 0
  property string clipboardOutput: ""
  property string clipboardPeerId: ""
  readonly property string clipboardPreview:
    capturedContent ? capturedContent.value : ""
  property string selectedPeerId: ""
  property bool selectionArmed: false
  readonly property bool selectedPeerIsCurrent: {
    if (selectedPeerId.length === 0) return false
    var peers = status.endpointSnapshot.peers || []
    for (var i = 0; i < peers.length; i++) {
      if (String(peers[i].id || "") === selectedPeerId) return true
    }
    return false
  }
  onSelectedPeerIsCurrentChanged: {
    if (!selectedPeerIsCurrent) selectionArmed = false
  }
  readonly property bool readClipboardOnSelect:
    status.appliedPreferences.read_clipboard_on_select === true
  onReadClipboardOnSelectChanged: {
    if (readClipboardOnSelect) return
    if (clipboardAction === "send") clipboardAction = ""
    if (pendingClipboardAction === "send") pendingClipboardAction = ""
  }
  property real iconOpacity: 1.0
  readonly property bool opened: popupOpen
  readonly property bool transferring:
    status.activeShare.phase === "transferring"
  readonly property bool showPasteBadge: pasteLatch
  readonly property bool clipboardBusy:
    clipboardUriProbe.running || clipboardTextProbe.running
  readonly property bool protocolReady: status.protocolState === "ready"
  readonly property color foreground:
    bar ? bar.foreground : Color.foreground
  readonly property string fontFamily:
    bar ? bar.fontFamily : Style.font.family
  readonly property string protocolMeta:
    status.protocolState === "checking" ? "Checking" : "Unavailable"

  function open() {
    if (popupOpen) return
    popupOpen = true
    if (!status.discover()) status.refresh()
  }

  function close() {
    popupOpen = false
    clearPasteBadge()
  }

  function closeForPopoutSwitch() {
    popoutSwitchClosing = true
    close()
    Qt.callLater(function() { root.popoutSwitchClosing = false })
  }

  function toggle() {
    if (opened) close()
    else open()
  }

  function switchPanel(direction) {
    if (bar && typeof bar.switchPanelFrom === "function")
      return bar.switchPanelFrom(root, direction)
    return false
  }

  function invalidateClipboardRead() {
    captureGeneration += 1
    clipboardAction = ""
    pendingClipboardAction = ""
  }

  function clearPasteBadge() {
    invalidateClipboardRead()
    selectionArmed = false
    selectedPeerId = ""
    capturedContent = null
  }

  function captureClipboard(value) {
    if (String(value).length === 0) {
      status.actionError = "Clipboard is empty or unavailable."
      return false
    }
    capturedContent = {value: String(value)}
    status.actionError = ""
    return true
  }

  function finishClipboard(value) {
    clipboardDeadline.stop()
    if (pendingClipboardAction.length > 0) {
      var pending = pendingClipboardAction
      var generation = captureGeneration
      Qt.callLater(function() {
        if (generation === root.captureGeneration
            && root.pendingClipboardAction === pending) {
          root.pendingClipboardAction = ""
          root.readClipboard(pending, root.selectedPeerId)
        }
      })
      return
    }
    var action = clipboardAction
    var peerId = clipboardPeerId
    clipboardAction = ""
    clipboardPeerId = ""
    clipboardOutput = ""
    if (action === "send" && peerId !== selectedPeerId) return
    if (action.length === 0) return
    if (!captureClipboard(value)) return
    if (action === "send" || action === "preview") submitCaptured()
  }

  function readClipboard(action, peerId) {
    if (action === "send"
        && (!readClipboardOnSelect || !selectionArmed || !selectedPeerIsCurrent
          || clipboardAction === "preview"
          || pendingClipboardAction === "preview")) return false
    if (clipboardBusy) {
      if (action === "send" && clipboardAction === "send"
          && peerId === clipboardPeerId) {
        return false
      }
      pendingClipboardAction = action
      clipboardAction = ""
      return true
    }
    pendingClipboardAction = ""
    clipboardAction = action
    clipboardPeerId = String(peerId || "")
    clipboardOutput = ""
    clipboardDeadline.restart()
    clipboardUriProbe.running = true
    return true
  }

  function submitCaptured() {
    if (!opened || !pasteLatch || !selectionArmed || !selectedPeerIsCurrent) {
      return
    }
    if (clipboardAction === "preview" || pendingClipboardAction === "preview") {
      return
    }
    var peerId = selectedPeerId
    selectionArmed = false
    selectedPeerId = ""
    if (!status.submitTo(peerId, clipboardPreview)) {
      status.actionError = "Quick Share is busy. Select a device to try again."
      return
    }
    submittedCapture = capturedContent
  }

  function paste(value) {
    invalidateClipboardRead()
    if (!opened) open()
    if (!captureClipboard(value)) return "empty"
    submitCaptured()
    return "ok"
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  StatusProbe { id: status }


  property Process clipboardUriProbe: Process {
    command: ["wl-paste", "--type", "text/uri-list", "--no-newline"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.clipboardOutput = String(text || "")
    }
    onExited: function(exitCode) {
      if (root.pendingClipboardAction.length > 0) {
        root.finishClipboard("")
        return
      }
      if (root.clipboardAction.length === 0) {
        clipboardDeadline.stop()
        return
      }
      if (exitCode === 0 && root.clipboardOutput.length > 0) {
        root.finishClipboard(root.clipboardOutput)
        return
      }
      root.clipboardOutput = ""
      root.clipboardTextProbe.running = true
    }
  }

  property Process clipboardTextProbe: Process {
    command: ["wl-paste", "--type", "text", "--no-newline"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.clipboardOutput = String(text || "")
    }
    onExited: function(exitCode) {
      root.finishClipboard(exitCode === 0 ? root.clipboardOutput : "")
    }
  }

  Timer {
    id: clipboardDeadline
    interval: 3000
    onTriggered: {
      if (root.clipboardAction.length > 0
          && root.pendingClipboardAction.length === 0) {
        status.actionError =
          "Clipboard capture timed out. Paste again to retry."
      }
      root.clipboardAction = ""
      root.clipboardOutput = ""
      root.clipboardPeerId = ""
      root.clipboardUriProbe.running = false
      root.clipboardTextProbe.running = false
    }
  }

  Connections {
    target: status

    function onActionFinished(succeeded) {
      var submitted = root.submittedCapture
      if (submitted === null) return
      root.submittedCapture = null
      if (succeeded && root.capturedContent === submitted) {
        root.capturedContent = null
      }
    }

  }

  IpcHandler {
    target: root.moduleName

    function close(): void { root.close() }
    function hide(): void { root.close() }
    function open(): void { root.open() }
    function paste(value: string): string {
      return root.paste(value)
    }
    function show(): void { root.open() }
    function toggle(): void { root.toggle() }
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: ""
    active: root.opened
    tooltipText: "Quick Share"
    iconComponent: Component {
      OpticalGlyph {
        anchors.fill: parent
        text: ""
        fontFamily: button.fontFamily
        fontSize: button.fontSize
        color: button.active && button.useActiveColor
          ? button.activeColor
          : button.foreground
        opacity: root.iconOpacity
      }
    }
    onPressed: root.toggle()
  }

  SequentialAnimation {
    loops: Animation.Infinite
    running: root.transferring

    NumberAnimation {
      target: root
      property: "iconOpacity"
      from: 1.0
      to: 0.2
      duration: 1000
      easing.type: Easing.OutQuart
    }

    NumberAnimation {
      target: root
      property: "iconOpacity"
      from: 0.2
      to: 1.0
      duration: 1000
      easing.type: Easing.InQuart
    }

    onRunningChanged: if (!running) root.iconOpacity = 1.0
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    bar: root.bar
    owner: root
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(
      panelColumn.implicitHeight,
      Style.space(560),
    )

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function(dx, dy) {
        sharePanel.moveCursor(dx, dy)
      }
      onActivateRequested: sharePanel.activateCursor()
      onCloseRequested: root.close()
      onTabRequested: function(direction) {
        root.switchPanel(direction)
      }
      onTextKey: function(text) {
        if (text === "p" || text === "P") sharePanel.toggleSelectedPin()
      }

      Shortcut {
        enabled: root.opened
        sequences: [StandardKey.Paste]
        onActivated: root.readClipboard("preview", "")
      }

      Column {
        id: panelColumn
        width: parent.width
        spacing: Style.spacing.panelGap

        PanelHero {
          visible: !root.protocolReady
          width: parent.width
          title: "Quick Share"
          meta: root.protocolMeta
          foreground: root.foreground
          fontFamily: root.fontFamily
          iconComponent: Component {
            Text {
              text: ""
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.display
              textFormat: Text.PlainText
            }
          }
        }

        Text {
          visible: !root.protocolReady
          width: parent.width
          text: status.statusDetail
          color: Color.muted
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
          wrapMode: Text.WordWrap
          textFormat: Text.PlainText
        }

        SharePanel {
          id: sharePanel
          width: parent.width
          visible: root.protocolReady
          snapshot: status.endpointSnapshot
          actionError: status.actionError
          actionBusy: status.actionBusy
          clipboardPreview: root.clipboardPreview
          showPasteBadge: root.showPasteBadge
          onAcceptRequested: function(shareId) {
            status.accept(shareId)
          }
          onCancelRequested: function(shareId) {
            root.clearPasteBadge()
            if (shareId.length > 0) status.cancel(shareId)
            else status.stopDiscovery()
          }
          onDismissRequested: function(shareId) {
            root.clearPasteBadge()
            status.dismiss(shareId)
          }
          onDiscoverRequested: function() {
            status.discover()
          }
          onStopDiscoveryRequested: function() {
            status.stopDiscovery()
          }
          onPeerSelected: function(shareId, peerId) {
            if (shareId.length > 0) {
              status.sendTo(shareId, peerId)
              return
            }
            root.selectedPeerId = peerId
            root.selectionArmed = root.selectedPeerIsCurrent
            if (!root.selectionArmed) {
              status.actionError = "Select a currently available device."
              return
            }
            if (root.pasteLatch) root.submitCaptured()
            else if (root.readClipboardOnSelect) {
              root.readClipboard("send", peerId)
            } else {
              status.actionError =
                "Paste content to send to the selected device."
            }
          }
          onPinRequested: function(peerId, shouldPin) {
            if (shouldPin) status.pin(peerId)
            else status.unpin()
          }
          onRejectRequested: function(shareId) {
            status.reject(shareId)
          }
          onVisibilityRequested: function(shouldOpen) {
            status.setVisibility(shouldOpen)
          }
        }
      }
    }
  }
}
