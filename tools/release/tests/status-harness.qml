import QtQuick
import Quickshell

ShellRoot {
  id: root

  property int checks: 0
  readonly property string exactShareId: "18446744073709551615"
  property string snapshotJson: '{"response":{"type":"snapshot",'
    + '"snapshot":{"active_share":{"id":7,'
    + '"id_string":"18446744073709551615","medium":"wifi_lan",'
    + '"phase":"transferring","remaining_seconds":12},'
    + '"visibility_status":{"discoverable":false,"requested":false,'
    + '"temporary":false,"remaining_secs":null,"available_media":[],'
    + '"error":null}},'
    + '"preferences":{"saved":null,"applied":null,'
    + '"pending":false,"error":null}},"version":6}'

  function statesSettled() {
    return ready.protocolState !== "checking"
      && unavailable.protocolState !== "checking"
      && incompatible.protocolState !== "checking"
      && staleV2.protocolState !== "checking"
      && unsupported.protocolState !== "checking"
      && missing.protocolState !== "checking"
      && silent.protocolState !== "checking"
  }


  function verifyStates() {
    var statesMatch = ready.protocolState === "ready"
      && ready.activeShare.id_string === root.exactShareId
      && ready.activeShare.medium === "wifi_lan"
      && ready.activeShare.phase === "transferring"
      && ready.activeShare.remaining_seconds === 12
      && unavailable.protocolState === "unavailable"
      && incompatible.protocolState === "incompatible"
      && staleV2.protocolState === "incompatible"
      && unsupported.protocolState === "incompatible"
      && missing.protocolState === "missing"
      && silent.protocolState === "incompatible"
    if (statesMatch) {
      console.log("HARNESS_OK native availability and protocol compatibility")
      Qt.quit()
      return
    }
    console.error(
      "HARNESS_FAIL",
      ready.protocolState,
      unavailable.protocolState,
      incompatible.protocolState,
      unsupported.protocolState,
      missing.protocolState,
      silent.protocolState,
    )
  }

  StatusProbe {
    id: ready
    versionCommand: ["printf", "6"]
    runtimeCommand: ["true"]
    statusCommand: ["printf", root.snapshotJson]
  }

  StatusProbe {
    id: unavailable
    versionCommand: ["printf", "6"]
    runtimeCommand: ["false"]
  }

  StatusProbe {
    id: incompatible
    versionCommand: ["printf", "1"]
    runtimeCommand: ["true"]
  }

  StatusProbe {
    id: staleV2
    versionCommand: ["printf", "2"]
    runtimeCommand: ["true"]
  }

  StatusProbe {
    id: unsupported
    versionCommand: ["printf", "5"]
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
