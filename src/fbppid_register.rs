// fbppid_register.rs
//
// Diese Datei ist für PID 1 / pivot / syscored.
//
// Zweck:
// - /dev/fbppid öffnen
// - den Broker beim Kernel registrieren
//
// Typischer Ablauf:
// 1. PID 1 startet fbportscore
// 2. PID 1 kennt danach dessen PID
// 3. PID 1 ruft register_broker(pid) auf
//
// Danach darf genau dieser Prozess /dev/fbppid als Broker benutzen.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use crate::constants::DEVICE_PATH;
use crate::fallback::register_broker_fallback;
use crate::fbppid_uapi::{
    FbppidRegisterBrokerArgs,
    FBPPID_IOC_REGISTER_BROKER,
};

/// Fehler bei der Broker-Registrierung.
#[derive(Debug)]
pub enum RegisterError {
    /// Das Device konnte nicht geöffnet werden.
    OpenDevice(io::Error),

    /// Die übergebene PID war ungültig.
    InvalidPid(i32),

    /// Der ioctl-Aufruf selbst ist fehlgeschlagen.
    Ioctl(io::Error),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenDevice(e) => write!(f, "fbppid: failed to open {DEVICE_PATH}: {e}"),
            Self::InvalidPid(pid) => write!(f, "fbppid: invalid broker PID: {pid}"),
            Self::Ioctl(e) => write!(f, "fbppid: REGISTER_BROKER ioctl failed: {e}"),
        }
    }
}

impl std::error::Error for RegisterError {
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

/// Registriert `broker_pid` beim Kernel als autorisierten Broker.
///
/// Diese Funktion ist für PID 1 gedacht.
///
/// Voraussetzungen:
/// - Aufrufer ist PID 1 oder hat CAP_SYS_ADMIN
/// - `broker_pid` ist eine gültige, laufende TGID
///
/// Nach Erfolg:
/// - der Kernel akzeptiert genau diese PID als Broker
pub fn register_broker(broker_pid: i32) -> Result<(), RegisterError> {
    if broker_pid <= 0 {
        return Err(RegisterError::InvalidPid(broker_pid));
    }

    tracing::info!(
        pid = broker_pid,
        device = DEVICE_PATH,
        "fbppid: registering broker PID with kernel"
    );

    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(DEVICE_PATH)
    {
        Ok(file) => file,
        Err(err) => {
            tracing::warn!(
                pid = broker_pid,
                errno = err.raw_os_error().unwrap_or(-1),
                "fbppid: opening device failed"
            );

            if should_fallback(err.raw_os_error()) {
                tracing::warn!(
                    pid = broker_pid,
                    "fbppid: kernel interface unavailable, using register fallback"
                );
                register_broker_fallback(broker_pid).map_err(RegisterError::OpenDevice)?;
                return Ok(());
            }

            return Err(RegisterError::OpenDevice(err));
        }
    };

    let args = FbppidRegisterBrokerArgs {
        pid: broker_pid,
        __pad: 0,
    };

    // SAFETY:
    // - file.as_raw_fd() ist ein gültiger offener FD
    // - args hat das erwartete #[repr(C)]-Layout
    // - der Kernel liest die Daten nur aus
    let ret = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            FBPPID_IOC_REGISTER_BROKER as _,
            &args as *const FbppidRegisterBrokerArgs,
        )
    };

    if ret < 0 {
        let err = io::Error::last_os_error();

        tracing::error!(
            pid = broker_pid,
            errno = err.raw_os_error().unwrap_or(-1),
            "fbppid: REGISTER_BROKER failed"
        );

        if should_fallback(err.raw_os_error()) {
            tracing::warn!(
                pid = broker_pid,
                "fbppid: ioctl interface unavailable, using register fallback"
            );
            register_broker_fallback(broker_pid).map_err(RegisterError::Ioctl)?;
            return Ok(());
        }

        return Err(RegisterError::Ioctl(err));
    }

    tracing::info!(
        pid = broker_pid,
        "fbppid: broker PID registered successfully"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_invalid_pid() {
        assert!(matches!(register_broker(0), Err(RegisterError::InvalidPid(0))));
        assert!(matches!(register_broker(-1), Err(RegisterError::InvalidPid(-1))));
    }
}