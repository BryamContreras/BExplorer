//! Extraction writes are relative to an opened directory, never to a checked
//! path that can subsequently be redirected through a link.
use super::*;

pub(super) struct ExtractionRoot {
    path: PathBuf,
    directory: File,
    #[cfg(windows)]
    _ancestors: Vec<File>,
}

pub(super) struct OutputFile {
    file: File,
    // Windows directory handles deny both replacement and reparse-point edits
    // until the output file is closed.
    #[cfg(windows)]
    _parents: Vec<File>,
}

impl Write for OutputFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn unsafe_path(name: &str) -> BExplorerError {
    BExplorerError::Operation(format!("Unsafe archive path blocked: {name}"))
}

fn parts(name: &str) -> Result<Vec<String>> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains('\0') {
        return Err(unsafe_path(name));
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".."
            || (parts.is_empty()
                && part.len() == 2
                && part.as_bytes()[0].is_ascii_alphabetic()
                && part.ends_with(':'))
        {
            return Err(unsafe_path(name));
        }
        #[cfg(windows)]
        {
            // Win32 silently normalizes these spellings; device names can
            // address objects outside the destination rather than files.
            let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            if part.contains(':')
                || part.ends_with([' ', '.'])
                || matches!(
                    base.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                || base
                    .strip_prefix("COM")
                    .or_else(|| base.strip_prefix("LPT"))
                    .is_some_and(|suffix| {
                        matches!(
                            suffix,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
            {
                return Err(unsafe_path(name));
            }
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

impl ExtractionRoot {
    pub(super) fn new(path: &Path) -> Result<Self> {
        // The user may intentionally choose an alias (a mounted home, a
        // redirected Documents folder, or a symlink). Resolve that choice once,
        // then open and pin the physical path without following further links.
        // Archive entry names are always resolved beneath that pinned root.
        let absolute = physical_destination(path)?;
        #[cfg(windows)]
        {
            let mut current = PathBuf::new();
            let mut ancestors = Vec::new();
            for component in absolute.components() {
                match component {
                    Component::Prefix(_) | Component::RootDir => {
                        current.push(component.as_os_str())
                    }
                    Component::CurDir => continue,
                    Component::ParentDir => return Err(unsafe_path(&path.display().to_string())),
                    Component::Normal(_) => {
                        current.push(component.as_os_str());
                        match fs::create_dir(&current) {
                            Ok(()) => (),
                            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                            Err(error) => return Err(error.into()),
                        }
                        ancestors.push(open_directory(&current)?);
                    }
                }
            }
            let directory = open_directory(&absolute)?;
            Ok(Self {
                path: absolute,
                directory,
                _ancestors: ancestors,
            })
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut directory = open_directory(Path::new("/"))?;
            for component in absolute.components() {
                match component {
                    Component::RootDir | Component::CurDir => (),
                    Component::Normal(name) => directory = child_directory(&directory, name)?,
                    _ => return Err(unsafe_path(&path.display().to_string())),
                }
            }
            Ok(Self {
                path: absolute,
                directory,
            })
        }
        #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
        {
            let _ = absolute;
            Err(BExplorerError::Operation(
                "Secure archive extraction is unavailable on this platform".into(),
            ))
        }
    }

    pub(super) fn create_directory(&self, name: &str) -> Result<()> {
        self.parents(&parts(name)?)?;
        Ok(())
    }

    pub(super) fn create_file(&self, name: &str) -> Result<OutputFile> {
        let components = parts(name)?;
        let (name, directories) = components.split_last().ok_or_else(|| unsafe_path(name))?;
        #[allow(unused_mut)]
        let (_parent, _parent_path, mut _guards) = self.parents(directories)?;
        for index in 0..10_000 {
            let candidate = if index == 0 {
                name.clone()
            } else {
                let path = Path::new(name);
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                match path.extension() {
                    Some(extension) => format!("{stem} ({index}).{}", extension.to_string_lossy()),
                    None => format!("{stem} ({index})"),
                }
            };
            #[cfg(windows)]
            let result = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(_parent_path.join(&candidate));
            #[cfg(all(unix, not(target_os = "macos")))]
            let result = rustix::fs::openat(
                &_parent,
                candidate.as_str(),
                rustix::fs::OFlags::WRONLY
                    | rustix::fs::OFlags::CREATE
                    | rustix::fs::OFlags::EXCL
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::from_raw_mode(0o600),
            )
            .map(File::from)
            .map_err(io::Error::from);
            #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
            let result: io::Result<File> = Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Secure extraction unavailable",
            ));
            match result {
                Ok(file) => {
                    #[cfg(windows)]
                    _guards.push(_parent);
                    return Ok(OutputFile {
                        file,
                        #[cfg(windows)]
                        _parents: _guards,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(BExplorerError::Operation(
            "Too many extraction name conflicts".into(),
        ))
    }

    fn parents(&self, components: &[String]) -> Result<(File, PathBuf, Vec<File>)> {
        let mut parent = self.directory.try_clone()?;
        let mut path = self.path.clone();
        #[allow(unused_mut)]
        let mut guards = Vec::new();
        for component in components {
            path.push(component);
            #[cfg(windows)]
            {
                match fs::create_dir(&path) {
                    Ok(()) => (),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                    Err(error) => return Err(error.into()),
                }
                guards.push(parent);
                parent = open_directory(&path)?;
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                parent = child_directory(&parent, OsStr::new(component))?;
            }
        }
        Ok((parent, path, guards))
    }
}

fn physical_destination(path: &Path) -> Result<PathBuf> {
    let mut existing = std::path::absolute(path)?;
    let mut missing = Vec::new();
    loop {
        match fs::canonicalize(&existing) {
            Ok(mut physical) => {
                for name in missing.iter().rev() {
                    physical.push(name);
                }
                return Ok(physical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing
                    .file_name()
                    .ok_or_else(|| BExplorerError::InvalidPath(path.to_path_buf()))?
                    .to_os_string();
                missing.push(name);
                if !existing.pop() {
                    return Err(BExplorerError::InvalidPath(path.to_path_buf()));
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(windows)]
fn open_directory(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    let file = fs::OpenOptions::new()
        .access_mode(0x80) // FILE_READ_ATTRIBUTES
        .share_mode(1) // FILE_SHARE_READ; deny writes, removal and rename
        .custom_flags(0x0200_0000 | 0x0020_0000) // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Archive destination contains a reparse point or non-directory",
        ));
    }
    Ok(file)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_directory(path: &Path) -> io::Result<File> {
    rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn child_directory(parent: &File, name: &OsStr) -> io::Result<File> {
    match rustix::fs::mkdirat(parent, name, rustix::fs::Mode::from_raw_mode(0o700)) {
        Ok(()) => (),
        Err(error) if error == rustix::io::Errno::EXIST => (),
        Err(error) => return Err(error.into()),
    }
    rustix::fs::openat(
        parent,
        name,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)
}

pub(super) fn open_source(path: &Path) -> Result<File> {
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(path)?
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let file = File::from(
        rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map_err(io::Error::from)?,
    );
    #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(unsafe_path(&path.display().to_string()));
        }
    }
    if !metadata.is_file() {
        return Err(unsafe_path(&path.display().to_string()));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names_reject_cross_platform_traversal() {
        for name in [
            "../escape",
            r"..\escape",
            "/absolute",
            r"C:\escape",
            "a/../../escape",
        ] {
            assert!(parts(name).is_err(), "{name}");
        }
        #[cfg(windows)]
        assert!(parts("file:stream").is_err());
        #[cfg(unix)]
        assert_eq!(parts("file:stream").unwrap(), ["file:stream"]);
        assert_eq!(parts("./nested/file.txt").unwrap(), ["nested", "file.txt"]);
    }

    #[test]
    fn output_creation_preserves_existing_files_and_uses_unique_names() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        fs::write(staging.path().join("same.txt"), b"existing").unwrap();
        let root = ExtractionRoot::new(staging.path()).unwrap();
        root.create_file("same.txt")
            .unwrap()
            .write_all(b"new")
            .unwrap();
        root.create_file("nested/item.txt")
            .unwrap()
            .write_all(b"nested")
            .unwrap();
        assert_eq!(
            fs::read(staging.path().join("same.txt")).unwrap(),
            b"existing"
        );
        assert_eq!(
            fs::read(staging.path().join("same (1).txt")).unwrap(),
            b"new"
        );
        assert_eq!(
            fs::read(staging.path().join("nested/item.txt")).unwrap(),
            b"nested"
        );
    }

    #[cfg(windows)]
    #[test]
    fn opened_output_directories_cannot_be_replaced_while_writing() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let root = ExtractionRoot::new(staging.path()).unwrap();
        let mut output = root.create_file("nested/item.txt").unwrap();
        assert!(fs::rename(staging.path().join("nested"), staging.path().join("moved")).is_err());
        output.write_all(b"safe").unwrap();
        drop(output);
        fs::rename(staging.path().join("nested"), staging.path().join("moved")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn explicitly_selected_junction_is_resolved_but_nested_junction_is_blocked() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let physical = staging.path().join("physical");
        let outside = staging.path().join("outside");
        fs::create_dir(&physical).unwrap();
        fs::create_dir(&outside).unwrap();
        let alias = staging.path().join("alias");
        let nested = physical.join("nested-link");
        for (link, target) in [(&alias, &physical), (&nested, &outside)] {
            let status = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command",
                "New-Item -ItemType Junction -Path $env:BEXPLORER_TEST_LINK -Target $env:BEXPLORER_TEST_TARGET -ErrorAction Stop | Out-Null"])
                .env("BEXPLORER_TEST_LINK", link).env("BEXPLORER_TEST_TARGET", target)
                .creation_flags(CREATE_NO_WINDOW).status().unwrap();
            assert!(status.success());
        }
        let root = ExtractionRoot::new(&alias).unwrap();
        root.create_file("file.txt")
            .unwrap()
            .write_all(b"explicit destination")
            .unwrap();
        assert!(root.create_file("nested-link/escape.txt").is_err());
        assert_eq!(
            fs::read(physical.join("file.txt")).unwrap(),
            b"explicit destination"
        );
        assert!(!outside.join("escape.txt").exists());
        drop(root);
        fs::remove_dir(&nested).unwrap();
        fs::remove_dir(&alias).unwrap();
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn output_does_not_follow_preexisting_directory_or_file_symlinks() {
        use std::os::unix::fs::symlink;
        let staging = TemporaryExtractDirectory::new().unwrap();
        fs::create_dir(staging.path().join("out")).unwrap();
        fs::create_dir(staging.path().join("outside")).unwrap();
        symlink(
            staging.path().join("outside"),
            staging.path().join("out/link"),
        )
        .unwrap();
        symlink(
            staging.path().join("outside/file.txt"),
            staging.path().join("out/file.txt"),
        )
        .unwrap();
        let root = ExtractionRoot::new(&staging.path().join("out")).unwrap();
        assert!(root.create_file("link/escape.txt").is_err());
        root.create_file("file.txt")
            .unwrap()
            .write_all(b"safe")
            .unwrap();
        assert!(!staging.path().join("outside/escape.txt").exists());
        assert!(!staging.path().join("outside/file.txt").exists());
        assert_eq!(
            fs::read(staging.path().join("out/file (1).txt")).unwrap(),
            b"safe"
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn explicitly_selected_symlink_destination_is_resolved_before_pinning() {
        use std::os::unix::fs::symlink;
        let staging = TemporaryExtractDirectory::new().unwrap();
        let physical = staging.path().join("physical");
        fs::create_dir(&physical).unwrap();
        let alias = staging.path().join("alias");
        symlink(&physical, &alias).unwrap();
        let root = ExtractionRoot::new(&alias.join("new-subdirectory")).unwrap();
        root.create_file("file.txt")
            .unwrap()
            .write_all(b"explicit destination")
            .unwrap();
        assert_eq!(
            fs::read(physical.join("new-subdirectory/file.txt")).unwrap(),
            b"explicit destination"
        );
    }
}
