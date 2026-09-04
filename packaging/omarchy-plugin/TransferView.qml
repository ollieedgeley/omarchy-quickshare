import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui

Item {
  id: root

  required property bool actionBusy
  required property bool cursorActive
  required property string direction
  required property string etaText
  required property int progressPercent
  required property string progressText
  required property string selectedTarget
  required property bool totalKnown

  signal cancelRequested()
  signal cursorRequested(string target)

  implicitHeight: content.implicitHeight

  Column {
    id: content
    width: parent.width
    spacing: Style.spacing.panelGap

    PanelHero {
      width: parent.width
      title: root.direction === "inbound" ? "Receiving" : "Sending"
      meta: "Keep both devices nearby"
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

    PanelSeparator {
      foreground: Color.foreground
    }
    BorderSurface {
      width: parent.width
      height: Style.spacing.controlHeight
      color: Style.normalFillFor(Color.foreground, Color.accent)
      borderSpec: Border.controlSpec(
        "normal",
        Color.foreground,
        Color.accent
      )
      radius: Style.cornerRadius
      clip: true

      Rectangle {
        width: root.totalKnown
          ? parent.width * root.progressPercent / 100
          : 0
        height: parent.height
        color: Style.selectedFillFor(Color.foreground, Color.accent)
      }
      Text {
        anchors.centerIn: parent
        text: root.totalKnown
          ? root.progressPercent + "%"
          : "Preparing transfer…"
        color: Color.foreground
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        font.bold: root.totalKnown
        textFormat: Text.PlainText
      }
    }
    RowLayout {
      width: parent.width

      Text {
        Layout.fillWidth: true
        text: root.progressText
        color: Color.foreground
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        elide: Text.ElideRight
        textFormat: Text.PlainText
      }
      Text {
        text: root.etaText
        color: Color.muted
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
        textFormat: Text.PlainText
      }
    }
    Button {
      width: parent.width
      text: "Cancel"
      bordered: true
      hasCursor: root.cursorActive && root.selectedTarget === "cancel"
      enabled: !root.actionBusy
      foreground: Color.urgent
      fontFamily: Style.font.family
      opacity: enabled ? 1 : 0.5
      Accessible.role: Accessible.Button
      Accessible.name: "Cancel transfer"
      Accessible.onPressAction: if (!root.actionBusy) root.cancelRequested()
      onHovered: function(on) {
        if (on) root.cursorRequested("cancel")
      }
      onClicked: if (!root.actionBusy) root.cancelRequested()
    }
  }
}
