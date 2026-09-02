The user can run omarchy-quickshare "text" | <url> | ./relative/path/to/file from pwd to send a file.

use can click  plugin icon to open the panel and "omarchy global paste" the text | url | file to a file.

User should be able to bind a hyprland keybinding to send "text" | <url> | file from their clipboard. If no pinned device then it opens the plugin to chose a device. We do not need to do any form of automated hyprland config change - that is well out of scope. A simple code block in the README is fine.

Send to a device:

if they have a device pinned it auto sends. if they dont, then we search for device in the panel and they can click the one they want to send it to. Right click the device to pin it. only 1 pinned device. While searching, we add the device to the list as soon as it is seen, while we carry on searching. Sensible time out.

Sending or receiving:

while something is being sent or received the icon will slow fade to 0.2 opacity and back to 1 over two seconds.

If the panel is open, then you will see a progress bar, with est remaining time, transferred / total size, %. Below will be a cancel button.

Omarchy Notification on:

Sent
Received
Error
