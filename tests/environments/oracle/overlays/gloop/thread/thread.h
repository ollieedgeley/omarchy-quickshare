#ifndef OMARCHY_QUICKSHARE_GLOOP_THREAD_THREAD_H_
#define OMARCHY_QUICKSHARE_GLOOP_THREAD_THREAD_H_

#include <sys/syscall.h>
#include <unistd.h>

struct LiveThread {};

inline const LiveThread* Thread_GetMyLiveThread() {
  thread_local LiveThread thread;
  return &thread;
}

inline int LiveThread_Pthread_TID(const LiveThread*) {
  return static_cast<int>(syscall(SYS_gettid));
}

#endif  // OMARCHY_QUICKSHARE_GLOOP_THREAD_THREAD_H_
