//! Demangle Rust symbols in a Mach-O binary's symbol table, in place.

use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use object::Endianness;
use object::macho;
use object::read::macho::{LoadCommandVariant, MachHeader, Segment};

/// Metadata extracted from a Mach-O binary's load commands.
struct MachoInfo {
    /// Byte ranges containing null-terminated symbol name strings that we can
    /// patch in place. Typically the `LC_SYMTAB` string table and the
    /// `__debug_str` DWARF section.
    string_regions: Vec<Range<usize>>,
    /// The binary's UUID from `LC_UUID`, used to invalidate the
    /// `coresymbolicationd` symbol cache after patching.
    uuid: Option<String>,
}

/// Demangle Rust symbol names in the binary and its dSYM bundle (if present).
///
/// This is a hack, but it makes it much easier to read the names of functions
/// in Instruments, so instead of:
///
/// ```no_format
/// _$LT$alloc..vec..Vec$LT$T$C$A$GT$$u20$as$u20$alloc..vec..spec_extend..SpecExtend$LT$T$C$I$GT$$GT$::spec_extend::hb9d6b49df4cfa5e3
/// ```
/// we get,
///
/// ```no_format
/// <alloc::vec::Vec<T, A> as alloc::vec::spec_extend::SpecExtend<T, I>>::spec_extend
/// ```
pub fn demangle_binary(path: &Path) -> Result<()> {
    let uuid = demangle_macho(path)?;

    // Invalidate the coresymbolicationd symbol cache for this binary, just in
    // case a previous profiling session cached the old (mangled) names.
    // Best-effort: symbolscache may not exist or the daemon may not be running.
    if let Some(uuid) = uuid {
        invalidate_symbol_cache(&uuid);
    }

    if let Some(dsym_path) = find_dsym_dwarf(path) {
        demangle_macho(&dsym_path)?;
    }

    Ok(())
}

/// Find the DWARF binary inside a dSYM bundle adjacent to `path`.
///
/// Given `target/debug/foo`, looks for
/// `target/debug/foo.dSYM/Contents/Resources/DWARF/*` and returns
/// the first Mach-O file found (the DWARF file name may include a hash).
fn find_dsym_dwarf(binary_path: &Path) -> Option<PathBuf> {
    let file_name = binary_path.file_name()?;
    let dsym_dir = binary_path.parent()?.join(format!("{}.dSYM", file_name.to_str()?));
    let dwarf_dir = dsym_dir.join("Contents/Resources/DWARF");
    let entry = std::fs::read_dir(&dwarf_dir).ok()?.next()?.ok()?;
    Some(entry.path())
}

/// Demangle Rust symbol names in a Mach-O file's string tables, in place.
///
/// Patches both the `LC_SYMTAB` string table and the `__debug_str` DWARF
/// section (which is what `atos`/Instruments actually reads).
/// Returns the binary's UUID if found.
fn demangle_macho(path: &Path) -> Result<Option<String>> {
    let mut data = std::fs::read(path)?;
    let info = parse_macho(&data)?;

    if info.string_regions.is_empty() {
        log::debug!("no string tables found in {}, skipping", path.display());
        return Ok(info.uuid);
    }

    let mut total = 0usize;
    for region in &info.string_regions {
        total += demangle_region(&mut data, region.clone());
    }

    if total > 0 {
        std::fs::write(path, &data)?;
        log::debug!("demangled {total} symbols in {}", path.display());
    }

    Ok(info.uuid)
}

/// Walk a region of \0 terminated strings, demangling in place.
///
/// Returns the number of names replaced.
fn demangle_region(data: &mut [u8], region: Range<usize>) -> usize {
    let end = region.end;
    let mut pos = region.start;
    let mut count = 0;

    while pos < end {
        let nul = data[pos..end].iter().position(|&b| b == 0).map(|i| pos + i).unwrap_or(end);

        let name = std::str::from_utf8(&data[pos..nul]);
        let available = nul - pos;
        if let Ok(name) = name {
            let demangled = format!("{:#}", rustc_demangle::demangle(name));
            if demangled != name && demangled.len() <= available {
                data[pos..pos + demangled.len()].copy_from_slice(demangled.as_bytes());
                data[pos + demangled.len()] = 0;
                count += 1;
            }
        }

        pos = nul + 1;
    }

    count
}

/// Extract the string regions and UUID from a Mach-O binary's load commands.
fn parse_macho(data: &[u8]) -> Result<MachoInfo> {
    let header = macho::MachHeader64::<Endianness>::parse(data, 0)
        .map_err(|e| anyhow!("failed to parse Mach-O header: {e}"))?;
    let endian = header.endian().map_err(|e| anyhow!("bad endianness: {e}"))?;

    let mut string_regions = Vec::new();
    let mut uuid = None;

    let mut commands = header
        .load_commands(endian, data, 0)
        .map_err(|e| anyhow!("failed to read load commands: {e}"))?;

    while let Some(command) =
        commands.next().map_err(|e| anyhow!("failed to iterate load commands: {e}"))?
    {
        let variant =
            command.variant().map_err(|e| anyhow!("failed to parse load command: {e}"))?;

        match variant {
            LoadCommandVariant::Symtab(symtab) => {
                let off = symtab.stroff.get(endian) as usize;
                let size = symtab.strsize.get(endian) as usize;
                if off + size <= data.len() {
                    string_regions.push(off..off + size);
                }
            }
            LoadCommandVariant::Segment64(segment, section_data) => {
                if let Ok(sections) = segment.sections(endian, section_data) {
                    for section in sections {
                        if section.sectname.starts_with(b"__debug_str\0") {
                            let off = section.offset.get(endian) as usize;
                            let size = section.size.get(endian) as usize;
                            if off + size <= data.len() {
                                string_regions.push(off..off + size);
                            }
                        }
                    }
                }
            }
            LoadCommandVariant::Uuid(cmd) => {
                uuid = Some(format_uuid(&cmd.uuid));
            }
            _ => {}
        }
    }

    Ok(MachoInfo { string_regions, uuid })
}

/// Invalidate the symbol cache for a binary without demangling it.
///
/// Used when `--no-demangle` is passed: we still want to clear any stale
/// cached symbols from a previous profiling session.
pub fn just_invalidate_symbol_cache(path: &Path) {
    let uuid = std::fs::read(path).ok().and_then(|data| parse_uuid(&data));

    if let Some(uuid) = uuid {
        invalidate_symbol_cache(&uuid);
    }
}

/// Extract just the UUID from a Mach-O binary's load commands.
fn parse_uuid(data: &[u8]) -> Option<String> {
    let header = macho::MachHeader64::<Endianness>::parse(data, 0).ok()?;
    let endian = header.endian().ok()?;
    let mut commands = header.load_commands(endian, data, 0).ok()?;

    while let Some(command) = commands.next().ok()? {
        if let Ok(LoadCommandVariant::Uuid(cmd)) = command.variant() {
            return Some(format_uuid(&cmd.uuid));
        }
    }

    None
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    use std::fmt::Write;
    let write_bytes = |s: &mut String, bytes, suffix| {
        for b in bytes {
            write!(s, "{b:02X}").unwrap()
        }
        s.push_str(suffix)
    };
    let mut out = String::new();
    write_bytes(&mut out, &bytes[0..4], "-");
    write_bytes(&mut out, &bytes[4..6], "-");
    write_bytes(&mut out, &bytes[6..8], "-");
    write_bytes(&mut out, &bytes[8..10], "-");
    write_bytes(&mut out, &bytes[10..16], "");
    out
}

/// Invalidate the `coresymbolicationd` symbol cache for a given UUID.
///
/// macOS caches symbolication data per-binary (keyed by UUID) via the
/// `coresymbolicationd` daemon. If the user previously profiled this binary,
/// the cache may contain the old mangled names. This is best-effort: we
/// silently ignore failures since `symbolscache` may not be available.
fn invalidate_symbol_cache(uuid: &str) {
    match std::process::Command::new("symbolscache").args(["delete", uuid]).output() {
        Ok(output) if output.status.success() => {
            log::debug!("invalidated symbol cache for {uuid}");
        }
        _ => {
            log::debug!("symbolscache delete for {uuid} failed or not available (this is ok)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn demangle_test_binary() {
        // Work on a copy of our own test binary
        let self_exe = std::env::current_exe().unwrap();
        let dir = std::env::temp_dir().join("cargo_instruments_test");
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("test_bin");
        std::fs::copy(&self_exe, &bin).unwrap();

        // verify mangled symbols exist and demangled ones don't
        let before = Command::new("nm").arg("--no-demangle").arg(&bin).output().unwrap();
        let before = String::from_utf8_lossy(&before.stdout);
        assert!(before.contains("_ZN"), "expected mangled symbols in binary");
        assert!(
            !before.contains("demangle::demangle_region"),
            "demangled symbol should not be present before patching"
        );

        // run demangling
        demangle_binary(&bin).unwrap();

        // verify our own symbols are demangled in the binary itself
        let after = Command::new("nm").arg("--no-demangle").arg(&bin).output().unwrap();
        let after = String::from_utf8_lossy(&after.stdout);
        assert!(
            after.contains("demangle::demangle_region"),
            "expected demangled symbol 'demangle::demangle_region', got:\n{after}"
        );
    }

    #[test]
    fn format_uuid_inserts_hyphens() {
        let bytes: [u8; 16] = [
            0x55, 0x0E, 0x84, 0x00, 0xE2, 0x9B, 0x41, 0xD4, 0xA7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ];
        assert_eq!(format_uuid(&bytes), "550E8400-E29B-41D4-A716-446655440000");
    }
}
