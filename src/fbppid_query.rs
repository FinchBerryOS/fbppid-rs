// fbppid_query.rs
//
// Diese Datei ist für fbportscore.
//
// Zweck:
// - /dev/fbppid öffnen
// - aktuelle Parent-PID einer Ziel-PID abfragen
//
// Typischer Ablauf:
// 1. fbportscore wurde von PID 1 bereits registriert
// 2. fbportscore ruft query_ppid(pid) beliebig oft auf
//
// Dieses Modul kapselt:
// - Device öffnen
// - Backend global für die gesamte Prozesslaufzeit festlegen
// - ioctl aufrufen
// - Fallback über /proc/<pid>/status
// - Fehler in Rust-Form zurückgeben
//
// Wichtige Semantik:
// - Es gibt absichtlich KEIN öffentliches "Objekt" mehr.
// - Beim ersten Aufruf von query_ppid() wird genau EINMAL entschieden:
//   * Kernel-Backend (/dev/fbppid) verwenden
//   * oder dauerhaft Fallback verwenden
// - Danach wird diese Entscheidung für den Rest der Prozesslaufzeit
//   wiederverwendet.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::OnceLock;

use crate::constants::DEVICE_PATH;
use crate::fallback::query_ppid_fallback;
use crate::fbppid_uapi::{FbppidQueryArgs, FBPPID_IOC_QUERY_PPID};

/// Global festgelegtes Backend für die gesamte Prozesslaufzeit.
static QUERY_BACKEND: OnceLock<QueryBackend> = OnceLock::new();

/// Einmalig ausgewähltes Query-Backend.
enum QueryBackend {
    /// Kernel-Interface über offenen FD zu /dev/fbppid.
    Kernel(OwnedFd),

    /// Fallback über /proc/<pid>/status.
    Fallback,
}

/// Fehler beim Arbeiten mit dem PPID-Device.
#[derive(Debug)]
pub enum QueryError {
    /// Device konnte nicht geöffnet werden.
    OpenDevice(io::Error),

    /// Ziel-PID ist ungültig.
    InvalidPid(i32),

    /// ioctl selbst ist fehlgeschlagen.
    Ioctl(io::Error),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenDevice(e) => write!(f, "fbppid: failed to open {DEVICE_PATH}: {e}"),
            Self::InvalidPid(pid) => write!(f, "fbppid: invalid target PID: {pid}"),
            Self::Ioctl(e) => write!(f, "fbppid: QUERY_PPID ioctl failed: {e}"),
        }
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OpenDevice(e) | Self::Ioctl(e) => Some(e),
            Self::InvalidPid(_) => None,
        }
    }
}

fn should_fallback(errno: Option<i32>) -> bool {
    matches!(
        errno,
        Some(libc::ENOENT | libc::ENODEV | libc::ENOTTY | libc::EOPNOTSUPP)
    )
}

/// Öffnet /dev/fbppid einmalig für das Kernel-Backend.
fn open_device() -> Result<OwnedFd, QueryError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(DEVICE_PATH)
        .map_err(QueryError::OpenDevice)?;

    Ok(file.into())
}

/// Initialisiert genau einmal das zu verwendende Backend.
///
/// Logik:
/// - Wenn /dev/fbppid erfolgreich geöffnet werden kann -> Kernel-Backend
/// - Wenn das Interface nicht verfügbar ist -> dauerhafter Fallback
/// - Bei anderen Open-Fehlern -> echter Fehler
fn init_backend() -> Result<&'static QueryBackend, QueryError> {
    if let Some(backend) = QUERY_BACKEND.get() {
        return Ok(backend);
    }

    let backend = match open_device() {
        Ok(fd) => {
            tracing::info!(
                device = DEVICE_PATH,
                "fbppid: query backend initialized to kernel device"
            );
            QueryBackend::Kernel(fd)
        }
        Err(QueryError::OpenDevice(err)) if should_fallback(err.raw_os_error()) => {
            tracing::warn!(
                errno = err.raw_os_error().unwrap_or(-1),
                device = DEVICE_PATH,
                "fbppid: kernel query interface unavailable, switching to procfs fallback"
            );
            QueryBackend::Fallback
        }
        Err(e) => return Err(e),
    };

    let _ = QUERY_BACKEND.set(backend);
    Ok(QUERY_BACKEND
        .get()
        .expect("QUERY_BACKEND must be initialized after set attempt"))
}

/// Interner Helfer: rohen FD aus dem Kernel-Backend holen.
fn backend_fd(backend: &QueryBackend) -> Option<RawFd> {
    match backend {
        QueryBackend::Kernel(fd) => Some(fd.as_raw_fd()),
        QueryBackend::Fallback => None,
    }
}

/// Fragt die aktuelle Parent-PID einer Ziel-PID ab.
///
/// Das ist eine Momentaufnahme.
///
/// Beispiel:
/// - query_ppid(D) -> C
/// - query_ppid(C) -> B
/// - query_ppid(B) -> A
///
/// Wichtiger Hinweis:
/// Das ist die aktuelle Parent-PID, nicht zwingend
/// der ursprüngliche Starterprozess.
pub fn query_ppid(pid: i32) -> Result<i32, QueryError> {
    if pid <= 0 {
        return Err(QueryError::InvalidPid(pid));
    }

    let backend = init_backend()?;

    if let QueryBackend::Fallback = backend {
        tracing::debug!(
            pid = pid,
            "fbppid: using cached procfs fallback backend"
        );
        return query_ppid_fallback(pid).map_err(QueryError::OpenDevice);
    }

    let fd = backend_fd(backend).expect("Kernel backend must provide an fd");

    let mut args = FbppidQueryArgs {
        pid,
        ppid: 0,
        flags: 0,
        __pad: 0,
    };

    // SAFETY:
    // - fd ist ein gültiger offener FD
    // - args hat korrektes #[repr(C)]-Layout
    // - QUERY_PPID ist ein bidirektionaler ioctl:
    //   Userspace schreibt `pid`, Kernel schreibt `ppid`
    let ret = unsafe {
        libc::ioctl(
            fd,
            FBPPID_IOC_QUERY_PPID as _,
            &mut args as *mut FbppidQueryArgs,
        )
    };

    if ret < 0 {
        let err = io::Error::last_os_error();

        tracing::error!(
            pid = pid,
            errno = err.raw_os_error().unwrap_or(-1),
            "fbppid: QUERY_PPID failed"
        );

        if should_fallback(err.raw_os_error()) {
            tracing::warn!(
                pid = pid,
                "fbppid: ioctl interface unavailable, using procfs fallback for this query"
            );
            return query_ppid_fallback(pid).map_err(QueryError::Ioctl);
        }

        return Err(QueryError::Ioctl(err));
    }

    Ok(args.ppid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_invalid_pid() {
        assert!(matches!(query_ppid(0), Err(QueryError::InvalidPid(0))));
        assert!(matches!(query_ppid(-1), Err(QueryError::InvalidPid(-1))));
    }
}