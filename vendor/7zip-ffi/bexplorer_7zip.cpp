#include "../7zip-src/CPP/7zip/Bundles/Alone7z/StdAfx.h"

#include "../7zip-src/C/CpuArch.h"

#include "../7zip-src/CPP/Common/MyException.h"
#include "../7zip-src/CPP/Common/MyString.h"
#include "../7zip-src/CPP/Common/StdOutStream.h"
#include "../7zip-src/CPP/Windows/ErrorMsg.h"
#include "../7zip-src/CPP/7zip/UI/Common/ExitCode.h"
#include "../7zip-src/CPP/7zip/UI/Common/EnumDirItems.h"
#include "../7zip-src/CPP/7zip/UI/Console/ConsoleClose.h"
#include "../7zip-src/CPP/7zip/UI/Console/PercentPrinter.h"

// Includes for bfp_7zr_list_archive
#include "../7zip-src/CPP/Common/MyCom.h"
#include "../7zip-src/CPP/7zip/Archive/IArchive.h"
#include "../7zip-src/CPP/7zip/IStream.h"
#include "../7zip-src/CPP/7zip/UI/Common/OpenArchive.h"
#include "../7zip-src/CPP/7zip/UI/Common/LoadCodecs.h"
#include "../7zip-src/CPP/7zip/UI/Console/OpenCallbackConsole.h"
#include "../7zip-src/CPP/7zip/Common/FileStreams.h"

extern thread_local CStdOutStream *g_StdStream;
thread_local CStdOutStream *g_StdStream = NULL;
extern thread_local CStdOutStream *g_ErrStream;
thread_local CStdOutStream *g_ErrStream = NULL;

extern int Main2(
  #ifndef _WIN32
  int numArgs, char *args[]
  #endif
);

static void bfp_flush_streams()
{
  if (g_StdStream)
    g_StdStream->Flush();
  if (g_ErrStream)
    g_ErrStream->Flush();
}

static int bfp_call_7z_main2()
{
  NConsoleClose::g_BreakCounter = 0;
  g_StdStream = &g_StdOut;
  g_ErrStream = &g_StdErr;

  int result = NExitCode::kFatalError;

  {
    NConsoleClose::CCtrlHandlerSetter ctrlHandler;

    try
    {
      #ifdef _WIN32
      result = Main2();
      #else
      // POSIX uses bfp_7zr_run_argv() instead
      #endif
    }
    catch (const CNewException &)
    {
      result = NExitCode::kMemoryError;
    }
    catch (const CSystemException &system_error)
    {
      if (g_ErrStream)
      {
        *g_ErrStream << "\n\nSystem ERROR:\n";
        *g_ErrStream << NWindows::NError::MyFormatMessage(system_error.ErrorCode) << "\n";
      }
      result = system_error.ErrorCode == E_OUTOFMEMORY
        ? NExitCode::kMemoryError
        : NExitCode::kFatalError;
    }
    catch (const CMessagePathException &error)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\nCommand Line Error:\n" << error << "\n";
      result = NExitCode::kUserError;
    }
    catch (NExitCode::EEnum exit_code)
    {
      result = exit_code;
    }
    catch (const UString &message)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip ERROR:\n" << message << "\n";
      result = NExitCode::kFatalError;
    }
    catch (const AString &message)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip ERROR:\n" << message << "\n";
      result = NExitCode::kFatalError;
    }
    catch (const char *message)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip ERROR:\n" << message << "\n";
      result = NExitCode::kFatalError;
    }
    catch (const wchar_t *message)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip ERROR:\n" << message << "\n";
      result = NExitCode::kFatalError;
    }
    catch (int value)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip internal error: " << value << "\n";
      result = NExitCode::kFatalError;
    }
    catch (...)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\nUnknown 7-Zip FFI error\n";
      result = NExitCode::kFatalError;
    }
  }

  bfp_flush_streams();
  return result;
}

// ---- Progress callback & cancel (platform-independent) ----

extern "C" void bfp_7zr_set_progress_callback(BfpProgressCb cb)
{
  g_BfpProgressCb = cb;
}

extern "C" void bfp_7zr_set_progress_user_data(void *user_data)
{
  g_BfpProgressUserData = user_data;
}

/// Set the cancel flag pointer for the current 7-Zip invocation.
/// Call before bfp_7zr_run_w / bfp_7zr_run_argv.  Pass a pointer to a
/// non-zero unsigned to signal cancellation; pass NULL to clear.
extern "C" void bfp_7zr_set_cancel_flag(const volatile unsigned *flag)
{
  NConsoleClose::g_BfpCancelFlag = flag;
  NConsoleClose::t_BfpCancelFlag = flag;
}

#ifdef _WIN32

extern thread_local const wchar_t *g_BExplorerCommandLineOverride;

extern "C" int bfp_7zr_run_w(const wchar_t *command_line)
{
  if (!command_line || !*command_line)
    return NExitCode::kUserError;

  g_BExplorerCommandLineOverride = command_line;
  int result = bfp_call_7z_main2();
  g_BExplorerCommandLineOverride = NULL;
  return result;
}

#else

extern "C" int bfp_7zr_run_argv(int argc, const char *const *argv)
{
  if (argc < 1 || !argv)
    return NExitCode::kUserError;

  g_StdStream = &g_StdOut;
  g_ErrStream = &g_StdErr;

  int result = NExitCode::kFatalError;

  {
    NConsoleClose::CCtrlHandlerSetter ctrlHandler;

    try
    {
      // Main2 on POSIX takes char *args[] (mutable), but it only reads.
      // Each arg is copied into AString via MultiByteToUnicodeString,
      // so the cast away from const is safe.
      result = Main2(argc, const_cast<char **>(argv));
    }
    catch (const CNewException &)
    {
      result = NExitCode::kMemoryError;
    }
    catch (const CSystemException &system_error)
    {
      if (g_ErrStream)
      {
        *g_ErrStream << "\n\nSystem ERROR:\n";
        *g_ErrStream << NWindows::NError::MyFormatMessage(system_error.ErrorCode) << "\n";
      }
      result = system_error.ErrorCode == E_OUTOFMEMORY
        ? NExitCode::kMemoryError
        : NExitCode::kFatalError;
    }
    catch (const CMessagePathException &error)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\nCommand Line Error:\n" << error << "\n";
      result = NExitCode::kUserError;
    }
    catch (NExitCode::EEnum exit_code)
    {
      result = exit_code;
    }
    catch (const UString &message)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip ERROR:\n" << message << "\n";
      result = NExitCode::kFatalError;
    }
    catch (const AString &message)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip ERROR:\n" << message << "\n";
      result = NExitCode::kFatalError;
    }
    catch (const char *message)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip ERROR:\n" << message << "\n";
      result = NExitCode::kFatalError;
    }
    catch (const wchar_t *message)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip ERROR:\n" << message << "\n";
      result = NExitCode::kFatalError;
    }
    catch (int value)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\n7-Zip internal error: " << value << "\n";
      result = NExitCode::kFatalError;
    }
    catch (...)
    {
      if (g_ErrStream)
        *g_ErrStream << "\n\nUnknown 7-Zip FFI error\n";
      result = NExitCode::kFatalError;
    }
  }

  bfp_flush_streams();
  return result;
}

#endif

// ---- Archive listing FFI (IInArchive) ----

typedef void (*BfpListEntryCb)(
    const wchar_t *path,
    int isDir,
    UInt64 size,
    int sizeDefined,
    UInt64 packSize,
    int packSizeDefined,
    UInt64 mtimeLow,
    UInt64 mtimeHigh,
    int mtimeDefined,
    void *userData);

extern "C" int bfp_7zr_list_archive(
    const wchar_t *archive_path,
    BfpListEntryCb callback,
    void *userData)
{
  int result = NExitCode::kFatalError;

  // Suppress stdout/stderr during listing
  g_StdStream = NULL;
  g_ErrStream = NULL;

  NConsoleClose::CCtrlHandlerSetter ctrlHandler;

  try
  {
    CREATE_CODECS_OBJECT
    codecs->Load();

    CInFileStream *inFile = new CInFileStream;
    CMyComPtr<IInStream> inStreamRef = inFile;

    if (!inFile->Open(archive_path))
    {
      result = NExitCode::kFatalError;
      return result;
    }

    CArchiveLink archiveLink;
    COpenCallbackConsole openCallback;
    openCallback.Init(NULL, NULL, NULL, true);

    COpenOptions openOptions;
    openOptions.codecs = codecs;
    openOptions.stream = inStreamRef;
    openOptions.filePath = archive_path;

    HRESULT openResult = archiveLink.Open_Strict(openOptions, &openCallback);

    if (openResult != S_OK)
    {
      return (int)openResult;
    }

    IInArchive *archive = archiveLink.GetArchive();
    if (!archive)
      return NExitCode::kFatalError;

    UInt32 numItems = 0;
    if (archive->GetNumberOfItems(&numItems) != S_OK)
      return NExitCode::kFatalError;

    for (UInt32 i = 0; i < numItems; i++)
    {
      UString path;
      archiveLink.GetArc()->GetItem_Path2(i, path);

      bool isDir = false;
      Archive_IsItem_Dir(archive, i, isDir);

      bool sizeDefined = false;
      UInt64 size = 0;
      {
        NWindows::NCOM::CPropVariant sizeProp;
        archive->GetProperty(i, kpidSize, &sizeProp);
        if (sizeProp.vt == VT_UI8 || sizeProp.vt == VT_UI4)
        {
          sizeDefined = true;
          if (sizeProp.vt == VT_UI8)
            size = sizeProp.uhVal.QuadPart;
          else
            size = sizeProp.ulVal;
        }
      }

      bool packSizeDefined = false;
      UInt64 packSize = 0;
      {
        NWindows::NCOM::CPropVariant packProp;
        archive->GetProperty(i, kpidPackSize, &packProp);
        if (packProp.vt == VT_UI8 || packProp.vt == VT_UI4)
        {
          packSizeDefined = true;
          if (packProp.vt == VT_UI8)
            packSize = packProp.uhVal.QuadPart;
          else
            packSize = packProp.ulVal;
        }
      }

      UInt64 mtimeLow = 0;
      UInt64 mtimeHigh = 0;
      int mtimeDefined = 0;
      {
        CArcTime at;
        if (archiveLink.GetArc()->GetItem_MTime(i, at) == S_OK && at.Def)
        {
          mtimeDefined = 1;
          mtimeLow = (UInt64)at.FT.dwLowDateTime;
          mtimeHigh = (UInt64)at.FT.dwHighDateTime;
        }
      }

      if (callback)
        callback(path, isDir ? 1 : 0, size, sizeDefined ? 1 : 0,
                 packSize, packSizeDefined ? 1 : 0,
                 mtimeLow, mtimeHigh, mtimeDefined, userData);
    }

    archiveLink.Close();
    result = NExitCode::kSuccess;
  }
  catch (const CNewException &)
  {
    result = NExitCode::kMemoryError;
  }
  catch (...)
  {
    result = NExitCode::kFatalError;
  }

  return result;
}
