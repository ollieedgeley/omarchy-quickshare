import QtQuick
import qs.Commons
import qs.Ui

Item {
  id: root

  required property bool actionBusy
  required property bool cursorActive
  required property string phase
  required property string selectedTarget
  required property string summary
  required property string title
  required property color tone

  signal cursorRequested(string target)
  signal doneRequested()

  implicitHeight: content.implicitHeight

  Column {
    id: content
    width: parent.width
    spacing: Style.spacing.panelGap

    PanelHero {
      width: parent.width
      title: root.title
      meta: root.phase === "completed"
        ? "Complete"
        : (root.phase === "failed" ? "Needs attention" : "Finished")
      foreground: root.tone
      fontFamily: Style.font.family
      iconComponent: Component {
        Text {
          text: root.phase === "completed"
            ? "󰄬"
            : (root.phase === "failed" ? "󰅚" : "󰜺")
          color: root.tone
          font.family: Style.font.family
          font.pixelSize: Style.font.display
          textFormat: Text.PlainText
        }
      }
    }

    PanelSeparator {
      foreground: root.tone
    }
    Text {
      width: parent.width
      text: root.summary
      color: root.phase === "failed" ? Color.urgent : Color.muted
      font.family: Style.font.family
      font.pixelSize: Style.font.body
      wrapMode: Text.WordWrap
      textFormat: Text.PlainText
    }
    Button {
      width: parent.width
      text: "Done"
      bordered: true
      hasCursor: root.cursorActive && root.selectedTarget === "done"
      enabled: !root.actionBusy
      foreground: root.phase === "failed" ? Color.urgent : Color.accent
      fontFamily: Style.font.family
      opacity: enabled ? 1 : 0.5
      Accessible.role: Accessible.Button
      Accessible.name: "Close transfer result"
      Accessible.onPressAction: if (!root.actionBusy) root.doneRequested()
      onHovered: function(on) {
        if (on) root.cursorRequested("done")
      }
      onClicked: if (!root.actionBusy) root.doneRequested()
    }
  }
}
