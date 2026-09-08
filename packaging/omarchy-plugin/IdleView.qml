import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui

Column {
    id: root

    required property bool actionBusy
    required property bool cursorActive
    required property string selectedTarget
    required property string visibilityMessage
    required property bool visibilityRequestedByDaemon

    signal cursorRequested(string target)
    signal visibilityRequested

    spacing: Style.spacing.panelGap

    PanelHero {
        width: parent.width
        title: "Quick Share"
        meta: root.visibilityMessage
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
    PanelSectionHeader {
        text: "RECEIVE"
        foreground: Color.foreground
        fontFamily: Style.font.family
        textFormat: Text.PlainText
    }
    CursorSurface {
        width: parent.width
        implicitHeight: receiveRow.implicitHeight + Style.spacing.rowPaddingX
        hasCursor: root.cursorActive && root.selectedTarget === "visibility"
        foreground: Color.foreground
        enabled: !root.actionBusy
        opacity: enabled ? 1 : 0.5
        Accessible.role: Accessible.CheckBox
        Accessible.name: root.visibilityRequestedByDaemon
                         ? "Close Quick Share visibility" :
                           "Open Quick Share visibility"
        Accessible.checked: root.visibilityRequestedByDaemon
        Accessible.onPressAction: root.visibilityRequested()

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
                text: root.visibilityMessage
                color: Color.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.body
                elide: Text.ElideRight
                textFormat: Text.PlainText
            }
            ToggleSwitch {
                checked: root.visibilityRequestedByDaemon
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
            onEntered: root.cursorRequested("visibility")
            onClicked: root.visibilityRequested()
        }
    }
}
