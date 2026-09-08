import QtQuick
import QtTest
import "../../packaging/omarchy-plugin" as Plugin

// Only presentation dependencies are doubled in imports/qs. PeerChoiceView,
// its peer labels and its MouseArea are production QML. Input properties stand
// in for service state. No daemon, peer, shell process or compositor
// is started.
// This does not establish Omarchy input routing or installed-plugin
// compatibility.
TestCase {
    id: testCase
    name: "PeerChoiceViewComponent"
    when: windowShown
    // QtTest's TestCase defaults to invisible.
    // It owns the rendered fixture here.
    visible: true
    width: 420
    height: 500

    property var view: null

    Component {
        id: peerChoice
        Plugin.PeerChoiceView {
            width: 400
            height: implicitHeight
            actionBusy: false
            cursorActive: false
            discoveryMessage: ""
            discoveryState: "complete"
            orderedPeers: [
                {
                    id: "controlled-peer",
                    name: "Workshop laptop",
                    pinned: false
                }
            ]
            previewIcon: ""
            previewText: ""
            selectedTarget: ""
            showPasteBadge: false
        }
    }

    SignalSpy {
        id: sendSpy
        target: testCase.view
        signalName: "peerRequested"
    }

    SignalSpy {
        id: pinSpy
        target: testCase.view
        signalName: "pinRequested"
    }

    function init() {
        view = createTemporaryObject(peerChoice, testCase);
        verify(view !== null);
        tryVerify(function () {
            return findChild(view, "quickshare-peer-controlled-peer") !== null;
        });
        verify(waitForRendering(view));
        sendSpy.clear();
        pinSpy.clear();
    }

    function cleanup() {
        view = null;
    }

    function test_rendered_peer_can_be_selected_with_mouse() {
        const label = findChild(view, "quickshare-peer-name-controlled-peer");
        verify(label !== null);
        verify(label.visible);
        compare(label.text, "Workshop laptop");
        verify(label.paintedWidth > 0);
        verify(label.paintedHeight > 0);

        const recipient = findChild(view, "quickshare-peer-controlled-peer");
        verify(recipient.visible);
        verify(recipient.enabled);
        mouseClick(recipient, recipient.width / 2, recipient.height / 2);
        compare(sendSpy.count, 1);
        compare(sendSpy.signalArguments[0][0], "controlled-peer");
        compare(pinSpy.count, 0);
    }

    function test_busy_blocks_selection_and_pinning_until_released() {
        const recipient = findChild(view, "quickshare-peer-controlled-peer");
        view.actionBusy = true;
        tryCompare(recipient, "enabled", false);
        mouseClick(recipient, recipient.width / 2, recipient.height / 2);
        mouseClick(recipient, recipient.width / 2, recipient.height / 2,
                   Qt.RightButton);
        compare(sendSpy.count, 0);
        compare(pinSpy.count, 0);

        view.actionBusy = false;
        tryCompare(recipient, "enabled", true);
        mouseClick(recipient, recipient.width / 2, recipient.height / 2,
                   Qt.RightButton);
        compare(pinSpy.count, 1);
        compare(pinSpy.signalArguments[0][0], "controlled-peer");
        compare(pinSpy.signalArguments[0][1], false);
        compare(sendSpy.count, 0);
    }
}
