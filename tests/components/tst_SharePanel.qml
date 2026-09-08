import QtQuick
import QtTest
import "../../packaging/omarchy-plugin" as Plugin

TestCase {
    id: testCase
    name: "SharePanelRecovery"
    when: windowShown
    visible: true
    width: 420
    height: 600
    property var view: null

    Component {
        id: panel
        Plugin.SharePanel {
            width: 400
            height: implicitHeight
        }
    }
    SignalSpy {
        id: dismissed
        target: testCase.view
        signalName: "dismissRequested"
    }

    function init() {
        view = createTemporaryObject(panel, testCase);
        verify(view !== null);
        dismissed.clear();
    }

    function cleanup() {
        view = null;
    }

    function operation(id, phase, hidden) {
        return {
            id_string: id,
            request_id: "retained-request",
            direction: "outbound",
            phase: phase,
            total_bytes: 16,
            transferred_bytes: phase === "completed" ? 16 : 0,
            terminal_reason: phase === "failed" ? "interrupted" : null,
            recovery_guidance: phase === "failed"
                               ? "Cannot resume; select content again." : null,
            dismissed: hidden
        };
    }

    function test_recovered_terminal_data() {
        return [
                    {
                        tag: "completed",
                        phase: "completed"
                    },
                    {
                        tag: "interrupted",
                        phase: "failed"
                    }
                ];
    }

    function test_recovered_terminal(data) {
        const latest = operation("18446744073709551615", data.phase, false);
        view.snapshot = {
            active_share: null
        };
        view.operations = [latest];
        compare(view.viewState, "terminal");
        compare(view.phase, data.phase);
        verify(!view.hasActiveShare);
        compare(Object.keys(view.attachment).length, 0);
        if (latest.recovery_guidance) {
            verify(view.terminalSummary.indexOf(latest.recovery_guidance) >= 0);
        }
        view.setCursor("done");
        view.activateCursor();
        compare(dismissed.count, 1);
        compare(dismissed.signalArguments[0][0], latest.id_string);
    }

    function test_latest_dismissal_does_not_reveal_older_work() {
        const older = operation("7", "completed", false);
        const latest = operation("8", "failed", true);
        view.snapshot = {
            active_share: null
        };
        view.operations = [older, latest];
        compare(view.recoveredOperation, null);
        compare(view.viewState, "idle");
        view.dismiss();
        compare(dismissed.count, 0);
    }
}
