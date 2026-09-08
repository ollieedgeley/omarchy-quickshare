import QtQuick
import Quickshell
import Quickshell.Io

QtObject {
    id: root

    property string socketPath: String(Quickshell.env("XDG_RUNTIME_DIR") || "")
                                + "/omarchy-quickshare/control.sock"
    property var observation: ({
                                   state: "checking",
                                   snapshot: {},
                                   preferences: {}
                               })
    readonly property string protocolState: observation.state
    readonly property string observationError: observation.error || ""
    readonly property bool snapshotStale: protocolState !== "ready"
    readonly property var endpointSnapshot: observation.snapshot
    readonly property var activeShare: endpointSnapshot.active_share || ({})
    readonly property var operations: observation.operations || []
    property string pendingRequestId: ""
    property bool reconciliationBusy: false
    property var reconciledOperation: null
    property int submissionSequence: 0
    readonly property var preferenceStatus: observation.preferences
    readonly property var appliedPreferences: preferenceStatus.applied || ({})
    readonly property var savedPreferences: preferenceStatus.saved || ({})
    property string actionError: ""
    property bool actionBusy: false
    property bool snapshotBusy: false
    property int snapshotGeneration: 0
    property int minimumProtocol: -1
    property int maximumProtocol: -1
    property bool releaseReady: false
    signal actionFinished(bool succeeded)

    readonly property string statusTitle: {
        if (protocolState === "ready")
            return "Quick Share is ready";
        if (protocolState === "incompatible")
            return "Incompatible service";
        if (protocolState === "checking")
            return "Connecting to Quick Share";
        return "Local service unavailable";
    }
    readonly property string statusDetail: {
        if (protocolState === "ready")
            return "Connected to the local Quick Share service.";
        if (protocolState === "incompatible")
            return "The service response is incompatible with this plugin. "
                    + "Update the daemon and plugin together.";
        if (protocolState === "checking")
            return "Connecting to the local Quick Share service.";
        return "Disconnected from Quick Share. Displayed state is stale; "
                + "actions are disabled until the service reconnects.";
    }

    function markStale(state, error) {
        observation = {
            state: state,
            snapshot: endpointSnapshot,
            preferences: preferenceStatus,
            operations: operations,
            error: String(error || "")
        };
    }

    function acceptRelease(source) {
        try {
            var range = JSON.parse(source).controlProtocol;
            if (!range || !Number.isInteger(range.minimum) || !Number.isInteger(
                        range.maximum) || range.minimum < 1 || range.minimum
                    > range.maximum)
                throw new Error("protocol range");
            minimumProtocol = range.minimum;
            maximumProtocol = range.maximum;
            releaseReady = true;
            refresh();
        } catch (error) {
            releaseReady = false;
            markStale("incompatible", error);
        }
    }

    function decodeReply(frame) {
        var envelope = JSON.parse(frame);
        if (!envelope || !Number.isInteger(envelope.version)
                || envelope.version < minimumProtocol || envelope.version
                > maximumProtocol || !envelope.response
                || typeof envelope.response.type !== "string") {
            throw new Error("incompatible response");
        }
        return envelope.response;
    }

    function installSnapshot(response) {
        var snapshot = response.snapshot;
        var visibility = snapshot && snapshot.visibility_status;
        var preferences = response.preferences;
        if (response.type !== "snapshot" || !snapshot || Array.isArray(
                    snapshot) || !("active_share" in snapshot))
            throw new Error("invalid snapshot");
        if (!Array.isArray(response.operations) || !response.operations.every(
                    validOperation))
            throw new Error("invalid snapshot");
        if ((snapshot.peers !== undefined && !Array.isArray(snapshot.peers)) ||
                !visibility)
            throw new Error("invalid snapshot");
        var remaining = visibility.remaining_secs;
        if (typeof visibility.discoverable !== "boolean"
                || typeof visibility.requested !== "boolean"
                || typeof visibility.temporary !== "boolean" || !Array.isArray(
                    visibility.available_media) || (remaining !== null && (
                                                        !Number.isInteger(
                                                            remaining)
                                                        || remaining < 0)) || (
                    visibility.error !== null && typeof visibility.error
                    !== "string"))
            throw new Error("invalid snapshot");
        var states = ["closed", "starting", "open", "unavailable"];
        if (states.indexOf(snapshot.visibility || "closed") < 0 || !preferences
                || typeof preferences.pending !== "boolean" || !(
                    "saved" in preferences) || !("applied" in preferences) || (
                    snapshot.active_share && !validShareId(
                        snapshot.active_share.id_string))) {
            throw new Error("invalid snapshot");
        }
        observation = {
            state: "ready",
            snapshot: snapshot,
            preferences: preferences,
            operations: response.operations
        };
    }

    function exchange(frame, callback) {
        var connection = connectionFactory.createObject(root, {
                                                            path: socketPath
                                                        });
        connection.completed.connect(callback);
        // Admission is synchronous; completion signals are asynchronous.
        Qt.callLater(() => connection.begin(frame));
    }

    function validOperation(operation) {
        if (!operation || !validShareId(operation.id_string))
            return false;
        var requestId = operation.request_id;
        if (requestId !== null) {
            if (typeof requestId !== "string" || requestId.length === 0)
                return false;
            var bytes = encodeURIComponent(requestId).replace(/%[0-9A-F]{2}|./g,
                                                              "x").length;
            if (bytes > 128)
                return false;
        }
        return ["inbound", "outbound"].indexOf(operation.direction) >= 0
                && ["waiting_for_peer", "awaiting_peer_consent",
                    "awaiting_local_consent", "transferring", "completed",
                    "cancelled", "rejected", "failed"].indexOf(operation.phase)
                >= 0 && Number.isInteger(operation.total_bytes)
                && operation.total_bytes >= 0 && Number.isInteger(
                    operation.transferred_bytes) && operation.transferred_bytes
                >= 0 && typeof operation.dismissed === "boolean" && (
                    operation.terminal_reason === null
                    || typeof operation.terminal_reason === "string") && (
                    operation.recovery_guidance === null
                    || typeof operation.recovery_guidance === "string");
    }

    function newRequestId() {
        submissionSequence += 1;
        var sequence = submissionSequence.toString(36);
        var id = "panel-" + Date.now().toString(36) + "-" + sequence;
        for (var i = 0; i < 4; i++) {
            id += "-" + Math.floor(Math.random() * 4294967296).toString(16);
        }
        return id;
    }

    function acceptOperation(operation, requestId) {
        if (!validOperation(operation) || operation.request_id !== requestId) {
            throw new Error("invalid operation acknowledgement");
        }
        reconciledOperation = operation;
        pendingRequestId = "";
        actionError = "";
    }

    function reconcile() {
        if (!pendingRequestId || reconciliationBusy || snapshotStale
                || actionBusy)
            return;
        var requestId = pendingRequestId;
        reconciliationBusy = true;
        var request = encodeRequest({
                                        type: "operation",
                                        request_id: requestId
                                    });
        exchange(request, function (frame, failure) {
            root.reconciliationBusy = false;
            if (root.pendingRequestId !== requestId)
                return;
            try {
                if (failure)
                    throw new Error(failure);
                var response = root.decodeReply(frame);
                if (response.type !== "operation")
                    throw new Error(String(response.code || response.type));
                root.acceptOperation(response.operation, requestId);
                root.actionFinished(true);
            } catch (error) {
                root.actionError = "Submission outcome is unknown. "
                        + "Quick Share is checking the retained request ID; "
                        + "do not resend. " + String(error);
            }
        });
    }

    function refresh() {
        if (!releaseReady || snapshotBusy || actionBusy)
            return false;
        snapshotBusy = true;
        var generation = snapshotGeneration;
        var request = encodeRequest({
                                        type: "snapshot"
                                    });
        exchange(request, function (frame, failure) {
            root.snapshotBusy = false;
            if (generation !== root.snapshotGeneration) {
                root.refresh();
                return;
            }
            if (failure)
                root.markStale("unavailable", failure);
            else {
                try {
                    root.installSnapshot(root.decodeReply(frame));
                    root.reconcile();
                } catch (error) {
                    root.markStale("incompatible", error);
                }
            }
        });
        return true;
    }

    function validShareId(value) {
        return typeof value === "string" && /^(0|[1-9][0-9]*)$/.test(value) && (
                    value.length < 20 || (value.length === 20 && value
                                          <= "18446744073709551615"));
    }

    function encodeRequest(request, requestId) {
        var encoded = JSON.stringify(request);
        if (Object.prototype.hasOwnProperty.call(request, "share_id")) {
            if (!validShareId(request.share_id))
                throw new Error("Invalid share identifier");
            // Unquote this property only after decimal/u64 validation.
            encoded = encoded.replace(/"share_id":"[0-9]+"/, '"share_id":'
                                      + request.share_id);
        }
        var identity = "";
        if (requestId)
            identity = ',"request_id":' + JSON.stringify(requestId);
        return '{"version":' + maximumProtocol + identity + ',"request":'
                + encoded + '}';
    }

    function expectedReply(type) {
        if (type.indexOf("submit_") === 0)
            return "queued";
        if (type === "patch_preferences" || type === "pin_peer" || type
                === "unpin_peer")
            return "preferences";
        return type === "cancel" ? "cancelled" : "applied";
    }

    function applyActionReply(request, requestId, reply) {
        var response = decodeReply(reply);
        var succeeded = response.type === expectedReply(request.type);
        if (succeeded && requestId)
            acceptOperation(response.operation, requestId);
        var refusalCodes = ["busy", "invalid_request", "unsupported_version"];
        if (!succeeded && requestId && response.type === "error"
                && refusalCodes.indexOf(response.code) >= 0) {
            pendingRequestId = "";
        }
        if (response.type === "preferences") {
            if (!response.preferences)
                throw new Error("invalid preferences");
            if (response.preferences.error) {
                succeeded = false;
                actionError = String(response.preferences.error);
            }
        }
        if (!succeeded && !actionError) {
            actionError = response.type === "not_found"
                    ? "This share is no longer available." :
                      "The service did not accept the action: " + String(
                          response.code || response.type);
            if (response.type === "error")
                markStale("unavailable");
        }
        return succeeded;
    }

    function runAction(request, requestId) {
        if (snapshotStale || actionBusy)
            return false;
        var frame;
        try {
            frame = encodeRequest(request, requestId);
        } catch (error) {
            actionError = String(error);
            return false;
        }
        if (requestId)
            pendingRequestId = requestId;
        actionBusy = true;
        actionError = "";
        snapshotGeneration += 1;
        exchange(frame, function (reply, failure) {
            var succeeded = false;
            if (failure) {
                root.markStale("unavailable");
                root.actionError = failure === "message_too_large"
                        ? "The request or reply exceeds "
                          + "the control message limit." :
                          "The service reply was lost. "
                          + "The action may have been accepted; "
                          + "check refreshed state before trying again.";
            } else {
                try {
                    succeeded = root.applyActionReply(request, requestId,
                                                      reply);
                } catch (error) {
                    root.markStale("incompatible");
                    root.actionError = "The service reply was invalid. "
                            + "Check refreshed state before trying again.";
                }
            }
            if (requestId && root.pendingRequestId) {
                root.actionError = "Submission outcome is unknown. "
                        + "Quick Share will query its retained request ID; "
                        + "do not resend.";
            }
            // Admission only; the next snapshot owns outcomes.
            if (!root.snapshotStale)
                root.markStale("checking");
            root.actionBusy = false;
            if (!requestId || !root.pendingRequestId)
                root.actionFinished(succeeded);
            root.refresh();
        });
        return true;
    }

    function accept(shareId) {
        return runAction({
                             type: "accept",
                             share_id: shareId
                         });
    }
    function cancel(shareId) {
        return runAction({
                             type: "cancel",
                             share_id: shareId
                         });
    }
    function reject(shareId) {
        return runAction({
                             type: "reject",
                             share_id: shareId
                         });
    }
    function dismiss(shareId) {
        return runAction({
                             type: "dismiss",
                             share_id: shareId
                         });
    }
    function sendTo(shareId, peerId) {
        return runAction({
                             type: "select_peer",
                             share_id: shareId,
                             peer_id: String(peerId)
                         });
    }
    function discover() {
        return runAction({
                             type: "discover"
                         });
    }
    function stopDiscovery() {
        return runAction({
                             type: "stop_discovery"
                         });
    }
    function pin(peerId) {
        return runAction({
                             type: "pin_peer",
                             peer_id: String(peerId)
                         });
    }
    function unpin() {
        return runAction({
                             type: "unpin_peer"
                         });
    }
    function setVisibility(shouldOpen) {
        return runAction({
                             type: shouldOpen ? "open_visibility" :
                                                "close_visibility"
                         });
    }
    function patchPreferences(key, value) {
        return runAction({
                             type: "patch_preferences",
                             key: String(key),
                             value: String(value)
                         });
    }

    function fileUriPath(uri) {
        var rest = uri.slice(5);
        if (rest.indexOf("//") === 0) {
            rest = rest.slice(2);
            var slash = rest.indexOf("/");
            if (slash < 0)
                throw new Error("File URI is malformed");
            var host = decodeURIComponent(rest.slice(0, slash));
            if (host && host.toLowerCase() !== "localhost")
                throw new Error("File URI host is not local");
            rest = rest.slice(slash);
        }
        if (rest.charAt(0) !== "/" || /[?#]/.test(rest))
            throw new Error("File URI is malformed");
        var path = decodeURIComponent(rest);
        if (path.indexOf("\u0000") >= 0)
            throw new Error("File URI contains a NUL");
        return path;
    }

    function submitTo(peerId, value) {
        if (snapshotStale || actionBusy)
            return false;
        if (pendingRequestId) {
            actionError = "The previous submission outcome is still unknown. "
                    + "Do not resend; Quick Share is checking its request ID.";
            return false;
        }
        var content = String(value);
        var lines = content.split(/[\r\n]/).map(line => line.trim());
        lines = lines.filter(line => line.length > 0 && line.charAt(0) !== "#");
        var request = {
            type: "submit_text",
            peer_id: String(peerId),
            text: content
        };
        try {
            if (lines.some(line => line.indexOf("file:") === 0)) {
                if (lines.length !== 1)
                    throw new Error("File URI list has multiple entries");
                request = {
                    type: "submit_file",
                    peer_id: String(peerId),
                    path: fileUriPath(lines[0])
                };
            } else if (content.indexOf("http://") === 0 || content.indexOf(
                           "https://") === 0) {
                request = {
                    type: "submit_url",
                    peer_id: String(peerId),
                    url: content
                };
            }
        } catch (error) {
            actionError = String(error);
            return false;
        }
        return runAction(request, newRequestId());
    }

    property Component connectionFactory: Component {
        ControlConnection {}
    }
    property FileView releaseFile: FileView {
        path: Qt.resolvedUrl("release.json")
        printErrors: false
        onLoaded: root.acceptRelease(text())
        onLoadFailed: root.markStale("incompatible")
    }
    property Timer refreshTimer: Timer {
        interval: 1000
        repeat: true
        running: root.releaseReady
        onTriggered: root.refresh()
    }
}
