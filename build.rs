use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/windows/bexplorer.ico");
    println!("cargo:rerun-if-changed=assets/windows/bexplorer.rc");
    println!("cargo:rerun-if-changed=vendor/7zip-ffi/bexplorer_7zip.cpp");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/Main.cpp");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/List.cpp");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/ConsoleClose.h");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/ConsoleClose.cpp");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/PercentPrinter.h");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/PercentPrinter.cpp");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/UpdateCallbackConsole.h");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/OpenCallbackConsole.h");
    println!("cargo:rerun-if-changed=vendor/7zip-src/CPP/7zip/UI/Console/ExtractCallbackConsole.h");

    if env::var("CARGO_CFG_WINDOWS").is_ok() {
        compile_windows_resources();
        build_7zip_lib("msvc");
    } else if env::var("CARGO_CFG_UNIX").is_ok() {
        build_7zip_lib("gcc");
    }
}

fn compile_windows_resources() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let resource_dir = root.join("assets").join("windows");
    let rc_path = resource_dir.join("bexplorer.rc");
    let ico_path = resource_dir.join("bexplorer.ico");
    if !rc_path.exists() || !ico_path.exists() {
        println!("cargo:warning=Windows app icon resources are missing; skipping icon embed");
        return;
    }

    let Some(rc_exe) = find_windows_resource_compiler() else {
        println!("cargo:warning=rc.exe was not found; skipping Windows app icon embed");
        return;
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let res_path = out_dir.join("bexplorer.res");
    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_path)
        .current_dir(&resource_dir)
        .status();

    match status {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg={}", res_path.display());
        }
        Ok(status) => {
            println!(
                "cargo:warning=rc.exe failed with status {status}; skipping Windows app icon embed"
            );
        }
        Err(error) => {
            println!(
                "cargo:warning=Failed to run rc.exe ({error}); skipping Windows app icon embed"
            );
        }
    }
}

fn find_windows_resource_compiler() -> Option<PathBuf> {
    find_executable_in_path("rc.exe").or_else(find_windows_sdk_rc)
}

fn find_executable_in_path(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|path| path.join(name))
        .find(|candidate| candidate.is_file())
}

fn find_windows_sdk_rc() -> Option<PathBuf> {
    let program_files_x86 = env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .or_else(|| env::var_os("ProgramFiles").map(PathBuf::from))?;
    let bin_root = program_files_x86
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        _ => "x64",
    };

    let mut versions = fs::read_dir(bin_root)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    versions.sort();
    versions
        .into_iter()
        .rev()
        .map(|version| version.join(arch).join("rc.exe"))
        .find(|candidate| candidate.is_file())
}

fn build_7zip_lib(compiler: &str) {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root_src = root.join("vendor").join("7zip-src");
    let cpp_dir = root_src.join("CPP");
    let c_dir = root_src.join("C");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .include(&cpp_dir)
        .include(&c_dir)
        .include(cpp_dir.join("7zip").join("UI").join("Console"))
        .include(cpp_dir.join("7zip").join("Bundles").join("Format7zF"))
        .include(cpp_dir.join("7zip").join("Bundles").join("Alone7z"))
        .include(root.join("vendor").join("7zip-ffi"));

    // Platform-agnostic defines
    build
        .define("NDEBUG", None)
        .define("_REENTRANT", None)
        .define("_FILE_OFFSET_BITS", "64")
        .define("Z7_PROG_VARIANT_R", None)
        .define("Z7_DEVICE_FILE", None)
        .define("_CONSOLE", None);

    match compiler {
        "msvc" => {
            build
                .static_crt(true)
                .flag("-EHsc")
                .flag("-GR-")
                .flag("-GS-")
                .flag("-Zc:wchar_t")
                .flag("-Zc:throwingNew")
                .define("USE_NO_ASM", "1")
                .define("USE_C_AES", "1")
                .define("USE_C_SHA", "1")
                .define("USE_C_CRC64", "1")
                .define("USE_C_CRC", "1")
                .define("USE_C_SORT", "1")
                .define("USE_C_LZFINDOPT", "1")
                .define("_UNICODE", None)
                .define("UNICODE", None);
        }
        "gcc" => {
            build
                .flag("-std=c++17")
                .flag("-fPIC")
                .flag("-O2")
                .flag("-Wno-unused-parameter")
                .flag("-Wno-unused-variable");
            if cfg!(target_os = "macos") {
                // macOS doesn't use ASM (asmc can't emit Mach-O)
                // pure C fallbacks are the default when USE_ASM is not defined
            }
        }
        _ => {}
    }

    // ------------------------------------------------------------------
    // Source files grouped by directory to match the Format7zF + Console
    // bundles
    // ------------------------------------------------------------------
    let mut cpp_sources: Vec<(&str, &[&str])> = Vec::new();

    macro_rules! s {
        ($dir:expr, $($name:expr),+ $(,)?) => {
            cpp_sources.push(($dir, &[$($name),+]));
        };
    }

    // CPP/Common/
    // Includes Arc.mak COMMON_OBJS + Alone7z/makefile extras
    s!(
        "Common",
        "CRC",
        "CrcReg",
        "DynLimBuf",
        "IntToString",
        "LzFindPrepare",
        "Md5Reg",
        "MyMap",
        "MyString",
        "MyVector",
        "MyXml",
        "MyWindows",
        "NewHandler",
        "Sha1Prepare",
        "Sha1Reg",
        "Sha256Prepare",
        "Sha256Reg",
        "Sha3Reg",
        "Sha512Prepare",
        "Sha512Reg",
        "StringConvert",
        "StringToInt",
        "UTFConvert",
        "Wildcard",
        "Xxh64Reg",
        "XzCrc64Init",
        "XzCrc64Reg",
        // Alone7z makefile additions
        "CommandLineParser",
        "ListFileUtils",
        "StdInStream",
        "StdOutStream",
    );

    // CPP/Windows/ — these have #ifdef _WIN32 / #else branches
    // Synchronization is needed by Format7zF (Arc_gcc.mak MT_OBJS)
    s!(
        "Windows",
        "DLL",
        "ErrorMsg",
        "FileDir",
        "FileFind",
        "FileIO",
        "FileLink",
        "FileName",
        "FileSystem",
        "MemoryLock",
        "PropVariant",
        "PropVariantConv",
        "PropVariantUtils",
        "Registry",
        "SecurityUtils",
        "Synchronization",
        "System",
        "SystemInfo",
        "TimeUtils",
    );

    // CPP/7zip/Common/
    s!(
        "7zip/Common",
        "CreateCoder",
        "CWrappers",
        "FilePathAutoRename",
        "FileStreams",
        "FilterCoder",
        "InBuffer",
        "InOutTempBuffer",
        "LimitedStreams",
        "LockedStream",
        "MemBlocks",
        "MethodId",
        "MethodProps",
        "MultiOutStream",
        "OffsetStream",
        "OutBuffer",
        "OutMemStream",
        "ProgressMt",
        "ProgressUtils",
        "PropId",
        "StreamBinder",
        "StreamObjects",
        "StreamUtils",
        "UniqBlocks",
        "VirtThread",
    );

    // CPP/7zip/Archive/Common/
    s!(
        "7zip/Archive/Common",
        "CoderMixer2",
        "DummyOutStream",
        "FindSignature",
        "HandlerOut",
        "InStreamWithCRC",
        "ItemNameUtils",
        "MultiStream",
        "OutStreamWithCRC",
        "OutStreamWithSha1",
        "ParseProperties",
    );

    // CPP/7zip/Archive/ — individual handler files that live directly
    // in Archive/ (most handlers have their own subdirectory)
    // AR_OBJS files: Apfs, Apm, Ar, Arj, Base64, Bz2, Com, Cpio,
    // Cramfs, DeflateProps, Dmg, Elf, Ext, Fat, Flv, Gz, Gpt,
    // HandlerCont, Hfs, Ihex, Lp, Lzh, Lzma, Macho, Mbr, Mslz,
    // Mub, Ntfs, Pe, Ppmd, Qcow, Rpm, Sparse, Split, Squashfs,
    // Swf, Uefi, Vdi, Vhd, Vhdx, Vmdk, Xar, Xz, Z, Zstd
    s!(
        "7zip/Archive",
        "ApfsHandler",
        "ApmHandler",
        "ArHandler",
        "ArjHandler",
        "Base64Handler",
        "Bz2Handler",
        "ComHandler",
        "CpioHandler",
        "CramfsHandler",
        "DeflateProps",
        "DmgHandler",
        "ElfHandler",
        "ExtHandler",
        "FatHandler",
        "FlvHandler",
        "GzHandler",
        "GptHandler",
        "HandlerCont",
        "HfsHandler",
        "IhexHandler",
        "LpHandler",
        "LzhHandler",
        "LzmaHandler",
        "MachoHandler",
        "MbrHandler",
        "MslzHandler",
        "MubHandler",
        "NtfsHandler",
        "PeHandler",
        "PpmdHandler",
        "QcowHandler",
        "RpmHandler",
        "SparseHandler",
        "SplitHandler",
        "SquashfsHandler",
        "SwfHandler",
        "UefiHandler",
        "VdiHandler",
        "VhdHandler",
        "VhdxHandler",
        "VmdkHandler",
        "XarHandler",
        "XzHandler",
        "ZHandler",
        "ZstdHandler",
    );

    // CPP/7zip/Archive/7z/
    s!(
        "7zip/Archive/7z",
        "7zCompressionMode",
        "7zDecode",
        "7zEncode",
        "7zExtract",
        "7zFolderInStream",
        "7zHandler",
        "7zHandlerOut",
        "7zHeader",
        "7zIn",
        "7zOut",
        "7zProperties",
        "7zRegister",
        "7zSpecStream",
        "7zUpdate",
    );

    // CPP/7zip/Archive/Cab/
    s!(
        "7zip/Archive/Cab",
        "CabBlockInStream",
        "CabHandler",
        "CabHeader",
        "CabIn",
        "CabRegister",
    );

    // CPP/7zip/Archive/Chm/
    s!("7zip/Archive/Chm", "ChmHandler", "ChmIn",);

    // CPP/7zip/Archive/Iso/
    s!(
        "7zip/Archive/Iso",
        "IsoHandler",
        "IsoHeader",
        "IsoIn",
        "IsoRegister",
    );

    // CPP/7zip/Archive/Nsis/
    s!(
        "7zip/Archive/Nsis",
        "NsisDecode",
        "NsisHandler",
        "NsisIn",
        "NsisRegister",
    );

    // CPP/7zip/Archive/Rar/
    s!("7zip/Archive/Rar", "RarHandler", "Rar5Handler",);

    // CPP/7zip/Archive/Tar/
    s!(
        "7zip/Archive/Tar",
        "TarHandler",
        "TarHandlerOut",
        "TarHeader",
        "TarIn",
        "TarOut",
        "TarUpdate",
        "TarRegister",
    );

    // CPP/7zip/Archive/Udf/
    s!("7zip/Archive/Udf", "UdfHandler", "UdfIn",);

    // CPP/7zip/Archive/Wim/
    s!(
        "7zip/Archive/Wim",
        "WimHandler",
        "WimHandlerOut",
        "WimIn",
        "WimRegister",
    );

    // CPP/7zip/Archive/Zip/
    s!(
        "7zip/Archive/Zip",
        "ZipAddCommon",
        "ZipHandler",
        "ZipHandlerOut",
        "ZipIn",
        "ZipItem",
        "ZipOut",
        "ZipUpdate",
        "ZipRegister",
    );

    // CPP/7zip/Compress/
    s!(
        "7zip/Compress",
        "Bcj2Coder",
        "Bcj2Register",
        "BcjCoder",
        "BcjRegister",
        "BitlDecoder",
        "BranchMisc",
        "BranchRegister",
        "ByteSwap",
        "BZip2Crc",
        "BZip2Decoder",
        "BZip2Encoder",
        "BZip2Register",
        "CopyCoder",
        "CopyRegister",
        "Deflate64Register",
        "DeflateDecoder",
        "DeflateEncoder",
        "DeflateRegister",
        "DeltaFilter",
        "ImplodeDecoder",
        "LzfseDecoder",
        "LzhDecoder",
        "Lzma2Decoder",
        "Lzma2Encoder",
        "Lzma2Register",
        "LzmaDecoder",
        "LzmaEncoder",
        "LzmaRegister",
        "LzmsDecoder",
        "LzOutWindow",
        "LzxDecoder",
        "PpmdDecoder",
        "PpmdEncoder",
        "PpmdRegister",
        "PpmdZip",
        "QuantumDecoder",
        "Rar1Decoder",
        "Rar2Decoder",
        "Rar3Decoder",
        "Rar3Vm",
        "Rar5Decoder",
        "RarCodecsRegister",
        "ShrinkDecoder",
        "XpressDecoder",
        "XzDecoder",
        "XzEncoder",
        "ZlibDecoder",
        "ZlibEncoder",
        "ZDecoder",
        "ZstdDecoder",
    );

    // CPP/7zip/Crypto/
    s!(
        "7zip/Crypto",
        "7zAes",
        "7zAesRegister",
        "HmacSha1",
        "HmacSha256",
        "MyAes",
        "MyAesReg",
        "Pbkdf2HmacSha1",
        "RandGen",
        "Rar20Crypto",
        "Rar5Aes",
        "RarAes",
        "WzAes",
        "ZipCrypto",
        "ZipStrong",
    );

    // C/ (C sources — compiled as C++ which is fine for 7-Zip)
    s!(
        "../C",
        "7zBuf2",
        "7zCrc",
        "7zCrcOpt",
        "7zStream",
        "Alloc",
        "Bcj2",
        "Bcj2Enc",
        "Blake2s",
        "Bra",
        "Bra86",
        "BraIA64",
        "BwtSort",
        "CpuArch",
        "Delta",
        "HuffEnc",
        "LzFind",
        "LzFindMt",
        "LzFindOpt",
        "Lzma2Dec",
        "Lzma2DecMt",
        "Lzma2Enc",
        "LzmaDec",
        "LzmaEnc",
        "Md5",
        "MtCoder",
        "MtDec",
        "Ppmd7",
        "Ppmd7Dec",
        "Ppmd7aDec",
        "Ppmd7Enc",
        "Ppmd8",
        "Ppmd8Dec",
        "Ppmd8Enc",
        "Sha1",
        "Sha1Opt",
        "Sha256",
        "Sha256Opt",
        "Sha3",
        "Sha512",
        "Sha512Opt",
        "Sort",
        "SwapBytes",
        "Threads",
        "Xxh64",
        "Xz",
        "XzDec",
        "XzEnc",
        "XzIn",
        "XzCrc64",
        "XzCrc64Opt",
        "ZstdDec",
        "Aes",
        "AesOpt",
        "DllSecur",
    );

    // ---- Console UI (exclude MainAr.cpp — has main()) ----
    // CONSOLE_OBJS from Console.mak
    s!(
        "7zip/UI/Console",
        "BenchCon",
        "ConsoleClose",
        "ExtractCallbackConsole",
        "HashCon",
        "List",
        "Main",
        "OpenCallbackConsole",
        "PercentPrinter",
        "UpdateCallbackConsole",
        "UserInputUtils",
    );

    // UI_COMMON_OBJS from Console.mak
    s!(
        "7zip/UI/Common",
        "ArchiveCommandLine",
        "ArchiveExtractCallback",
        "ArchiveOpenCallback",
        "Bench",
        "DefaultName",
        "EnumDirItems",
        "Extract",
        "ExtractingFilePath",
        "HashCalc",
        "LoadCodecs",
        "OpenArchive",
        "PropIDUtils",
        "SetProperties",
        "SortUtils",
        "TempFiles",
        "Update",
        "UpdateAction",
        "UpdateCallback",
        "UpdatePair",
        "UpdateProduce",
    );

    // ---- FFI wrapper ----
    build.file(
        root.join("vendor")
            .join("7zip-ffi")
            .join("bexplorer_7zip.cpp"),
    );

    for (rel_dir, names) in &cpp_sources {
        for name in *names {
            let mut path = cpp_dir.clone();
            for part in rel_dir.split('/') {
                path.push(part);
            }
            path.push(name);
            path.set_extension("cpp");
            if path.exists() {
                build.file(&path);
            } else {
                // Try .c extension for C/ sources
                path.set_extension("c");
                if path.exists() {
                    build.file(&path);
                } else {
                    panic!("Source file not found: {}/{}", rel_dir, name);
                }
            }
        }
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    // ---- Header-hash cache invalidation ----
    // The cc crate does NOT track header dependencies; it only checks
    // whether each .cpp file has changed.  When a tracked header is
    // modified we must nuke the cached artefacts so that every .cpp
    // that includes it gets recompiled with the new declarations.
    invalidate_cc_cache_if_headers_changed(&out_dir, &root);

    build.compile("bfp7z");

    // Whole-archive: ensure static initializers (REGISTER_ARC, etc.)
    // are not dropped by the linker
    if compiler == "msvc" {
        println!(
            "cargo:rustc-link-arg=/WHOLEARCHIVE:{}",
            out_dir.join(format!("{}.lib", "bfp7z")).display()
        );
        println!("cargo:rustc-link-lib=oleaut32");
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=user32");
        println!("cargo:rustc-link-lib=advapi32");
        println!("cargo:rustc-link-lib=shell32");
        println!("cargo:rustc-link-lib=uuid");
        println!("cargo:rustc-link-lib=crypt32");
        println!("cargo:rustc-link-lib=version");
        println!("cargo:rustc-link-lib=bcrypt");
        println!("cargo:rustc-link-lib=ntdll");
        println!("cargo:rustc-link-lib=mpr");
        println!("cargo:rustc-link-lib=imm32");
        println!("cargo:rustc-link-lib=wbemuuid");
        println!("cargo:rustc-link-lib=credui");
        println!("cargo:rustc-link-lib=comctl32");
    } else {
        println!("cargo:rustc-link-arg=-Wl,--whole-archive");
        // cc crate already emitted `cargo:rustc-link-lib=static=bfp7z`
        println!("cargo:rustc-link-arg=-Wl,--no-whole-archive");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=m");
    }
}

/// The cc crate only tracks .cpp timestamps.  If a header changes, the
/// stale object files that include it are silently reused, leading to
/// mismatched thread_local / extern declarations.  This function
/// computes a content hash of every header that is known to carry
/// C++ `thread_local` declarations and nukes the cc crate's cache
/// whenever that hash changes, guaranteeing a full recompilation.
fn invalidate_cc_cache_if_headers_changed(out_dir: &std::path::Path, root: &std::path::Path) {
    let tracked = [
        "PercentPrinter.h",
        "ConsoleClose.h",
        "UpdateCallbackConsole.h",
        "ExtractCallbackConsole.h",
        "OpenCallbackConsole.h",
    ];
    let hash_path = out_dir.join("bfp7z_header_hash.txt");

    let mut hasher = DefaultHasher::new();
    for name in &tracked {
        let path = root
            .join("vendor")
            .join("7zip-src")
            .join("CPP")
            .join("7zip")
            .join("UI")
            .join("Console")
            .join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            content.hash(&mut hasher);
        }
    }
    let cur = hasher.finish();

    let prev = fs::read_to_string(&hash_path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);

    if cur == prev {
        return;
    }

    let lib_name = if cfg!(windows) {
        "bfp7z.lib"
    } else {
        "libbfp7z.a"
    };
    let _ = fs::remove_file(out_dir.join(lib_name));
    let _ = fs::remove_dir_all(out_dir.join(".fingerprint"));
    if let Ok(entries) = fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .map(|e| e == "o" || e == "obj")
                .unwrap_or(false)
            {
                let _ = fs::remove_file(&path);
            }
        }
    }
    if let Err(e) = fs::write(&hash_path, cur.to_string()) {
        panic!("Failed to write header hash: {e}");
    }
}
