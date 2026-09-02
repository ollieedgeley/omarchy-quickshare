#ifndef QUICKSHARE_FIXTURE_NOOP_ANALYTICS_RECORDER_H_
#define QUICKSHARE_FIXTURE_NOOP_ANALYTICS_RECORDER_H_

#include <atomic>

#include "sharing/analytics/analytics_recorder.h"

namespace nearby::sharing::fixture {

class NoopAnalyticsRecorder final : public analytics::AnalyticsRecorder {
public:
  void NewEstablishConnection(
      int64_t, location::nearby::proto::sharing::EstablishConnectionStatus,
      const ShareTarget &, int, int, int64_t,
      std::optional<std::string>) override {}
  void NewAcceptAgreements() override {}
  void NewDeclineAgreements() override {}
  void NewAddContact() override {}
  void NewRemoveContact() override {}
  void NewTapFeedback() override {}
  void NewTapHelp() override {}
  void NewLaunchDeviceContactConsent(
      location::nearby::proto::sharing::ConsentAcceptanceStatus) override {}
  void NewAdvertiseDevicePresenceEnd(int64_t) override {}
  void NewAdvertiseDevicePresenceStart(
      int64_t, proto::DeviceVisibility,
      location::nearby::proto::sharing::SessionStatus, proto::DataUsage,
      std::optional<std::string>) override {}
  void NewDescribeAttachments(const AttachmentContainer &) override {}
  void NewDiscoverShareTarget(const ShareTarget &, int64_t, int64_t, int64_t,
                              std::optional<std::string>, int64_t) override {}
  void NewEnableNearbySharing(
      location::nearby::proto::sharing::NearbySharingStatus) override {}
  void NewOpenReceivedAttachments(const AttachmentContainer &,
                                  int64_t) override {}
  void NewProcessReceivedAttachmentsEnd(
      int64_t,
      location::nearby::proto::sharing::ProcessReceivedAttachmentsStatus)
      override {}
  void NewReceiveAttachmentsEnd(
      int64_t, int64_t,
      location::nearby::proto::sharing::AttachmentTransmissionStatus,
      std::optional<std::string>) override {}
  void NewReceiveAttachmentsStart(int64_t,
                                  const AttachmentContainer &) override {}
  void NewReceiveFastInitialization(int64_t) override {}
  void NewAcceptFastInitialization() override {}
  void NewDismissFastInitialization() override {}
  void NewReceiveIntroduction(
      int64_t, const ShareTarget &, std::optional<std::string>,
      location::nearby::proto::sharing::OSType,
      location::nearby::proto::sharing::SharingUseCase,
      location::nearby::proto::sharing::PowerStatus) override {}
  void NewRespondToIntroduction(
      location::nearby::proto::sharing::ResponseToIntroduction,
      int64_t) override {}
  void NewTapPrivacyNotification() override {}
  void NewDismissPrivacyNotification() override {}
  void NewScanForShareTargetsEnd(int64_t) override {}
  void
  NewScanForShareTargetsStart(int64_t,
                              location::nearby::proto::sharing::SessionStatus,
                              analytics::AnalyticsInformation, int64_t,
                              std::optional<std::string>) override {}
  void NewSendAttachmentsEnd(
      int64_t, int64_t, const ShareTarget &,
      location::nearby::proto::sharing::AttachmentTransmissionStatus, int, int,
      int64_t, std::optional<std::string>,
      location::nearby::proto::sharing::ConnectionLayerStatus,
      location::nearby::proto::sharing::OSType) override {}
  void NewSendAttachmentsStart(int64_t, const AttachmentContainer &, int, int,
                               bool) override {}
  void NewSendFastInitialization() override {}
  void NewSendStart(int64_t, int, int, const ShareTarget &) override {}
  void NewSendIntroduction(ShareTargetType, int64_t,
                           location::nearby::proto::sharing::DeviceRelationship,
                           location::nearby::proto::sharing::OSType) override {}
  void
  NewSendIntroduction(int64_t, const ShareTarget &, int, int,
                      location::nearby::proto::sharing::OSType,
                      location::nearby::proto::sharing::PowerStatus) override {}
  void NewSetVisibility(proto::DeviceVisibility, proto::DeviceVisibility,
                        int64_t) override {}
  void NewDeviceSettings(analytics::AnalyticsDeviceSettings) override {}
  void NewSetDataUsage(proto::DataUsage, proto::DataUsage) override {}
  void NewAddQuickSettingsTile() override {}
  void NewRemoveQuickSettingsTile() override {}
  void NewTapQuickSettingsTile() override {}
  void NewToggleShowNotification(
      location::nearby::proto::sharing::ShowNotificationStatus,
      location::nearby::proto::sharing::ShowNotificationStatus) override {}
  void NewSetDeviceName(int) override {}
  void NewRequestSettingPermissions(
      location::nearby::proto::sharing::PermissionRequestType,
      location::nearby::proto::sharing::PermissionRequestResult) override {}
  void
  NewInstallAPKStatus(location::nearby::proto::sharing::InstallAPKStatus,
                      location::nearby::proto::sharing::ApkSource) override {}
  void
  NewVerifyAPKStatus(location::nearby::proto::sharing::VerifyAPKStatus,
                     location::nearby::proto::sharing::ApkSource) override {}
  void NewRpcCallStatus(absl::string_view, RpcDirection, int,
                        absl::Duration) override {}
  int64_t GenerateNextId() override { return next_id_++; }

private:
  std::atomic<int64_t> next_id_{1};
};

} // namespace nearby::sharing::fixture

#endif
