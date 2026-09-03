import QtQuick
import qs.Commons
import qs.Ui

BarWidget {
  id: root

  moduleName: "io.github.ollieedgeley.omarchy-quickshare"

  property bool popupOpen: false
  readonly property bool opened: popupOpen

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

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  StatusProbe { id: status }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: ""
    active: root.opened
    tooltipText: "Quick Share"
    onPressed: root.toggle()
  }

  PopupCard {
    anchorItem: button
    bar: root.bar
    owner: root
    open: root.opened
    contentWidth: Style.space(300)
    contentHeight: Style.space(260)

    Column {
      anchors.fill: parent
      spacing: Style.space(8)

      Text {
        width: parent.width
        text: "Quick Share"
        color: Color.foreground
        font.family: Style.font.family
        font.pixelSize: Style.font.heading
      }

      Text {
        width: parent.width
        text: status.statusTitle
        color: status.protocolState === "ready"
          ? Color.accent
          : Color.foreground
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        visible: status.protocolState !== "ready"
      }

      Text {
        width: parent.width
        text: status.statusDetail
        color: Qt.darker(Color.foreground, 1.4)
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        wrapMode: Text.WordWrap
        visible: status.protocolState !== "ready"
      }

      SharePanel {
        width: parent.width
        accentColor: Color.accent
        dangerColor: Color.urgent
        foregroundColor: Color.foreground
        mutedColor: Qt.darker(Color.foreground, 1.4)
        snapshot: status.endpointSnapshot
        surfaceColor: Color.popups.background
        visible: status.protocolState === "ready"
        onAcceptRequested: function(shareId) {
          status.accept(shareId)
        }
        onCancelRequested: function(shareId) {
          status.cancel(shareId)
        }
        onPeerSelected: function(shareId, peerId) {
          status.sendTo(shareId, peerId)
        }
        onPinRequested: function(peerId) {
          status.pin(peerId)
        }
        onRejectRequested: function(shareId) {
          status.reject(shareId)
        }
      }
    }
  }
}
