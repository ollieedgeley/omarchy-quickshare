#include <filesystem>
#include <fstream>
#include <iostream>
#include <limits>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "absl/strings/string_view.h"
#include "internal/base/file_path.h"
#include "internal/base/files.h"
#include "internal/test/fake_clock.h"
#include "internal/test/fake_device_info.h"
#include "internal/test/fake_task_runner.h"
#include "quickshare_fixture_generator/fake_nearby_connections_manager.h"
#include "sharing/attachment_container.h"
#include "sharing/file_attachment.h"
#include "sharing/incoming_share_session.h"
#include "sharing/nearby_connection_impl.h"
#include "sharing/outgoing_share_session.h"
#include "sharing/proto/wire_format.pb.h"
#include "sharing/text_attachment.h"
#include "tools/quickshare_fixture_generator/noop_analytics_recorder.h"
#include "tools/quickshare_fixture_generator/sharing_fixture_trace.h"

namespace {
using nearby::FakeClock;
using nearby::FakeDeviceInfo;
using nearby::FakeTaskRunner;
using nearby::FilePath;
using nearby::Files;
using nearby::sharing::AttachmentContainer;
using nearby::sharing::FakeNearbyConnectionsManager;
using nearby::sharing::FileAttachment;
using nearby::sharing::IncomingShareSession;
using nearby::sharing::NearbyConnectionImpl;
using nearby::sharing::OutgoingShareSession;
using nearby::sharing::ShareTarget;
using nearby::sharing::TextAttachment;
using nearby::sharing::TransferMetadata;
using nearby::sharing::fixture::ExpectedFrame;
using nearby::sharing::fixture::TraceCollector;
using nearby::sharing::service::proto::ConnectionResponseFrame;
using nearby::sharing::service::proto::FileMetadata;
using nearby::sharing::service::proto::Frame;
using nearby::sharing::service::proto::IntroductionFrame;
using nearby::sharing::service::proto::TextMetadata;

bool Write(const std::filesystem::path &path, const std::string &bytes) {
  std::ofstream output(path, std::ios::binary);
  output.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
  return output.good();
}
bool Directory(const std::filesystem::path &path) {
  std::error_code error;
  std::filesystem::create_directories(path, error);
  return !error;
}
class Capture {
public:
  void Attach(FakeNearbyConnectionsManager &manager) {
    manager.set_send_payload_callback(
        [this](std::unique_ptr<nearby::sharing::Payload> payload,
               std::weak_ptr<nearby::sharing::NearbyConnectionsManager::
                                 PayloadStatusListener>) {
          bytes_ = std::move(payload->content.bytes_payload.bytes);
        });
  }
  bool Save(const std::filesystem::path &path) const {
    return Write(path, std::string(bytes_.begin(), bytes_.end()));
  }

private:
  std::vector<uint8_t> bytes_;
};

IntroductionFrame File() {
  IntroductionFrame frame;
  auto *file = frame.add_file_metadata();
  file->set_id(101);
  file->set_name("fixture-file.bin");
  file->set_size(12);
  file->set_mime_type("application/octet-stream");
  file->set_type(FileMetadata::DOCUMENT);
  file->set_payload_id(201);
  return frame;
}

IntroductionFrame Text(TextMetadata::Type type) {
  IntroductionFrame frame;
  auto *text = frame.add_text_metadata();
  const bool is_text = type == TextMetadata::TEXT;
  text->set_id(is_text ? 102 : 103);
  text->set_text_title(is_text ? "fixture text" : "https://x.y");
  text->set_size(is_text ? 12 : 11);
  text->set_type(type);
  text->set_payload_id(is_text ? 202 : 203);
  return frame;
}

IntroductionFrame Apk() {
  IntroductionFrame frame;
  auto *app = frame.add_app_metadata();
  app->set_id(104);
  app->set_app_name("FixtureApp");
  app->set_package_name("dev.fixture.app");
  app->set_size(16);
  app->add_file_name("FixtureApp.apk");
  app->add_file_size(16);
  app->add_payload_id(204);
  return frame;
}

IntroductionFrame TooLarge() {
  IntroductionFrame frame = File();
  frame.mutable_file_metadata(0)->set_size(std::numeric_limits<int64_t>::max());
  auto *extra = frame.add_file_metadata();
  extra->set_id(105);
  extra->set_name("overflow.bin");
  extra->set_size(1);
  extra->set_type(FileMetadata::DOCUMENT);
  extra->set_payload_id(205);
  return frame;
}

bool SaveIntroduction(const std::filesystem::path &path,
                      const IntroductionFrame &introduction,
                      TraceCollector &trace, const ExpectedFrame &expected) {
  Frame frame;
  frame.set_version(Frame::V1);
  *frame.mutable_v1()->mutable_introduction() = introduction;
  return Write(path, frame.SerializeAsString()) && trace.Record(path, expected);
}

ExpectedFrame IncomingIntroduction(const char *path, const char *attachment) {
  return {
      path,           "incoming", "IncomingShareSession.ProcessIntroduction",
      "introduction", attachment, "",
      "accepted"};
}

ExpectedFrame IncomingResponse(const char *path, const char *seam,
                               const char *status, const char *outcome) {
  return {path, "incoming", seam, "response", "", status, outcome};
}

ExpectedFrame OutgoingIntroduction(const char *path, const char *attachment) {
  return {path,           "outgoing", "OutgoingShareSession.SendIntroduction",
          "introduction", attachment, "",
          "written"};
}

class Incoming {
public:
  Incoming()
      : runner_(&clock_, 1), connection_(device_),
        session_(
            &clock_, runner_, &manager_, analytics_, "fixture-peer", target_,
            [](const IncomingShareSession &, const TransferMetadata &) {}) {
    session_.OnConnected(&connection_);
    capture_.Attach(manager_);
  }
  bool Failure(const std::filesystem::path &path, TraceCollector &trace,
               const IntroductionFrame &frame, TransferMetadata::Status status,
               const ExpectedFrame &expected) {
    if (!session_.ProcessIntroduction(frame).has_value())
      return false;
    session_.SendFailureResponse(status);
    return capture_.Save(path) && trace.Record(path, expected);
  }
  bool Accept(const std::filesystem::path &path, TraceCollector &trace,
              const ExpectedFrame &expected) {
    if (session_.ProcessIntroduction(File()).has_value())
      return false;
    session_.ReadyForTransfer(
        [] {},
        [](bool, std::optional<nearby::sharing::service::proto::V1Frame>) {});
    return session_.AcceptTransfer([] {}) && capture_.Save(path) &&
           trace.Record(path, expected);
  }
  bool Response(const std::filesystem::path &path, TraceCollector &trace,
                ConnectionResponseFrame::Status status,
                const ExpectedFrame &expected) {
    session_.WriteResponseFrame(status);
    return capture_.Save(path) && trace.Record(path, expected);
  }
  bool Cancel(const std::filesystem::path &path, TraceCollector &trace,
              const ExpectedFrame &expected) {
    session_.WriteCancelFrame();
    return capture_.Save(path) && trace.Record(path, expected);
  }

private:
  FakeClock clock_;
  FakeTaskRunner runner_;
  nearby::sharing::fixture::NoopAnalyticsRecorder analytics_;
  ShareTarget target_;
  FakeNearbyConnectionsManager manager_;
  FakeDeviceInfo device_;
  NearbyConnectionImpl connection_;
  IncomingShareSession session_;
  Capture capture_;
};

bool IncomingIntroductions(const std::filesystem::path &inputs,
                           TraceCollector &trace) {
  return SaveIntroduction(
             inputs / "file.bin", File(), trace,
             IncomingIntroduction("incoming/introductions/file.bin", "file")) &&
         SaveIntroduction(
             inputs / "text.bin", Text(TextMetadata::TEXT), trace,
             IncomingIntroduction("incoming/introductions/text.bin", "text")) &&
         SaveIntroduction(
             inputs / "url.bin", Text(TextMetadata::URL), trace,
             IncomingIntroduction("incoming/introductions/url.bin", "url")) &&
         SaveIntroduction(
             inputs / "apk.bin", Apk(), trace,
             IncomingIntroduction("incoming/introductions/apk.bin", "apk"));
}

bool IncomingFiles(const std::filesystem::path &root, TraceCollector &trace) {
  const auto inputs = root / "incoming" / "introductions";
  const auto responses = root / "incoming" / "responses";
  Incoming accept;
  Incoming insufficient;
  Incoming unsupported;
  Incoming timeout;
  Incoming reject;
  Incoming cancel;
  return Directory(inputs) && Directory(responses) &&
         IncomingIntroductions(inputs, trace) &&
         accept.Accept(responses / "accept.bin", trace,
                       IncomingResponse("incoming/responses/accept.bin",
                                        "IncomingShareSession.AcceptTransfer",
                                        "accept",
                                        "awaiting-remote-acceptance")) &&
         insufficient.Failure(
             responses / "not-enough-space.bin", trace, TooLarge(),
             TransferMetadata::Status::kNotEnoughSpace,
             IncomingResponse("incoming/responses/not-enough-space.bin",
                              "IncomingShareSession.SendFailureResponse",
                              "not-enough-space", "final")) &&
         unsupported.Failure(
             responses / "unsupported.bin", trace, IntroductionFrame(),
             TransferMetadata::Status::kUnsupportedAttachmentType,
             IncomingResponse("incoming/responses/unsupported.bin",
                              "IncomingShareSession.SendFailureResponse",
                              "unsupported", "final")) &&
         timeout.Response(responses / "timed-out.bin", trace,
                          ConnectionResponseFrame::TIMED_OUT,
                          IncomingResponse("incoming/responses/timed-out.bin",
                                           "ShareSession.WriteResponseFrame",
                                           "timed-out", "frame-written")) &&
         reject.Response(responses / "reject.bin", trace,
                         ConnectionResponseFrame::REJECT,
                         IncomingResponse("incoming/responses/reject.bin",
                                          "ShareSession.WriteResponseFrame",
                                          "reject", "frame-written")) &&
         cancel.Cancel(responses / "cancel.bin", trace,
                       {"incoming/responses/cancel.bin", "incoming",
                        "ShareSession.WriteCancelFrame", "cancel", "", "",
                        "cancel-frame-written"});
}

class Outgoing {
public:
  Outgoing()
      : runner_(&clock_, 1),
        session_(
            &clock_, runner_, &manager_, analytics_, "fixture-peer", target_,
            [](const OutgoingShareSession &, const TransferMetadata &) {}) {
    capture_.Attach(manager_);
  }
  bool File(const std::filesystem::path &path, TraceCollector &trace,
            const ExpectedFrame &expected) {
    const FilePath file = Files::GetTemporaryDirectory().append(
        FilePath("quickshare-fixture.bin"));
    if (!Write(file.GetPath(), "fixture-data"))
      return false;
    FileAttachment attachment(111, 12, "quickshare-fixture.bin",
                              "application/octet-stream",
                              FileMetadata::DOCUMENT);
    attachment.set_file_path(file);
    return Send(path, {}, {attachment}, trace, expected);
  }
  bool Text(const std::filesystem::path &path, TextMetadata::Type type,
            TraceCollector &trace, const ExpectedFrame &expected) {
    const std::string content =
        type == TextMetadata::TEXT ? "fixture text" : "https://x.y";
    TextAttachment attachment(
        type == TextMetadata::TEXT ? 112 : 113, type, content, content,
        content.size(), "text/plain", 0,
        location::nearby::proto::sharing::ATTACHMENT_SOURCE_UNKNOWN);
    return Send(path, {attachment}, {}, trace, expected);
  }

private:
  bool Send(const std::filesystem::path &path, std::vector<TextAttachment> text,
            std::vector<FileAttachment> files, TraceCollector &trace,
            const ExpectedFrame &expected) {
    auto attachments =
        AttachmentContainer::Builder(std::move(text), std::move(files), {})
            .Build();
    if (!session_.InitiateSendAttachments(std::move(attachments)))
      return false;
    NearbyConnectionImpl connection(device_);
    manager_.set_nearby_connection(&connection);
    session_.Connect({}, nearby::sharing::proto::DataUsage::ONLINE_DATA_USAGE,
                     false,
                     [](absl::string_view, nearby::sharing::NearbyConnection *,
                        nearby::sharing::Status) {});
    return session_.OnConnectResult(&connection,
                                    nearby::sharing::Status::kSuccess) &&
           session_.SendIntroduction([] {}) && capture_.Save(path) &&
           trace.Validate(path, expected);
  }
  FakeClock clock_;
  FakeTaskRunner runner_;
  nearby::sharing::fixture::NoopAnalyticsRecorder analytics_;
  ShareTarget target_;
  FakeNearbyConnectionsManager manager_;
  FakeDeviceInfo device_;
  OutgoingShareSession session_;
  Capture capture_;
};

bool Canonicalize(const std::filesystem::path &path, TraceCollector &trace,
                  const ExpectedFrame &expected) {
  std::ifstream input(path, std::ios::binary);
  std::string bytes{std::istreambuf_iterator<char>(input), {}};
  Frame frame;
  if (!frame.ParseFromString(bytes) || !trace.Validate(path, expected))
    return false;
  auto *introduction = frame.mutable_v1()->mutable_introduction();
  for (int index = 0; index < introduction->file_metadata_size(); ++index)
    introduction->mutable_file_metadata(index)->set_payload_id(300 + index);
  for (int index = 0; index < introduction->text_metadata_size(); ++index)
    introduction->mutable_text_metadata(index)->set_payload_id(400 + index);
  return Write(path, frame.SerializeAsString()) && trace.Record(path, expected);
}

bool OutgoingFiles(const std::filesystem::path &root, TraceCollector &trace) {
  const auto directory = root / "outgoing" / "introductions";
  Outgoing file;
  Outgoing text;
  Outgoing url;
  const auto file_expected =
      OutgoingIntroduction("outgoing/introductions/file.bin", "file");
  const auto text_expected =
      OutgoingIntroduction("outgoing/introductions/text.bin", "text");
  const auto url_expected =
      OutgoingIntroduction("outgoing/introductions/url.bin", "url");
  return Directory(directory) &&
         file.File(directory / "file.bin", trace, file_expected) &&
         Canonicalize(directory / "file.bin", trace, file_expected) &&
         text.Text(directory / "text.bin", TextMetadata::TEXT, trace,
                   text_expected) &&
         Canonicalize(directory / "text.bin", trace, text_expected) &&
         url.Text(directory / "url.bin", TextMetadata::URL, trace,
                  url_expected) &&
         Canonicalize(directory / "url.bin", trace, url_expected);
}
} // namespace
int main(int argc, char **argv) {
  if (argc != 2)
    return 2;
  const std::filesystem::path root(argv[1]);
  TraceCollector trace;
  if (!Directory(root) || !IncomingFiles(root, trace) ||
      !OutgoingFiles(root, trace) || !trace.Write(root / "trace.json")) {
    std::cerr << "could not generate Google Sharing session fixtures\n";
    return 1;
  }
  return 0;
}
