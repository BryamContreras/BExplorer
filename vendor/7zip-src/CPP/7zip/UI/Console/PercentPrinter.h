// PercentPrinter.h

#ifndef ZIP7_INC_PERCENT_PRINTER_H
#define ZIP7_INC_PERCENT_PRINTER_H

#include "../../../Common/MyTypes.h"
#include "../../../Common/MyString.h"
#include "../../../Common/StdOutStream.h"

// BExplorer progress hook — set to a non-null callback to receive
// progress updates from CPercentPrinter::Print().
// The callback is invoked with (completed, total, files, command, fileNameUtf8)
// at a throttled cadence (~12 Hz) regardless of the percent stream state.
// command and fileNameUtf8 are UTF-8 encoded, NULL if empty.
typedef void (Z7_CDECL *BfpProgressCb)(UInt64 completed, UInt64 total, UInt64 files,
    const char *command, const char *fileNameUtf8, void *userData);
extern thread_local BfpProgressCb g_BfpProgressCb;
extern thread_local void *g_BfpProgressUserData;

struct CPercentPrinterState
{
  UInt64 Completed;
  UInt64 Total;
  
  UInt64 Files;

  AString Command;
  UString FileName;

  void ClearCurState();

  CPercentPrinterState():
      Completed(0),
      Total((UInt64)(Int64)-1),
      Files(0)
    {}
};

class CPercentPrinter: public CPercentPrinterState
{
public:
  CStdOutStream *_so;
  bool DisablePrint;
  bool NeedFlush;
  unsigned MaxLen;
  
private:
  UInt32 _tickStep;
  DWORD _prevTick;
  DWORD _bfpPrevTick;

  AString _s;

  AString _printedString;
  AString _temp;
  UString _tempU;
  BfpProgressCb _bfpProgressCb;
  void *_bfpProgressUserData;

  CPercentPrinterState _printedState;
  AString _printedPercents;

  void GetPercents();

public:
  
  CPercentPrinter(UInt32 tickStep = 200):
      DisablePrint(false),
      NeedFlush(true),
      MaxLen(80 - 1),
      _tickStep(tickStep),
      _prevTick(0),
      _bfpPrevTick(0),
      _bfpProgressCb(g_BfpProgressCb),
      _bfpProgressUserData(g_BfpProgressUserData)
  {}

  bool HasBfpProgress() const { return _bfpProgressCb != NULL; }
  ~CPercentPrinter();

  void ClosePrint(bool needFlush);
  void Print();
};

#endif
