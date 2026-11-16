//! Files on the clipboard as text: the paths this program puts there, and
//! the `file://` addresses or paths other programs do.

use std::ffi::OsString;
use std::path::PathBuf;

/// The files' text: their paths, a line each.
pub fn text_of(paths: &[PathBuf]) -> String {
    let lines: Vec<String> = paths.iter().map(|path| path.display().to_string()).collect();
    lines.join("\n")
}

/// Windows: the files added to what is on the clipboard as a file list, as
/// Explorer puts it (CF_HDROP), copied or cut; other programs and remote
/// desktops paste that.
#[cfg(windows)]
pub fn add_files(paths: &[PathBuf], cut: bool) -> windows::core::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{CloseClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::DROPFILES;

    let mut list: Vec<u16> = Vec::new();
    for path in paths {
        list.extend(path.as_os_str().encode_wide());
        list.push(0);
    }
    list.push(0);
    let head = DROPFILES { pFiles: size_of::<DROPFILES>() as u32, fWide: true.into(), ..Default::default() };
    // DROPEFFECT_MOVE or DROPEFFECT_COPY.
    let effect: u32 = if cut { 2 } else { 1 };
    unsafe fn global(bytes: &[&[u8]]) -> windows::core::Result<HGLOBAL> {
        let size = bytes.iter().map(|part| part.len()).sum();
        let global = unsafe { GlobalAlloc(GMEM_MOVEABLE, size)? };
        let mut at = unsafe { GlobalLock(global) } as *mut u8;
        for part in bytes {
            unsafe {
                std::ptr::copy_nonoverlapping(part.as_ptr(), at, part.len());
                at = at.add(part.len());
            }
        }
        let _ = unsafe { GlobalUnlock(global) };
        Ok(global)
    }
    unsafe {
        let head = std::slice::from_raw_parts((&head as *const DROPFILES).cast::<u8>(), size_of::<DROPFILES>());
        let files = std::slice::from_raw_parts(list.as_ptr().cast::<u8>(), list.len() * 2);
        let drop = global(&[head, files])?;
        let effect = global(&[&effect.to_le_bytes()])?;
        let effect_format = RegisterClipboardFormatW(windows::core::w!("Preferred DropEffect"));
        if let Err(error) = OpenClipboard(None) {
            let _ = (GlobalFree(Some(drop)), GlobalFree(Some(effect)));
            return Err(error);
        }
        // Once set, the system owns the memory.
        let mut result = Ok(());
        for (format, data) in [(CF_HDROP.0 as u32, drop), (effect_format, effect)] {
            if result.is_ok() {
                result = SetClipboardData(format, Some(HANDLE(data.0))).map(|_| ());
            }
            if result.is_err() {
                let _ = GlobalFree(Some(data));
            }
        }
        let _ = CloseClipboard();
        result
    }
}

/// A path as a `file://` address, other bytes than letters, digits and
/// `-._~/` written `%XX`.
pub fn file_uri(path: &std::path::Path) -> String {
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()).to_vec();
    #[cfg(not(unix))]
    let bytes = path.to_string_lossy().replace('\\', "/").into_bytes();
    let mut uri = String::from("file://");
    for byte in bytes {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// The files a text names, a line each, as `file://` addresses or
/// absolute paths, all of them there; none for any other text.
pub fn paths_in(text: &str) -> Option<Vec<PathBuf>> {
    let lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let paths = lines.map(|line| {
        let path = match line.strip_prefix("file://") {
            Some(rest) => PathBuf::from(decode(rest.strip_prefix("localhost").unwrap_or(rest))?),
            None => PathBuf::from(line),
        };
        (path.is_absolute() && path.exists()).then_some(path)
    });
    paths.collect::<Option<Vec<_>>>().filter(|paths| !paths.is_empty())
}

/// `%20` and the like, as addresses have them.
fn decode(text: &str) -> Option<OsString> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                at += 3;
            }
            byte => {
                decoded.push(byte);
                at += 1;
            }
        }
    }
    #[cfg(unix)]
    return Some(std::os::unix::ffi::OsStringExt::from_vec(decoded));
    #[cfg(not(unix))]
    return String::from_utf8(decoded).ok().map(OsString::from);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    #[ignore = "replaces what is on the clipboard"]
    fn puts_a_file_list() {
        use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
        use windows::Win32::System::Ole::CF_HDROP;
        use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
        let paths = [std::env::temp_dir(), std::env::current_dir().unwrap()];
        add_files(&paths, false).unwrap();
        let mut read = Vec::new();
        unsafe {
            OpenClipboard(None).unwrap();
            let drop = HDROP(GetClipboardData(CF_HDROP.0 as u32).unwrap().0);
            for at in 0..DragQueryFileW(drop, u32::MAX, None) {
                let mut name = vec![0u16; 1024];
                let len = DragQueryFileW(drop, at, Some(&mut name)) as usize;
                read.push(PathBuf::from(String::from_utf16_lossy(&name[..len])));
            }
            let _ = CloseClipboard();
        }
        assert_eq!(read, paths);
    }

    #[test]
    fn reads_files_from_text() {
        let dir = std::env::temp_dir().join(format!("sc-clip-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a b")).unwrap();
        std::fs::write(dir.join("é.txt"), "x").unwrap();
        let (folder, file) = (dir.join("a b"), dir.join("é.txt"));
        let text = text_of(&[folder.clone(), file.clone()]);
        assert_eq!(paths_in(&text), Some(vec![folder.clone(), file.clone()]));
        let uri = format!("file://{}\r\n", dir.display()).replace(' ', "%20") + "a%20b";
        assert_eq!(paths_in(&uri.replace("\r\na%20b", "/a%20b")), Some(vec![folder]));
        let encoded = format!("file://{}/%C3%A9.txt", dir.display());
        assert_eq!(paths_in(&encoded), Some(vec![file.clone()]));
        let uri = file_uri(&dir.join("a b"));
        assert!(uri.ends_with("/a%20b"), "{uri}");
        assert_eq!(paths_in(&file_uri(&file)), Some(vec![file]));
        assert_eq!(paths_in("just some words"), None);
        assert_eq!(paths_in("/no/such/file"), None);
        assert_eq!(paths_in(""), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
