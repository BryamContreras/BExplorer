// ConsoleClose.h

#ifndef ZIP7_INC_CONSOLE_CLOSE_H
#define ZIP7_INC_CONSOLE_CLOSE_H

namespace NConsoleClose {

// class CCtrlBreakException {};

#ifdef UNDER_CE

inline bool TestBreakSignal() { return false; }
struct CCtrlHandlerSetter {};

#else

extern unsigned g_BreakCounter;

extern const volatile unsigned *g_BfpCancelFlag;

// Per-thread cancel flag pointer (set before each FFI call, checked alongside g_BreakCounter).
// UI thread writes to the pointed-to atomic; the worker thread reads it here.
extern thread_local const volatile unsigned *t_BfpCancelFlag;

inline bool TestBreakSignal()
{
  if (g_BreakCounter != 0)
    return true;
  if (t_BfpCancelFlag && *t_BfpCancelFlag != 0)
    return true;
  if (g_BfpCancelFlag && *g_BfpCancelFlag != 0)
    return true;
  return false;
}

class CCtrlHandlerSetter Z7_final
{
  #ifndef _WIN32
  void (*memo_sig_int)(int);
  void (*memo_sig_term)(int);
  #endif
public:
  CCtrlHandlerSetter();
  ~CCtrlHandlerSetter();
};

#endif

}

#endif
