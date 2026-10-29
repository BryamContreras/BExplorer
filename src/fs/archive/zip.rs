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
    is_link: bool,
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

    let temp_path = temp_path_for(destination, "tmpzip");
    let temp_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;
    // Only arm cleanup after exclusive creation proves that we own this path.
    let temp_archive = TemporaryZipArchive(temp_path);
    let mut writer = BufWriter::new(temp_file);
    let mut central_entries = Vec::with_capacity(sources.len());

    let result = (|| {
        for source in sources {
            check_archive_cancelled(cancel_flag)?;
            let local_header_offset = writer.stream_position()?;
            let name = source.name.as_bytes().to_vec();

            let zip_method = if source.is_dir || method == ArchiveCompressionMethod::Store {
                ZIP_METHOD_STORE
            } else {
                ZIP_METHOD_DEFLATE
            };
            // Reserve ZIP64 size fields before streaming. Source files may
            // grow while being read, so metadata cannot choose ZIP32 safely.
            write_zip_local_header(&mut writer, &name, zip_method)?;
            let data_start = writer.stream_position()?;
            let (uncompressed_size, crc32) = if source.is_dir {
                (0, 0)
            } else {
                let mut input = BufReader::new(File::open(&source.path)?);
                if zip_method == ZIP_METHOD_STORE {
                    copy_with_crc(
                        &mut input,
                        &mut writer,
                        progress.as_deref_mut(),
                        cancel_flag,
                        &source.name,
                    )?
                } else {
                    let mut encoder = DeflateEncoder::new(&mut writer, method.zip_compression());
                    let result = copy_with_crc(
                        &mut input,
                        &mut encoder,
                        progress.as_deref_mut(),
                        cancel_flag,
                        &source.name,
                    )?;
                    encoder.finish()?;
                    result
                }
            };
            let data_end = writer.stream_position()?;
            let compressed_size = data_end - data_start;
            // CRC and sizes describe exactly the bytes from the one source
            // read, including when a source changes during compression.
            writer.seek(SeekFrom::Start(local_header_offset + 14))?;
            write_u32(&mut writer, crc32)?;
            writer.seek(SeekFrom::Start(
                local_header_offset + 30 + name.len() as u64 + 4,
            ))?;
            write_u64(&mut writer, uncompressed_size)?;
            write_u64(&mut writer, compressed_size)?;
            writer.seek(SeekFrom::Start(data_end))?;
            central_entries.push(ZipCentralEntry {
                name,
                method: zip_method,
                crc32,
                compressed_size,
                uncompressed_size,
                local_header_offset,
                external_attributes: if source.is_dir { 0x10 } else { 0 },
            });
            if let Some(progress) = progress.as_deref_mut() {
                progress.finish_file(&source.name);
            }
        }

        write_zip_central_directory(&mut writer, &central_entries)?;
        writer.flush()?;
        Ok::<(), BExplorerError>(())
    })();

    result?;
    drop(writer);

    // Both paths are on the destination filesystem; a copy fallback could
    // expose a partial archive and is unnecessary. Rename replaces an existing
    // file atomically, so an error cannot first destroy the old archive.
    fs::rename(&temp_archive.0, destination)?;

    Ok(())
}

struct TemporaryZipArchive(PathBuf);

impl Drop for TemporaryZipArchive {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn write_zip_local_header<W: Write>(writer: &mut W, name: &[u8], method: u16) -> Result<()> {
    let name_len = to_zip_u16(name.len(), "file name length")?;

    write_u32(writer, ZIP_LOCAL_FILE_HEADER)?;
    write_u16(writer, 45)?;
    write_u16(writer, ZIP_UTF8_FLAG)?;
    write_u16(writer, method)?;
    write_u16(writer, DOS_TIME_MIDNIGHT)?;
    write_u16(writer, DOS_DATE_1980_01_01)?;
    write_u32(writer, 0)?;
    write_u32(writer, u32::MAX)?;
    write_u32(writer, u32::MAX)?;
    write_u16(writer, name_len)?;
    write_u16(writer, 20)?;
    writer.write_all(name)?;
    write_u16(writer, 1)?; // ZIP64 extended information
    write_u16(writer, 16)?;
    write_u64(writer, 0)?;
    write_u64(writer, 0)?;

    Ok(())
}

fn write_zip_central_directory<W: Write + Seek>(
    writer: &mut W,
    entries: &[ZipCentralEntry],
) -> Result<()> {
    let central_offset = writer.stream_position()?;

    for entry in entries {
        write_u32(writer, ZIP_CENTRAL_DIRECTORY_HEADER)?;
        write_u16(writer, 45)?;
        write_u16(writer, 45)?;
        write_u16(writer, ZIP_UTF8_FLAG)?;
        write_u16(writer, entry.method)?;
        write_u16(writer, DOS_TIME_MIDNIGHT)?;
        write_u16(writer, DOS_DATE_1980_01_01)?;
        write_u32(writer, entry.crc32)?;
        write_u32(writer, u32::MAX)?;
        write_u32(writer, u32::MAX)?;
        write_u16(writer, to_zip_u16(entry.name.len(), "file name length")?)?;
        write_u16(writer, 28)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, entry.external_attributes)?;
        write_u32(writer, u32::MAX)?;
        writer.write_all(&entry.name)?;
        write_u16(writer, 1)?;
        write_u16(writer, 24)?;
        write_u64(writer, entry.uncompressed_size)?;
        write_u64(writer, entry.compressed_size)?;
        write_u64(writer, entry.local_header_offset)?;
    }

    let central_size = writer.stream_position()?.saturating_sub(central_offset);
    let zip64_offset = writer.stream_position()?;
    write_u32(writer, 0x0606_4b50)?;
    write_u64(writer, 44)?;
    write_u16(writer, 45)?;
    write_u16(writer, 45)?;
    write_u32(writer, 0)?;
    write_u32(writer, 0)?;
    write_u64(writer, entries.len() as u64)?;
    write_u64(writer, entries.len() as u64)?;
    write_u64(writer, central_size)?;
    write_u64(writer, central_offset)?;
    write_u32(writer, 0x0706_4b50)?;
    write_u32(writer, 0)?;
    write_u64(writer, zip64_offset)?;
    write_u32(writer, 1)?;
    write_u32(writer, ZIP_END_OF_CENTRAL_DIRECTORY)?;
    write_u16(writer, 0)?;
    write_u16(writer, 0)?;
    write_u16(writer, u16::MAX)?;
    write_u16(writer, u16::MAX)?;
    write_u32(writer, u32::MAX)?;
    write_u32(writer, u32::MAX)?;
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
    let output_root = output::ExtractionRoot::new(destination)?;
    let archive_len = file.metadata()?.len();
    for entry in entries {
        check_archive_cancelled(cancel_flag)?;
        if entry.encrypted {
            return Err(archive_password_required_error());
        }
        safe_output_path(destination, &entry.name)?;
        if entry.is_link {
            return Err(BExplorerError::Operation(format!(
                "Archive links are not extracted: {}",
                entry.name
            )));
        }
        if entry.is_dir {
            output_root.create_directory(&entry.name)?;
            continue;
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
        let data_start = file.stream_position()?;
        if data_start
            .checked_add(entry.compressed_size)
            .is_none_or(|end| end > archive_len)
        {
            return Err(BExplorerError::Operation(format!(
                "ZIP file data points outside the archive: {}",
                entry.name
            )));
        }

        let mut output = BufWriter::new(output_root.create_file(&entry.name)?);
        let bytes_and_crc = match entry.method {
            ZIP_METHOD_STORE => {
                let mut source = (&mut file).take(
                    entry
                        .compressed_size
                        .min(entry.uncompressed_size.saturating_add(1)),
                );
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
                let mut decoder =
                    DeflateDecoder::new(source).take(entry.uncompressed_size.saturating_add(1));
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
    read_zip_central_directory_cancellable(file, None)
}

fn read_zip_central_directory_cancellable(
    file: &mut File,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Vec<ZipReadEntry>> {
    let file_len = file.metadata()?.len();
    let search_len = file_len.min(66_000) as usize;
    file.seek(SeekFrom::End(-(search_len as i64)))?;
    let mut buffer = vec![0_u8; search_len];
    file.read_exact(&mut buffer)?;

    let mut eocd_at = None;
    for index in (0..=search_len.saturating_sub(22)).rev() {
        if buffer.get(index..index + 4) == Some(&ZIP_END_OF_CENTRAL_DIRECTORY.to_le_bytes())
            && index + 22 + read_u16(&buffer, index + 20)? as usize == search_len
        {
            eocd_at = Some(index);
            break;
        }
    }

    let Some(eocd_at) = eocd_at else {
        return Err(BExplorerError::Operation(
            "Could not find ZIP central directory".into(),
        ));
    };

    let mut entries = read_u16(&buffer, eocd_at + 10)? as u64;
    let mut central_size = read_u32(&buffer, eocd_at + 12)? as u64;
    let mut central_offset = read_u32(&buffer, eocd_at + 16)? as u64;
    if read_u16(&buffer, eocd_at + 4)? != 0 || read_u16(&buffer, eocd_at + 6)? != 0 {
        return Err(BExplorerError::Operation(
            "Multi-volume ZIP archives are not supported".into(),
        ));
    }
    'zip64: {
        if entries == u16::MAX as u64
            || central_size == u32::MAX as u64
            || central_offset == u32::MAX as u64
        {
            let eocd_offset = file_len - search_len as u64 + eocd_at as u64;
            let locator_offset = eocd_offset
                .checked_sub(20)
                .ok_or_else(|| BExplorerError::Operation("Missing ZIP64 locator".into()))?;
            file.seek(SeekFrom::Start(locator_offset))?;
            let mut locator = [0; 20];
            file.read_exact(&mut locator)?;
            // ZIP32 writers also use 0xffff for exactly 65,535 entries. In that
            // one ambiguous case, a missing locator is a valid legacy archive.
            if read_u32(&locator, 0)? != 0x0706_4b50
                && entries == u16::MAX as u64
                && central_size != u32::MAX as u64
                && central_offset != u32::MAX as u64
            {
                break 'zip64;
            }
            if read_u32(&locator, 0)? != 0x0706_4b50
                || read_u32(&locator, 4)? != 0
                || read_u32(&locator, 16)? != 1
            {
                return Err(BExplorerError::Operation(
                    "Invalid or multi-volume ZIP64 locator".into(),
                ));
            }
            let record_offset = read_u64(&locator, 8)?;
            if record_offset
                .checked_add(56)
                .is_none_or(|end| end > locator_offset)
            {
                return Err(BExplorerError::Operation(
                    "ZIP64 end record points outside the archive".into(),
                ));
            }
            file.seek(SeekFrom::Start(record_offset))?;
            let mut record = [0; 56];
            file.read_exact(&mut record)?;
            if read_u32(&record, 0)? != 0x0606_4b50
                || read_u64(&record, 4)? < 44
                || record_offset
                    .checked_add(12)
                    .and_then(|at| at.checked_add(read_u64(&record, 4).ok()?))
                    .is_none_or(|end| end > locator_offset)
                || read_u32(&record, 16)? != 0
                || read_u32(&record, 20)? != 0
                || read_u64(&record, 24)? != read_u64(&record, 32)?
            {
                return Err(BExplorerError::Operation(
                    "Invalid or multi-volume ZIP64 end record".into(),
                ));
            }
            entries = read_u64(&record, 32)?;
            central_size = read_u64(&record, 40)?;
            central_offset = read_u64(&record, 48)?;
        }
    }

    let central_end = central_offset
        .checked_add(central_size)
        .filter(|end| *end <= file_len)
        .ok_or_else(|| {
            BExplorerError::Operation("ZIP central directory points outside the archive".into())
        })?;
    if entries > central_size / 46 {
        return Err(BExplorerError::Operation(
            "ZIP entry count exceeds the central directory size".into(),
        ));
    }

    file.seek(SeekFrom::Start(central_offset))?;
    let mut output = Vec::new();

    for _ in 0..entries {
        if cancel.is_some_and(|flag| flag.load(AtomicOrdering::Relaxed)) {
            return Err(BExplorerError::Operation(
                "Archive listing cancelled".into(),
            ));
        }
        if file
            .stream_position()?
            .checked_add(46)
            .is_none_or(|end| end > central_end)
        {
            return Err(BExplorerError::Operation(
                "Truncated ZIP central directory".into(),
            ));
        }
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
        let mut compressed_size = read_u32(&header, 20)? as u64;
        let mut uncompressed_size = read_u32(&header, 24)? as u64;
        let name_len = read_u16(&header, 28)? as usize;
        let extra_len = read_u16(&header, 30)? as usize;
        let comment_len = read_u16(&header, 32)? as usize;
        let mut local_header_offset = read_u32(&header, 42)? as u64;
        let external_attributes = read_u32(&header, 38)?;
        let mut disk = read_u16(&header, 34)? as u32;
        if file
            .stream_position()?
            .checked_add((name_len + extra_len + comment_len) as u64)
            .is_none_or(|end| end > central_end)
        {
            return Err(BExplorerError::Operation(
                "ZIP entry extends past the central directory".into(),
            ));
        }

        let mut name = vec![0_u8; name_len];
        file.read_exact(&mut name)?;
        let mut extra = vec![0; extra_len];
        file.read_exact(&mut extra)?;
        file.seek(SeekFrom::Current(comment_len as i64))?;
        if compressed_size == u32::MAX as u64
            || uncompressed_size == u32::MAX as u64
            || local_header_offset == u32::MAX as u64
            || disk == u16::MAX as u32
        {
            let zip64 = zip64_extra(&extra)?;
            let mut offset = 0;
            if uncompressed_size == u32::MAX as u64 {
                uncompressed_size = read_u64(zip64, offset)?;
                offset += 8;
            }
            if compressed_size == u32::MAX as u64 {
                compressed_size = read_u64(zip64, offset)?;
                offset += 8;
            }
            if local_header_offset == u32::MAX as u64 {
                local_header_offset = read_u64(zip64, offset)?;
                offset += 8;
            }
            if disk == u16::MAX as u32 {
                disk = read_u32(zip64, offset)?;
            }
        }
        if disk != 0 {
            return Err(BExplorerError::Operation(
                "Multi-volume ZIP entries are not supported".into(),
            ));
        }
        if local_header_offset
            .checked_add(30)
            .is_none_or(|end| end > central_offset)
        {
            return Err(BExplorerError::Operation(
                "ZIP local header points outside file data".into(),
            ));
        }

        let name_str = String::from_utf8_lossy(&name).into_owned();
        let is_dir =
            name_str.ends_with('/') || name_str.ends_with('\\') || external_attributes & 0x10 != 0;
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
            is_link: (external_attributes >> 16) & 0xf000 == 0xa000,
            modified,
        });
    }

    Ok(output)
}

fn zip64_extra(extra: &[u8]) -> Result<&[u8]> {
    let mut offset = 0;
    while offset + 4 <= extra.len() {
        let kind = read_u16(extra, offset)?;
        let size = read_u16(extra, offset + 2)? as usize;
        let data = extra
            .get(offset + 4..offset + 4 + size)
            .ok_or_else(|| BExplorerError::Operation("Truncated ZIP extra field".into()))?;
        if kind == 1 {
            return Ok(data);
        }
        offset += 4 + size;
    }
    Err(BExplorerError::Operation(
        "Missing ZIP64 extended information".into(),
    ))
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

fn write_u64<W: Write>(writer: &mut W, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn read_u64(buffer: &[u8], offset: usize) -> Result<u64> {
    let bytes: [u8; 8] = buffer
        .get(offset..offset + 8)
        .ok_or_else(|| BExplorerError::Operation("Unexpected end of ZIP64 data".into()))?
        .try_into()
        .map_err(|_| BExplorerError::Operation("Invalid ZIP64 value".into()))?;
    Ok(u64::from_le_bytes(bytes))
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
    list_zip_entries_cancellable(path, None)
}

pub(super) fn list_zip_entries_cancellable(
    path: &Path,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Vec<ArchiveListEntry>> {
    let mut file = File::open(path)?;
    let zip_entries = read_zip_central_directory_cancellable(&mut file, cancel)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip64_store_and_deflate_round_trip_with_native_and_7zip_readers() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let source_dir = staging.path().join("source");
        fs::create_dir(&source_dir).unwrap();
        let source = source_dir.join("payload.txt");
        let bytes = b"ZIP64 payload with a stable CRC and no source-side temporary file";
        fs::write(&source, bytes).unwrap();
        let cancel = AtomicU32::new(0);
        for method in [
            ArchiveCompressionMethod::Store,
            ArchiveCompressionMethod::Normal,
        ] {
            let archive = staging.path().join(format!("{method:?}.zip"));
            create_zip_archive(std::slice::from_ref(&source), &archive, method, &cancel).unwrap();
            let listed = list_zip_entries(&archive).unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].size, Some(bytes.len() as u64));
            let native = staging.path().join(format!("native-{method:?}"));
            extract_zip_archive(&archive, &native, &cancel).unwrap();
            assert_eq!(fs::read(native.join("payload.txt")).unwrap(), bytes);
            let embedded = staging.path().join(format!("embedded-{method:?}"));
            extract_with_7zip(&archive, &embedded, None, &cancel).unwrap();
            assert_eq!(fs::read(embedded.join("payload.txt")).unwrap(), bytes);
        }
        assert_eq!(fs::read_dir(&source_dir).unwrap().count(), 1);
        assert!(!fs::read_dir(staging.path()).unwrap().any(|item| {
            item.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".bexplorer-")
        }));
    }

    #[test]
    fn cancelled_zip_preserves_destination_and_removes_temporary_output() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let source = staging.path().join("source.txt");
        let destination = staging.path().join("existing.zip");
        fs::write(&source, b"source").unwrap();
        fs::write(&destination, b"existing archive").unwrap();
        assert!(
            create_zip_archive(
                std::slice::from_ref(&source),
                &destination,
                ArchiveCompressionMethod::Normal,
                &AtomicU32::new(1)
            )
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), b"existing archive");
        assert_eq!(fs::read_dir(staging.path()).unwrap().count(), 2);
        create_zip_archive(
            &[source],
            &destination,
            ArchiveCompressionMethod::Store,
            &AtomicU32::new(0),
        )
        .unwrap();
        assert_eq!(list_zip_entries(&destination).unwrap()[0].size, Some(6));
    }

    #[test]
    fn zip64_metadata_keeps_sizes_above_four_gibibytes() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let archive = staging.path().join("large-metadata.zip");
        let mut file = File::create(&archive).unwrap();
        write_zip_local_header(&mut file, b"large.bin", ZIP_METHOD_STORE).unwrap();
        let size = u32::MAX as u64 + 4096;
        write_zip_central_directory(
            &mut file,
            &[ZipCentralEntry {
                name: b"large.bin".to_vec(),
                method: ZIP_METHOD_STORE,
                crc32: 0,
                compressed_size: size,
                uncompressed_size: size,
                local_header_offset: 0,
                external_attributes: 0,
            }],
        )
        .unwrap();
        drop(file);
        let entries = list_zip_entries(&archive).unwrap();
        assert_eq!(entries[0].size, Some(size));
        assert_eq!(entries[0].pack_size, Some(size));
        // The metadata fixture intentionally has no multi-gigabyte payload.
        assert!(
            extract_zip_archive(&archive, &staging.path().join("out"), &AtomicU32::new(0)).is_err()
        );
    }

    #[test]
    fn zip64_entry_count_exceeds_zip32_without_truncating_the_listing() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let archive = staging.path().join("many-entries.zip");
        let mut file = BufWriter::new(File::create(&archive).unwrap());
        write_zip_local_header(&mut file, b"empty", ZIP_METHOD_STORE).unwrap();
        let entry = ZipCentralEntry {
            name: b"empty".to_vec(),
            method: ZIP_METHOD_STORE,
            crc32: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            local_header_offset: 0,
            external_attributes: 0,
        };
        write_zip_central_directory(&mut file, &vec![entry; u16::MAX as usize + 1]).unwrap();
        file.flush().unwrap();
        drop(file);
        assert_eq!(
            list_zip_entries(&archive).unwrap().len(),
            u16::MAX as usize + 1
        );
    }

    #[test]
    fn zip32_exactly_65535_entries_does_not_require_a_zip64_locator() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let archive = staging.path().join("zip32-limit.zip");
        let mut file = BufWriter::new(File::create(&archive).unwrap());
        let mut local = [0; 30];
        local[..4].copy_from_slice(&ZIP_LOCAL_FILE_HEADER.to_le_bytes());
        local[4..6].copy_from_slice(&20_u16.to_le_bytes());
        local[26..28].copy_from_slice(&5_u16.to_le_bytes());
        file.write_all(&local).unwrap();
        file.write_all(b"empty").unwrap();
        let central_offset = file.stream_position().unwrap();
        let mut central = [0; 46];
        central[..4].copy_from_slice(&ZIP_CENTRAL_DIRECTORY_HEADER.to_le_bytes());
        central[4..6].copy_from_slice(&20_u16.to_le_bytes());
        central[6..8].copy_from_slice(&20_u16.to_le_bytes());
        central[28..30].copy_from_slice(&5_u16.to_le_bytes());
        for _ in 0..u16::MAX {
            file.write_all(&central).unwrap();
            file.write_all(b"empty").unwrap();
        }
        let central_size = file.stream_position().unwrap() - central_offset;
        write_u32(&mut file, ZIP_END_OF_CENTRAL_DIRECTORY).unwrap();
        write_u16(&mut file, 0).unwrap();
        write_u16(&mut file, 0).unwrap();
        write_u16(&mut file, u16::MAX).unwrap();
        write_u16(&mut file, u16::MAX).unwrap();
        write_u32(&mut file, central_size as u32).unwrap();
        write_u32(&mut file, central_offset as u32).unwrap();
        write_u16(&mut file, 0).unwrap();
        file.flush().unwrap();
        drop(file);
        assert_eq!(list_zip_entries(&archive).unwrap().len(), u16::MAX as usize);
    }

    #[cfg(windows)]
    #[test]
    fn real_zip_extraction_rejects_an_existing_junction() {
        let staging = TemporaryExtractDirectory::new().unwrap();
        let source = staging.path().join("source");
        fs::create_dir_all(source.join("link")).unwrap();
        fs::write(source.join("link/escaped.txt"), b"must stay inside").unwrap();
        let archive = staging.path().join("junction.zip");
        create_zip_archive(
            &[source.join("link")],
            &archive,
            ArchiveCompressionMethod::Store,
            &AtomicU32::new(0),
        )
        .unwrap();
        let destination = staging.path().join("destination");
        let outside = staging.path().join("outside");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&outside).unwrap();
        let junction = destination.join("link");
        let status = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command",
            "New-Item -ItemType Junction -Path $env:BEXPLORER_TEST_LINK -Target $env:BEXPLORER_TEST_TARGET -ErrorAction Stop | Out-Null"])
            .env("BEXPLORER_TEST_LINK", &junction).env("BEXPLORER_TEST_TARGET", &outside)
            .creation_flags(CREATE_NO_WINDOW).status().unwrap();
        assert!(status.success(), "create owned test junction");
        let result = extract_zip_archive(&archive, &destination, &AtomicU32::new(0));
        let embedded_result = extract_with_7zip(&archive, &destination, None, &AtomicU32::new(0));
        fs::remove_dir(&junction).unwrap();
        assert!(result.is_err());
        assert!(embedded_result.is_err());
        assert!(!outside.join("escaped.txt").exists());
    }
}
