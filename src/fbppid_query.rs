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
// 2. fbportscore öffnet /dev/fbppid
// 3. fbportscore ruft query_ppid(pid) beliebig oft auf
//
// Dieses Modul kapselt:
// - Device öffnen
// - ioctl aufrufen
// - Fehler in Rust-Form zurückgeben

use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;

use crate::fbppid_uapi::{
    FbppidQueryArgs,
    FBPPID_IOC_QUERY_PPID,
};

/// Pfad zum Kernel-Device.
const DEVICE_PATH: &str = "/dev/fbppid";

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

/// Diese Struktur hält eine geöffnete Verbindung zu /dev/fbppid.
///
/// Vorteil:
/// - fbportscore muss das Device nicht bei jeder Anfrage neu öffnen
/// - ein offener FD kann wiederverwendet werden
pub struct FbppidQuery {
    file: std::fs::File,
}

impl FbppidQuery {
    /// Öffnet /dev/fbppid für den bereits registrierten Broker.
    ///
    /// Wenn der aufrufende Prozess nicht der Broker ist,
    /// schlägt das Öffnen fehl.
    pub fn open() -> Result<Self, QueryError> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(DEVICE_PATH)
            .map_err(QueryError::OpenDevice)?;

        Ok(Self { file })
    }

    /// Interner Helfer für den rohen FD.
    fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
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
    pub fn query_ppid(&self, pid: i32) -> Result<i32, QueryError> {
        if pid <= 0 {
            return Err(QueryError::InvalidPid(pid));
        }

        let mut args = FbppidQueryArgs {
            pid,
            ppid: 0,
            flags: 0,
            __pad: 0,
        };

        // SAFETY:
        // - self.fd() ist ein gültiger offener FD
        // - args hat korrektes #[repr(C)]-Layout
        // - QUERY_PPID ist ein bidirektionaler ioctl:
        //   Userspace schreibt `pid`, Kernel schreibt `ppid`
        let ret = unsafe {
            libc::ioctl(
                self.fd(),
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
            return Err(QueryError::Ioctl(err));
        }

        Ok(args.ppid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_is_runtime_dependent() {
        let _ = FbppidQuery::open();
    }
}
