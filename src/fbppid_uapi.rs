// fbppid_uapi.rs
//
// Gemeinsame UAPI-Definitionen für /dev/fbppid.
//
// Diese Datei ist die zentrale ABI-Basis zwischen Kernelmodul und Rust-Userspace.
// Beide anderen Dateien importieren nur von hier:
//
// - fbppid_register.rs  -> Broker registrieren
// - fbppid_query.rs     -> PPID abfragen
//
// Diese Datei enthält absichtlich keine Logik für open()/ioctl-Aufrufe,
// sondern nur:
// - Structs
// - ioctl-Nummern
//
// Vorteil:
// Wenn sich die ABI später ändert, muss nur diese Datei angepasst werden.

const _IOC_NRBITS: u32 = 8;
const _IOC_TYPEBITS: u32 = 8;
const _IOC_SIZEBITS: u32 = 14;

const _IOC_NRSHIFT: u32 = 0;
const _IOC_TYPESHIFT: u32 = _IOC_NRSHIFT + _IOC_NRBITS;
const _IOC_SIZESHIFT: u32 = _IOC_TYPESHIFT + _IOC_TYPEBITS;
const _IOC_DIRSHIFT: u32 = _IOC_SIZESHIFT + _IOC_SIZEBITS;

const _IOC_WRITE: u32 = 1;
const _IOC_READ: u32 = 2;

/// Berechnet eine ioctl-Nummer.
const fn _ioc(dir: u32, ty: u32, nr: u32, size: u32) -> libc::c_ulong {
    ((dir << _IOC_DIRSHIFT)
        | (ty << _IOC_TYPESHIFT)
        | (nr << _IOC_NRSHIFT)
        | (size << _IOC_SIZESHIFT)) as libc::c_ulong
}

/// ioctl für "Userspace schreibt Daten in den Kernel".
const fn _iow(ty: u32, nr: u32, size: u32) -> libc::c_ulong {
    _ioc(_IOC_WRITE, ty, nr, size)
}

/// ioctl für "Userspace schreibt rein, Kernel schreibt zurück".
const fn _iowr(ty: u32, nr: u32, size: u32) -> libc::c_ulong {
    _ioc(_IOC_READ | _IOC_WRITE, ty, nr, size)
}

/// Magic-Wert dieser ioctl-Familie.
/// Muss mit dem Kernelmodul übereinstimmen.
const FBPPID_IOC_MAGIC: u32 = b'P' as u32;

/// Struktur für die Broker-Registrierung.
///
/// Wofür:
/// PID 1 teilt dem Kernel mit, welche PID jetzt als Broker gilt.
///
/// Regeln:
/// - `pid` muss > 0 sein
/// - `__pad` muss 0 sein
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct FbppidRegisterBrokerArgs {
    pub pid: i32,
    pub __pad: u32,
}

/// Struktur für die PPID-Abfrage.
///
/// Wofür:
/// fbportscore fragt den Kernel:
/// "Was ist die aktuelle Parent-PID von Prozess X?"
///
/// Ablauf:
/// - Userspace setzt `pid`
/// - `flags = 0`
/// - `__pad = 0`
/// - Kernel schreibt `ppid`
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct FbppidQueryArgs {
    pub pid: i32,
    pub ppid: i32,
    pub flags: u32,
    pub __pad: u32,
}

/// ioctl-Nummer für Broker-Registrierung.
pub const FBPPID_IOC_REGISTER_BROKER: libc::c_ulong = _iow(
    FBPPID_IOC_MAGIC,
    1,
    core::mem::size_of::<FbppidRegisterBrokerArgs>() as u32,
);

/// ioctl-Nummer für PPID-Abfrage.
pub const FBPPID_IOC_QUERY_PPID: libc::c_ulong = _iowr(
    FBPPID_IOC_MAGIC,
    2,
    core::mem::size_of::<FbppidQueryArgs>() as u32,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_sizes_match_kernel_expectation() {
        assert_eq!(core::mem::size_of::<FbppidRegisterBrokerArgs>(), 8);
        assert_eq!(core::mem::size_of::<FbppidQueryArgs>(), 16);
    }

    #[test]
    fn ioctl_values_match_expected() {
        assert_eq!(FBPPID_IOC_REGISTER_BROKER, 0x4008_5001);
        assert_eq!(FBPPID_IOC_QUERY_PPID, 0xC010_5002);
    }
}