// Copyright 2026 Omarchy Quick Share contributors
// SPDX-License-Identifier: Apache-2.0

#include "connections_peer.h"

#include <iostream>
#include <string_view>
#include <utility>

namespace quickshare::connections_peer {
namespace {
constexpr std::string_view kServiceId =
    "dev.omarchy.quickshare.connections-peer";

bool StartsWith(std::string_view value, std::string_view prefix) {
  return value.starts_with(prefix);
}

bool ReadValue(std::string_view argument, std::string_view flag, int *index,
               int argc, char **argv, std::string *value) {
  if (argument == flag && *index + 1 < argc) {
    *value = argv[++*index];
    return true;
  }
  const std::string prefix = std::string(flag) + "=";
  if (!StartsWith(argument, prefix))
    return false;
  *value = std::string(argument.substr(prefix.size()));
  return true;
}

bool SetMedium(nearby::connections::BooleanMediumSelector *selector,
               std::string_view medium) {
  selector->SetAll(false);
  if (medium == "wifi_lan")
    selector->wifi_lan = true;
  else if (medium == "ble")
    selector->ble = true;
  else if (medium == "bluetooth")
    selector->bluetooth = true;
  else if (medium == "wifi_hotspot")
    selector->wifi_hotspot = true;
  else if (medium == "wifi_direct")
    selector->wifi_direct = true;
  else
    return false;
  return true;
}

bool ParsePayloadOption(std::string_view argument, int *index, int argc,
                        char **argv, Options *options) {
  std::string value;
  if (ReadValue(argument, "--send-file", index, argc, argv, &value))
    options->send_file = std::move(value);
  else if (ReadValue(argument, "--send-text", index, argc, argv, &value))
    options->send_text = std::move(value);
  else if (argument == "--cancel-on-progress")
    options->cancel_on_progress = true;
  else if (argument == "--disconnect-on-bandwidth-changed")
    options->disconnect_on_bandwidth_changed = true;
  else if (argument == "--disconnect-on-connect")
    options->disconnect_on_connect = true;
  else if (argument == "--disconnect-on-progress")
    options->disconnect_on_progress = true;
  else
    return false;
  return true;
}

bool ParseIdentityOption(std::string_view argument, int *index, int argc,
                         char **argv, Options *options) {
  std::string value;
  if (ReadValue(argument, "--decision", index, argc, argv, &value)) {
    if (value == "accept")
      options->decision = Decision::kAccept;
    else if (value == "reject")
      options->decision = Decision::kReject;
    else
      return false;
  } else if (ReadValue(argument, "--endpoint-name", index, argc, argv,
                       &value)) {
    options->endpoint_name = std::move(value);
  } else if (ReadValue(argument, "--service-id", index, argc, argv, &value)) {
    options->service_id = std::move(value);
  } else {
    return false;
  }
  return true;
}

bool ParseMediumOption(std::string_view argument, int *index, int argc,
                       char **argv, Options *options) {
  std::string value;
  if (ReadValue(argument, "--initial-medium", index, argc, argv, &value))
    return SetMedium(&options->initial_mediums, value);
  if (ReadValue(argument, "--upgrade-medium", index, argc, argv, &value))
    return SetMedium(&options->upgrade_mediums, value);
  return false;
}
} // namespace

void Usage(const char *program) {
  std::cerr << "Usage: " << program << " (--advertise|--discover)"
            << " --initial-medium=wifi_lan [--upgrade-medium=wifi_lan]"
            << " [--auto-upgrade] [--initiate-upgrade-on-connect]"
            << " [--decision=accept|reject] [--endpoint-name=NAME]"
            << " [--service-id=ID] [--send-file=PATH|--send-text=TEXT]\n"
            << "       [--cancel-on-progress] [--disconnect-on-connect]"
            << " [--disconnect-on-progress]"
            << " [--disconnect-on-bandwidth-changed]\n";
}

bool Parse(int argc, char **argv, Options *options) {
  options->service_id = kServiceId;
  for (int index = 1; index < argc; ++index) {
    const std::string_view argument(argv[index]);
    if (argument == "--advertise")
      options->advertise = true;
    else if (argument == "--discover")
      options->discover = true;
    else if (argument == "--auto-upgrade")
      options->auto_upgrade = true;
    else if (argument == "--initiate-upgrade-on-connect")
      options->initiate_upgrade_on_connect = true;
    else if (ParsePayloadOption(argument, &index, argc, argv, options) ||
             ParseIdentityOption(argument, &index, argc, argv, options) ||
             ParseMediumOption(argument, &index, argc, argv, options))
      continue;
    else
      return false;
  }
  const bool proposes_upgrade =
      options->auto_upgrade || options->initiate_upgrade_on_connect;
  return (options->advertise || options->discover) &&
         options->initial_mediums.Any(true) &&
         (!proposes_upgrade || options->upgrade_mediums.Any(true)) &&
         !options->endpoint_name.empty() &&
         !(options->send_file.has_value() && options->send_text.has_value());
}

} // namespace quickshare::connections_peer
