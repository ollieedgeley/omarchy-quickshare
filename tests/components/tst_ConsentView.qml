import QtQuick
import QtTest
import "../../packaging/omarchy-plugin" as Plugin

TestCase {
    id: testCase
    name: "ConsentViewComponent"
    when: windowShown
    visible: true
    width: 420
    height: 600
    property var view: null

    Component {
        id: consent
        Plugin.ConsentView {
            width: 400
            height: implicitHeight
            actionBusy: false
            attachmentName: "photo.png"
            attachmentIcon: ""
            cursorActive: false
            peerName: "Workshop laptop"
            selectedTarget: ""
            verificationCode: "1234"
            showPasteBadge: false
            waiting: false
        }
    }

    SignalSpy {
        id: acceptSpy
        target: testCase.view
        signalName: "acceptRequested"
    }
    SignalSpy {
        id: rejectSpy
        target: testCase.view
        signalName: "rejectRequested"
    }
    SignalSpy {
        id: cancelSpy
        target: testCase.view
        signalName: "cancelRequested"
    }

    function init() {
        view = createTemporaryObject(consent, testCase);
        verify(view !== null);
        verify(waitForRendering(view));
        acceptSpy.clear();
        rejectSpy.clear();
        cancelSpy.clear();
    }

    function cleanup() {
        view = null;
    }

    function test_rendered_consent_controls_data() {
        return [
                    {
                        tag: "accept",
                        control: "accept",
                        waiting: false
                    },
                    {
                        tag: "reject",
                        control: "reject",
                        waiting: false
                    },
                    {
                        tag: "cancel",
                        control: "cancel",
                        waiting: true
                    }
                ];
    }

    function test_rendered_consent_controls(data) {
        view.waiting = data.waiting;
        const button = findChild(view, "quickshare-consent-" + data.control);
        verify(button !== null);
        tryCompare(button, "visible", true);
        // Settle positioners after visibility changes without requiring a
        // future frame when the requested control was already visible.
        verify(waitForPolish(testCase.Window.window));
        const center = button.mapToItem(testCase, button.width / 2,
                                        button.height / 2);
        verify(center.x > 0 && center.x < testCase.width);
        verify(center.y > 0 && center.y < testCase.height);
        view.actionBusy = true;
        tryCompare(button, "enabled", false);
        mouseClick(button, button.width / 2, button.height / 2);
        compare(acceptSpy.count + rejectSpy.count + cancelSpy.count, 0);

        view.actionBusy = false;
        tryCompare(button, "enabled", true);
        mouseClick(button, button.width / 2, button.height / 2);
        compare(acceptSpy.count, data.control === "accept" ? 1 : 0);
        compare(rejectSpy.count, data.control === "reject" ? 1 : 0);
        compare(cancelSpy.count, data.control === "cancel" ? 1 : 0);
    }
}
