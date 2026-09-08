pragma Singleton
import QtQuick

QtObject {
    readonly property color accent: "#7aa2f7"
    readonly property color background: "#24283b"
    readonly property color foreground: "#d8dee9"
    readonly property color muted: "#8f98a8"
    readonly property color urgent: "#f7768e"
    readonly property QtObject popups: QtObject {
        readonly property color background: "#24283b"
    }
}
