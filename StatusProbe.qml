import QtQuick
import Quickshell.Io

QtObject {
  id: root

  property string protocolState: "checking"
  property var activeShare: ({})
  property var actionCommand: []
  property string actionError: ""
  property bool actionBusy: false
  property var endpointSnapshot: ({})
  property int minimumProtocol: -1
  property int maximumProtocol: -1
  property string statusOutput: ""
  property string versionOutput: ""
  property bool releaseReady: false
  property bool probeOnStartup: true
  property var executableCommand: ["env", "omarchy-quickshare"]
  property var versionCommand: command(["protocol-version"])
  property var runtimeCommand: command(["health"])
  property var statusCommand: command(["status", "--json"])
  signal actionFinished(bool succeeded)
  readonly property string releasePath:
    Qt.resolvedUrl("release.json").toString()
  readonly property string installRoutes:
    "Use the native Arch package when published, or build from source at "
    + "github.com/ollieedgeley/omarchy-quickshare."
  readonly property string statusTitle: {
    if (protocolState === "ready") return "Quick Share is ready"
    if (protocolState === "incompatible") return "Incompatible binary"
    if (protocolState === "missing") return "Native binary not found"
    if (protocolState === "unavailable") return "Local service unavailable"
    return "Checking native binary"
  }
  readonly property string statusDetail: {
    if (protocolState === "ready") {
      return "The binary and local endpoint are ready."
    }
    if (protocolState === "incompatible") {
      return "The binary does not support this plugin's control protocol. "
        + installRoutes
    }
    if (protocolState === "missing") {
      return "Install omarchy-quickshare. " + installRoutes
    }
    if (protocolState === "unavailable") {
      return "The binary is compatible, but its local service is not running. "
        + installRoutes
    }
    return "Looking for omarchy-quickshare on PATH."
  }

  function accept(shareId) {
    runAction(["share", "accept", String(shareId)])
  }

  function cancel(shareId) {
    runAction(["share", "cancel", String(shareId)])
  }


  function command(arguments) {
    return executableCommand.concat(arguments)
  }
  function dismiss(shareId) {
    runAction(["share", "dismiss", String(shareId)])
  }

  function discover() {
    runAction(["discover", "start"])
  }

  function acceptRelease(source) {
    var release
    try {
      release = JSON.parse(source)
    } catch (error) {
      protocolState = "incompatible"
      return
    }
    var range = release.controlProtocol
    if (!range || !Number.isInteger(range.minimum)
        || !Number.isInteger(range.maximum)
        || range.minimum > range.maximum) {
      protocolState = "incompatible"
      return
    }
    minimumProtocol = range.minimum
    maximumProtocol = range.maximum
    releaseReady = true
    root.versionProbe.running = true
  }

  function acceptProtocol(value) {
    var protocol = Number(value)
    if (!Number.isInteger(protocol) || protocol < minimumProtocol
        || protocol > maximumProtocol) {
      protocolState = "incompatible"
      return
    }
    root.runtimeProbe.running = true
  }

  function acceptSnapshot(source) {
    var envelope
    try {
      envelope = JSON.parse(source)
    } catch (error) {
      protocolState = "incompatible"
      return
    }
    var response = envelope.response
    var snapshot = response && response.snapshot
    if (envelope.version < minimumProtocol
        || envelope.version > maximumProtocol
        || !response || response.type !== "snapshot"
        || !snapshot || typeof snapshot !== "object") {
      protocolState = "incompatible"
      return
    }
    endpointSnapshot = snapshot
    activeShare = snapshot.active_share || ({})
    protocolState = "ready"
  }

  function finishVersionProbe(exitCode) {
    if (exitCode !== 0) {
      protocolState = "missing"
      return
    }
    acceptProtocol(versionOutput)
  }

  function finishRuntimeProbe(exitCode) {
    if (exitCode !== 0) {
      protocolState = "unavailable"
      return
    }
    statusOutput = ""
    root.statusProbe.running = true
  }

  function finishStatusProbe(exitCode) {
    if (exitCode !== 0) {
      protocolState = "unavailable"
      return
    }
    acceptSnapshot(statusOutput)
  }

  function pin(peerId) {
    runAction(["peer", "pin", String(peerId)])
  }

  function unpin() {
    runAction(["peer", "unpin"])
  }

  function refresh() {
    protocolState = "checking"
    versionOutput = ""
    if (releaseReady && !root.versionProbe.running) {
      root.versionProbe.running = true
    } else if (!releaseReady) {
      root.releaseFile.reload()
    }
  }

  function reject(shareId) {
    runAction(["share", "reject", String(shareId)])
  }

  function runAction(arguments) {
    if (root.actionBusy) return false
    root.actionBusy = true
    actionError = ""
    actionCommand = command(arguments)
    root.actionProbe.running = true
    return true
  }

  function sendTo(shareId, peerId) {
    runAction(["share", "select", String(shareId), String(peerId)])
  }

  function submit(value) {
    return runAction(["send", String(value)])
  }

  function submitTo(peerId, value) {
    return runAction([
      "send",
      "--peer",
      String(peerId),
      String(value),
    ])
  }

  function setVisibility(shouldOpen) {
    runAction([
      "visibility",
      shouldOpen ? "open" : "close",
    ])
  }

  function stopDiscovery() {
    runAction(["discover", "stop"])
  }

  property Process actionProbe: Process {
    command: root.actionCommand
    onExited: function(exitCode) {
      var succeeded = exitCode === 0
      root.actionBusy = false
      if (!succeeded) {
        root.actionError = "Quick Share action failed (exit code "
          + exitCode + ")."
      }
      root.actionFinished(succeeded)
      root.refresh()
    }
  }

  property FileView releaseFile: FileView {
    path: root.probeOnStartup ? root.releasePath : ""
    printErrors: false
    onLoaded: root.acceptRelease(text())
    onLoadFailed: if (root.probeOnStartup) root.protocolState = "incompatible"
  }

  property Process versionProbe: Process {
    command: root.versionCommand
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.versionOutput = String(text || "").trim()
    }
    onExited: function(exitCode) {
      root.finishVersionProbe(exitCode)
    }
  }

  property Process runtimeProbe: Process {
    command: root.runtimeCommand
    onExited: function(exitCode) {
      root.finishRuntimeProbe(exitCode)
    }
  }

  property Process statusProbe: Process {
    command: root.statusCommand
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.statusOutput = String(text || "").trim()
    }
    onExited: function(exitCode) {
      root.finishStatusProbe(exitCode)
    }
  }

  property Timer refreshTimer: Timer {
    interval: 1000
    repeat: true
    running: root.protocolState === "ready"
    onTriggered: {
      if (!root.statusProbe.running) root.statusProbe.running = true
    }
  }
}
