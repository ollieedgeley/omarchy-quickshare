import QtQuick

Item {
    property bool bordered: false
    property color foreground: "#d8dee9"
    property bool hasCursor: false
    property string fontFamily: "monospace"
    property string text: ""
    signal clicked
    signal hovered(bool on)
    implicitHeight: 28

    MouseArea {
        anchors.fill: parent
        onClicked: parent.clicked()
    }
}
