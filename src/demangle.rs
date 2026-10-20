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
            // v0 mangled names can be shorter than their demangled form, since
            // repeated paths are encoded as backreferences
            let demangled = shorten(demangled, available);
            if let Some(demangled) = demangled.filter(|demangled| demangled != name) {
                data[pos..pos + demangled.len()].copy_from_slice(demangled.as_bytes());
                data[pos + demangled.len()] = 0;
                count += 1;
            }
        }

        pos = nul + 1;
    }

    count
}

/// Generic arguments shorter than this are never replaced by [`shorten`].
const MIN_ERASED_ARG_LEN: usize = 8;

/// Shorten a demangled name to at most `max_len` bytes, if possible.
///
/// Each step is lossier than the last, and is only taken if the name is still
/// too long: first the largest generic arguments are replaced with `_`, one at
/// a time; then the inner segments of each path are elided; and finally each
/// path is reduced to its last segment.
fn shorten(mut name: String, max_len: usize) -> Option<String> {
    while name.len() > max_len {
        let Some(arg) = generic_args(&name)
            .into_iter()
            .filter(|arg| arg.len() >= MIN_ERASED_ARG_LEN)
            .max_by_key(|arg| arg.len())
        else {
            break;
        };
        name.replace_range(arg, "_");
    }
    if name.len() <= max_len {
        return Some(name);
    }

    [1, 0].into_iter().map(|keep_first| elide_paths(&name, keep_first)).find(|n| n.len() <= max_len)
}

/// Return the byte range of each generic argument in a demangled name.
///
/// This includes arguments nested inside other arguments.
fn generic_args(name: &str) -> Vec<Range<usize>> {
    let bytes = name.as_bytes();
    let mut args = Vec::new();
    // one entry per open bracket, holding the start of the current argument
    // if that bracket opened a list of generic arguments
    let mut open: Vec<Option<usize>> = Vec::new();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            // a `<` directly after a path opens a list of generic arguments,
            // as in `Vec<u8>` or `size_of::<u8>`; otherwise it starts a
            // qualified path, as in `<Vec<u8> as Clone>::clone`
            b'<' => {
                let prev = i.checked_sub(1).map(|j| bytes[j]);
                let follows_path = prev.is_some_and(|prev| {
                    prev.is_ascii_alphanumeric() || !prev.is_ascii() || matches!(prev, b'_' | b':')
                });
                open.push(follows_path.then_some(i + 1));
            }
            b'(' | b'[' | b'{' => open.push(None),
            // the arrow in `fn() -> T`
            b'>' if i > 0 && bytes[i - 1] == b'-' => (),
            b'>' | b')' | b']' | b'}' => {
                if let Some(Some(start)) = open.pop() {
                    args.push(start..i);
                }
            }
            b',' => {
                if let Some(Some(start)) = open.last_mut() {
                    args.push(*start..i);
                    *start = if bytes.get(i + 1) == Some(&b' ') { i + 2 } else { i + 1 };
                }
            }
            _ => (),
        }
    }
    args
}

/// Shorten paths in a demangled name by eliding their inner segments.
///
/// Each path keeps its first `keep_first` segments and its last one, and the
/// segments in between are replaced with `..`. With a `keep_first` of 1,
/// `core::iter::adapters::map::map_try_fold` becomes `core::..::map_try_fold`;
/// with 0, it becomes `..::map_try_fold`.
fn elide_paths(name: &str, keep_first: usize) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(c) = rest.chars().next() {
        let segments = path_segments(rest);
        if segments.is_empty() {
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        }

        let path_len = segments.iter().map(|seg| seg.len() + 2).sum::<usize>() - 2;
        // trailing segments like `{closure#0}` stay with the item they belong to
        let keep_last = 1 + segments.iter().rev().take_while(|seg| seg.starts_with('{')).count();
        if segments.len() > keep_first + keep_last {
            for seg in &segments[..keep_first] {
                out.push_str(seg);
                out.push_str("::");
            }
            out.push_str("..::");
            out.push_str(&segments[segments.len() - keep_last..].join("::"));
        } else {
            out.push_str(&rest[..path_len]);
        }
        rest = &rest[path_len..];
    }
    out
}

/// Return the segments of the `::`-separated path at the start of `s`.
///
/// The first segment must be an identifier; later segments may also be
/// braced, like `{closure#0}`. Returns an empty vec if `s` does not start
/// with an identifier.
fn path_segments(s: &str) -> Vec<&str> {
    fn ident_len(s: &str) -> usize {
        s.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(s.len())
    }

    fn braced_len(s: &str) -> usize {
        let mut depth = 0;
        for (i, c) in s.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => (),
            }
        }
        0
    }

    let mut segments = Vec::new();
    let mut len = ident_len(s);
    if len == 0 {
        return segments;
    }
    segments.push(&s[..len]);

    while let Some(next) = s[len..].strip_prefix("::") {
        let seg_len = if next.starts_with('{') { braced_len(next) } else { ident_len(next) };
        if seg_len == 0 {
            break;
        }
        segments.push(&next[..seg_len]);
        len += 2 + seg_len;
    }
    segments
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
        // rustc may use either the legacy (`_ZN`) or v0 (`_R`) mangling scheme
        assert!(
            before.split_whitespace().any(|s| rustc_demangle::try_demangle(s).is_ok()),
            "expected mangled symbols in binary"
        );
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
    fn elide_long_paths() {
        let name = "core::iter::adapters::map::map_try_fold::<u8>";
        assert_eq!(elide_paths(name, 1), "core::..::map_try_fold::<u8>");
        assert_eq!(elide_paths(name, 0), "..::map_try_fold::<u8>");
        assert_eq!(
            elide_paths("<core::slice::Iter<u8> as core::iter::Iterator>::next", 1),
            "<core::..::Iter<u8> as core::..::Iterator>::next"
        );
        assert_eq!(
            elide_paths("<a::b::E>::run::{closure#0}::{closure#1}", 1),
            "<a::..::E>::run::{closure#0}::{closure#1}"
        );
        assert_eq!(elide_paths("a::b::c::{closure#0}", 1), "a::..::c::{closure#0}");
    }

    #[test]
    fn elide_leaves_short_paths() {
        let name = "<alloc::Vec<u8>>::get::<&[u8; 4]>";
        assert_eq!(elide_paths(name, 1), name);
    }

    #[test]
    fn find_generic_args() {
        let name = "<Vec<u8> as Into<Box<[u8]>>>::into::<fn(u32) -> u8, ()>";
        let args: Vec<_> = generic_args(name).into_iter().map(|arg| &name[arg]).collect();
        assert_eq!(args, ["u8", "[u8]", "Box<[u8]>", "fn(u32) -> u8", "()"]);
    }

    #[test]
    fn shorten_erases_largest_args_first() {
        let name = "foo::<alloc::string::String, u8, core::option::Option<alloc::string::String>>";
        assert_eq!(shorten(name.into(), name.len()).as_deref(), Some(name));
        assert_eq!(
            shorten(name.into(), 60).as_deref(),
            Some("foo::<alloc::string::String, u8, _>")
        );
        assert_eq!(shorten(name.into(), 30).as_deref(), Some("foo::<_, u8, _>"));
        assert_eq!(shorten(name.into(), 10), None);
    }

    #[test]
    fn shorten_keeps_small_args() {
        let name = "<alloc::vec::Vec<u8> as core::fmt::Debug>::fmt";
        assert_eq!(
            shorten(name.into(), 44).as_deref(),
            Some("<alloc::..::Vec<u8> as core::..::Debug>::fmt")
        );
        assert_eq!(shorten(name.into(), 33).as_deref(), Some("<..::Vec<u8> as ..::Debug>::fmt"));
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
