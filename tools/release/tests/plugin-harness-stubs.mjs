export const HARNESS_STUBS = {
  "Commons/Border.qml": `pragma Singleton
import QtQuick
QtObject {
  function controlSpec() { return ({}) }
}
`,
  "Commons/Color.qml": `pragma Singleton
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
`,
  "Commons/qmldir": `module qs.Commons
singleton Border 1.0 Border.qml
singleton Color 1.0 Color.qml
singleton Style 1.0 Style.qml
`,
  "Commons/Style.qml": `pragma Singleton
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
  function normalFillFor() { return "#18212f" }
  function selectedFillFor() { return "#263a5c" }
  function space(value) { return value }
}
`,
  "Ui/BarIconButton.qml": `import QtQuick
Item {
  property bool active: false
  property color activeColor: "#7aa2f7"
  property var bar
  property color foreground: "#d8dee9"
  property string fontFamily: "monospace"
  property int fontSize: 16
  property Component iconComponent
  property string text: ""
  property string tooltipText: ""
  property bool useActiveColor: true
  signal pressed()
  implicitHeight: 24
  implicitWidth: 24
}
`,
  "Ui/BarWidget.qml": `import QtQuick
Item {
  property var bar
  property string moduleName: ""
}
`,
  "Ui/BorderSurface.qml": `import QtQuick
Rectangle {
  property var borderSpec: ({})
}
`,
  "Ui/Button.qml": `import QtQuick
Item {
  property bool bordered: false
  property color foreground: "#d8dee9"
  property bool hasCursor: false
  property string fontFamily: "monospace"
  property string text: ""
  signal clicked()
  signal hovered(bool on)
  implicitHeight: 28
}
`,
  "Ui/CursorSurface.qml": `import QtQuick
Item {
  property bool current: false
  property color foreground: "#d8dee9"
  property bool hasCursor: false
}
`,
  "Ui/KeyboardPanel.qml": `import QtQuick
Item {
  property Item anchorItem
  property var bar
  property real contentHeight: 0
  property real contentWidth: 0
  property Item focusTarget
  property bool open: false
  property var owner
  function fittedContentHeight(value, maximum) {
    return Math.min(value, maximum)
  }
  function fittedContentWidth(value) { return value }
}
`,
  "Ui/OpticalGlyph.qml": `import QtQuick
Item {
  property color color: "#d8dee9"
  property string fontFamily: "monospace"
  property int fontSize: 16
  property string text: ""
}
`,
  "Ui/PanelHero.qml": `import QtQuick
Item {
  property color foreground: "#d8dee9"
  property string fontFamily: "monospace"
  property Component iconComponent
  property string meta: ""
  property string title: ""
  implicitHeight: 36
}
`,
  "Ui/PanelKeyCatcher.qml": `import QtQuick
Item {
  signal activateRequested()
  signal closeRequested()
  signal moveRequested(int dx, int dy)
  signal tabRequested(int direction)
  signal textKey(string text)
}
`,
  "Ui/PanelSectionHeader.qml": `import QtQuick
Item {
  property color foreground: "#d8dee9"
  property string fontFamily: "monospace"
  property string text: ""
  property int textFormat: Text.PlainText
  implicitHeight: 18
}
`,
  "Ui/PanelSeparator.qml": `import QtQuick
Item {
  property color foreground: "#d8dee9"
  implicitHeight: 1
}
`,
  "Ui/qmldir": `module qs.Ui
BarIconButton 1.0 BarIconButton.qml
BarWidget 1.0 BarWidget.qml
BorderSurface 1.0 BorderSurface.qml
Button 1.0 Button.qml
CursorSurface 1.0 CursorSurface.qml
KeyboardPanel 1.0 KeyboardPanel.qml
OpticalGlyph 1.0 OpticalGlyph.qml
PanelHero 1.0 PanelHero.qml
PanelKeyCatcher 1.0 PanelKeyCatcher.qml
PanelSectionHeader 1.0 PanelSectionHeader.qml
PanelSeparator 1.0 PanelSeparator.qml
ToggleSwitch 1.0 ToggleSwitch.qml
`,
  "Ui/ToggleSwitch.qml": `import QtQuick
Item {
  property bool busy: false
  property bool checked: false
  property color foreground: "#d8dee9"
  property bool interactive: true
  implicitHeight: 18
  implicitWidth: 32
}
`,
};
