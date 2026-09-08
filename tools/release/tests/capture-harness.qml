import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root
    property int checks: 0
    property int step: 0
    property var panel: null
    property int waitingChecks: 0
    property bool failureObserved: false
    property int failureSnapshots: 0
    readonly property var journey: JSON.parse(Quickshell.env("CAPTURE_JOURNEY"))

    function findPanel(item) {
        if (item.choosePeer !== undefined)
            return item;
        var children = item.children || [];
        for (var i = 0; i < children.length; i++) {
            var found = findPanel(children[i]);
            if (found)
                return found;
        }
        return null;
    }

    function paste() {
        if (widget.paste(journey.value) !== "ok" || widget.clipboardPreview
                !== journey.value || !widget.showPasteBadge) {
            throw new Error("Paste did not display captured A");
        }
    }

    function invalidate() {
        if (journey.invalidate === "close")
            widget.close();
        else if (journey.invalidate === "clear")
            widget.clearPasteBadge();
        else
            root.panel.cancel();
    }

    FileView {
        id: clipboardStarted
        path: journey.replacement || journey.invalidate || journey.peerChange
              || journey.explicitPending || journey.timeoutReplacement
              || journey.providedEmpty ? Quickshell.env("CLIPBOARD_STARTED") :
                                         ""
        printErrors: false
        onLoaded: {
            if (root.step !== 2 || text() !== "started"
                    || root.panel.actionBusy) {
                return;
            }
            if (journey.queuedReplacement)
                widget.readClipboard("preview", "");
            if (journey.providedEmpty) {
                if (widget.paste("") !== "empty") {
                    throw new Error(
                                "Provided empty value did not report failure");
                }
            } else if (journey.invalidate)
                root.invalidate();
            else if (journey.peerChange)
                root.panel.choosePeer(journey.peer);
            else if (!journey.explicitPending)
                widget.readClipboard("preview", "");
            if (!journey.timeoutReplacement)
                releaseClipboard.running = true;
            root.step = 3;
        }
    }
    Process {
        id: releaseClipboard
        command: ["touch", Quickshell.env("CLIPBOARD_RELEASE")]
    }
    BarWidget {
        id: widget
    }

    FileView {
        id: submissionStarted
        path: journey.submissionMode && !journey.clientReconnect
              ? Quickshell.env("SUBMISSION_STARTED") : ""
        printErrors: false
        onLoaded: {
            if (root.step !== 2 || text() !== "started")
                return;
            if (journey.submissionMode === "drop") {
                if (!widget.pendingRequestId || !root.panel.actionBusy) {
                    throw new Error("Accepted submission lost "
                                    + "its pending identity");
                }
                widget.clearPasteBadge();
                releaseSubmission.running = true;
                root.step = 11;
                return;
            }
            if (journey.failSubmission) {
                removeCapture.running = true;
                root.step = 6;
                return;
            }
            var next = journey.newer === "same" ? journey.value :
                                                  "next independent B";
            widget.paste(next);
            releaseSubmission.running = true;
            root.step = 3;
        }
    }
    Process {
        id: releaseSubmission
        command: ["touch", Quickshell.env("SUBMISSION_RELEASE")]
    }

    Process {
        id: removeCapture
        command: ["rm", Quickshell.env("CAPTURE_FILE")]
        onExited: function (code) {
            if (code !== 0)
                throw new Error("Fixture file removal failed");
            releaseSubmission.running = true;
            root.step = 7;
        }
    }
    Connections {
        target: root.panel
        function onSnapshotChanged() {
            if (root.failureObserved)
                root.failureSnapshots += 1;
        }
    }
    FileView {
        id: submissionAttempts
        path: journey.failSubmission ? Quickshell.env("SUBMISSION_ATTEMPTS") :
                                       ""
        printErrors: false
        onLoaded: {
            if (root.step !== 8)
                return;
            if (text() !== "submit\n")
                throw new Error("Failed admission retried itself");
            restoreCapture.running = true;
            root.step = 9;
        }
    }
    Process {
        id: restoreCapture
        command: ["cp", Quickshell.env("CAPTURE_RESTORE_FILE"), Quickshell.env(
                "CAPTURE_FILE"),]
        onExited: function (code) {
            if (code !== 0)
                throw new Error("Fixture file restore failed");
            root.panel.choosePeer(journey.peer);
            root.step = 10;
        }
    }

    Timer {
        interval: 25
        repeat: true
        running: true
        onTriggered: {
            root.checks += 1;
            if (root.checks > 240) {
                console.error("CAPTURE_TIMEOUT", root.step);
                Qt.exit(2);
                return;
            }
            if (!widget.protocolReady)
                return;
            if (journey.clientOutcome) {
                if (!root.panel) {
                    widget.open();
                    root.panel = root.findPanel(widget);
                }
                if (root.panel.actionBusy || widget.pendingRequestId
                        || widget.reconciliationBusy)
                    return;
                if (widget.capturedContent !== null || widget.showPasteBadge
                        || widget.clipboardPreview.length > 0
                        || widget.selectedPeerId.length > 0) {
                    throw new Error("Client reconnect restored "
                                    + "cleared preparation");
                }
                var clientOperations = root.panel.operations;
                var clientLatest = clientOperations[clientOperations.length
                                                    - 1];
                if (!clientLatest)
                    return;
                var expectedTerminalPhase = journey.terminalPhase
                        || 'completed';
                if (clientLatest.id_string !== journey.operationId
                        || clientLatest.request_id !== journey.requestId
                        || clientLatest.phase !== expectedTerminalPhase) {
                    throw new Error("Client reconnect did not retain "
                                    + "the terminal operation");
                }
                if (journey.clientOutcome === "dismissed" || root.step === 14) {
                    if (!clientLatest.dismissed) {
                        if (root.step === 14)
                            return;
                        throw new Error("Client reconnect lost "
                                        + "the durable dismissal");
                    }
                    if (root.panel.hasActiveShare
                            || root.panel.recoveredOperation
                            || root.panel.viewState === "terminal") {
                        throw new Error("Client reconnect resurfaced "
                                        + "the dismissed outcome");
                    }
                    console.log("HARNESS_OK terminal client outcome "
                                + "remains dismissed");
                    Qt.quit();
                    return;
                }
                if (clientLatest.dismissed || !root.panel.hasActiveShare
                        || root.panel.activeShareId !== journey.operationId
                        || root.panel.activeShare.phase
                        !== expectedTerminalPhase || root.panel.outcomeId
                        !== journey.operationId || root.panel.phase
                        !== expectedTerminalPhase || root.panel.viewState
                        !== "terminal") {
                    throw new Error("Client reconnect did not render "
                                    + "the same terminal outcome");
                }
                root.panel.dismiss();
                root.step = 14;
                return;
            }
            if (journey.reload) {
                if (!root.panel) {
                    widget.open();
                    root.panel = root.findPanel(widget);
                }
                if (root.panel.actionBusy)
                    return;
                if (widget.showPasteBadge || widget.clipboardPreview.length > 0
                        || widget.selectedPeerId.length > 0
                        || root.panel.hasActiveShare) {
                    throw new Error("Reload fabricated captured content, "
                                    + "recipient, or active share");
                }
                var operations = root.panel.operations;
                var latest = operations[operations.length - 1];
                if (!latest || latest.id_string !== journey.operationId) {
                    throw new Error("Reload did not retain "
                                    + "newest durable operation");
                }
                if (journey.reload === "dismissed" || root.step === 13) {
                    if (!latest.dismissed)
                        return;
                    if (root.panel.recoveredOperation || root.panel.viewState
                            === "terminal") {
                        throw new Error("Dismissed durable outcome resurfaced");
                    }
                    console.log("HARNESS_OK dismissed outcome remains hidden");
                    Qt.quit();
                    return;
                }
                if (!root.panel.recoveredOperation
                        || root.panel.outcome.id_string !== latest.id_string
                        || root.panel.phase !== "failed"
                        || root.panel.outcome.terminal_reason !== "interrupted"
                        || !root.panel.outcome.recovery_guidance
                        || root.panel.terminalSummary.indexOf(
                            root.panel.outcome.recovery_guidance) < 0
                        || root.panel.terminalSummary.toLowerCase().indexOf(
                            "cannot resume") < 0 || root.panel.viewState
                        !== "terminal" || root.panel.outcome.attachment
                        !== undefined || root.panel.activeShare.attachment
                        !== undefined) {
                    throw new Error("Reload did not render "
                                    + "durable terminal outcome "
                                    + "without payload");
                }
                root.panel.dismiss();
                root.step = 13;
                return;
            }
            if (root.step === 11) {
                if (widget.showPasteBadge || widget.clipboardPreview.length > 0
                        || widget.selectedPeerId.length > 0) {
                    throw new Error("Lost-reply reconciliation restored "
                                    + "cleared preparation");
                }
                if (root.panel.actionBusy || widget.pendingRequestId
                        || widget.reconciliationBusy)
                    return;
                if (!root.panel.hasActiveShare)
                    return;
                console.log("HARNESS_OK accepted reply reconciled "
                            + "without restoring capture");
                Qt.quit();
                return;
            }
            if (root.step === 0) {
                if (journey.closedPaste) {
                    root.panel = root.findPanel(widget);
                    widget.paste(journey.value);
                    root.step = 5;
                    return;
                }
                widget.open();
                root.panel = root.findPanel(widget);
                if (journey.captureFirst)
                    root.paste();
                if (journey.explicitPending)
                    widget.readClipboard("preview", "");
                root.step = 1;
                if (journey.invalidate && !journey.automatic) {
                    root.paste();
                    widget.readClipboard("preview", "");
                    root.step = 2;
                }
            } else if (root.step === 1 && !root.panel.actionBusy) {
                root.panel.choosePeer("pixel-8");
                root.step = 2;
            } else if (root.step === 2 && journey.submissionMode &&
                       !journey.clientReconnect) {
                submissionStarted.reload();
            } else if (root.step === 2 && (journey.replacement
                                           || journey.invalidate
                                           || journey.peerChange
                                           || journey.explicitPending
                                           || journey.timeoutReplacement
                                           || journey.providedEmpty)) {
                clipboardStarted.reload();
            } else if (root.step === 5 && !root.panel.actionBusy) {
                if (root.panel.activeShareId.length > 0 || !widget.opened ||
                        !widget.showPasteBadge || widget.selectedPeerId.length
                        > 0 || widget.clipboardPreview !== journey.value) {
                    console.error("CLOSED_PASTE_HIDDEN_SUBMISSION");
                    Qt.exit(3);
                    return;
                }
                console.log("HARNESS_OK closed Paste captures "
                            + "without selection");
                Qt.quit();
            } else if (root.step === 2 && journey.contentCase === "flag" &&
                       !root.panel.actionBusy
                       && root.panel.activeShareId.length === 0) {
                console.error("LITERAL_NOT_ADMITTED");
                Qt.exit(3);
            } else if (root.step === 3 && journey.providedEmpty &&
                       !widget.clipboardBusy && root.panel.actionError.length
                       > 0) {
                if (root.panel.activeShareId.length > 0
                        || widget.showPasteBadge) {
                    throw new Error("Provided empty value restored "
                                    + "superseded capture");
                }
                if (!root.failureObserved) {
                    root.failureObserved = true;
                    root.failureSnapshots = 0;
                }
                if (root.failureSnapshots < 2)
                    return;
                root.paste();
                root.step = 4;
            } else if (root.step === 3 && journey.timeoutReplacement
                       && root.panel.actionError.length > 0) {
                throw new Error(
                            "Expired old read overrode newer explicit capture");
            } else if (root.step === 2 && journey.readTimeout &&
                       !widget.clipboardBusy && root.panel.actionError.length
                       > 0) {
                if (root.panel.activeShareId.length > 0
                        || widget.showPasteBadge) {
                    throw new Error(
                                "Timed-out clipboard capture admitted content");
                }
                root.paste();
                root.step = 4;
            } else if (root.step === 2 && journey.invalidRead &&
                       !widget.clipboardBusy && root.panel.actionError.length
                       > 0) {
                if (root.panel.activeShareId.length > 0
                        || widget.showPasteBadge) {
                    throw new Error(
                                "Invalid clipboard capture admitted content");
                }
                if (!root.failureObserved) {
                    root.failureObserved = true;
                    root.failureSnapshots = 0;
                }
                if (root.failureSnapshots < 2)
                    return;
                root.paste();
                root.step = 4;
            } else if (root.step === 2 && !journey.captureFirst &&
                       !journey.automatic) {
                if (root.panel.activeShareId.length > 0
                        || widget.clipboardBusy) {
                    console.error("OFF_SELECTION_READ_OR_SENT");
                    Qt.exit(3);
                    return;
                }
                root.waitingChecks += 1;
                if (root.waitingChecks < 8)
                    return;
                root.paste();
                root.step = 3;
            } else if (root.step === 3 && journey.invalidate &&
                       !widget.clipboardBusy) {
                if (!widget.opened)
                    widget.open();
                if (widget.showPasteBadge || widget.clipboardPreview.length > 0
                        || root.panel.activeShareId.length > 0) {
                    console.error("CLOSED_CAPTURE_RESTORED");
                    Qt.exit(3);
                    return;
                }
                if (journey.recover) {
                    if (root.panel.actionBusy)
                        return;
                    root.paste();
                    root.panel.choosePeer("galaxy-tab");
                    root.step = 4;
                    return;
                }
                console.log("HARNESS_OK closed capture cannot replay");
                Qt.quit();
            } else if (root.step === 3 && journey.failReplacement &&
                       !widget.clipboardBusy && root.panel.actionError.length
                       > 0) {
                if (root.panel.activeShareId.length > 0
                        || widget.showPasteBadge) {
                    throw new Error("Failed replacement restored "
                                    + "superseded content");
                }
                console.log("HARNESS_OK failed replacement waits "
                            + "without fallback");
                Qt.quit();
            } else if (root.step === 7 && !root.panel.actionBusy
                       && root.panel.actionError.length > 0) {
                if (!widget.showPasteBadge || widget.clipboardPreview
                        !== journey.value || root.panel.activeShareId.length
                        > 0) {
                    throw new Error("Failed admission did not retain "
                                    + "independent capture");
                }
                if (!root.failureObserved) {
                    root.failureObserved = true;
                    root.failureSnapshots = 0;
                }
                if (root.failureSnapshots < 2)
                    return;
                root.step = 8;
                submissionAttempts.reload();
            } else if (root.step >= 2 && root.panel.activeShareId.length > 0) {
                var share = root.panel.activeShare;
                var expected = journey.attachment;
                var actual = share.attachment;
                if (actual.type !== expected.type || actual.value
                        !== expected.value || actual.name !== expected.name
                        || actual.size_bytes !== expected.size_bytes
                        || share.peer.id !== journey.peer) {
                    console.error("CAPTURE_MISMATCH", JSON.stringify(share));
                    Qt.exit(3);
                    return;
                }
                if (journey.newer) {
                    if (root.panel.actionBusy)
                        return;
                    var next = journey.newer === "same" ? journey.value :
                                                          "next independent B";
                    if (!widget.showPasteBadge || widget.clipboardPreview
                            !== next) {
                        console.error("INDEPENDENT_CAPTURE_CONSUMED");
                        Qt.exit(3);
                        return;
                    }
                }
                if (journey.consume) {
                    if (root.panel.actionBusy)
                        return;
                    if (widget.showPasteBadge || widget.clipboardPreview.length
                            > 0) {
                        console.error("ADMITTED_CAPTURE_RETAINED");
                        Qt.exit(3);
                        return;
                    }
                }
                if (journey.clientReconnect) {
                    if (root.panel.actionBusy || widget.pendingRequestId
                            || widget.reconciliationBusy || share.phase
                            !== "transferring")
                        return;
                    var transferId = root.panel.activeShareId;
                    widget.clearPasteBadge();
                    if (widget.capturedContent !== null
                            || widget.showPasteBadge
                            || widget.clipboardPreview.length > 0
                            || widget.selectedPeerId.length > 0
                            || root.panel.activeShareId !== transferId
                            || root.panel.activeShare.phase
                            !== "transferring") {
                        throw new Error("Clearing preparation changed "
                                        + "the active transfer");
                    }
                    console.log("HARNESS_OK client exits "
                                + "during active transfer", transferId);
                    Qt.quit();
                    return;
                }
                console.log("HARNESS_OK captured A admitted to", journey.peer);
                Qt.quit();
            }
        }
    }
}
