import QtQuick
import Quickshell

ShellRoot {
  id: root

  property int checks: 0
  property string snapshotJson: '{"response":{"type":"snapshot",'
    + '"snapshot":{"active_share":{"id":7,"phase":"transferring"}}},'
    + '"version":1}'

  function statesSettled() {
    return ready.protocolState !== "checking"
      && unavailable.protocolState !== "checking"
      && incompatible.protocolState !== "checking"
      && missing.protocolState !== "checking"
      && silent.protocolState !== "checking"
  }

  function verifyStates() {
    var matches = ready.protocolState === "ready"
      && ready.activeShare.id === 7
      && ready.activeShare.phase === "transferring"
      && unavailable.protocolState === "unavailable"
      && incompatible.protocolState === "incompatible"
      && missing.protocolState === "missing"
      && silent.protocolState === "incompatible"
    if (matches) {
      console.log("HARNESS_OK native availability states observed")
      Qt.quit()
      return
    }
    console.error(
      "HARNESS_FAIL",
      ready.protocolState,
      unavailable.protocolState,
      incompatible.protocolState,
      missing.protocolState,
      silent.protocolState,
    )
    Qt.exit(1)
  }

  StatusProbe {
    id: ready
    versionCommand: ["printf", "1"]
    runtimeCommand: ["true"]
    statusCommand: ["printf", root.snapshotJson]
  }

  StatusProbe {
    id: unavailable
    versionCommand: ["printf", "1"]
    runtimeCommand: ["false"]
  }

  StatusProbe {
    id: incompatible
    versionCommand: ["printf", "2"]
    runtimeCommand: ["true"]
  }

  StatusProbe {
    id: missing
    versionCommand: ["env", "quickshare-missing-binary"]
    runtimeCommand: ["true"]
  }

  StatusProbe {
    id: silent
    versionCommand: ["true"]
    runtimeCommand: ["true"]
  }

  Timer {
    interval: 25
    repeat: true
    running: true
    onTriggered: {
      root.checks += 1
      if (root.statesSettled()) {
        running = false
        root.verifyStates()
      } else if (root.checks >= 200) {
        console.error("HARNESS_TIMEOUT")
        Qt.exit(2)
      }
    }
  }
}
