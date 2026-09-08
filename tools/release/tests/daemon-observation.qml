import QtQuick
import Quickshell
import "./plugin" as Plugin

ShellRoot {
    Plugin.QuickShareService {
        id: service
    }
    Timer {
        interval: 25
        repeat: true
        running: true
        onTriggered: {
            if (service.protocolState === "ready") {
                console.log("HARNESS_OK direct daemon observation",
                            JSON.stringify(service.endpointSnapshot));
                Qt.quit();
            }
        }
    }
    Timer {
        interval: 7000
        running: true
        onTriggered: {
            console.error("HARNESS_FAIL daemon observation",
                          service.protocolState, service.observationError);
            Qt.exit(2);
        }
    }
}
