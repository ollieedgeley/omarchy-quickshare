import QtQuick
import Quickshell

ShellRoot {
    id: root
    readonly property string socketRoot: Quickshell.env(
                                             "QUICKSHARE_SOCKET_ROOT")
    readonly property string exactShareId: "18446744073709551615"
    property int step: 0
    property int ticks: 0
    property int finished: 0
    property bool lastSucceeded: false
    property double deadlineStarted: Date.now()
    property bool deadlineObserved: false
    property string lostRequestId: ""
    property string unknownRequestId: ""
    property int lostFinished: 0
    property int unknownFinished: 0
    property int queuedFinished: 0
    property int invalidFinished: 0
    property int unknownQueries: 0
    property bool unknownChecked: false
    property var reloaded: null

    function checkPending(target, requestId) {
        check(target.pendingRequestId === requestId && requestId.length > 0,
              "uncertain submission identity discarded");
        check(!target.submitTo("unicode-peer", "socket payload"),
              "uncertain submission admitted a retry");
    }

    function check(condition, message) {
        if (!condition) {
            console.error("HARNESS_FAIL", message);
            Qt.exit(2);
            throw new Error(message);
        }
    }

    QuickShareService {
        id: service
        socketPath: root.socketRoot + "/ready.sock"
        onActionFinished: function (succeeded) {
            root.finished += 1;
            root.lastSucceeded = succeeded;
        }
    }
    QuickShareService {
        id: version
        socketPath: root.socketRoot + "/version.sock"
    }
    QuickShareService {
        id: eof
        socketPath: root.socketRoot + "/eof.sock"
    }
    QuickShareService {
        id: overflow
        socketPath: root.socketRoot + "/overflow.sock"
    }
    QuickShareService {
        id: boundary
        socketPath: root.socketRoot + "/boundary.sock"
    }
    QuickShareService {
        id: invalidUtf8
        socketPath: root.socketRoot + "/invalid-utf8.sock"
    }
    QuickShareService {
        id: deadline
        socketPath: root.socketRoot + "/deadline.sock"
        onProtocolStateChanged: {
            if (protocolState === "unavailable") {
                root.check(Date.now() - root.deadlineStarted >= 4500,
                           "deadline fired before total request budget");
                root.deadlineObserved = true;
            }
        }
    }
    QuickShareService {
        id: lost
        socketPath: root.socketRoot + "/lost-submit.sock"
        onActionFinished: function (succeeded) {
            root.check(succeeded, "reconciled acceptance reported failure");
            root.lostFinished += 1;
        }
    }
    QuickShareService {
        id: unknown
        socketPath: root.socketRoot + "/unknown-submit.sock"
        onActionFinished: root.unknownFinished += 1
        onReconciliationBusyChanged: {
            if (reconciliationBusy)
                root.unknownQueries += 1;
        }
    }
    QuickShareService {
        id: queued
        socketPath: root.socketRoot + "/queued-submit.sock"
        onActionFinished: function (succeeded) {
            root.check(succeeded, "queued acceptance reported failure");
            root.queuedFinished += 1;
        }
    }
    QuickShareService {
        id: invalid
        socketPath: root.socketRoot + "/invalid-submit.sock"
        onActionFinished: function (succeeded) {
            root.check(succeeded,
                       "invalid reply reconciliation reported failure");
            root.invalidFinished += 1;
        }
    }
    Component {
        id: reloadFactory
        QuickShareService {
            socketPath: root.socketRoot + "/lost-submit.sock"
        }
    }

    Timer {
        interval: 25
        running: true
        repeat: true
        onTriggered: {
            root.ticks += 1;
            root.check(root.ticks < 900, "runtime fixture timed out at step "
                       + root.step);
            if (root.step === 0 && service.protocolState === "ready") {
                root.check(!service.snapshotStale,
                           "fresh snapshot marked stale");
                root.check(service.endpointSnapshot.peers[0].name
                           === "Café 東京 📱",
                           "fragmented UTF-8 peer name corrupted");
                root.check(service.activeShare.id_string === root.exactShareId,
                           "u64 identity changed");
                root.check(service.appliedPreferences.device_name
                           === "Café 東京 📱"
                           && service.savedPreferences.device_name
                           === "Café 東京 📱" && service.preferenceStatus.pending
                           === false, "preferences not observed");
                service.socketPath = root.socketRoot + "/eof.sock";
                service.refresh();
                root.step = 1;
            } else if (root.step === 1 && service.protocolState
                       === "unavailable") {
                root.check(service.snapshotStale, "lost snapshot not stale");
                root.check(service.endpointSnapshot.peers[0].name
                           === "Café 東京 📱" && service.activeShare.id_string
                           === root.exactShareId
                           && service.appliedPreferences.device_name
                           === "Café 東京 📱",
                           "last complete observation was discarded");
                root.check(!service.accept(root.exactShareId),
                           "stale action admitted");
                service.socketPath = root.socketRoot + "/recovery.sock";
                service.refresh();
                root.step = 2;
            } else if (root.step === 2 && service.protocolState === "ready") {
                root.check(!service.snapshotStale
                           && service.endpointSnapshot.peers[0].name
                           === "Recovered 📱"
                           && service.savedPreferences.device_name
                           === "Recovered 📱",
                           "recovery did not replace observation");
                root.check(service.accept(root.exactShareId),
                           "accept not admitted");
                root.check(service.actionBusy, "action not busy before reply");
                root.check(!service.reject(root.exactShareId),
                           "second action admitted");
                root.step = 3;
            } else if (root.step === 3 && root.finished === 1
                       && service.protocolState === "ready") {
                root.check(root.lastSucceeded && !service.actionBusy,
                           "ack not reported");
                root.check(service.activeShare.phase
                           === "awaiting_local_consent",
                           "admission acknowledgment invented "
                           + "a transfer outcome");
                root.check(service.cancel(root.exactShareId),
                           "cancel not admitted");
                root.step = 4;
            } else if (root.step === 4 && root.finished === 2) {
                root.check(!root.lastSucceeded && !service.actionBusy
                           && service.actionError.length > 0,
                           "lost action reply reported success");
                root.step = 5;
            } else if (root.step === 5 && root.deadlineObserved
                       && service.protocolState === "ready") {
                root.check(version.protocolState === "incompatible",
                           "version mismatch accepted");
                root.check(eof.protocolState === "unavailable",
                           "EOF accepted without LF");
                root.check(overflow.protocolState === "unavailable",
                           "oversized UTF-8 frame accepted");
                root.check(boundary.protocolState === "ready" &&
                           !boundary.snapshotStale
                           && boundary.endpointSnapshot.peers[0].name
                           === "Café 東京 📱",
                           "exact-limit frame with fragmented "
                           + "final Unicode refused");
                root.check(invalidUtf8.protocolState === "unavailable",
                           "malformed UTF-8 frame accepted");
                root.check(root.finished === 2,
                           "lost mutation automatically retried");
                root.step = 6;
            } else if (root.step === 6 && lost.protocolState === "ready"
                       && unknown.protocolState === "ready"
                       && queued.protocolState === "ready"
                       && invalid.protocolState === "ready") {
                root.check(lost.submitTo("unicode-peer", "socket payload"),
                           "lost submit refused");
                root.lostRequestId = lost.pendingRequestId;
                root.checkPending(lost, root.lostRequestId);
                root.check(unknown.submitTo("unicode-peer", "socket payload"),
                           "unknown submit refused");
                root.unknownRequestId = unknown.pendingRequestId;
                root.checkPending(unknown, root.unknownRequestId);
                root.check(queued.submitTo("unicode-peer", "socket payload"),
                           "queued submit refused");
                root.check(invalid.submitTo("unicode-peer", "socket payload"),
                           "invalid submit refused");
                root.step = 7;
            } else if (root.step === 7) {
                if (lost.reconciliationBusy) {
                    root.checkPending(lost, root.lostRequestId);
                    root.check(root.lostFinished === 0,
                               "lost reply signalled acceptance before query");
                }
                if (invalid.reconciliationBusy) {
                    root.check(invalid.pendingRequestId.length > 0
                               && root.invalidFinished === 0,
                               "invalid queued operation accepted "
                               + "before reconciliation");
                }
                if (root.unknownQueries >= 2 && !unknown.reconciliationBusy) {
                    root.checkPending(unknown, root.unknownRequestId);
                    root.check(root.unknownFinished === 0
                               && unknown.actionError.length > 0,
                               "outcome_unknown signalled success "
                               + "or discarded error");
                    root.unknownChecked = true;
                }
                if (root.lostFinished === 1 && root.queuedFinished === 1
                        && root.invalidFinished === 1 && root.unknownChecked
                        && lost.protocolState === "ready") {
                    root.check(lost.pendingRequestId === ""
                               && lost.reconciledOperation.request_id
                               === root.lostRequestId
                               && lost.reconciledOperation.id_string
                               === root.exactShareId,
                               "query did not reconcile exact "
                               + "request and u64 operation");
                    root.check(queued.pendingRequestId === ""
                               && queued.reconciledOperation.phase
                               === "waiting_for_peer"
                               && queued.reconciledOperation.transferred_bytes
                               === 0, "queued acceptance invented completion");
                    root.check(invalid.pendingRequestId === ""
                               && invalid.reconciledOperation.request_id
                               !== "wrong-request",
                               "invalid acknowledgement request ID trusted");
                    root.check(lost.endpointSnapshot.active_share === null
                               && lost.operations.length === 2
                               && lost.operations[0].id_string === "7"
                               && lost.operations[1].id_string
                               === root.exactShareId,
                               "oldest-first recovered operation "
                               + "history changed");
                    root.check(lost.dismiss(root.exactShareId),
                               "recovered operation dismiss refused");
                    root.step = 8;
                }
            } else if (root.step === 8 && root.lostFinished === 2
                       && lost.protocolState === "ready" &&
                       !lost.snapshotStale) {
                root.check(lost.operations.length === 2
                           && lost.operations[1].dismissed &&
                           !lost.operations[0].dismissed,
                           "dismiss deleted or rewrote historical outcome");
                root.reloaded = reloadFactory.createObject(root);
                root.step = 9;
            } else if (root.step === 9 && root.reloaded.protocolState
                       === "ready") {
                root.check(root.reloaded.endpointSnapshot.active_share === null
                           && root.reloaded.operations.length === 2
                           && root.reloaded.operations[1].id_string
                           === root.exactShareId
                           && root.reloaded.operations[1].dismissed &&
                           !root.reloaded.operations[0].dismissed,
                           "fresh service lost latest dismissal "
                           + "or historical operation");
                root.checkPending(unknown, root.unknownRequestId);
                root.check(root.unknownFinished === 0,
                           "unknown outcome eventually signalled acceptance");
                console.log("HARNESS_OK socket boundaries, "
                            + "reconciliation without resend, "
                            + "retained dismissal");
                Qt.quit();
            }
        }
    }
}
