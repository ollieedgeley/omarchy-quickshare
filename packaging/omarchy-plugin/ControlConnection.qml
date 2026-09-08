import QtQuick
import Quickshell.Io

Socket {
  id: root

  property string requestFrame: ""
  property bool finished: false
  property bool started: false
  readonly property int maximumFrameBytes: 1048576
  signal completed(string frame, string failure)

  function utf8Bytes(value) {
    var bytes = 0
    for (var i = 0; i < value.length; i++) {
      var code = value.charCodeAt(i)
      if (code < 0x80) bytes += 1
      else if (code < 0x800) bytes += 2
      else if (code >= 0xd800 && code <= 0xdbff
          && i + 1 < value.length
          && value.charCodeAt(i + 1) >= 0xdc00
          && value.charCodeAt(i + 1) <= 0xdfff) {
        bytes += 4
        i += 1
      } else bytes += 3
    }
    return bytes
  }

  function begin(frame) {
    if (started) return
    started = true
    if (utf8Bytes(frame) > maximumFrameBytes || frame.indexOf("\n") >= 0) {
      finish("", "message_too_large")
      return
    }
    requestFrame = frame + "\n"
    deadline.start()
    connected = true
  }

  function finish(frame, failure) {
    if (finished) return
    finished = true
    deadline.stop()
    connected = false
    requestFrame = ""
    completed(frame, failure)
    // A failed connect cannot reliably reuse a Quickshell Socket.
    destroy()
  }

  function validUtf8(bytes, length) {
    for (var i = 0; i < length; i++) {
      var first = bytes[i]
      if (first < 0x80) continue
      var count = first >= 0xc2 && first <= 0xdf ? 1
        : (first >= 0xe0 && first <= 0xef ? 2
          : (first >= 0xf0 && first <= 0xf4 ? 3 : -1))
      if (count < 0 || i + count >= length) return false
      var second = bytes[i + 1]
      if ((first === 0xe0 && second < 0xa0)
          || (first === 0xed && second >= 0xa0)
          || (first === 0xf0 && second < 0x90)
          || (first === 0xf4 && second >= 0x90)) return false
      for (var j = 0; j < count; j++) {
        i += 1
        if (bytes[i] < 0x80 || bytes[i] > 0xbf) return false
      }
    }
    return true
  }

  function consume() {
    if (finished) return
    var bytes = new Uint8Array(collector.data)
    var newline = bytes.indexOf(10)
    var length = newline < 0 ? bytes.length : newline
    // The collector re-decodes cumulative bytes, preserving split UTF-8.
    // Qt can supply one larger transient chunk before this bound is checked.
    if (length > maximumFrameBytes) {
      finish("", "message_too_large")
    } else if (newline >= 0) {
      if (newline !== bytes.length - 1 || !validUtf8(bytes, length)) {
        finish("", "invalid_response")
      } else {
        var value = String(collector.text)
        finish(value.slice(0, -1), "")
      }
    }
  }

  parser: StdioCollector {
    id: collector
    waitForEnd: false
    onDataChanged: root.consume()
    onStreamFinished: {
      root.consume()
      if (!root.finished) root.finish("", "disconnected")
    }
  }
  onConnectedChanged: {
    if (connected && !finished) {
      write(requestFrame)
      flush()
    }
  }
  onError: if (!finished) finish("", "disconnected")

  property Timer deadline: Timer {
    interval: 5000
    onTriggered: root.finish("", "request_timeout")
  }
}
