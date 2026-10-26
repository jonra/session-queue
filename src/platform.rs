//! Process helpers: stop an old watcher, and check stdin for input without
//! blocking.
//!
//! Windows and macOS are the targets; the Unix version also works on Linux.

use std::time::Duration;

use crate::error::{CqError, Result};

pub use imp::{stdin_has_input, terminate};

/// How long `stdin_has_input` waits for the first byte on an open, empty
/// pipe. A pipeline writer (`echo ... | cq done`) can start after `cq`;
/// a heredoc is written before `cq` starts.
const PIPE_GRACE: Duration = Duration::from_millis(100);

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::ptr::null_mut;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileSizeEx, GetFileType,
    };
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    };

    use super::{CqError, PIPE_GRACE, Result};

    /// Owns a process handle and closes it on drop.
    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcess and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn open(pid: u32, access: u32) -> Option<Handle> {
        // SAFETY: plain FFI call; a null result means failure.
        let h = unsafe { OpenProcess(access, 0, pid) };
        (!h.is_null()).then_some(Handle(h))
    }

    /// Bytes waiting in a pipe, or `None` when the pipe is closed and empty
    /// (or is not a pipe after all).
    fn pipe_bytes(h: HANDLE) -> Option<u32> {
        let mut avail = 0u32;
        // SAFETY: no buffer is passed; avail is a live u32.
        let ok = unsafe { PeekNamedPipe(h, null_mut(), 0, null_mut(), &mut avail, null_mut()) };
        (ok != 0).then_some(avail)
    }

    /// True when stdin holds input, checked without reading it and without
    /// blocking: bytes waiting in a pipe, or a non-empty file (Git Bash may
    /// pass a heredoc either way). A console, `NUL`, a closed pipe, or an
    /// open pipe that stays empty for `PIPE_GRACE` means no input.
    pub fn stdin_has_input() -> bool {
        let h = std::io::stdin().as_raw_handle() as HANDLE;
        if h.is_null() || h as isize == -1 {
            return false;
        }
        // SAFETY: GetFileType accepts any handle value.
        match unsafe { GetFileType(h) } {
            FILE_TYPE_PIPE => {
                let until = Instant::now() + PIPE_GRACE;
                loop {
                    match pipe_bytes(h) {
                        Some(0) if Instant::now() < until => {
                            sleep(Duration::from_millis(10));
                        }
                        Some(n) => return n > 0,
                        None => return false,
                    }
                }
            }
            FILE_TYPE_DISK => {
                let mut size = 0i64;
                // SAFETY: size is a live i64.
                unsafe { GetFileSizeEx(h, &mut size) != 0 && size > 0 }
            }
            // A console or NUL (FILE_TYPE_CHAR), or unknown.
            _ => false,
        }
    }

    fn image_name(h: HANDLE) -> Option<OsString> {
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        // SAFETY: buf holds len u16 values; len is updated to the length used.
        let ok = unsafe {
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
        };
        (ok != 0).then(|| OsString::from_wide(&buf[..len as usize]))
    }

    /// Stop the watcher with this process ID and wait up to 5 seconds for it
    /// to exit. Refuses a process that is not a `cq` executable.
    pub fn terminate(pid: u32) -> Result<()> {
        let Some(h) = open(
            pid,
            PROCESS_TERMINATE | PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
        ) else {
            // Already gone (or not ours to open); the lock retry decides.
            return Ok(());
        };
        let theirs =
            image_name(h.0).and_then(|p| Path::new(&p).file_name().map(|n| n.to_ascii_lowercase()));
        let mine = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
        if theirs.is_none() || theirs != mine {
            return Err(CqError::error(format!(
                "process {pid} is not a cq watcher ({theirs:?}); not stopping it"
            )));
        }
        // SAFETY: valid handle with PROCESS_TERMINATE and SYNCHRONIZE access.
        unsafe {
            if TerminateProcess(h.0, 1) == 0 {
                return Err(CqError::error(format!(
                    "could not stop the old watcher (process {pid}): {}",
                    std::io::Error::last_os_error()
                )));
            }
            WaitForSingleObject(h.0, 5000);
        }
        Ok(())
    }
}

#[cfg(unix)]
mod imp {
    use std::ffi::OsString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsFd, AsRawFd, RawFd};
    use std::os::unix::fs::FileTypeExt;
    use std::path::{Path, PathBuf};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use super::{CqError, PIPE_GRACE, Result};

    /// How long `terminate` waits after SIGTERM before it sends SIGKILL.
    const KILL_AFTER: Duration = Duration::from_secs(5);

    /// Bytes waiting to be read on `fd` (a pipe or socket), or `None` when
    /// the count is not available.
    fn bytes_waiting(fd: RawFd) -> Option<usize> {
        let mut n: libc::c_int = 0;
        // SAFETY: FIONREAD writes one c_int to a live c_int.
        let r = unsafe { libc::ioctl(fd, libc::FIONREAD, &mut n) };
        (r == 0).then_some(n.max(0) as usize)
    }

    /// True when stdin holds input, checked without reading it and without
    /// blocking: bytes waiting in a pipe or socket, or a non-empty file. A
    /// terminal, `/dev/null`, a closed pipe, or an open pipe that stays
    /// empty for `PIPE_GRACE` means no input. A socket counts as a pipe:
    /// Node's child-process pipes (Claude Code's tools and hooks) are
    /// socket pairs on Unix.
    pub fn stdin_has_input() -> bool {
        let stdin = io::stdin();
        let fd = stdin.as_fd();
        let Ok(meta) = fd
            .try_clone_to_owned()
            .map(File::from)
            .and_then(|f| f.metadata())
        else {
            return false;
        };
        let kind = meta.file_type();
        if kind.is_file() {
            return meta.len() > 0;
        }
        if !(kind.is_fifo() || kind.is_socket()) {
            // A terminal, /dev/null, or unknown.
            return false;
        }
        let raw = fd.as_raw_fd();
        let until = Instant::now() + PIPE_GRACE;
        loop {
            match bytes_waiting(raw) {
                Some(0) => {}
                Some(_) => return true,
                None => return false,
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let mut p = libc::pollfd {
                fd: raw,
                events: libc::POLLIN,
                revents: 0,
            };
            let ms = left.as_millis().clamp(1, 1000) as libc::c_int;
            // SAFETY: one live pollfd.
            let n = unsafe { libc::poll(&mut p, 1, ms) };
            if n > 0 {
                // Readable or hung up: bytes, or the end of the input.
                return bytes_waiting(raw).is_some_and(|b| b > 0);
            }
            if n < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return false;
            }
            // Timed out or interrupted: check the clock again.
        }
    }

    /// The executable of a running process.
    #[cfg(target_os = "macos")]
    fn exe_path(pid: libc::pid_t) -> Option<PathBuf> {
        use std::os::unix::ffi::OsStringExt;
        let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: buf holds buf.len() bytes; the call writes at most that many.
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if n <= 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(PathBuf::from(OsString::from_vec(buf)))
    }

    /// The executable of a running process.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn exe_path(pid: libc::pid_t) -> Option<PathBuf> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let p = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        // A binary replaced by a rebuild shows as "<path> (deleted)".
        let bytes = p.as_os_str().as_bytes();
        let bytes = bytes.strip_suffix(b" (deleted)").unwrap_or(bytes);
        Some(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }

    /// The command name of a running process, from `ps`.
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
    fn exe_path(pid: libc::pid_t) -> Option<PathBuf> {
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        let name = text.trim();
        (out.status.success() && !name.is_empty()).then(|| PathBuf::from(name))
    }

    fn exe_name(p: &Path) -> Option<OsString> {
        p.file_name().map(|n| n.to_ascii_lowercase())
    }

    /// `Ok` when the process runs an executable with our file name; else
    /// its file name, if known.
    fn check_cq(pid: libc::pid_t) -> std::result::Result<(), Option<OsString>> {
        let theirs = exe_path(pid).and_then(|p| exe_name(&p));
        let mine = std::env::current_exe().ok().and_then(|p| exe_name(&p));
        if theirs.is_some() && theirs == mine {
            Ok(())
        } else {
            Err(theirs)
        }
    }

    /// True while the process exists and we may signal it.
    fn alive(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 only checks; pid is positive.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    /// Stop the watcher with this process ID: SIGTERM, then SIGKILL if it
    /// has not exited after 5 seconds. Refuses a process that is not a `cq`
    /// executable.
    pub fn terminate(pid: u32) -> Result<()> {
        // 0 and negative values would signal process groups.
        let raw = libc::pid_t::try_from(pid).unwrap_or(0);
        if raw <= 0 || !alive(raw) {
            // Already gone (or not ours to signal); the lock retry decides.
            return Ok(());
        }
        if let Err(theirs) = check_cq(raw) {
            return Err(CqError::error(format!(
                "process {pid} is not a cq watcher ({theirs:?}); not stopping it"
            )));
        }
        // SAFETY: plain FFI call on a positive pid.
        if unsafe { libc::kill(raw, libc::SIGTERM) } != 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(CqError::error(format!(
                "could not stop the old watcher (process {pid}): {e}"
            )));
        }
        let until = Instant::now() + KILL_AFTER;
        while alive(raw) {
            if Instant::now() >= until {
                // Check again: the ID may belong to a new process by now.
                if check_cq(raw).is_ok() {
                    // SAFETY: plain FFI call on a positive pid.
                    unsafe { libc::kill(raw, libc::SIGKILL) };
                }
                break;
            }
            sleep(Duration::from_millis(50));
        }
        Ok(())
    }
}
