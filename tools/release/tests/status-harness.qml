import QtQuick
import Quickshell

ShellRoot {
  id: root

  property int checks: 0
  property string firstPasteResult: ""
  property bool firstPasteRunning: false
  property bool secondPasteRunning: false
  property string secondPasteResult: ""
  property bool startedBusyPasteCheck: false
  readonly property string exactShareId: "18446744073709551615"
  property string snapshotJson: '{"response":{"type":"snapshot",'
    + '"snapshot":{"active_share":{"id":7,'
    + '"id_string":"18446744073709551615","medium":"wifi_lan",'
    + '"phase":"transferring","remaining_seconds":12}}},'
    + '"version":2}'

  function statesSettled() {
    return ready.protocolState !== "checking"
      && unavailable.protocolState !== "checking"
      && incompatible.protocolState !== "checking"
      && unsupported.protocolState !== "checking"
      && missing.protocolState !== "checking"
      && silent.protocolState !== "checking"
      && actionFailure.actionError !== ""
      && root.startedBusyPasteCheck
      && root.firstPasteResult === "ok"
      && root.secondPasteResult === "busy"
  }

  function actionMatches(probe, arguments) {
    return JSON.stringify(probe.actionCommand)
      === JSON.stringify(["true"].concat(arguments))
  }

  function verifyStates() {
    var statesMatch = ready.protocolState === "ready"
      && ready.activeShare.id_string === root.exactShareId
      && ready.activeShare.medium === "wifi_lan"
      && ready.activeShare.phase === "transferring"
      && ready.activeShare.remaining_seconds === 12
      && unavailable.protocolState === "unavailable"
      && incompatible.protocolState === "incompatible"
      && unsupported.protocolState === "incompatible"
      && missing.protocolState === "missing"
      && silent.protocolState === "incompatible"
      && actionFailure.actionError
        === "Quick Share action failed (exit code 7)."
      && !actionFailure.actionError.includes("clipboard-secret")
    var commandsMatch = JSON.stringify(commands.command(["plain <b>text</b>"]))
        === '["env","omarchy-quickshare","plain <b>text</b>"]'
      && JSON.stringify(commands.command([
        "share", "accept", root.exactShareId,
      ]))
        === '["env","omarchy-quickshare","share","accept","'
          + root.exactShareId + '"]'
      && JSON.stringify(commands.command([
        "share", "reject", root.exactShareId,
      ]))
        === '["env","omarchy-quickshare","share","reject","'
          + root.exactShareId + '"]'
      && JSON.stringify(commands.command([
        "share", "cancel", root.exactShareId,
      ]))
        === '["env","omarchy-quickshare","share","cancel","'
          + root.exactShareId + '"]'
      && JSON.stringify(commands.command([
        "share", "dismiss", root.exactShareId,
      ]))
        === '["env","omarchy-quickshare","share","dismiss","'
          + root.exactShareId + '"]'
      && JSON.stringify(commands.command(["discover", "start"]))
        === '["env","omarchy-quickshare","discover","start"]'
      && JSON.stringify(commands.command(["peer", "pin", "pixel-8"]))
        === '["env","omarchy-quickshare","peer","pin","pixel-8"]'
      && JSON.stringify(commands.command(["peer", "unpin"]))
        === '["env","omarchy-quickshare","peer","unpin"]'
      && JSON.stringify(commands.command([
        "share", "select", root.exactShareId, "pixel-8",
      ])) === '["env","omarchy-quickshare","share","select","'
        + root.exactShareId + '","pixel-8"]'
      && JSON.stringify(commands.command(["visibility", "open"]))
        === '["env","omarchy-quickshare","visibility","open"]'
      && JSON.stringify(commands.command(["visibility", "close"]))
        === '["env","omarchy-quickshare","visibility","close"]'
      && JSON.stringify(commands.command(["discover", "stop"]))
        === '["env","omarchy-quickshare","discover","stop"]'
    var methodsMatch = actionMatches(
      acceptAction, ["share", "accept", root.exactShareId],
    ) && actionMatches(
      rejectAction, ["share", "reject", root.exactShareId],
    ) && actionMatches(
      cancelAction, ["share", "cancel", root.exactShareId],
    ) && actionMatches(
      dismissAction, ["share", "dismiss", root.exactShareId],
    ) && actionMatches(discoverAction, ["discover", "start"])
      && actionMatches(pinAction, ["peer", "pin", "pixel-8"])
      && actionMatches(unpinAction, ["peer", "unpin"])
      && actionMatches(
        sendToAction, ["share", "select", root.exactShareId, "pixel-8"],
      )
      && actionMatches(openAction, ["visibility", "open"])
      && actionMatches(closeAction, ["visibility", "close"])
      && actionMatches(stopAction, ["discover", "stop"])
    var pasteMatch = actionMatches(
      filePaste, ["send", "file:///tmp/Quick%20Share.apk"],
    ) && actionMatches(
      folderPaste, ["send", "/tmp/Quick Share Folder"],
    ) && actionMatches(
      textPaste, ["send", "plain <b>text</b> with spaces"],
    ) && actionMatches(
      urlPaste, ["send", "https://example.test/a?x=1&y=<b>2</b>"],
    )
    var integrationMatch = root.firstPasteResult === "ok"
      && root.secondPasteResult === "busy"
      && root.firstPasteRunning
      && root.secondPasteRunning
      && !busyProbe.submit("third-paste")
    if (statesMatch && commandsMatch && methodsMatch && pasteMatch
        && integrationMatch) {
      console.log("HARNESS_OK native availability states and commands observed")
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
      commandsMatch,
      methodsMatch,
      pasteMatch,
      integrationMatch,
      actionFailure.actionError,
    )
  }

  StatusProbe {
    id: ready
    versionCommand: ["printf", "2"]
    runtimeCommand: ["true"]
    statusCommand: ["printf", root.snapshotJson]
  }

  StatusProbe {
    id: unavailable
    versionCommand: ["printf", "2"]
    runtimeCommand: ["false"]
  }

  StatusProbe {
    id: incompatible
    versionCommand: ["printf", "1"]
    runtimeCommand: ["true"]
  }

  StatusProbe {
    id: unsupported
    versionCommand: ["printf", "3"]
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

  StatusProbe {
    id: commands
    probeOnStartup: false
  }

  component ActionProbe: StatusProbe {
    executableCommand: ["true"]
    probeOnStartup: false
  }

  ActionProbe {
    id: acceptAction
    Component.onCompleted: accept(root.exactShareId)
  }
  ActionProbe {
    id: cancelAction
    Component.onCompleted: cancel(root.exactShareId)
  }
  ActionProbe {
    id: closeAction
    Component.onCompleted: setVisibility(false)
  }
  ActionProbe {
    id: discoverAction
    Component.onCompleted: discover()
  }
  ActionProbe {
    id: dismissAction
    Component.onCompleted: dismiss(root.exactShareId)
  }
  ActionProbe {
    id: openAction
    Component.onCompleted: setVisibility(true)
  }
  ActionProbe {
    id: pinAction
    Component.onCompleted: pin("pixel-8")
  }
  ActionProbe {
    id: rejectAction
    Component.onCompleted: reject(root.exactShareId)
  }
  ActionProbe {
    id: sendToAction
    Component.onCompleted: sendTo(root.exactShareId, "pixel-8")
  }
  ActionProbe {
    id: stopAction
    Component.onCompleted: stopDiscovery()
  }
  ActionProbe {
    id: unpinAction
    Component.onCompleted: unpin()
  }

  ActionProbe {
    id: filePaste
    Component.onCompleted: submit("file:///tmp/Quick%20Share.apk")
  }
  ActionProbe {
    id: folderPaste
    Component.onCompleted: submit("/tmp/Quick Share Folder")
  }
  ActionProbe {
    id: textPaste
    Component.onCompleted: submit("plain <b>text</b> with spaces")
  }
  ActionProbe {
    id: urlPaste
    Component.onCompleted: submit(
      "https://example.test/a?x=1&y=<b>2</b>",
    )
  }

  StatusProbe {
    id: actionFailure
    executableCommand: ["sh", "-c", "exit 7", "quickshare-action"]
    probeOnStartup: false
    Component.onCompleted: submit("clipboard-secret <b>value</b>")
  }

  StatusProbe {
    id: busyProbe
    executableCommand: [
      "sh",
      "-c",
      "printf '%s %s\\n' \"$0\" \"$1\" >> \"$QUICKSHARE_TEST_LOG\"; sleep 2",
    ]
    probeOnStartup: false
    Component.onCompleted: {
      root.firstPasteResult = submit("first-paste") ? "ok" : "busy"
      root.firstPasteRunning = actionBusy
      root.secondPasteResult = submit("second-paste") ? "ok" : "busy"
      root.secondPasteRunning = actionBusy
      root.startedBusyPasteCheck = true
    }
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
