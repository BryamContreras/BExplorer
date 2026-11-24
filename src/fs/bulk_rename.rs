use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::explorer;
use super::transfer_queue::{FileIdentity, path_identity, rename_without_replace};
use crate::utils::errors::{BExplorerError, Result};

#[derive(Debug)]
pub(crate) struct Selection {
    directory: PathBuf,
    physical_directory: PathBuf,
    directory_identity: FileIdentity,
    sources: Vec<Source>,
    occupied: HashSet<OsString>,
}

#[derive(Debug)]
struct Source {
    name: String,
    identity: FileIdentity,
    is_directory: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Rule<'a> {
    Replace { find: &'a str, replacement: &'a str },
    Numbered { prefix: &'a str },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Issue {
    Invalid,
    Duplicate,
    Occupied,
}

#[derive(Clone)]
pub(crate) struct PreviewRow {
    pub before: String,
    pub after: String,
    pub issue: Option<Issue>,
}

#[derive(Clone, Default)]
pub(crate) struct Preview {
    pub rows: Vec<PreviewRow>,
    pub changes: usize,
    pub issues: usize,
}

impl Preview {
    pub(crate) fn can_apply(&self) -> bool {
        self.changes > 0 && self.issues == 0
    }
}

impl Selection {
    pub(crate) fn open(paths: Vec<PathBuf>) -> Result<Self> {
        if !cfg!(any(windows, target_os = "linux")) {
            return Err(operation_error(
                "Bulk rename is available on Windows and Linux",
            ));
        }
        Self::read(paths, 2)
    }

    fn read(mut paths: Vec<PathBuf>, minimum: usize) -> Result<Self> {
        paths.sort_unstable();
        paths.dedup();
        let directory = paths
            .first()
            .and_then(|source| source.parent())
            .filter(|_| paths.len() >= minimum)
            .ok_or_else(|| operation_error("Select at least two entries to rename"))?
            .to_path_buf();
        if paths.iter().any(|source| {
            !source.is_absolute()
                || explorer::is_virtual_path(source)
                || source.parent() != Some(directory.as_path())
        }) {
            return Err(operation_error(
                "Select real files or folders in the same directory",
            ));
        }
        let physical_directory = directory.canonicalize()?;
        let directory_identity = path_identity(&physical_directory)?;
        let sources = paths
            .iter()
            .map(|path| {
                let name = path.file_name().and_then(OsStr::to_str).ok_or_else(|| {
                    operation_error("The selection contains a filename that is not valid Unicode")
                })?;
                let physical_path = physical_directory.join(name);
                Ok(Source {
                    name: name.to_owned(),
                    identity: path_identity(&physical_path)?,
                    is_directory: fs::symlink_metadata(physical_path)?.is_dir(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let occupied = fs::read_dir(&physical_directory)?
            .map(|entry| entry.map(|entry| name_key(&entry.file_name())))
            .collect::<io::Result<HashSet<_>>>()?;
        Ok(Self {
            directory,
            physical_directory,
            directory_identity,
            sources,
            occupied,
        })
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn preview(&self, rule: Rule<'_>, preserve_extension: bool) -> Preview {
        let width = self.sources.len().to_string().len().max(2);
        let mut rows = self
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let original = Path::new(&source.name);
                let extension = (preserve_extension && !source.is_directory)
                    .then(|| original.extension().and_then(OsStr::to_str))
                    .flatten();
                let stem = if extension.is_some() {
                    original
                        .file_stem()
                        .and_then(OsStr::to_str)
                        .unwrap_or(&source.name)
                } else {
                    &source.name
                };
                let after = match rule {
                    Rule::Replace { find, replacement } if !find.is_empty() => {
                        Some(stem.replace(find, replacement))
                    }
                    Rule::Numbered { prefix } if !prefix.is_empty() => {
                        Some(format!("{prefix} {:0width$}", index + 1))
                    }
                    _ => None,
                };
                let (after, issue) = match after {
                    Some(mut after) => {
                        if after.is_empty() {
                            (after, Some(Issue::Invalid))
                        } else {
                            if let Some(extension) = extension {
                                after.push('.');
                                after.push_str(extension);
                            }
                            let issue = (!valid_name(&after)).then_some(Issue::Invalid);
                            (after, issue)
                        }
                    }
                    None => (source.name.clone(), None),
                };
                PreviewRow {
                    before: source.name.clone(),
                    after,
                    issue,
                }
            })
            .collect::<Vec<_>>();

        let vacated = rows
            .iter()
            .filter(|row| row.before != row.after)
            .map(|row| name_key(OsStr::new(&row.before)))
            .collect::<HashSet<_>>();
        let mut targets = HashMap::new();
        for index in 0..rows.len() {
            let row = &rows[index];
            if row.before == row.after || row.issue.is_some() {
                continue;
            }
            let key = name_key(OsStr::new(&row.after));
            if self.occupied.contains(&key) && !vacated.contains(&key) {
                rows[index].issue = Some(Issue::Occupied);
            } else if let Some(prior) = targets.insert(key, index) {
                rows[prior].issue = Some(Issue::Duplicate);
                rows[index].issue = Some(Issue::Duplicate);
            }
        }
        Preview {
            changes: rows.iter().filter(|row| row.before != row.after).count(),
            issues: rows.iter().filter(|row| row.issue.is_some()).count(),
            rows,
        }
    }

    pub(crate) fn apply(&self, preview: &Preview) -> Result<Vec<PathBuf>> {
        self.apply_with(preview, rename_without_replace)
    }

    fn apply_with(
        &self,
        preview: &Preview,
        mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
    ) -> Result<Vec<PathBuf>> {
        if !preview.can_apply()
            || preview.rows.len() != self.sources.len()
            || preview
                .rows
                .iter()
                .zip(&self.sources)
                .any(|(row, source)| row.before != source.name)
        {
            return Err(operation_error("A valid rename preview is required"));
        }
        if self.directory.canonicalize()? != self.physical_directory
            || path_identity(&self.physical_directory)? != self.directory_identity
        {
            return Err(operation_error("The directory changed after the preview"));
        }
        let planned = preview
            .rows
            .iter()
            .zip(&self.sources)
            .filter(|(row, _)| row.before != row.after)
            .collect::<Vec<_>>();
        if planned.iter().any(|(row, _)| !valid_name(&row.after)) {
            return Err(operation_error("The preview contains an invalid filename"));
        }
        let vacated = planned
            .iter()
            .map(|(row, _)| name_key(OsStr::new(&row.before)))
            .collect::<HashSet<_>>();
        for (row, source) in &planned {
            let original = self.physical_directory.join(&row.before);
            if path_identity(&original)? != source.identity {
                return Err(changed(&original));
            }
            let target = self.physical_directory.join(&row.after);
            if !vacated.contains(&name_key(OsStr::new(&row.after))) {
                match fs::symlink_metadata(&target) {
                    Ok(_)
                        if row.before.to_lowercase() == row.after.to_lowercase()
                            && path_identity(&target)? == source.identity => {}
                    Ok(_) => {
                        return Err(operation_error(format!(
                            "Destination already exists: {}",
                            target.display()
                        )));
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }

        let workspace = create_workspace(&self.physical_directory)?;
        let mut staged = 0;
        let mut committed = 0;
        let result = (|| {
            for (row, source) in &planned {
                let original = self.physical_directory.join(&row.before);
                let temporary = workspace.join(&row.before);
                if path_identity(&original)? != source.identity {
                    return Err(changed(&original));
                }
                rename(&original, &temporary)?;
                staged += 1;
                if path_identity(&temporary)? != source.identity {
                    return Err(changed(&original));
                }
            }
            for (row, source) in &planned {
                let temporary = workspace.join(&row.before);
                if path_identity(&temporary)? != source.identity {
                    return Err(changed(&temporary));
                }
                rename(&temporary, &self.physical_directory.join(&row.after))?;
                committed += 1;
            }
            Ok(())
        })();

        if let Err(error) = result {
            let mut restored = true;
            for (row, source) in planned[..committed].iter().rev() {
                let target = self.physical_directory.join(&row.after);
                let temporary = workspace.join(&row.before);
                restored &= path_identity(&target).ok() == Some(source.identity)
                    && rename(&target, &temporary).is_ok();
            }
            for (row, source) in planned[..staged].iter().rev() {
                let temporary = workspace.join(&row.before);
                let original = self.physical_directory.join(&row.before);
                restored &= path_identity(&temporary).ok() == Some(source.identity)
                    && rename(&temporary, &original).is_ok();
            }
            if !restored {
                return Err(operation_error(format!(
                    "{error}. Some entries could not be restored. Check {} and the original directory.",
                    workspace.display()
                )));
            }
            let _ = fs::remove_dir(&workspace);
            return Err(error);
        }

        if let Err(error) = fs::remove_dir(&workspace) {
            crate::utils::log::error(format!("Could not remove empty rename work area: {error}"));
        }
        crate::utils::atomic_file::sync_parent(&workspace);
        Ok(planned
            .iter()
            .map(|(row, _)| self.directory.join(&row.after))
            .collect())
    }
}

pub(crate) fn rename_case_variant(source: &Path, target: &Path, error: io::Error) -> Result<()> {
    let Some(original) = source.file_name().and_then(OsStr::to_str) else {
        return Err(error.into());
    };
    let Some(proposed) = target.file_name().and_then(OsStr::to_str) else {
        return Err(error.into());
    };
    if error.kind() != io::ErrorKind::AlreadyExists
        || original.to_lowercase() != proposed.to_lowercase()
        || path_identity(source)? != path_identity(target)?
    {
        return Err(error.into());
    }

    // Some case-insensitive filesystems report a case-only change as a
    // collision. Vacate the old spelling, then publish the new one without
    // replacing any entry the filesystem treats as distinct.
    let selection = Selection::read(vec![std::path::absolute(source)?], 1)?;
    let preview = selection.preview(
        Rule::Replace {
            find: original,
            replacement: proposed,
        },
        false,
    );
    selection.apply(&preview).map(|_| ())
}

fn changed(path: &Path) -> BExplorerError {
    operation_error(format!(
        "Entry changed after the preview: {}",
        path.display()
    ))
}

fn operation_error(message: impl Into<String>) -> BExplorerError {
    BExplorerError::Operation(message.into())
}

fn create_workspace(parent: &Path) -> Result<PathBuf> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    loop {
        let token = getrandom::u64().map_err(|error| operation_error(error.to_string()))?;
        let path = parent.join(format!(".bexplorer-bulk-rename-{token:016x}"));
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

fn name_key(name: &OsStr) -> OsString {
    #[cfg(windows)]
    {
        name.to_string_lossy().to_lowercase().into()
    }
    #[cfg(not(windows))]
    {
        name.to_owned()
    }
}

pub(crate) fn valid_name(name: &str) -> bool {
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\0']) {
        return false;
    }
    #[cfg(windows)]
    {
        let stem = name
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        let reserved_port = stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"));
        name.encode_utf16().count() <= 255
            && !name.ends_with([' ', '.'])
            && !name.chars().any(|c| c < ' ' || "<>:\"\\|?*".contains(c))
            && !matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            )
            && !reserved_port.is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
    }
    #[cfg(not(windows))]
    {
        name.len() <= 255
    }
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let name = format!("bexplorer-rename-test-{:016x}", getrandom::u64().unwrap());
            let path = std::env::temp_dir().join(name);
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, contents).unwrap();
            path
        }

        fn entries(&self) -> Vec<OsString> {
            let mut names = fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>();
            names.sort_unstable();
            names
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn numbered_preview_is_stable_and_renames_files_and_folders() {
        let fixture = Fixture::new();
        let alpha = fixture.file("a.Report.txt", "first");
        let folder = fixture.0.join("b.Models");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("model"), "folder child").unwrap();
        let selection = Selection::open(vec![folder, alpha]).unwrap();
        let preview = selection.preview(Rule::Numbered { prefix: "Informe" }, true);

        assert!(preview.can_apply());
        assert_eq!(preview.rows[0].after, "Informe 01.txt");
        assert_eq!(preview.rows[1].after, "Informe 02");
        assert_eq!(selection.apply(&preview).unwrap().len(), 2);
        assert_eq!(
            fs::read(fixture.0.join("Informe 01.txt")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(fixture.0.join("Informe 02/model")).unwrap(),
            b"folder child"
        );
        assert_eq!(fixture.entries(), ["Informe 01.txt", "Informe 02"]);
    }

    #[test]
    fn preview_blocks_existing_duplicate_and_invalid_targets() {
        let fixture = Fixture::new();
        let alpha = fixture.file("aa.txt", "first");
        let beta = fixture.file("aaa.txt", "second");
        fixture.file("x.txt", "keep");
        let selection = Selection::open(vec![alpha, beta]).unwrap();

        let existing = selection.preview(
            Rule::Replace {
                find: "aa",
                replacement: "x",
            },
            true,
        );
        assert_eq!(existing.rows[0].issue, Some(Issue::Occupied));
        assert!(!existing.can_apply());

        let chained = selection.preview(
            Rule::Replace {
                find: "aa",
                replacement: "a",
            },
            true,
        );
        assert!(chained.can_apply());
        // The second target is the first source, which the same batch vacates.
        assert_eq!(chained.rows[1].after, "aa.txt");

        let invalid = selection.preview(
            Rule::Numbered {
                prefix: "../escape",
            },
            true,
        );
        assert_eq!(invalid.issues, 2);
        assert_eq!(invalid.rows[0].issue, Some(Issue::Invalid));
        assert!(!invalid.can_apply());
        assert!(selection.apply(&invalid).is_err());

        let _branch = fixture.file("aab.txt", "third");
        let mixture =
            Selection::open(vec![fixture.0.join("aaa.txt"), fixture.0.join("aab.txt")]).unwrap();
        let duplicate = mixture.preview(
            Rule::Replace {
                find: "a",
                replacement: "",
            },
            true,
        );
        assert_eq!(duplicate.rows[0].issue, Some(Issue::Invalid));
        // A single literal replacement can also map two distinct names to the same target.
        let equalizes = Selection::open(vec![
            fixture.file("xa x.txt", "x"),
            fixture.file("x ax.txt", "y"),
        ])
        .unwrap();
        let duplicate = equalizes.preview(
            Rule::Replace {
                find: " ",
                replacement: "",
            },
            true,
        );
        assert!(
            duplicate
                .rows
                .iter()
                .all(|row| row.issue == Some(Issue::Duplicate))
        );

        assert_eq!(selection.apply(&chained).unwrap().len(), 2);
        assert_eq!(fs::read(fixture.0.join("a.txt")).unwrap(), b"first");
        assert_eq!(fs::read(fixture.0.join("aa.txt")).unwrap(), b"second");
    }

    #[test]
    fn requires_entries_from_one_directory_and_rechecks_new_conflicts() {
        let first = Fixture::new();
        let second = Fixture::new();
        let alpha = first.file("a.txt", "first");
        let beta = second.file("b.txt", "second");
        assert!(Selection::open(vec![alpha.clone(), beta]).is_err());

        let beta = first.file("b.txt", "second");
        let selection = Selection::open(vec![alpha.clone(), beta.clone()]).unwrap();
        let preview = selection.preview(Rule::Numbered { prefix: "New" }, true);
        let collision = first.file("New 02.txt", "other writer");
        assert!(selection.apply(&preview).is_err());
        assert_eq!(fs::read(alpha).unwrap(), b"first");
        assert_eq!(fs::read(beta).unwrap(), b"second");
        assert_eq!(fs::read(collision).unwrap(), b"other writer");
        assert_eq!(first.entries().len(), 3);
    }

    #[test]
    fn restores_sources_when_commit_partially_fails() {
        let fixture = Fixture::new();
        let alpha = fixture.file("a.txt", "first");
        let beta = fixture.file("b.txt", "second");
        let selection = Selection::open(vec![alpha, beta]).unwrap();
        let preview = selection.preview(Rule::Numbered { prefix: "New" }, true);
        let mut rename_count = 0;
        let result = selection.apply_with(&preview, |source, target| {
            rename_count += 1;
            if rename_count == 4 {
                fs::write(target, "new writer")?;
            }
            rename_without_replace(source, target)
        });

        assert!(result.is_err());
        assert_eq!(fixture.entries(), ["New 02.txt", "a.txt", "b.txt"]);
        assert_eq!(fs::read(fixture.0.join("a.txt")).unwrap(), b"first");
        assert_eq!(fs::read(fixture.0.join("b.txt")).unwrap(), b"second");
        assert_eq!(
            fs::read(fixture.0.join("New 02.txt")).unwrap(),
            b"new writer"
        );
    }

    #[test]
    fn case_only_changes_keep_original_content() {
        let fixture = Fixture::new();
        let alpha = fixture.file("original-a.txt", "first");
        let beta = fixture.file("original-b.png", "second");
        let selection = Selection::open(vec![alpha, beta]).unwrap();
        let preview = selection.preview(
            Rule::Replace {
                find: "original",
                replacement: "Original",
            },
            true,
        );
        assert_eq!(selection.apply(&preview).unwrap().len(), 2);
        assert_eq!(fixture.entries(), ["Original-a.txt", "Original-b.png"]);
    }
}
