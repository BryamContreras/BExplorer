use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime};

use walkdir::WalkDir;

use crate::fs::explorer::{self, FileCategory};

const SNAPSHOT_FILE_INTERVAL: usize = 256;
const SNAPSHOT_TIME_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(usize)]
pub enum StorageCategory {
    Documents,
    Images,
    Videos,
    Audio,
    Archives,
    WindowsExecutables,
    LinuxPackages,
    MacOsPackages,
    OtherApplications,
    Databases,
    Backups,
    DiskImages,
    VirtualMachines,
    SystemFiles,
    Other,
}

const STORAGE_CATEGORY_COUNT: usize = StorageCategory::Other as usize + 1;

impl StorageCategory {
    pub const ALL: [Self; STORAGE_CATEGORY_COUNT] = [
        Self::Documents,
        Self::Images,
        Self::Videos,
        Self::Audio,
        Self::Archives,
        Self::WindowsExecutables,
        Self::LinuxPackages,
        Self::MacOsPackages,
        Self::OtherApplications,
        Self::Databases,
        Self::Backups,
        Self::DiskImages,
        Self::VirtualMachines,
        Self::SystemFiles,
        Self::Other,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StorageCategoryUsage {
    pub bytes: u64,
    pub files: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub struct StorageFile {
    pub path: PathBuf,
    pub size: u64,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct StorageFiles {
    by_category: [Vec<StorageFile>; STORAGE_CATEGORY_COUNT],
}

impl Default for StorageFiles {
    fn default() -> Self {
        Self {
            by_category: std::array::from_fn(|_| Vec::new()),
        }
    }
}

impl StorageFiles {
    pub fn get(&self, category: StorageCategory) -> &[StorageFile] {
        &self.by_category[category.index()]
    }

    pub fn get_mut(&mut self, category: StorageCategory) -> &mut Vec<StorageFile> {
        &mut self.by_category[category.index()]
    }

    pub fn total_files(&self) -> usize {
        self.by_category.iter().map(Vec::len).sum()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageAnalysisSummary {
    bytes: [u64; STORAGE_CATEGORY_COUNT],
    files: [u64; STORAGE_CATEGORY_COUNT],
    pub total_bytes: u64,
    pub total_files: u64,
}

impl Default for StorageAnalysisSummary {
    fn default() -> Self {
        Self {
            bytes: [0; STORAGE_CATEGORY_COUNT],
            files: [0; STORAGE_CATEGORY_COUNT],
            total_bytes: 0,
            total_files: 0,
        }
    }
}

impl StorageAnalysisSummary {
    pub fn usage(&self, category: StorageCategory) -> StorageCategoryUsage {
        StorageCategoryUsage {
            bytes: self.bytes[category.index()],
            files: self.files[category.index()],
        }
    }

    pub(crate) fn add_file(&mut self, category: StorageCategory, size: u64) {
        let index = category.index();
        self.bytes[index] = self.bytes[index].saturating_add(size);
        self.files[index] = self.files[index].saturating_add(1);
        self.total_bytes = self.total_bytes.saturating_add(size);
        self.total_files = self.total_files.saturating_add(1);
    }

    #[cfg(test)]
    pub(crate) fn add_file_for_test(&mut self, category: StorageCategory, size: u64) {
        self.add_file(category, size);
    }
}

#[derive(Debug)]
pub enum StorageAnalysisEvent {
    Scanning {
        scanned: usize,
        current_path: Option<PathBuf>,
        summary: StorageAnalysisSummary,
        skipped: usize,
    },
    Finished {
        summary: StorageAnalysisSummary,
        files: StorageFiles,
        skipped: usize,
    },
    Cancelled {
        summary: StorageAnalysisSummary,
        files: StorageFiles,
        skipped: usize,
    },
    Failed(String),
}

/// Recursively analyzes one mounted directory without following nested links,
/// opening archive containers, or crossing into another mounted filesystem. A
/// linked root is followed so the folder selected by the user is analyzed.
pub fn scan_folder(root: PathBuf, sender: Sender<StorageAnalysisEvent>, cancel: Arc<AtomicBool>) {
    if !root.is_dir() {
        let _ = sender.send(StorageAnalysisEvent::Failed(format!(
            "{} is not a directory",
            root.display()
        )));
        return;
    }

    let mut summary = StorageAnalysisSummary::default();
    let mut files = StorageFiles::default();
    let mut scanned = 0_usize;
    let mut skipped = 0_usize;
    let mut last_snapshot = Instant::now();

    for entry in WalkDir::new(&root)
        .follow_links(false)
        .follow_root_links(true)
        .same_file_system(true)
    {
        if cancel.load(Ordering::Relaxed) {
            send_cancelled(&sender, summary, files, skipped);
            return;
        }
        let entry = match entry {
            Ok(entry) if entry.file_type().is_file() => entry,
            Ok(_) => continue,
            Err(error) if error.depth() == 0 => {
                let _ = sender.send(StorageAnalysisEvent::Failed(error.to_string()));
                return;
            }
            Err(_) => {
                skipped = skipped.saturating_add(1);
                continue;
            }
        };
        let path = entry.into_path();
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                skipped = skipped.saturating_add(1);
                continue;
            }
        };
        let category = classify_storage_category(&path);
        let size = metadata.len();
        summary.add_file(category, size);
        scanned = scanned.saturating_add(1);

        let should_publish = scanned.is_multiple_of(SNAPSHOT_FILE_INTERVAL)
            || last_snapshot.elapsed() >= SNAPSHOT_TIME_INTERVAL;
        let current_path = should_publish.then(|| path.clone());
        files.get_mut(category).push(StorageFile {
            path,
            size,
            created: metadata.created().ok(),
            modified: metadata.modified().ok(),
        });

        if should_publish {
            let _ = sender.send(StorageAnalysisEvent::Scanning {
                scanned,
                current_path,
                summary: summary.clone(),
                skipped,
            });
            last_snapshot = Instant::now();
        }
    }

    let _ = sender.send(StorageAnalysisEvent::Scanning {
        scanned,
        current_path: None,
        summary: summary.clone(),
        skipped,
    });
    if cancel.load(Ordering::Relaxed) {
        send_cancelled(&sender, summary, files, skipped);
        return;
    }
    let _ = sender.send(StorageAnalysisEvent::Finished {
        summary,
        files,
        skipped,
    });
}

pub fn classify_storage_category(path: &Path) -> StorageCategory {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if has_any_suffix(
        &name,
        &[
            ".vhd",
            ".vhdx",
            ".vhdset",
            ".avhd",
            ".avhdx",
            ".vmdk",
            ".vdi",
            ".qcow",
            ".qcow2",
            ".hdd",
            ".ova",
            ".ovf",
            ".vmx",
            ".vmxf",
            ".vbox",
            ".vbox-prev",
            ".nvram",
        ],
    ) {
        return StorageCategory::VirtualMachines;
    }
    if has_any_suffix(
        &name,
        &[
            ".iso",
            ".img",
            ".dmg",
            ".sparseimage",
            ".toast",
            ".wim",
            ".esd",
        ],
    ) {
        return StorageCategory::DiskImages;
    }
    if has_any_suffix(
        &name,
        &[
            ".exe",
            ".msi",
            ".msix",
            ".msixbundle",
            ".appx",
            ".appxbundle",
            ".appinstaller",
            ".msp",
            ".com",
            ".scr",
            ".bat",
            ".cmd",
            ".ps1",
            ".dll",
            ".ocx",
            ".cpl",
            ".msu",
        ],
    ) {
        return StorageCategory::WindowsExecutables;
    }
    if has_any_suffix(
        &name,
        &[
            ".deb",
            ".rpm",
            ".appimage",
            ".snap",
            ".flatpak",
            ".flatpakref",
            ".flatpakrepo",
            ".run",
            ".pkg.tar.zst",
            ".pkg.tar.xz",
            ".pkg.tar.gz",
        ],
    ) {
        return StorageCategory::LinuxPackages;
    }
    if has_any_suffix(&name, &[".pkg", ".mpkg", ".xip"]) {
        return StorageCategory::MacOsPackages;
    }
    if has_any_suffix(
        &name,
        &[
            ".apk", ".aab", ".apks", ".xapk", ".ipa", ".jar", ".war", ".ear",
        ],
    ) {
        return StorageCategory::OtherApplications;
    }
    if has_any_suffix(
        &name,
        &[
            ".db",
            ".db3",
            ".sqlite",
            ".sqlite3",
            ".mdb",
            ".accdb",
            ".dbf",
            ".parquet",
            ".feather",
            ".duckdb",
            ".realm",
            ".sql",
            ".sqlitedb",
            ".mdf",
            ".ndf",
            ".ldf",
            ".ibd",
        ],
    ) {
        return StorageCategory::Databases;
    }
    if has_any_suffix(
        &name,
        &[
            ".bak", ".backup", ".bkp", ".bkf", ".old", ".orig", ".dump", ".gho", ".tib", ".tibx",
            ".mrimg", ".fbk", ".nbk",
        ],
    ) {
        return StorageCategory::Backups;
    }

    match explorer::classify_file_category(path) {
        FileCategory::Image => StorageCategory::Images,
        FileCategory::Audio => StorageCategory::Audio,
        FileCategory::Video => StorageCategory::Videos,
        FileCategory::Archive => StorageCategory::Archives,
        FileCategory::Document | FileCategory::Spreadsheet | FileCategory::Presentation => {
            StorageCategory::Documents
        }
        FileCategory::Code => StorageCategory::Other,
        FileCategory::DiskImage => StorageCategory::DiskImages,
        FileCategory::System | FileCategory::Font => StorageCategory::SystemFiles,
        FileCategory::Application => StorageCategory::OtherApplications,
        FileCategory::Other => StorageCategory::Other,
    }
}

fn has_any_suffix(name: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|suffix| name.ends_with(suffix))
}

fn send_cancelled(
    sender: &Sender<StorageAnalysisEvent>,
    summary: StorageAnalysisSummary,
    files: StorageFiles,
    skipped: usize,
) {
    let _ = sender.send(StorageAnalysisEvent::Cancelled {
        summary,
        files,
        skipped,
    });
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use flate2::Compression;
    use flate2::GzBuilder;

    use super::*;

    fn temporary_folder() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bexplorer-storage-analysis-test-{}-{unique}",
            std::process::id()
        ))
    }

    fn finished_result(root: PathBuf) -> (StorageAnalysisSummary, StorageFiles) {
        let (sender, receiver) = mpsc::channel();
        scan_folder(root, sender, Arc::new(AtomicBool::new(false)));
        receiver
            .into_iter()
            .find_map(|event| match event {
                StorageAnalysisEvent::Finished { summary, files, .. } => Some((summary, files)),
                _ => None,
            })
            .expect("finished storage analysis")
    }

    fn assert_summary_matches_files(summary: &StorageAnalysisSummary, files: &StorageFiles) {
        assert_eq!(
            StorageCategory::ALL
                .into_iter()
                .map(|category| summary.usage(category).bytes)
                .sum::<u64>(),
            summary.total_bytes
        );
        assert_eq!(
            StorageCategory::ALL
                .into_iter()
                .map(|category| summary.usage(category).files)
                .sum::<u64>(),
            summary.total_files
        );
        assert_eq!(files.total_files() as u64, summary.total_files);
        for category in StorageCategory::ALL {
            let category_files = files.get(category);
            let usage = summary.usage(category);
            assert_eq!(category_files.len() as u64, usage.files);
            assert_eq!(
                category_files.iter().map(|file| file.size).sum::<u64>(),
                usage.bytes
            );
        }
    }

    #[test]
    fn storage_categories_distinguish_platform_packages_and_common_media() {
        assert_eq!(
            classify_storage_category(Path::new("setup.MSIX")),
            StorageCategory::WindowsExecutables
        );
        assert_eq!(
            classify_storage_category(Path::new("package.pkg.tar.zst")),
            StorageCategory::LinuxPackages
        );
        assert_eq!(
            classify_storage_category(Path::new("installer.pkg")),
            StorageCategory::MacOsPackages
        );
        assert_eq!(
            classify_storage_category(Path::new("developer-tools.xip")),
            StorageCategory::MacOsPackages
        );
        assert_eq!(
            classify_storage_category(Path::new("report.xlsx")),
            StorageCategory::Documents
        );
        assert_eq!(
            classify_storage_category(Path::new("photo.avif")),
            StorageCategory::Images
        );
        assert_eq!(
            classify_storage_category(Path::new("catalog.sqlite3")),
            StorageCategory::Databases
        );
        assert_eq!(
            classify_storage_category(Path::new("warehouse.duckdb")),
            StorageCategory::Databases
        );
        assert_eq!(
            classify_storage_category(Path::new("project.backup")),
            StorageCategory::Backups
        );
        assert_eq!(
            classify_storage_category(Path::new("machine.qcow2")),
            StorageCategory::VirtualMachines
        );
        assert_eq!(
            classify_storage_category(Path::new("appliance.ova")),
            StorageCategory::VirtualMachines
        );
        assert_eq!(
            classify_storage_category(Path::new("main.rs")),
            StorageCategory::Other
        );
        assert_eq!(
            classify_storage_category(Path::new("system.wim")),
            StorageCategory::DiskImages
        );
        assert_eq!(
            classify_storage_category(Path::new("installer.dmg")),
            StorageCategory::DiskImages
        );
        assert_eq!(
            classify_storage_category(Path::new("driver.msu")),
            StorageCategory::WindowsExecutables
        );
    }

    #[test]
    fn equal_files_remain_in_their_storage_category() {
        let root = temporary_folder();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("create nested test folder");
        let original = root.join("original.txt");
        let copy = nested.join("copy.txt");
        fs::write(&original, b"abc").expect("write original");
        fs::write(&copy, b"abc").expect("write copy");

        let (summary, files) = finished_result(root.clone());
        assert_eq!(summary.total_bytes, 6);
        assert_eq!(summary.total_files, 2);
        assert_eq!(
            summary.usage(StorageCategory::Documents),
            StorageCategoryUsage { bytes: 6, files: 2 }
        );
        let document_files = files.get(StorageCategory::Documents);
        assert_eq!(document_files.len(), 2);
        assert!(document_files.iter().any(|file| file.path == original));
        assert!(document_files.iter().any(|file| file.path == copy));
        assert!(
            document_files
                .iter()
                .all(|file| file.size == 3 && file.modified.is_some())
        );
        assert_summary_matches_files(&summary, &files);

        fs::remove_dir_all(root).expect("remove test folder");
    }

    #[test]
    fn compressed_contents_are_not_enumerated_or_measured() {
        let root = temporary_folder();
        fs::create_dir_all(&root).expect("create test folder");
        let archive = root.join("payload.gz");
        let internal_name = "inside-only-storage-analysis.txt";
        let mut encoder = GzBuilder::new().filename(internal_name).write(
            fs::File::create(&archive).expect("create gzip"),
            Compression::fast(),
        );
        encoder
            .write_all(&vec![b'x'; 256 * 1024])
            .expect("write compressed payload");
        encoder.finish().expect("finish gzip");
        fs::write(root.join("outside.txt"), b"outside").expect("write outside file");
        let archive_size = fs::metadata(&archive).expect("archive metadata").len();

        let (summary, files) = finished_result(root.clone());
        assert_eq!(summary.total_files, 2);
        assert_eq!(
            summary.usage(StorageCategory::Archives),
            StorageCategoryUsage {
                bytes: archive_size,
                files: 1,
            }
        );
        assert_eq!(
            summary.usage(StorageCategory::Documents),
            StorageCategoryUsage { bytes: 7, files: 1 }
        );
        assert_eq!(summary.total_bytes, archive_size + 7);
        assert!(
            StorageCategory::ALL
                .into_iter()
                .flat_map(|category| files.get(category))
                .all(|file| file.path.file_name() != Some(std::ffi::OsStr::new(internal_name)))
        );
        assert_summary_matches_files(&summary, &files);

        fs::remove_dir_all(root).expect("remove test folder");
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_root_is_followed_without_following_nested_links() {
        use std::os::unix::fs::symlink;

        let parent = temporary_folder();
        let target = parent.join("target");
        let linked_root = parent.join("linked-root");
        fs::create_dir_all(&target).expect("create target folder");
        fs::write(target.join("report.txt"), b"report").expect("write target file");
        symlink(&target, &linked_root).expect("create root link");

        let (summary, files) = finished_result(linked_root);
        assert_eq!(summary.total_bytes, 6);
        assert_eq!(summary.total_files, 1);
        assert_summary_matches_files(&summary, &files);

        fs::remove_dir_all(parent).expect("remove test folder");
    }

    #[test]
    fn a_pre_cancelled_scan_stops_without_walking_the_folder() {
        let root = temporary_folder();
        fs::create_dir_all(&root).expect("create test folder");
        fs::write(root.join("file.bin"), b"data").expect("write file");
        let cancel = Arc::new(AtomicBool::new(true));
        let (sender, receiver) = mpsc::channel();

        scan_folder(root.clone(), sender, cancel);
        match receiver.recv().expect("cancel event") {
            StorageAnalysisEvent::Cancelled {
                summary,
                files,
                skipped,
            } => {
                assert_eq!(skipped, 0);
                assert_eq!(summary.total_files, 0);
                assert_eq!(files.total_files(), 0);
                assert_summary_matches_files(&summary, &files);
            }
            event => panic!("expected cancelled event, got {event:?}"),
        }

        fs::remove_dir_all(root).expect("remove test folder");
    }

    #[test]
    fn a_missing_root_reports_failure_instead_of_an_empty_result() {
        let root = temporary_folder();
        let (sender, receiver) = mpsc::channel();

        scan_folder(root, sender, Arc::new(AtomicBool::new(false)));

        assert!(matches!(
            receiver.recv().expect("failure event"),
            StorageAnalysisEvent::Failed(_)
        ));
    }
}
