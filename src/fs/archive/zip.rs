use super::*;

#[derive(Clone)]
struct ZipSource {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

#[derive(Clone)]
struct ZipCentralEntry {
    name: Vec<u8>,
    method: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    external_attributes: u32,
}

struct ZipReadEntry {
    name: String,
    method: u16,
    encrypted: bool,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    is_dir: bool,
    modified: Option<SystemTime>,
}

pub(super) fn create_zip_archive(
    paths: &[PathBuf],
    destination: &Path,
    method: ArchiveCompressionMethod,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    create_zip_archive_inner(paths, destination, method, None, cancel_flag)
}

pub(super) fn create_zip_archive_with_progress(
    paths: &[PathBuf],
    destination: &Path,
    method: ArchiveCompressionMethod,
    tx: Sender<ArchiveProgressMsg>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let total = archive_sources_total_bytes(paths);
    let mut progress = ArchiveProgressEmitter::new(tx, total, "Compress");
    create_zip_archive_inner(paths, destination, method, Some(&mut progress), cancel_flag)?;
    let file_name = current_name(destination).to_string();
    progress.finish(&file_name);
    Ok(())
}

fn create_zip_archive_inner(
    paths: &[PathBuf],
    destination: &Path,
    method: ArchiveCompressionMethod,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let sources = collect_zip_sources(paths)?;
    if sources.is_empty() {
        return Err(BExplorerError::Operation(
            "No readable items were found to compress".into(),
        ));
    }

    let temp_archive = temp_path_for(destination, "tmpzip");
    let mut writer = BufWriter::new(File::create(&temp_archive)?);
    let mut central_entries = Vec::with_capacity(sources.len());

    let result = (|| {
        for source in sources {
            check_archive_cancelled(cancel_flag)?;
            let local_header_offset = writer.stream_position()?;
            let name = source.name.as_bytes().to_vec();

            if source.is_dir {
                write_zip_local_header(
                    &mut writer,
                    &name,
                    ZIP_METHOD_STORE,
                    0,
                    0,
                    0,
                    local_header_offset,
                    &mut central_entries,
                    true,
                )?;
                continue;
            }

            if method == ArchiveCompressionMethod::Store {
                let stored =
                    stored_source_file(&source.path, progress.as_deref_mut(), cancel_flag)?;
                write_zip_local_header(
                    &mut writer,
                    &name,
                    ZIP_METHOD_STORE,
                    stored.crc32,
                    stored.size,
                    stored.size,
                    local_header_offset,
                    &mut central_entries,
                    false,
                )?;
                copy_source_file_to_writer(&source.path, &mut writer, cancel_flag)?;
            } else {
                let compressed = deflate_source_file(
                    &source.path,
                    method.zip_compression(),
                    progress.as_deref_mut(),
                    cancel_flag,
                )?;
                write_zip_local_header(
                    &mut writer,
                    &name,
                    ZIP_METHOD_DEFLATE,
                    compressed.crc32,
                    compressed.compressed_size,
                    compressed.uncompressed_size,
                    local_header_offset,
                    &mut central_entries,
                    false,
                )?;

                let mut temp_file = BufReader::new(File::open(&compressed.path)?);
                io::copy(&mut temp_file, &mut writer)?;
                let _ = fs::remove_file(&compressed.path);
            }
            if let Some(progress) = progress.as_deref_mut() {
                progress.finish_file(&source.name);
            }
        }

        write_zip_central_directory(&mut writer, &central_entries)?;
        writer.flush()?;
        Ok::<(), BExplorerError>(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_archive);
        return result;
    }

    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(&temp_archive, destination).or_else(|_| {
        fs::copy(&temp_archive, destination)?;
        fs::remove_file(&temp_archive)?;
        Ok::<(), std::io::Error>(())
    })?;

    Ok(())
}

struct DeflatedFile {
    path: PathBuf,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
}

struct StoredFile {
    crc32: u32,
    size: u64,
}

fn stored_source_file(
    path: &Path,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<StoredFile> {
    let mut input = BufReader::new(File::open(path)?);
    let mut hasher = Hasher::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 1024 * 128];
    let file_name = current_name(path).to_string();

    loop {
        check_archive_cancelled(cancel_flag)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
        if let Some(progress) = progress.as_deref_mut() {
            progress.add_bytes(read as u64, &file_name);
        }
    }

    Ok(StoredFile {
        crc32: hasher.finalize(),
        size,
    })
}

fn copy_source_file_to_writer<W: Write>(
    path: &Path,
    writer: &mut W,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let mut input = BufReader::new(File::open(path)?);
    let mut buffer = [0_u8; 1024 * 128];
    loop {
        check_archive_cancelled(cancel_flag)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
    }
    Ok(())
}

fn deflate_source_file(
    path: &Path,
    compression: Compression,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<DeflatedFile> {
    let temp = temp_path_for(path, "deflate");
    let mut input = BufReader::new(File::open(path)?);
    let output = File::create(&temp)?;
    let mut encoder = DeflateEncoder::new(output, compression);
    let mut hasher = Hasher::new();
    let mut uncompressed_size = 0_u64;
    let mut buffer = [0_u8; 1024 * 128];
    let file_name = current_name(path).to_string();

    loop {
        check_archive_cancelled(cancel_flag)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        encoder.write_all(&buffer[..read])?;
        uncompressed_size = uncompressed_size.saturating_add(read as u64);
        if let Some(progress) = progress.as_deref_mut() {
            progress.add_bytes(read as u64, &file_name);
        }
    }

    let output = encoder.finish()?;
    let compressed_size = output.metadata()?.len();

    Ok(DeflatedFile {
        path: temp,
        crc32: hasher.finalize(),
        compressed_size,
        uncompressed_size,
    })
}

#[allow(clippy::too_many_arguments)]
fn write_zip_local_header<W: Write>(
    writer: &mut W,
    name: &[u8],
    method: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    central_entries: &mut Vec<ZipCentralEntry>,
    is_dir: bool,
) -> Result<()> {
    let compressed_size_32 = to_zip_u32(compressed_size, "compressed size")?;
    let uncompressed_size_32 = to_zip_u32(uncompressed_size, "uncompressed size")?;
    let name_len = to_zip_u16(name.len(), "file name length")?;

    write_u32(writer, ZIP_LOCAL_FILE_HEADER)?;
    write_u16(writer, ZIP_VERSION)?;
    write_u16(writer, ZIP_UTF8_FLAG)?;
    write_u16(writer, method)?;
    write_u16(writer, DOS_TIME_MIDNIGHT)?;
    write_u16(writer, DOS_DATE_1980_01_01)?;
    write_u32(writer, crc32)?;
    write_u32(writer, compressed_size_32)?;
    write_u32(writer, uncompressed_size_32)?;
    write_u16(writer, name_len)?;
    write_u16(writer, 0)?;
    writer.write_all(name)?;

    central_entries.push(ZipCentralEntry {
        name: name.to_vec(),
        method,
        crc32,
        compressed_size,
        uncompressed_size,
        local_header_offset,
        external_attributes: if is_dir { 0x10 } else { 0 },
    });

    Ok(())
}

fn write_zip_central_directory<W: Write + Seek>(
    writer: &mut W,
    entries: &[ZipCentralEntry],
) -> Result<()> {
    let central_offset = writer.stream_position()?;

    for entry in entries {
        write_u32(writer, ZIP_CENTRAL_DIRECTORY_HEADER)?;
        write_u16(writer, ZIP_VERSION)?;
        write_u16(writer, ZIP_VERSION)?;
        write_u16(writer, ZIP_UTF8_FLAG)?;
        write_u16(writer, entry.method)?;
        write_u16(writer, DOS_TIME_MIDNIGHT)?;
        write_u16(writer, DOS_DATE_1980_01_01)?;
        write_u32(writer, entry.crc32)?;
        write_u32(
            writer,
            to_zip_u32(entry.compressed_size, "compressed size")?,
        )?;
        write_u32(
            writer,
            to_zip_u32(entry.uncompressed_size, "uncompressed size")?,
        )?;
        write_u16(writer, to_zip_u16(entry.name.len(), "file name length")?)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, entry.external_attributes)?;
        write_u32(
            writer,
            to_zip_u32(entry.local_header_offset, "local header offset")?,
        )?;
        writer.write_all(&entry.name)?;
    }

    let central_size = writer.stream_position()?.saturating_sub(central_offset);
    write_u32(writer, ZIP_END_OF_CENTRAL_DIRECTORY)?;
    write_u16(writer, 0)?;
    write_u16(writer, 0)?;
    write_u16(writer, to_zip_u16(entries.len(), "entry count")?)?;
    write_u16(writer, to_zip_u16(entries.len(), "entry count")?)?;
    write_u32(writer, to_zip_u32(central_size, "central directory size")?)?;
    write_u32(
        writer,
        to_zip_u32(central_offset, "central directory offset")?,
    )?;
    write_u16(writer, 0)?;

    Ok(())
}

fn collect_zip_sources(paths: &[PathBuf]) -> Result<Vec<ZipSource>> {
    let mut sources = Vec::new();

    for path in paths {
        if !path.exists() {
            continue;
        }

        let base = path.parent().unwrap_or_else(|| Path::new(""));
        if path.is_dir() {
            for entry in WalkDir::new(path).follow_links(false) {
                let entry = entry.map_err(|error| BExplorerError::Operation(error.to_string()))?;
                let entry_path = entry.path();
                let relative = entry_path
                    .strip_prefix(base)
                    .map_err(|error| BExplorerError::Operation(error.to_string()))?;
                let is_dir = entry.file_type().is_dir();
                sources.push(ZipSource {
                    path: entry_path.to_path_buf(),
                    name: archive_name(relative, is_dir)?,
                    is_dir,
                });
            }
        } else {
            let Some(name) = path.file_name() else {
                continue;
            };
            sources.push(ZipSource {
                path: path.to_path_buf(),
                name: archive_name(Path::new(name), false)?,
                is_dir: false,
            });
        }
    }

    Ok(sources)
}

#[allow(dead_code)]
pub(super) fn extract_zip_archive(
    archive: &Path,
    destination: &Path,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    extract_zip_archive_inner(archive, destination, None, cancel_flag)
}

pub(super) fn extract_zip_archive_with_progress(
    archive: &Path,
    destination: &Path,
    tx: Sender<ArchiveProgressMsg>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let mut file = File::open(archive)?;
    let entries = read_zip_central_directory(&mut file)?;
    let total = entries.iter().map(|entry| entry.uncompressed_size).sum();
    let mut progress = ArchiveProgressEmitter::new(tx, total, "Extract");
    extract_zip_entries(file, entries, destination, Some(&mut progress), cancel_flag)?;
    let file_name = current_name(archive).to_string();
    progress.finish(&file_name);
    Ok(())
}

#[allow(dead_code)]
fn extract_zip_archive_inner(
    archive: &Path,
    destination: &Path,
    progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    let mut file = File::open(archive)?;
    let entries = read_zip_central_directory(&mut file)?;
    extract_zip_entries(file, entries, destination, progress, cancel_flag)
}

fn extract_zip_entries(
    mut file: File,
    entries: Vec<ZipReadEntry>,
    destination: &Path,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
) -> Result<()> {
    for entry in entries {
        check_archive_cancelled(cancel_flag)?;
        if entry.encrypted {
            return Err(archive_password_required_error());
        }
        let output_path = safe_output_path(destination, &entry.name)?;
        if entry.name.ends_with('/') {
            fs::create_dir_all(&output_path)?;
            continue;
        }

        let output_path = unique_path(&output_path, false);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        file.seek(SeekFrom::Start(entry.local_header_offset))?;
        let mut local_header = [0_u8; 30];
        file.read_exact(&mut local_header)?;
        if read_u32(&local_header, 0)? != ZIP_LOCAL_FILE_HEADER {
            return Err(BExplorerError::Operation(format!(
                "Invalid ZIP local header for {}",
                entry.name
            )));
        }

        let name_len = read_u16(&local_header, 26)? as u64;
        let extra_len = read_u16(&local_header, 28)? as u64;
        file.seek(SeekFrom::Current((name_len + extra_len) as i64))?;

        let mut output = BufWriter::new(File::create(&output_path)?);
        let bytes_and_crc = match entry.method {
            ZIP_METHOD_STORE => {
                let mut source = (&mut file).take(entry.compressed_size);
                copy_with_crc(
                    &mut source,
                    &mut output,
                    progress.as_deref_mut(),
                    cancel_flag,
                    &entry.name,
                )?
            }
            ZIP_METHOD_DEFLATE => {
                let source = (&mut file).take(entry.compressed_size);
                let mut decoder = DeflateDecoder::new(source);
                copy_with_crc(
                    &mut decoder,
                    &mut output,
                    progress.as_deref_mut(),
                    cancel_flag,
                    &entry.name,
                )?
            }
            method => {
                return Err(BExplorerError::Operation(format!(
                    "Unsupported ZIP method {method} in {}",
                    entry.name
                )));
            }
        };
        output.flush()?;

        if bytes_and_crc.0 != entry.uncompressed_size || bytes_and_crc.1 != entry.crc32 {
            return Err(BExplorerError::Operation(format!(
                "CRC or size mismatch while extracting {}",
                entry.name
            )));
        }
        if let Some(progress) = progress.as_deref_mut() {
            progress.finish_file(&entry.name);
        }
    }

    Ok(())
}

fn read_zip_central_directory(file: &mut File) -> Result<Vec<ZipReadEntry>> {
    let file_len = file.metadata()?.len();
    let search_len = file_len.min(66_000) as usize;
    file.seek(SeekFrom::End(-(search_len as i64)))?;
    let mut buffer = vec![0_u8; search_len];
    file.read_exact(&mut buffer)?;

    let mut eocd_at = None;
    for index in (0..search_len.saturating_sub(3)).rev() {
        if read_u32(&buffer, index)? == ZIP_END_OF_CENTRAL_DIRECTORY {
            eocd_at = Some(index);
            break;
        }
    }

    let Some(eocd_at) = eocd_at else {
        return Err(BExplorerError::Operation(
            "Could not find ZIP central directory".into(),
        ));
    };

    let entries = read_u16(&buffer, eocd_at + 10)? as usize;
    let central_size = read_u32(&buffer, eocd_at + 12)? as u64;
    let central_offset = read_u32(&buffer, eocd_at + 16)? as u64;

    if central_offset.saturating_add(central_size) > file_len {
        return Err(BExplorerError::Operation(
            "ZIP central directory points outside the archive".into(),
        ));
    }

    file.seek(SeekFrom::Start(central_offset))?;
    let mut output = Vec::with_capacity(entries);

    for _ in 0..entries {
        let mut header = [0_u8; 46];
        file.read_exact(&mut header)?;
        if read_u32(&header, 0)? != ZIP_CENTRAL_DIRECTORY_HEADER {
            return Err(BExplorerError::Operation(
                "Invalid ZIP central directory entry".into(),
            ));
        }

        let flags = read_u16(&header, 8)?;
        let method = read_u16(&header, 10)?;
        let dos_time = read_u16(&header, 12)?;
        let dos_date = read_u16(&header, 14)?;
        let crc32 = read_u32(&header, 16)?;
        let compressed_size = read_u32(&header, 20)? as u64;
        let uncompressed_size = read_u32(&header, 24)? as u64;
        let name_len = read_u16(&header, 28)? as usize;
        let extra_len = read_u16(&header, 30)? as usize;
        let comment_len = read_u16(&header, 32)? as usize;
        let local_header_offset = read_u32(&header, 42)? as u64;

        let mut name = vec![0_u8; name_len];
        file.read_exact(&mut name)?;
        file.seek(SeekFrom::Current((extra_len + comment_len) as i64))?;

        let name_str = String::from_utf8_lossy(&name).into_owned();
        let is_dir = name_str.ends_with('/');
        let modified = dos_time_date_to_system_time(dos_time, dos_date);

        output.push(ZipReadEntry {
            name: name_str,
            method,
            encrypted: flags & ZIP_ENCRYPTED_FLAG != 0,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
            is_dir,
            modified,
        });
    }

    Ok(output)
}

fn copy_with_crc<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    mut progress: Option<&mut ArchiveProgressEmitter>,
    cancel_flag: &AtomicU32,
    file_name: &str,
) -> Result<(u64, u32)> {
    let mut hasher = Hasher::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 1024 * 128];

    loop {
        check_archive_cancelled(cancel_flag)?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
        total = total.saturating_add(read as u64);
        if let Some(progress) = progress.as_deref_mut() {
            progress.add_bytes(read as u64, file_name);
        }
    }

    Ok((total, hasher.finalize()))
}

fn write_u16<W: Write>(writer: &mut W, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn read_u16(buffer: &[u8], offset: usize) -> Result<u16> {
    let bytes = buffer
        .get(offset..offset + 2)
        .ok_or_else(|| BExplorerError::Operation("Unexpected end of ZIP data".into()))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(buffer: &[u8], offset: usize) -> Result<u32> {
    let bytes = buffer
        .get(offset..offset + 4)
        .ok_or_else(|| BExplorerError::Operation("Unexpected end of ZIP data".into()))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn to_zip_u16(value: usize, label: &str) -> Result<u16> {
    u16::try_from(value).map_err(|_| BExplorerError::Operation(format!("{label} exceeds ZIP32")))
}

fn to_zip_u32(value: u64, label: &str) -> Result<u32> {
    u32::try_from(value).map_err(|_| BExplorerError::Operation(format!("{label} exceeds ZIP32")))
}

fn dos_time_date_to_system_time(dos_time: u16, dos_date: u16) -> Option<SystemTime> {
    let hour = ((dos_time >> 11) & 0x1F) as u32;
    let minute = ((dos_time >> 5) & 0x3F) as u32;
    let second = ((dos_time & 0x1F) * 2) as u32;
    let day = (dos_date & 0x1F) as u32;
    let month = ((dos_date >> 5) & 0x0F) as u32;
    let year = ((dos_date >> 9) & 0x7F) as u32 + 1980;

    let dt = chrono::NaiveDate::from_ymd_opt(year as i32, month, day)?
        .and_hms_opt(hour, minute, second)?;
    Some(dt.and_utc().into())
}

pub fn list_zip_entries(path: &Path) -> Result<Vec<ArchiveListEntry>> {
    let mut file = File::open(path)?;
    let zip_entries = read_zip_central_directory(&mut file)?;
    Ok(zip_entries
        .into_iter()
        .map(|e| ArchiveListEntry {
            name: e.name,
            is_dir: e.is_dir,
            size: if e.is_dir {
                None
            } else {
                Some(e.uncompressed_size)
            },
            pack_size: Some(e.compressed_size),
            modified: e.modified,
        })
        .collect())
}
