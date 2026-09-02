#include "tools/quickshare_fixture_generator/sharing_fixture_trace.h"

#include <fstream>
#include <optional>
#include <string>
#include <vector>

#include "sharing/proto/wire_format.pb.h"

namespace nearby::sharing::fixture {
namespace {
using nearby::sharing::service::proto::ConnectionResponseFrame;
using nearby::sharing::service::proto::Frame;
using nearby::sharing::service::proto::IntroductionFrame;
using nearby::sharing::service::proto::TextMetadata;

struct DecodedFrame {
  std::string kind;
  std::string attachment;
  std::string status;
};

struct TraceRecord {
  ExpectedFrame expected;
  DecodedFrame decoded;
};

bool WriteFile(const std::filesystem::path &path, const std::string &bytes) {
  std::ofstream output(path, std::ios::binary);
  output.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
  return output.good();
}

std::optional<std::string> Attachment(const IntroductionFrame &introduction) {
  if (introduction.file_metadata_size() == 1 &&
      introduction.text_metadata_size() == 0 &&
      introduction.app_metadata_size() == 0)
    return "file";
  if (introduction.file_metadata_size() == 0 &&
      introduction.text_metadata_size() == 1 &&
      introduction.app_metadata_size() == 0) {
    switch (introduction.text_metadata(0).type()) {
    case TextMetadata::TEXT:
      return "text";
    case TextMetadata::URL:
      return "url";
    default:
      return std::nullopt;
    }
  }
  if (introduction.file_metadata_size() == 0 &&
      introduction.text_metadata_size() == 0 &&
      introduction.app_metadata_size() == 1)
    return "apk";
  return std::nullopt;
}

std::optional<DecodedFrame> Decode(const std::string &bytes) {
  Frame frame;
  if (!frame.ParseFromString(bytes) || frame.version() != Frame::V1 ||
      !frame.has_v1())
    return std::nullopt;
  const auto &v1 = frame.v1();
  if (v1.has_introduction()) {
    const auto attachment = Attachment(v1.introduction());
    if (!attachment.has_value())
      return std::nullopt;
    return DecodedFrame{"introduction", *attachment, ""};
  }
  if (v1.has_connection_response()) {
    switch (v1.connection_response().status()) {
    case ConnectionResponseFrame::ACCEPT:
      return DecodedFrame{"response", "", "accept"};
    case ConnectionResponseFrame::NOT_ENOUGH_SPACE:
      return DecodedFrame{"response", "", "not-enough-space"};
    case ConnectionResponseFrame::UNSUPPORTED_ATTACHMENT_TYPE:
      return DecodedFrame{"response", "", "unsupported"};
    case ConnectionResponseFrame::TIMED_OUT:
      return DecodedFrame{"response", "", "timed-out"};
    case ConnectionResponseFrame::REJECT:
      return DecodedFrame{"response", "", "reject"};
    default:
      return std::nullopt;
    }
  }
  if (v1.type() == service::proto::V1Frame::CANCEL)
    return DecodedFrame{"cancel", "", ""};
  return std::nullopt;
}

bool Matches(const DecodedFrame &decoded, const ExpectedFrame &expected) {
  return decoded.kind == expected.kind &&
         decoded.attachment == expected.attachment &&
         decoded.status == expected.status;
}
} // namespace

class TraceCollector::Impl {
public:
  std::vector<TraceRecord> records;
};

TraceCollector::TraceCollector() : impl_(std::make_unique<Impl>()) {}
TraceCollector::~TraceCollector() = default;

bool TraceCollector::Validate(const std::filesystem::path &path,
                              const ExpectedFrame &expected) const {
  std::ifstream input(path, std::ios::binary);
  const std::string bytes{std::istreambuf_iterator<char>(input), {}};
  const auto decoded = Decode(bytes);
  return decoded.has_value() && Matches(*decoded, expected);
}

bool TraceCollector::Record(const std::filesystem::path &path,
                            const ExpectedFrame &expected) {
  std::ifstream input(path, std::ios::binary);
  const std::string bytes{std::istreambuf_iterator<char>(input), {}};
  const auto decoded = Decode(bytes);
  if (!decoded.has_value() || !Matches(*decoded, expected))
    return false;
  impl_->records.push_back({expected, *decoded});
  return true;
}

bool TraceCollector::Write(const std::filesystem::path &path) const {
  std::string trace = "{\n  \"schema\": 5,\n  \"scope\": \"pinned Google "
                      "Sharing sessions\",\n  \"normalization\": [\"outgoing "
                      "payload_id\"],\n  \"records\": [\n";
  for (size_t index = 0; index < impl_->records.size(); ++index) {
    const auto &record = impl_->records[index];
    trace += "    {\n      \"path\": \"";
    trace += record.expected.path;
    trace += "\",\n      \"direction\": \"";
    trace += record.expected.direction;
    trace += "\",\n      \"seam\": \"";
    trace += record.expected.seam;
    trace += "\",\n      \"kind\": \"";
    trace += record.decoded.kind;
    trace += "\"";
    if (!record.decoded.attachment.empty())
      trace += ",\n      \"attachment\": \"" + record.decoded.attachment +
               "\"";
    if (!record.decoded.status.empty())
      trace += ",\n      \"status\": \"" + record.decoded.status + "\"";
    trace += ",\n      \"outcome\": \"";
    trace += record.expected.outcome;
    trace += "\"\n    }";
    trace += index + 1 == impl_->records.size() ? "\n" : ",\n";
  }
  return WriteFile(path, trace + "  ]\n}\n");
}

} // namespace nearby::sharing::fixture
