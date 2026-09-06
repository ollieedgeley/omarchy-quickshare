import QtQuick
import Quickshell.Io
import qs.Commons
import qs.Ui

BarWidget {
  id: root

  moduleName: "io.github.ollieedgeley.omarchy-quickshare"

  property bool popupOpen: false
  property bool popoutSwitchClosing: false
  property bool pasteLatch: false
  property bool pasteActionComplete: false
  property bool pastePending: false
  property string clipboardAction: ""
  property string clipboardOutput: ""
  property string clipboardPeerId: ""
  property string clipboardPreview: ""
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
    popupOpen = true
    if (!status.discover()) status.refresh()
  }

  function close() {
    popupOpen = false
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

  function clearPasteBadge() {
    pasteLatch = false
    clipboardPreview = ""
  }

  function captureClipboard(value) {
    if (String(value).length === 0) {
      status.actionError = "Clipboard is empty or unavailable."
      return false
    }
    clipboardPreview = String(value)
    pasteLatch = true
    status.actionError = ""
    return true
  }

  function finishClipboard(value) {
    var action = clipboardAction
    var peerId = clipboardPeerId
    clipboardAction = ""
    clipboardPeerId = ""
    clipboardOutput = ""
    if (!captureClipboard(value)) return
    if (action === "send") {
      if (!status.submitTo(peerId, value)) {
        status.actionError = "Quick Share is busy. Try again."
        return
      }
      clearPasteBadge()
    }
  }

  function readClipboard(action, peerId) {
    if (clipboardBusy) return false
    clipboardAction = action
    clipboardPeerId = String(peerId || "")
    clipboardOutput = ""
    clipboardUriProbe.running = true
    return true
  }

  function paste(value) {
    if (opened) return captureClipboard(value) ? "ok" : "empty"
    if (!status.submit(value)) return "busy"
    pasteActionComplete = false
    pastePending = true
    return "ok"
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  StatusProbe { id: status }

  Shortcut {
    enabled: root.opened
    sequence: StandardKey.Paste
    onActivated: root.readClipboard("preview", "")
  }

  property Process clipboardUriProbe: Process {
    command: ["wl-paste", "--type", "text/uri-list", "--no-newline"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.clipboardOutput = String(text || "")
    }
    onExited: function(exitCode) {
      if (exitCode === 0 && root.clipboardOutput.length > 0) {
        root.finishClipboard(root.clipboardOutput)
        return
      }
      root.clipboardOutput = ""
      root.clipboardTextProbe.running = true
    }
  }

  property Process clipboardTextProbe: Process {
    command: ["wl-paste", "--no-newline"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.clipboardOutput = String(text || "")
    }
    onExited: function(exitCode) {
      root.finishClipboard(exitCode === 0 ? root.clipboardOutput : "")
    }
  }

  Connections {
    target: status

    function onActionFinished(succeeded) {
      if (!root.pastePending) return
      if (succeeded) {
        root.pasteActionComplete = true
        return
      }
      root.pastePending = false
      root.pasteActionComplete = false
      root.open()
    }

    function onEndpointSnapshotChanged() {
      if (!root.pastePending || !root.pasteActionComplete) return
      var share = status.endpointSnapshot.active_share || ({})
      var phase = String(share.phase || "")
      root.pastePending = false
      root.pasteActionComplete = false
      if (phase === "waiting_for_peer") root.open()
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
            status.cancel(shareId)
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
            if (shareId.length > 0) status.sendTo(shareId, peerId)
            else root.readClipboard("send", peerId)
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
