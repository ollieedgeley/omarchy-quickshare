import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui

BorderSurface {
  id: root

  required property string previewIcon
  required property string previewText

  objectName: "pasteBadge"
  implicitHeight: pasteRow.implicitHeight + Style.spacing.rowPaddingX
  color: Style.selectedFillFor(Color.foreground, Color.accent)
  borderSpec: Border.controlSpec(
    "normal",
    Color.foreground,
    Color.accent
  )
  radius: Style.cornerRadius
  Accessible.role: Accessible.StaticText
  Accessible.name: root.previewText

  RowLayout {
    id: pasteRow
    anchors.fill: parent
    anchors.leftMargin: Style.spacing.controlPaddingX
    anchors.rightMargin: Style.spacing.controlPaddingX
    spacing: Style.spacing.controlGap

    Text {
      text: root.previewIcon
      color: Color.accent
      font.family: Style.font.family
      font.pixelSize: Style.font.icon
      textFormat: Text.PlainText
    }
    Text {
      Layout.fillWidth: true
      text: root.previewText
      color: Color.foreground
      font.family: Style.font.family
      font.pixelSize: Style.font.bodySmall
      elide: Text.ElideRight
      textFormat: Text.PlainText
    }
  }
}
