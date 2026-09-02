// Copyright 2026 Omarchy Quick Share contributors
// SPDX-License-Identifier: Apache-2.0

#include <signal.h>

#include <utility>

#include "absl/synchronization/notification.h"
#include "connections_peer.h"

namespace {
absl::Notification g_shutdown;

void QuitHandler(int, siginfo_t *, void *) {
  if (!g_shutdown.HasBeenNotified())
    g_shutdown.Notify();
}
} // namespace

int main(int argc, char **argv) {
  quickshare::connections_peer::Options options;
  if (!quickshare::connections_peer::Parse(argc, argv, &options)) {
    quickshare::connections_peer::Usage(argv[0]);
    return 2;
  }
  struct sigaction action{};
  action.sa_sigaction = QuitHandler;
  action.sa_flags = SA_SIGINFO;
  sigaction(SIGINT, &action, nullptr);
  sigaction(SIGTERM, &action, nullptr);
  quickshare::connections_peer::ConnectionsPeer peer(std::move(options));
  peer.Start();
  g_shutdown.WaitForNotification();
  return 0;
}
