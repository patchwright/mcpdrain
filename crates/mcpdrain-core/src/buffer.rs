//! OS pipe-capacity detection.
//!
//! MCP over stdio deadlocks when a server writes more bytes than the kernel
//! pipe *capacity* while the client is not concurrently draining every stream.
//! [`pipe_capacity_bytes`] returns the capacity of a fresh pipe on the host so
//! the proxy can size its thresholds. macOS default ≈ 16 KiB; Linux default
//! ≈ 64 KiB (exact on Linux via `fcntl(F_GETPIPE_SZ)`).

/// Conservative default pipe capacity when the kernel won't tell us exactly.
///
/// Note: the frequently-cited "8 KB" hang threshold in the wild reflects
/// line-buffering and partial-fill effects rather than raw capacity; we default
/// to the smaller platform capacity to stay safe.
#[cfg(target_os = "linux")]
pub const DEFAULT_PIPE_CAPACITY: usize = 64 * 1024;
#[cfg(all(unix, not(target_os = "linux")))]
pub const DEFAULT_PIPE_CAPACITY: usize = 16 * 1024;
#[cfg(not(unix))]
pub const DEFAULT_PIPE_CAPACITY: usize = 64 * 1024;

/// Detect the host's pipe capacity in bytes.
///
/// On Linux this is exact. On other platforms we fall back to the platform
/// default constant.
pub fn pipe_capacity_bytes() -> usize {
    #[cfg(target_os = "linux")]
    if let Some(n) = linux_pipe_size() {
        return n;
    }
    DEFAULT_PIPE_CAPACITY
}

#[cfg(target_os = "linux")]
fn linux_pipe_size() -> Option<usize> {
    let mut fds = [0i32; 2];
    // SAFETY: `pipe(2)` writes two valid file descriptors into a 2-element
    // array. We close both ends before returning.
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // F_GETPIPE_SZ returns the pipe's capacity (bytes), which on a fresh pipe
    // is the kernel default (commonly 64 KiB on Linux).
    let sz = unsafe { libc::fcntl(fds[0], libc::F_GETPIPE_SZ) };
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
    if sz <= 0 {
        None
    } else {
        Some(sz as usize)
    }
}
