import QtQuick
import Quickshell.Io
import qs.Commons
import qs.Ui

BarWidget {
  id: root

  moduleName: "io.github.ollieedgeley.omarchy-quickshare"

  property bool popupOpen: false
  readonly property bool opened: popupOpen
  property bool pastePending: false
  property bool pasteActionComplete: false
  readonly property bool transferring:
    status.activeShare.phase === "transferring"

  function open() {
    popupOpen = true
    status.refresh()
  }

  function close() {
    popupOpen = false
  }

  function closeForPopoutSwitch() {
    close()
  }

  function toggle() {
    if (opened) close()
    else open()
  }

  function submit(value) {
    if (!status.submit(value)) return false
    pastePending = true
    pasteActionComplete = false
    return true
  }

  function paste(value) {
    return submit(value) ? "ok" : "busy"
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  StatusProbe { id: status }

  Connections {
    target: status

    function onActionFinished(succeeded) {
      if (!root.pastePending) {
        root.pasteActionComplete = false
        return
      }
      if (succeeded) {
        root.pasteActionComplete = true
      } else {
        root.pastePending = false
        root.pasteActionComplete = false
        root.open()
      }
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
    onPressed: root.toggle()
  }

  SequentialAnimation {
    loops: Animation.Infinite
    running: root.transferring

    NumberAnimation {
      target: button
      property: "opacity"
      from: 1.0
      to: 0.2
      duration: 1000
      easing.type: Easing.OutQuart
    }

    NumberAnimation {
      target: button
      property: "opacity"
      from: 0.2
      to: 1.0
      duration: 1000
      easing.type: Easing.InQuart
    }

    onRunningChanged: if (!running) button.opacity = 1.0
  }

  PopupCard {
    anchorItem: button
    bar: root.bar
    owner: root
    open: root.opened
    contentWidth: Style.space(320)
    contentHeight: Style.space(360)

    Column {
      anchors.fill: parent
      spacing: Style.spacing.lg

      Text {
        width: parent.width
        text: "Quick Share"
        color: Color.foreground
        font.family: Style.font.family
        font.pixelSize: Style.font.heading
        textFormat: Text.PlainText
        visible: status.protocolState !== "ready"
      }

      Text {
        width: parent.width
        text: status.statusTitle
        color: status.protocolState === "ready"
          ? Color.accent
          : Color.foreground
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        textFormat: Text.PlainText
        visible: status.protocolState !== "ready"
      }

      Text {
        width: parent.width
        text: status.statusDetail
        color: Color.muted
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        wrapMode: Text.WordWrap
        textFormat: Text.PlainText
        visible: status.protocolState !== "ready"
      }

      SharePanel {
        width: parent.width
        accentColor: Color.accent
        actionError: status.actionError
        bodyFontSize: Style.font.body
        borderWidth: Style.normalBorderWidth
        focusBorderWidth: Style.focusBorderWidth
        hoverFillAlpha: Style.hoverFillAlpha
        pressedFillAlpha: Style.pressedFillAlpha
        controlHeight: Style.spacing.controlHeight
        dangerColor: Color.urgent
        fontFamily: Style.font.family
        foregroundColor: Color.foreground
        gap: Style.spacing.lg
        mutedColor: Color.muted
        radius: Style.cornerRadius
        smallFontSize: Style.font.bodySmall
        smallGap: Style.spacing.sm
        snapshot: status.endpointSnapshot
        surfaceColor: Color.popups.background
        visible: status.protocolState === "ready"
        onAcceptRequested: function(shareId) {
          status.accept(shareId)
        }
        onCancelRequested: function(shareId) {
          status.cancel(shareId)
        }
        onDismissRequested: function(shareId) {
          status.dismiss(shareId)
        }
        onDiscoverRequested: function() {
          status.discover()
        }
        onStopDiscoveryRequested: function() {
          status.stopDiscovery()
        }
        onPeerSelected: function(shareId, peerId) {
          status.sendTo(shareId, peerId)
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
