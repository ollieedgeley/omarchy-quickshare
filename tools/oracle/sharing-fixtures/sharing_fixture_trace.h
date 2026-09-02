#pragma once

#include <filesystem>
#include <memory>

namespace nearby::sharing::fixture {

struct ExpectedFrame {
  const char *path;
  const char *direction;
  const char *seam;
  const char *kind;
  const char *attachment;
  const char *status;
  const char *outcome;
};

class TraceCollector {
public:
  TraceCollector();
  ~TraceCollector();

  TraceCollector(const TraceCollector &) = delete;
  TraceCollector &operator=(const TraceCollector &) = delete;

  bool Validate(const std::filesystem::path &path,
                const ExpectedFrame &expected) const;
  bool Record(const std::filesystem::path &path, const ExpectedFrame &expected);
  bool Write(const std::filesystem::path &path) const;

private:
  class Impl;
  std::unique_ptr<Impl> impl_;
};

} // namespace nearby::sharing::fixture
