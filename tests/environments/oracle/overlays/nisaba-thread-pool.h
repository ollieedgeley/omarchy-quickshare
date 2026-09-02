#ifndef OMARCHY_QUICKSHARE_NISABA_THREAD_POOL_H_
#define OMARCHY_QUICKSHARE_NISABA_THREAD_POOL_H_

#include <condition_variable>
#include <cstddef>
#include <deque>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

#include "absl/functional/any_invocable.h"
#include "absl/time/clock.h"
#include "absl/time/time.h"

class ThreadPool {
 public:
  explicit ThreadPool(int thread_count) : thread_count_(thread_count) {}
  ThreadPool(const ThreadPool&) = delete;
  ThreadPool& operator=(const ThreadPool&) = delete;

  ~ThreadPool() {
    {
      std::lock_guard lock(mutex_);
      stopping_ = true;
    }
    ready_.notify_all();
    for (auto& worker : workers_) worker.join();
  }

  void StartWorkers() {
    workers_.reserve(thread_count_);
    for (int index = 0; index < thread_count_; ++index) {
      workers_.emplace_back([this] { Work(); });
    }
  }

  void Schedule(absl::AnyInvocable<void()> task) {
    {
      std::lock_guard lock(mutex_);
      tasks_.push_back(std::move(task));
    }
    ready_.notify_one();
  }

  void ScheduleAt(absl::Time deadline, absl::AnyInvocable<void()> task) {
    Schedule([deadline, task = std::move(task)]() mutable {
      absl::SleepFor(deadline - absl::Now());
      std::move(task)();
    });
  }

 private:
  void Work() {
    while (true) {
      absl::AnyInvocable<void()> task;
      {
        std::unique_lock lock(mutex_);
        ready_.wait(lock, [this] { return stopping_ || !tasks_.empty(); });
        if (stopping_ && tasks_.empty()) return;
        task = std::move(tasks_.front());
        tasks_.pop_front();
      }
      std::move(task)();
    }
  }

  int thread_count_;
  bool stopping_ = false;
  std::mutex mutex_;
  std::condition_variable ready_;
  std::deque<absl::AnyInvocable<void()>> tasks_;
  std::vector<std::thread> workers_;
};

#endif  // OMARCHY_QUICKSHARE_NISABA_THREAD_POOL_H_
