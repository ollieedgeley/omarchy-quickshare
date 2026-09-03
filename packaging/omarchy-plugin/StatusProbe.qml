import QtQuick
import Quickshell.Io

QtObject {
  id: root

  property string protocolState: "checking"
  property int minimumProtocol: -1
  property int maximumProtocol: -1
  property string versionOutput: ""
  property bool releaseReady: false
  property var versionCommand: [
    "env",
    "omarchy-quickshare",
    "--protocol-version",
  ]
  property var runtimeCommand: [
    "env",
    "omarchy-quickshare",
    "--runtime-status",
  ]
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
      return "The binary and local endpoint are ready. Device discovery and "
        + "transfers are not available in this development build."
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

  function finishVersionProbe(exitCode) {
    if (exitCode !== 0) {
      protocolState = "missing"
      return
    }
    acceptProtocol(versionOutput)
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

  property FileView releaseFile: FileView {
    path: root.releasePath
    printErrors: false
    onLoaded: root.acceptRelease(text())
    onLoadFailed: root.protocolState = "incompatible"
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
      root.protocolState = exitCode === 0 ? "ready" : "unavailable"
    }
  }
}
