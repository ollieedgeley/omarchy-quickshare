pragma Singleton
import QtQuick

QtObject {
    readonly property int cornerRadius: 6
    readonly property QtObject font: QtObject {
        readonly property int body: 12
        readonly property int bodySmall: 11
        readonly property int caption: 10
        readonly property int display: 20
        readonly property int displayLarge: 28
        readonly property string family: "monospace"
        readonly property int icon: 16
    }
    readonly property QtObject spacing: QtObject {
        readonly property int controlGap: 8
        readonly property int controlHeight: 28
        readonly property int controlPaddingX: 10
        readonly property int labelGap: 4
        readonly property int panelGap: 14
        readonly property int popupRowHeight: 28
        readonly property int rowPaddingX: 12
    }
    function normalFillFor() {
        return "#18212f";
    }
    function selectedFillFor() {
        return "#263a5c";
    }
    function space(value) {
        return value;
    }
}
