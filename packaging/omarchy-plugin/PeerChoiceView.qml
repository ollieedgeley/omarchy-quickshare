import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui

Item {
  id: root

  required property bool actionBusy
  required property bool cursorActive
  required property string discoveryMessage
  required property string discoveryState
  required property var orderedPeers
  required property string previewIcon
  required property string previewText
  required property string selectedTarget
  required property bool showPasteBadge

  signal cancelRequested()
  signal cursorRequested(string target)
  signal peerRequested(string peerId)
  signal pinRequested(string peerId, bool pinned)
  signal searchRequested()

  function hasCursor(target) {
    return cursorActive && selectedTarget === target
  }
  function showPeer(index) {
    if (index >= 0 && index < orderedPeers.length) {
      peerList.positionViewAtIndex(index, ListView.Contain)
    }
  }

  implicitHeight: content.implicitHeight

  Column {
    id: content
    width: parent.width
    spacing: Style.spacing.panelGap

    PanelHero {
      width: parent.width
      title: "Choose a device"
      meta: root.discoveryState === "searching"
        ? "Searching nearby"
        : "Select a recipient"
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

    AttachmentBadge {
      visible: root.showPasteBadge
      width: parent.width
      previewIcon: root.previewIcon
      previewText: root.previewText
    }

    PanelSeparator {
      foreground: Color.foreground
    }
    PanelSectionHeader {
      text: "NEARBY DEVICES"
      foreground: Color.foreground
      fontFamily: Style.font.family
      textFormat: Text.PlainText
    }
    ListView {
      id: peerList
      visible: root.orderedPeers.length > 0
      width: parent.width
      height: visible ? Math.min(
        root.orderedPeers.length * Style.spacing.popupRowHeight
          + Math.max(0, root.orderedPeers.length - 1) * spacing,
        Style.space(240)
      ) : 0
      spacing: Style.spacing.labelGap
      clip: true
      boundsBehavior: Flickable.StopAtBounds
      interactive: contentHeight > height
      model: root.orderedPeers

      delegate: CursorSurface {
        id: peerRow
        required property var modelData
        required property int index
        readonly property string peerId: String(modelData.id || "")
        readonly property string peerName: String(
          modelData.name || "Unnamed device"
        )
        readonly property string cursorTarget: "peer:" + peerId

        width: ListView.view.width
        implicitHeight: peerContent.implicitHeight + Style.spacing.rowPaddingX
        hasCursor: root.hasCursor(cursorTarget)
        current: Boolean(modelData.pinned)
        foreground: Color.foreground
        enabled: !root.actionBusy
        opacity: enabled ? 1 : 0.5
        Accessible.role: Accessible.Button
        Accessible.name: "Send to " + peerName
          + (modelData.pinned ? ", pinned" : "")
        Accessible.onPressAction: root.peerRequested(peerId)

        RowLayout {
          id: peerContent
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          anchors.leftMargin: Style.spacing.controlPaddingX
          anchors.rightMargin: Style.spacing.controlPaddingX
          spacing: Style.spacing.controlGap

          Text {
            Layout.fillWidth: true
            text: peerRow.peerName
            color: Color.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.body
            elide: Text.ElideRight
            textFormat: Text.PlainText
          }
          Text {
            visible: Boolean(peerRow.modelData.pinned)
            text: "Pinned"
            color: Color.accent
            font.family: Style.font.family
            font.pixelSize: Style.font.caption
            font.bold: true
            textFormat: Text.PlainText
          }
        }
        MouseArea {
          anchors.fill: parent
          enabled: !root.actionBusy
          hoverEnabled: true
          acceptedButtons: Qt.LeftButton | Qt.RightButton
          cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
          onEntered: root.cursorRequested(peerRow.cursorTarget)
          onClicked: function(mouse) {
            if (mouse.button === Qt.RightButton) {
              root.pinRequested(
                peerRow.peerId,
                Boolean(peerRow.modelData.pinned)
              )
            } else {
              root.peerRequested(peerRow.peerId)
            }
          }
        }
      }
    }

    Text {
      visible: root.discoveryMessage.length > 0
      width: parent.width
      text: root.discoveryMessage
      color: root.discoveryState === "timed_out"
          && root.orderedPeers.length === 0
        ? Color.urgent
        : Color.muted
      font.family: Style.font.family
      font.pixelSize: Style.font.bodySmall
      wrapMode: Text.WordWrap
      textFormat: Text.PlainText
    }

    Row {
      width: parent.width
      spacing: Style.spacing.controlGap

      Button {
        width: (parent.width - parent.spacing) / 2
        text: root.discoveryState === "searching"
          ? "Stop searching"
          : "Search again"
        bordered: true
        hasCursor: root.hasCursor("search")
        enabled: !root.actionBusy
        foreground: Color.foreground
        fontFamily: Style.font.family
        opacity: enabled ? 1 : 0.5
        Accessible.role: Accessible.Button
        Accessible.name: text
        Accessible.onPressAction: if (!root.actionBusy) root.searchRequested()
        onHovered: function(on) {
          if (on) root.cursorRequested("search")
        }
        onClicked: if (!root.actionBusy) root.searchRequested()
      }
      Button {
        width: (parent.width - parent.spacing) / 2
        text: "Cancel"
        bordered: true
        hasCursor: root.hasCursor("cancel")
        enabled: !root.actionBusy
        foreground: Color.urgent
        fontFamily: Style.font.family
        opacity: enabled ? 1 : 0.5
        Accessible.role: Accessible.Button
        Accessible.name: "Cancel this share"
        Accessible.onPressAction: if (!root.actionBusy) root.cancelRequested()
        onHovered: function(on) {
          if (on) root.cursorRequested("cancel")
        }
        onClicked: if (!root.actionBusy) root.cancelRequested()
      }
    }
  }
}
