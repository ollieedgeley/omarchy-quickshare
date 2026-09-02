// Copyright 2026 Omarchy Quick Share contributors
// SPDX-License-Identifier: Apache-2.0

#include "connections_peer.h"

#include <atomic>
#include <cstdint>
#include <filesystem>
#include <iostream>
#include <memory>
#include <mutex>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <utility>

#include "connections/connection_options.h"
#include "connections/core.h"
#include "connections/implementation/service_controller_router.h"
#include "connections/payload.h"
#include "connections/v3/advertising_options.h"
#include "connections/v3/connections_device.h"
#include "connections/v3/discovery_options.h"
#include "connections/v3/listeners.h"

namespace quickshare::connections_peer {
namespace {

const char *MediumName(nearby::connections::Medium medium) {
  using nearby::connections::Medium;
  switch (medium) {
  case Medium::BLUETOOTH:
    return "bluetooth";
  case Medium::BLE:
    return "ble";
  case Medium::WIFI_LAN:
    return "wifi_lan";
  case Medium::WIFI_HOTSPOT:
    return "wifi_hotspot";
  case Medium::WIFI_DIRECT:
    return "wifi_direct";
  default:
    return "unknown";
  }
}

const char *QualityName(nearby::connections::v3::Quality quality) {
  using nearby::connections::v3::Quality;
  switch (quality) {
  case Quality::kLow:
    return "low";
  case Quality::kMedium:
    return "medium";
  case Quality::kHigh:
    return "high";
  default:
    return "unknown";
  }
}

std::string
ProgressStatus(nearby::connections::PayloadProgressInfo::Status status) {
  using Status = nearby::connections::PayloadProgressInfo::Status;
  if (status == Status::kSuccess)
    return "success";
  if (status == Status::kFailure)
    return "failure";
  if (status == Status::kCanceled)
    return "cancelled";
  return "in_progress";
}

} // namespace

class ConnectionsPeer::State {
public:
  explicit State(Options options)
      : options_(std::move(options)),
        router_(
            std::make_unique<nearby::connections::ServiceControllerRouter>()),
        core_(std::make_unique<nearby::connections::Core>(router_.get())),
        local_(options_.endpoint_name, {}) {}

  void Start() {
    EmitConfiguration();
    if (options_.advertise)
      StartAdvertising();
    if (options_.discover)
      StartDiscovery();
  }

private:
  void Emit(std::string_view event, std::string_view detail = "") {
    std::lock_guard<std::mutex> lock(output_mutex_);
    std::cout << "{\"schema\":1,\"event\":\"" << event << "\"";
    if (!detail.empty())
      std::cout << "," << detail;
    std::cout << "}" << std::endl;
  }

  void EmitConfiguration() {
    const auto medium = options_.initial_mediums.GetMediums(true).front();
    Emit("ready",
         "\"initial_medium\":\"" + std::string(MediumName(medium)) +
             "\",\"auto_upgrade\":" +
             (options_.auto_upgrade ? "true" : "false") +
             ",\"initiate_upgrade_on_connect\":" +
             (options_.initiate_upgrade_on_connect ? "true" : "false"));
  }

  nearby::connections::ResultCallback StatusEvent(std::string event) {
    return
        [this, event = std::move(event)](nearby::connections::Status status) {
          Emit(event, "\"status\":\"" + status.ToString() + "\"");
        };
  }

  void StartAdvertising() {
    nearby::connections::v3::AdvertisingOptions options;
    options.strategy = nearby::connections::Strategy::kP2pCluster;
    options.advertising_mediums = options_.initial_mediums;
    options.upgrade_mediums = options_.upgrade_mediums;
    options.auto_upgrade_bandwidth = options_.auto_upgrade;
    options.use_stable_endpoint_id = true;
    core_->StartAdvertisingV3(options_.service_id, options, local_, Listener(),
                              StatusEvent("advertising"));
  }

  void StartDiscovery() {
    nearby::connections::v3::DiscoveryOptions options;
    options.strategy = nearby::connections::Strategy::kP2pCluster;
    options.discovery_mediums = options_.initial_mediums;
    core_->StartDiscoveryV3(options_.service_id, options, Discovery(),
                            StatusEvent("discovery"));
  }

  void RememberInitialMedium(const nearby::NearbyDevice &device) {
    const auto medium = options_.initial_mediums.GetMediums(true).front();
    RememberMedium(device, MediumName(medium));
  }

  void RememberMedium(const nearby::NearbyDevice &device, std::string medium) {
    std::lock_guard<std::mutex> lock(medium_mutex_);
    media_[device.GetEndpointId()] = std::move(medium);
  }

  std::string CurrentMedium(const nearby::NearbyDevice &device) {
    std::lock_guard<std::mutex> lock(medium_mutex_);
    const auto found = media_.find(device.GetEndpointId());
    return found == media_.end() ? "unknown" : found->second;
  }

  void InitiateUpgradeOnConnect(const nearby::NearbyDevice &device) {
    if (!options_.initiate_upgrade_on_connect)
      return;
    Emit("upgrade-request", "\"endpoint_id\":\"" + device.GetEndpointId() +
                                "\",\"old_medium\":\"" + CurrentMedium(device) +
                                "\",\"trigger\":\"on_connect\"");
    core_->InitiateBandwidthUpgradeV3(device, StatusEvent("upgrade-result"));
  }

  void Disconnect(const nearby::NearbyDevice &device) {
    if (!disconnected_.exchange(true))
      core_->DisconnectFromDeviceV3(device, StatusEvent("disconnect"));
  }

  nearby::connections::v3::ConnectionListener Listener() {
    nearby::connections::v3::ConnectionListener listener;
    listener.initiated_cb = [this](const nearby::NearbyDevice &device,
                                   const auto &info) {
      Emit("connection-initiated",
           "\"endpoint_id\":\"" + device.GetEndpointId() + "\",\"incoming\":" +
               (info.is_incoming_connection ? "true" : "false"));
      if (options_.decision == Decision::kAccept)
        core_->AcceptConnectionV3(device, PayloadListener(),
                                  StatusEvent("accepted"));
      else
        core_->RejectConnectionV3(device, StatusEvent("rejected"));
    };
    listener.result_cb = [this](const nearby::NearbyDevice &device,
                                auto result) {
      Emit("connection-result", "\"endpoint_id\":\"" + device.GetEndpointId() +
                                    "\",\"status\":\"" +
                                    result.status.ToString() + "\"");
      if (!result.status.Ok())
        return;
      RememberInitialMedium(device);
      SendConfiguredPayload(device);
      InitiateUpgradeOnConnect(device);
      if (options_.disconnect_on_connect)
        Disconnect(device);
    };
    listener.disconnected_cb = [this](const nearby::NearbyDevice &device) {
      Emit("disconnected",
           "\"endpoint_id\":\"" + device.GetEndpointId() + "\"");
    };
    listener.bandwidth_changed_cb = [this](const nearby::NearbyDevice &device,
                                           auto info) {
      const std::string old_medium = CurrentMedium(device);
      const std::string new_medium = MediumName(info.medium);
      RememberMedium(device, new_medium);
      Emit("bandwidth-changed", "\"endpoint_id\":\"" + device.GetEndpointId() +
                                    "\",\"old_medium\":\"" + old_medium +
                                    "\",\"new_medium\":\"" + new_medium +
                                    "\",\"quality\":\"" +
                                    QualityName(info.quality) + "\"");
      if (options_.disconnect_on_bandwidth_changed)
        Disconnect(device);
    };
    return listener;
  }

  nearby::connections::v3::DiscoveryListener Discovery() {
    nearby::connections::v3::DiscoveryListener listener;
    listener.endpoint_found_cb = [this](const nearby::NearbyDevice &device,
                                        absl::string_view) {
      nearby::connections::ConnectionOptions options;
      options.strategy = nearby::connections::Strategy::kP2pCluster;
      options.allowed = options_.upgrade_mediums.Any(true)
                            ? options_.upgrade_mediums
                            : options_.initial_mediums;
      options.auto_upgrade_bandwidth = options_.auto_upgrade;
      Emit("endpoint-found",
           "\"endpoint_id\":\"" + device.GetEndpointId() + "\"");
      core_->RequestConnectionV3(local_, device, options, Listener(),
                                 StatusEvent("request-connection"));
    };
    return listener;
  }

  nearby::connections::v3::PayloadListener PayloadListener() {
    nearby::connections::v3::PayloadListener listener;
    listener.payload_received_cb =
        [this](const nearby::NearbyDevice &device,
               nearby::connections::Payload payload) {
          RecordReceivedPayload(device, std::move(payload));
        };
    listener.payload_progress_cb = [this](const nearby::NearbyDevice &device,
                                          const auto &progress) {
      RecordPayloadProgress(device, progress);
    };
    return listener;
  }

  void SendConfiguredPayload(const nearby::NearbyDevice &device) {
    if (!options_.send_file.has_value() && !options_.send_text.has_value())
      return;
    std::lock_guard<std::mutex> lock(sent_mutex_);
    if (!sent_to_.insert(device.GetEndpointId()).second)
      return;
    if (options_.send_text.has_value())
      SendText(device, *options_.send_text);
    else
      SendFile(device, *options_.send_file);
  }

  void SendText(const nearby::NearbyDevice &device, const std::string &text) {
    nearby::connections::Payload payload{nearby::ByteArray(text)};
    SendPayload(device, std::move(payload),
                static_cast<std::int64_t>(text.size()), "bytes");
  }

  void SendFile(const nearby::NearbyDevice &device, const std::string &path) {
    std::error_code error;
    const std::filesystem::path file_path(path);
    const auto size = std::filesystem::file_size(file_path, error);
    const std::string name = file_path.filename().string();
    if (error || name.empty()) {
      Emit("send-file-error", "\"path\":\"" + path + "\"");
      return;
    }
    nearby::connections::Payload payload("", name,
                                         nearby::InputFile(file_path.string()));
    SendPayload(device, std::move(payload), static_cast<std::int64_t>(size),
                "file");
  }

  void SendPayload(const nearby::NearbyDevice &device,
                   nearby::connections::Payload payload, std::int64_t bytes,
                   std::string_view type) {
    const auto id = payload.GetId();
    Emit("payload-send", "\"endpoint_id\":\"" + device.GetEndpointId() +
                             "\",\"payload_id\":" + std::to_string(id) +
                             ",\"type\":\"" + std::string(type) +
                             "\",\"expected_bytes\":" + std::to_string(bytes));
    core_->SendPayloadV3(device, std::move(payload),
                         StatusEvent("payload-send-request"));
  }

  void RecordReceivedPayload(const nearby::NearbyDevice &device,
                             nearby::connections::Payload payload) {
    const auto id = payload.GetId();
    std::string detail = "\"endpoint_id\":\"" + device.GetEndpointId() +
                         "\",\"payload_id\":" + std::to_string(id);
    if (payload.GetType() == nearby::connections::PayloadType::kFile) {
      const auto *file = payload.AsFile();
      std::lock_guard<std::mutex> lock(payload_mutex_);
      received_files_[id] = file == nullptr ? "" : file->GetFilePath();
      detail += ",\"type\":\"file\"";
    } else if (payload.GetType() == nearby::connections::PayloadType::kBytes) {
      detail += ",\"type\":\"bytes\",\"received_bytes\":" +
                std::to_string(payload.AsBytes().size());
    } else {
      detail += ",\"type\":\"stream\"";
    }
    Emit("payload-received", detail);
  }

  void RecordPayloadProgress(
      const nearby::NearbyDevice &device,
      const nearby::connections::PayloadProgressInfo &progress) {
    const std::string detail =
        "\"endpoint_id\":\"" + device.GetEndpointId() +
        "\",\"payload_id\":" + std::to_string(progress.payload_id) +
        ",\"status\":\"" + ProgressStatus(progress.status) +
        "\",\"bytes_transferred\":" +
        std::to_string(progress.bytes_transferred) +
        ",\"total_bytes\":" + std::to_string(progress.total_bytes);
    Emit("payload-progress", detail);
    if (progress.status !=
        nearby::connections::PayloadProgressInfo::Status::kInProgress) {
      EmitTerminalPayload(progress, detail);
      return;
    }
    if (options_.cancel_on_progress && !cancelled_.exchange(true))
      core_->CancelPayloadV3(device, progress.payload_id,
                             StatusEvent("payload-cancel"));
    if (options_.disconnect_on_progress)
      Disconnect(device);
  }

  void
  EmitTerminalPayload(const nearby::connections::PayloadProgressInfo &progress,
                      const std::string &detail) {
    std::string terminal = detail;
    std::lock_guard<std::mutex> lock(payload_mutex_);
    const auto file = received_files_.find(progress.payload_id);
    if (file != received_files_.end()) {
      terminal += ",\"received_file\":\"" + file->second + "\"";
      received_files_.erase(file);
    }
    Emit("payload-terminal", terminal);
  }

  Options options_;
  std::unique_ptr<nearby::connections::ServiceControllerRouter> router_;
  std::unique_ptr<nearby::connections::Core> core_;
  nearby::connections::v3::ConnectionsDevice local_;
  std::atomic<bool> cancelled_{false};
  std::atomic<bool> disconnected_{false};
  std::mutex output_mutex_;
  std::mutex medium_mutex_;
  std::unordered_map<std::string, std::string> media_;
  std::mutex payload_mutex_;
  std::unordered_map<std::int64_t, std::string> received_files_;
  std::mutex sent_mutex_;
  std::unordered_set<std::string> sent_to_;
};

ConnectionsPeer::ConnectionsPeer(Options options)
    : state_(new State(std::move(options))) {}
ConnectionsPeer::~ConnectionsPeer() { delete state_; }
void ConnectionsPeer::Start() { state_->Start(); }

} // namespace quickshare::connections_peer
