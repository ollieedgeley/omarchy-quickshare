// Copyright 2026 Omarchy Quick Share contributors
// SPDX-License-Identifier: Apache-2.0

#ifndef QUICKSHARE_CONNECTIONS_PEER_H_
#define QUICKSHARE_CONNECTIONS_PEER_H_

#include <optional>
#include <string>

#include "connections/medium_selector.h"

namespace quickshare::connections_peer {

enum class Decision { kAccept, kReject };

struct Options {
  bool advertise = false;
  bool discover = false;
  bool auto_upgrade = false;
  bool initiate_upgrade_on_connect = false;
  bool cancel_on_progress = false;
  bool disconnect_on_bandwidth_changed = false;
  bool disconnect_on_connect = false;
  bool disconnect_on_progress = false;
  Decision decision = Decision::kAccept;
  std::string endpoint_name = "connections-peer";
  std::string service_id;
  std::optional<std::string> send_file;
  std::optional<std::string> send_text;
  nearby::connections::BooleanMediumSelector initial_mediums;
  nearby::connections::BooleanMediumSelector upgrade_mediums;
};

bool Parse(int argc, char **argv, Options *options);
void Usage(const char *program);

class ConnectionsPeer {
public:
  explicit ConnectionsPeer(Options options);
  ~ConnectionsPeer();
  ConnectionsPeer(const ConnectionsPeer &) = delete;
  ConnectionsPeer &operator=(const ConnectionsPeer &) = delete;
  void Start();

private:
  class State;
  State *state_;
};

} // namespace quickshare::connections_peer

#endif
