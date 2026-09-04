import QtQuick
import qs.Commons
import qs.Ui

Item {
  id: root

  required property bool actionBusy
  required property string attachmentName
  required property bool cursorActive
  required property string peerName
  required property string selectedTarget
  required property string verificationCode
  required property bool waiting

  signal acceptRequested()
  signal cancelRequested()
  signal cursorRequested(string target)
  signal rejectRequested()

  function hasCursor(target) {
    return cursorActive && selectedTarget === target
  }

  implicitHeight: content.implicitHeight

  Column {
    id: content
    width: parent.width
    spacing: Style.spacing.panelGap

    PanelHero {
      width: parent.width
      title: root.waiting ? "Waiting for approval" : "Incoming share"
      meta: root.waiting
        ? "Confirm on the other device"
        : "Verify both devices"
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
      visible: root.attachmentName.length > 0
      width: parent.width
      text: root.attachmentName
      color: Color.foreground
      font.family: Style.font.family
      font.pixelSize: Style.font.body
      elide: Text.ElideRight
      textFormat: Text.PlainText
    }

    PanelSeparator {
      foreground: Color.foreground
    }
    PanelSectionHeader {
      text: "VERIFICATION CODE"
      foreground: Color.foreground
      fontFamily: Style.font.family
      textFormat: Text.PlainText
    }
    Text {
      visible: root.peerName.length > 0
      width: parent.width
      text: root.waiting
        ? "Sharing with " + root.peerName
        : "From " + root.peerName
      color: Color.muted
      font.family: Style.font.family
      font.pixelSize: Style.font.bodySmall
      elide: Text.ElideRight
      textFormat: Text.PlainText
    }
    Text {
      width: parent.width
      text: root.verificationCode.length > 0
        ? root.verificationCode
        : "Waiting for a verification code…"
      color: root.verificationCode.length > 0
        ? Color.accent
        : Color.muted
      font.family: Style.font.family
      font.pixelSize: root.verificationCode.length > 0
        ? Style.font.displayLarge
        : Style.font.body
      font.bold: root.verificationCode.length > 0
      horizontalAlignment: Text.AlignHCenter
      textFormat: Text.PlainText
      Accessible.name: root.verificationCode.length > 0
        ? "Verification code " + root.verificationCode
        : text
    }
    Text {
      visible: root.verificationCode.length > 0
      width: parent.width
      text: root.waiting
        ? "Ask the other person to confirm this code."
        : "Make sure this code matches the other device."
      color: Color.muted
      font.family: Style.font.family
      font.pixelSize: Style.font.bodySmall
      horizontalAlignment: Text.AlignHCenter
      wrapMode: Text.WordWrap
      textFormat: Text.PlainText
    }

    Row {
      visible: !root.waiting
      width: parent.width
      spacing: Style.spacing.controlGap

      Button {
        width: (parent.width - parent.spacing) / 2
        text: "Accept"
        bordered: true
        hasCursor: root.hasCursor("accept")
        enabled: !root.actionBusy
        foreground: Color.accent
        fontFamily: Style.font.family
        opacity: enabled ? 1 : 0.5
        Accessible.role: Accessible.Button
        Accessible.name: "Accept incoming share"
        Accessible.onPressAction: if (!root.actionBusy) root.acceptRequested()
        onHovered: function(on) {
          if (on) root.cursorRequested("accept")
        }
        onClicked: if (!root.actionBusy) root.acceptRequested()
      }
      Button {
        width: (parent.width - parent.spacing) / 2
        text: "Reject"
        bordered: true
        hasCursor: root.hasCursor("reject")
        enabled: !root.actionBusy
        foreground: Color.urgent
        fontFamily: Style.font.family
        opacity: enabled ? 1 : 0.5
        Accessible.role: Accessible.Button
        Accessible.name: "Reject incoming share"
        Accessible.onPressAction: if (!root.actionBusy) root.rejectRequested()
        onHovered: function(on) {
          if (on) root.cursorRequested("reject")
        }
        onClicked: if (!root.actionBusy) root.rejectRequested()
      }
    }

    Button {
      visible: root.waiting
      width: parent.width
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
