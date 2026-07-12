fn icon_names_for_path(path: &Path, is_directory: bool) -> Vec<String> {
    if is_directory {
        if path == Path::new("/") {
            return names([
                "drive-harddisk",
                "drive-harddisk-symbolic",
                "folder-root",
                "folder",
            ]);
        }
        if path.starts_with("/media") || path.starts_with("/run/media") {
            return names([
                "drive-removable-media",
                "drive-removable-media-usb",
                "folder-removable",
                "folder",
            ]);
        }
        if path.starts_with("/mnt") {
            return names(["folder-remote", "network-server", "folder"]);
        }
        if let Some(icon_names) = user_directory_icon_names(path) {
            return icon_names;
        }
        return names(["folder"]);
    }

    let mime = mime_info()
        .mime_for_path(path)
        .unwrap_or_else(|| "application/octet-stream".into());
    let mut candidates = Vec::new();
    candidates.push(mime.replace('/', "-"));
    if let Some(generic) = mime_info().generic_icon_for_mime(&mime) {
        candidates.push(generic);
    }
    if let Some(generic) = fallback_generic_icon_for_mime(&mime) {
        candidates.push(generic);
    }
    candidates.push("text-x-generic".into());
    candidates.push("unknown".into());
    dedupe(candidates)
}

fn user_directory_icon_names(path: &Path) -> Option<Vec<String>> {
    let directories = UserDirs::new()?;
    let candidates: [(Option<&Path>, &[&str]); 9] = [
        (
            Some(directories.home_dir()),
            &["user-home", "folder-home", "folder"],
        ),
        (
            directories.desktop_dir(),
            &["user-desktop", "folder-desktop", "folder"],
        ),
        (
            directories.document_dir(),
            &["folder-documents", "folder-document", "folder"],
        ),
        (
            directories.download_dir(),
            &["folder-download", "folder-downloads", "folder"],
        ),
        (
            directories.audio_dir(),
            &["folder-music", "folder-audio", "folder"],
        ),
        (
            directories.picture_dir(),
            &["folder-pictures", "folder-images", "folder"],
        ),
        (
            directories.public_dir(),
            &["folder-publicshare", "folder-public", "folder"],
        ),
        (
            directories.template_dir(),
            &["folder-templates", "folder-template", "folder"],
        ),
        (
            directories.video_dir(),
            &["folder-videos", "folder-video", "folder"],
        ),
    ];
    candidates.into_iter().find_map(|(candidate, icon_names)| {
        candidate
            .filter(|candidate| *candidate == path)
            .map(|_| icon_names.iter().map(|name| (*name).to_owned()).collect())
    })
}

fn names(values: impl IntoIterator<Item = &'static str>) -> Vec<String> {
    values.into_iter().map(str::to_owned).collect()
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn fallback_generic_icon_for_mime(mime: &str) -> Option<String> {
    let (top, _subtype) = mime.split_once('/')?;
    match top {
        "application" => Some("application-x-generic".into()),
        "audio" => Some("audio-x-generic".into()),
        "font" => Some("font-x-generic".into()),
        "image" => Some("image-x-generic".into()),
        "inode" => Some("inode-x-generic".into()),
        "message" => Some("message-x-generic".into()),
        "model" => Some("model-x-generic".into()),
        "multipart" => Some("multipart-x-generic".into()),
        "text" => Some("text-x-generic".into()),
        "video" => Some("video-x-generic".into()),
        _ => None,
    }
}

fn load_icon_path(path: &Path, size: u32) -> Option<NativeIconImage> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => fs::read(path)
            .ok()
            .as_deref()
            .and_then(|bytes| load_png_icon(bytes, size)),
        Some("svg") => fs::read(path)
            .ok()
            .as_deref()
            .and_then(|bytes| load_svg_icon(bytes, size)),
        _ => None,
    }
}

fn load_png_icon(bytes: &[u8], size: u32) -> Option<NativeIconImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width().max(image.height()) > size {
        image.thumbnail(size, size)
    } else {
        image
    }
    .to_rgba8();
    Some(NativeIconImage {
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.into_raw(),
    })
}

fn load_svg_icon(bytes: &[u8], size: u32) -> Option<NativeIconImage> {
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(bytes, &options).ok()?;
    let base_size = tree.size().to_int_size();
    let max_edge = base_size.width().max(base_size.height()).max(1);
    let scale = (size as f32 / max_edge as f32).clamp(0.01, 8.0);
    let width = ((base_size.width() as f32 * scale).round() as u32).max(1);
    let height = ((base_size.height() as f32 * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.data().to_vec();
    unpremultiply_rgba(&mut rgba);
    Some(NativeIconImage {
        rgba,
        width: width as usize,
        height: height as usize,
    })
}

fn unpremultiply_rgba(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha == 0 || alpha == 255 {
            continue;
        }
        pixel[0] = ((pixel[0] as u32 * 255) / alpha).min(255) as u8;
        pixel[1] = ((pixel[1] as u32 * 255) / alpha).min(255) as u8;
        pixel[2] = ((pixel[2] as u32 * 255) / alpha).min(255) as u8;
    }
}

impl IconThemeStore {
    fn find_best_icon(&self, names: &[String], size: u32) -> Option<PathBuf> {
        let mut visited = HashSet::new();
        self.find_best_icon_in_theme(&self.current_theme, names, size, &mut visited)
            .or_else(|| {
                let mut visited = HashSet::new();
                self.find_best_icon_in_theme("hicolor", names, size, &mut visited)
            })
            .or_else(|| self.lookup_fallback_icon(names))
    }

    fn find_best_icon_in_theme(
        &self,
        theme_name: &str,
        names: &[String],
        size: u32,
        visited: &mut HashSet<String>,
    ) -> Option<PathBuf> {
        if !visited.insert(theme_name.to_string()) {
            return None;
        }
        let theme = self.load_theme(theme_name)?;
        if let Some(path) = self.lookup_icon_in_theme(theme_name, &theme, names, size) {
            return Some(path);
        }
        for parent in &theme.inherits {
            if let Some(path) = self.find_best_icon_in_theme(parent, names, size, visited) {
                return Some(path);
            }
        }
        if theme_name != "hicolor" {
            self.find_best_icon_in_theme("hicolor", names, size, visited)
        } else {
            None
        }
    }

    fn lookup_icon_in_theme(
        &self,
        theme_name: &str,
        theme: &IconTheme,
        names: &[String],
        size: u32,
    ) -> Option<PathBuf> {
        for name in names {
            for directory in &theme.directories {
                if directory.matches_size(size)
                    && let Some(path) = self.icon_file_in_directory(theme_name, directory, name) {
                        return Some(path);
                    }
            }
        }

        let mut best: Option<(u32, PathBuf)> = None;
        for name in names {
            for directory in &theme.directories {
                if let Some(path) = self.icon_file_in_directory(theme_name, directory, name) {
                    let distance = directory.size_distance(size);
                    if best
                        .as_ref()
                        .is_none_or(|(best_distance, _)| distance < *best_distance)
                    {
                        best = Some((distance, path));
                    }
                }
            }
        }
        best.map(|(_, path)| path)
    }

    fn icon_file_in_directory(
        &self,
        theme_name: &str,
        directory: &IconThemeDirectory,
        icon_name: &str,
    ) -> Option<PathBuf> {
        for base_dir in &self.themed_base_dirs {
            for extension in ICON_EXTENSIONS {
                let path = base_dir
                    .join(theme_name)
                    .join(&directory.name)
                    .join(format!("{icon_name}.{extension}"));
                if path.is_file() {
                    return Some(path);
                }
            }
        }
        None
    }

    fn lookup_fallback_icon(&self, names: &[String]) -> Option<PathBuf> {
        for name in names {
            for directory in self.themed_base_dirs.iter().chain(&self.fallback_dirs) {
                for extension in ICON_EXTENSIONS {
                    let path = directory.join(format!("{name}.{extension}"));
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    fn load_theme(&self, theme_name: &str) -> Option<IconTheme> {
        let index = self
            .themed_base_dirs
            .iter()
            .map(|base| base.join(theme_name).join("index.theme"))
            .find(|path| path.is_file())?;
        let content = fs::read_to_string(index).ok()?;
        parse_icon_theme(&content)
    }
}

impl IconThemeDirectory {
    fn matches_size(&self, size: u32) -> bool {
        match self.kind {
            IconThemeDirectoryKind::Fixed => self.size.saturating_mul(self.scale) == size,
            IconThemeDirectoryKind::Scalable => {
                let scaled_size = size.saturating_mul(self.scale);
                self.min_size <= scaled_size && scaled_size <= self.max_size
            }
            IconThemeDirectoryKind::Threshold => {
                let scaled_size = size.saturating_mul(self.scale);
                self.size.saturating_sub(self.threshold) <= scaled_size
                    && scaled_size <= self.size.saturating_add(self.threshold)
            }
        }
    }

    fn size_distance(&self, size: u32) -> u32 {
        let target = size.saturating_mul(self.scale);
        match self.kind {
            IconThemeDirectoryKind::Fixed => self.size.abs_diff(target),
            IconThemeDirectoryKind::Scalable => {
                if target < self.min_size {
                    self.min_size - target
                } else { target.saturating_sub(self.max_size) }
            }
            IconThemeDirectoryKind::Threshold => {
                let min = self.size.saturating_sub(self.threshold);
                let max = self.size.saturating_add(self.threshold);
                if target < min {
                    min - target
                } else { target.saturating_sub(max) }
            }
        }
    }
}

fn parse_icon_theme(content: &str) -> Option<IconTheme> {
    let sections = parse_ini_sections(content);
    let root = sections.get("Icon Theme")?;
    let mut inherits = csv_values(root.get("Inherits").map(String::as_str).unwrap_or(""));
    if inherits.is_empty() {
        inherits.push("hicolor".into());
    }
    let directories = csv_values(root.get("Directories").map(String::as_str).unwrap_or(""))
        .into_iter()
        .chain(csv_values(
            root.get("ScaledDirectories")
                .map(String::as_str)
                .unwrap_or(""),
        ))
        .filter_map(|name| {
            let section = sections.get(&name)?;
            Some(IconThemeDirectory {
                name,
                size: parse_u32(section.get("Size")).unwrap_or(48),
                scale: parse_u32(section.get("Scale")).unwrap_or(1),
                min_size: parse_u32(section.get("MinSize"))
                    .or_else(|| parse_u32(section.get("Size")))
                    .unwrap_or(48),
                max_size: parse_u32(section.get("MaxSize"))
                    .or_else(|| parse_u32(section.get("Size")))
                    .unwrap_or(48),
                threshold: parse_u32(section.get("Threshold")).unwrap_or(2),
                kind: match section.get("Type").map(String::as_str) {
                    Some("Fixed") => IconThemeDirectoryKind::Fixed,
                    Some("Scalable") => IconThemeDirectoryKind::Scalable,
                    _ => IconThemeDirectoryKind::Threshold,
                },
            })
        })
        .collect();

    Some(IconTheme {
        inherits: dedupe(inherits),
        directories,
    })
}

fn parse_ini_sections(content: &str) -> HashMap<String, HashMap<String, String>> {
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current = String::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].trim().to_string();
            sections.entry(current.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        sections
            .entry(current.clone())
            .or_default()
            .insert(key.trim().to_string(), value.trim().to_string());
    }
    sections
}

fn csv_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_u32(value: Option<&String>) -> Option<u32> {
    value?.trim().parse().ok()
}

impl MimeInfo {
    fn mime_for_path(&self, path: &Path) -> Option<String> {
        let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        let mut best: Option<(i32, usize, &str)> = None;
        for glob in &self.globs {
            let specificity = glob.specificity();
            let matches = glob
                .literal
                .as_ref()
                .is_some_and(|literal| literal == &name)
                || glob
                    .suffix
                    .as_ref()
                    .is_some_and(|suffix| name.ends_with(suffix))
                || glob_matches(&glob.pattern, &name);
            if matches
                && best.as_ref().is_none_or(|(priority, length, _)| {
                    glob.priority > *priority
                        || (glob.priority == *priority && specificity > *length)
                })
            {
                best = Some((glob.priority, specificity, &glob.mime));
            }
        }

        best.map(|(_, _, mime)| self.resolve_alias(mime))
    }

    fn generic_icon_for_mime(&self, mime: &str) -> Option<String> {
        self.generic_icons
            .get(mime)
            .cloned()
            .or_else(|| self.generic_icons.get(&self.resolve_alias(mime)).cloned())
    }

    fn resolve_alias(&self, mime: &str) -> String {
        self.aliases
            .get(mime)
            .cloned()
            .unwrap_or_else(|| mime.to_string())
    }
}

impl MimeGlob {
    fn specificity(&self) -> usize {
        self.literal
            .as_ref()
            .map(|value| value.len())
            .or_else(|| self.suffix.as_ref().map(|value| value.len()))
            .unwrap_or(self.pattern.len())
    }
}

fn glob_matches(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern.eq_ignore_ascii_case(name);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(&suffix.to_ascii_lowercase());
    }
    false
}

fn parse_mime_glob(line: &str) -> Option<MimeGlob> {
    let mut parts = line.splitn(4, ':');
    let priority = parts.next()?.trim().parse().ok()?;
    let mime = parts.next()?.trim().to_string();
    let pattern = parts.next()?.trim().to_ascii_lowercase();
    if mime.is_empty() || pattern.is_empty() || pattern == "__NOGLOBS__" {
        return None;
    }
    let suffix = pattern
        .strip_prefix("*.")
        .filter(|suffix| !suffix.contains(['*', '?', '[']))
        .map(|suffix| format!(".{suffix}"));
    let literal = (!pattern.contains(['*', '?', '['])).then_some(pattern.clone());
    Some(MimeGlob {
        priority,
        mime,
        pattern,
        suffix,
        literal,
    })
}

fn load_mime_info() -> MimeInfo {
    let mut globs = Vec::new();
    let mut aliases = HashMap::new();
    let mut generic_icons = HashMap::new();

    for base in xdg_data_dirs_for_mime() {
        let mime_dir = base.join("mime");
        if let Ok(content) = fs::read_to_string(mime_dir.join("globs2")) {
            globs.extend(
                content
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .filter_map(parse_mime_glob),
            );
        }
        if let Ok(content) = fs::read_to_string(mime_dir.join("aliases")) {
            for line in content.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                if let (Some(alias), Some(canonical)) = (parts.next(), parts.next()) {
                    aliases.insert(alias.to_string(), canonical.to_string());
                }
            }
        }
        if let Ok(content) = fs::read_to_string(mime_dir.join("generic-icons")) {
            for line in content.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((mime, icon)) = line.split_once(':') {
                    generic_icons.insert(mime.trim().to_string(), icon.trim().to_string());
                }
            }
        }
    }

    MimeInfo {
        globs,
        aliases,
        generic_icons,
    }
}

fn icon_theme_store() -> &'static IconThemeStore {
    static STORE: OnceLock<IconThemeStore> = OnceLock::new();
    STORE.get_or_init(|| {
        let themed_base_dirs = themed_icon_base_dirs();
        let current_theme = configured_icon_theme()
            .or_else(|| first_existing_theme(&themed_base_dirs, FALLBACK_THEMES))
            .unwrap_or_else(|| "hicolor".into());
        IconThemeStore {
            themed_base_dirs,
            fallback_dirs: vec![PathBuf::from("/usr/share/pixmaps")],
            current_theme,
        }
    })
}

fn mime_info() -> &'static MimeInfo {
    static MIME_INFO: OnceLock<MimeInfo> = OnceLock::new();
    MIME_INFO.get_or_init(load_mime_info)
}

fn themed_icon_base_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home_dir() {
        dirs.push(home.join(".icons"));
    }
    dirs.push(xdg_data_home().join("icons"));
    dirs.extend(
        xdg_data_dirs_for_mime()
            .into_iter()
            .map(|dir| dir.join("icons")),
    );
    dedupe_paths(dirs)
}

fn xdg_data_dirs_for_mime() -> Vec<PathBuf> {
    let mut dirs = vec![xdg_data_home()];
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    dirs.extend(data_dirs);
    dedupe_paths(dirs)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn configured_icon_theme() -> Option<String> {
    std::env::var("BEXPLORER_ICON_THEME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| gtk_icon_theme_from_settings("gtk-4.0/settings.ini"))
        .or_else(|| gtk_icon_theme_from_settings("gtk-3.0/settings.ini"))
        .or_else(kde_icon_theme_from_settings)
}

fn gtk_icon_theme_from_settings(relative_path: &str) -> Option<String> {
    let settings = fs::read_to_string(xdg_config_home().join(relative_path)).ok()?;
    parse_ini_sections(&settings)
        .get("Settings")
        .and_then(|settings| settings.get("gtk-icon-theme-name"))
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

fn kde_icon_theme_from_settings() -> Option<String> {
    let settings = fs::read_to_string(xdg_config_home().join("kdeglobals")).ok()?;
    parse_ini_sections(&settings)
        .get("Icons")
        .and_then(|settings| settings.get("Theme"))
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

fn first_existing_theme(base_dirs: &[PathBuf], names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        base_dirs
            .iter()
            .any(|base| base.join(name).join("index.theme").is_file())
            .then(|| (*name).to_string())
    })
}

fn xdg_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn xdg_data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/usr/local/share"))
}

fn xdg_cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn canonical_file_uri(path: &Path) -> Option<String> {
    let path = fs::canonicalize(path).ok().or_else(|| {
        path.is_absolute()
            .then(|| path.to_path_buf())
            .or_else(|| std::env::current_dir().ok().map(|dir| dir.join(path)))
    })?;
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        match *byte {
            b'/' => uri.push('/'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                uri.push(*byte as char)
            }
            value => uri.push_str(&format!("%{value:02X}")),
        }
    }
    Some(uri)
}

fn thumbnail_hash_for_uri(uri: &str) -> String {
    format!("{:x}", md5::compute(uri.as_bytes()))
}

fn thumbnail_metadata_is_current(
    thumbnail_bytes: &[u8],
    original_metadata: &fs::Metadata,
    original_uri: &str,
) -> bool {
    let text = png_text_chunks(thumbnail_bytes);
    if text.get("Thumb::URI").map(String::as_str) != Some(original_uri) {
        return false;
    }
    let Some(mtime) = original_metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().to_string())
    else {
        return false;
    };
    if text.get("Thumb::MTime") != Some(&mtime) {
        return false;
    }
    if let Some(size) = text.get("Thumb::Size")
        && size.parse::<u64>().ok() != Some(original_metadata.len()) {
            return false;
        }
    true
}

fn png_text_chunks(bytes: &[u8]) -> HashMap<String, String> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    let mut output = HashMap::new();
    if bytes.len() < 8 || &bytes[..8] != PNG_SIGNATURE {
        return output;
    }

    let mut index = 8;
    while index + 8 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[index],
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
        ]) as usize;
        let chunk_type = &bytes[index + 4..index + 8];
        let data_start = index + 8;
        let data_end = data_start.saturating_add(length);
        if data_end + 4 > bytes.len() {
            break;
        }
        let data = &bytes[data_start..data_end];
        if chunk_type == b"tEXt" {
            if let Some((key, value)) = split_png_text(data) {
                output.insert(key, value);
            }
        } else if chunk_type == b"iTXt" {
            if let Some((key, value)) = split_png_itxt(data) {
                output.insert(key, value);
            }
        } else if chunk_type == b"IEND" {
            break;
        }
        index = data_end + 4;
    }
    output
}

fn split_png_text(data: &[u8]) -> Option<(String, String)> {
    let split = data.iter().position(|byte| *byte == 0)?;
    let key = String::from_utf8_lossy(&data[..split]).to_string();
    let value = String::from_utf8_lossy(&data[split + 1..]).to_string();
    Some((key, value))
}

fn split_png_itxt(data: &[u8]) -> Option<(String, String)> {
    let key_end = data.iter().position(|byte| *byte == 0)?;
    let key = String::from_utf8_lossy(&data[..key_end]).to_string();
    let compression_flag = *data.get(key_end + 1)?;
    let _compression_method = *data.get(key_end + 2)?;
    if compression_flag != 0 {
        return None;
    }
    let mut rest = data.get(key_end + 3..)?;
    let language_end = rest.iter().position(|byte| *byte == 0)?;
    rest = &rest[language_end + 1..];
    let translated_end = rest.iter().position(|byte| *byte == 0)?;
    let value = String::from_utf8_lossy(&rest[translated_end + 1..]).to_string();
    Some((key, value))
}
