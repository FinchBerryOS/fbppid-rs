# fbppid-rs

`fbppid-rs` is a small Rust userspace library for the FinchBerryOS `fbppid` kernel interface.

It provides two public functions:

- `register_broker(pid)`  
  Registers the broker process with the kernel interface.
- `query_ppid(pid)`  
  Queries the current parent PID of a target process.

The crate is designed for the FinchBerryOS boot stack:

- `pivot` / PID 1 uses `register_broker(...)`
- `fbportscore` uses `query_ppid(...)`

---

## Design goals

This crate is intentionally small and low-level.

It is meant to:

- talk directly to `/dev/fbppid`
- keep the kernel ABI in one dedicated file
- work with both musl and glibc userspace builds
- provide a controlled fallback path if the kernel interface is unavailable

---

## Public API

The crate exports exactly these two functions:

```rust
use fbppid_rs::{register_broker, query_ppid};
```

### `register_broker(pid: i32) -> Result<(), RegisterError>`

Registers a PID as the authorized broker process.

Typical usage:

- PID 1 starts `fbportscore`
- PID 1 knows the new process PID
- PID 1 calls `register_broker(pid)`

If the kernel interface is unavailable, the function falls back to a no-op fallback implementation that:

- logs a warning to `/dev/kmsg` if possible
- logs to `stderr`
- returns `Ok(())`

This means broker registration is treated as best-effort in fallback mode.

### `query_ppid(pid: i32) -> Result<i32, QueryError>`

Returns the current parent PID of a target process.

Typical usage:

- `fbportscore` calls `query_ppid(pid)` repeatedly for different processes

The function does not expose any query object publicly.
Instead, the crate internally decides once per process lifetime which backend to use:

- kernel backend via `/dev/fbppid`
- or fallback backend via `/proc/<pid>/status`

That backend choice is then reused for the rest of the process runtime.

---

## Crate layout

The crate is split into small focused files:

- `lib.rs`
  - exports the public API
- `constants.rs`
  - shared constants such as `/dev/fbppid`
- `fbppid_uapi.rs`
  - shared ABI and ioctl definitions
- `fbppid_register.rs`
  - broker registration logic
- `fbppid_query.rs`
  - PPID query logic
- `fallback.rs`
  - fallback helpers and procfs-based fallback behavior

---

## Kernel interface

The primary backend uses:

```text
/dev/fbppid
```

The ioctl ABI is defined centrally in:

```text
fbppid_uapi.rs
```

That file contains:

- the `#[repr(C)]` structs shared with the kernel
- ioctl number construction
- exported ioctl constants

Keeping the ABI in one file makes it easier to maintain compatibility between kernel and userspace.

---

## Fallback behavior

This crate includes a fallback path for environments where the kernel interface is unavailable.

### `register_broker` fallback

If broker registration cannot use the kernel interface because it is unavailable, the fallback:

- logs that the kernel interface is unavailable
- skips broker registration
- returns success

So the fallback function is intentionally:

- non-fatal
- best-effort
- logging-only

### `query_ppid` fallback

If the query backend cannot use the kernel interface, the crate falls back to:

```text
/proc/<pid>/status
```

It reads the `PPid:` field and returns that value.

This is used as a procfs-based fallback when the device or ioctl path is not available.

---

## Backend selection

`query_ppid()` does not reopen `/dev/fbppid` on every call.

Instead, the crate caches the backend selection globally for the lifetime of the process:

- if `/dev/fbppid` opens successfully, the open FD is reused
- if the kernel interface is unavailable, the crate switches to a permanent fallback backend for that process

This avoids repeated open attempts when fallback mode is already known.

---

## Error handling

### `register_broker`

`register_broker` returns `RegisterError` for real failures such as:

- invalid PID
- device open failure
- ioctl failure

If the error indicates that the kernel interface is unavailable, the function uses the fallback and returns success.

### `query_ppid`

`query_ppid` returns `QueryError` for real failures such as:

- invalid PID
- device open failure
- ioctl failure
- procfs parsing failure in fallback mode

---

## musl / glibc compatibility

The crate is written so it can be used from both:

- glibc-based targets
- musl-based targets

The ioctl definitions are written using portable Rust/C layout assumptions and fixed-size UAPI structs.

This is especially relevant because `pivot` and related boot components are often built statically against musl.

---

## Example

```rust
use fbppid_rs::register_broker;
use nix::unistd::{close, execve, fork, pipe, read, write, ForkResult};
use std::ffi::{CStr, CString};

fn spawn_broker() -> Result<(), Box<dyn std::error::Error>> {
    // Sync-Pipe:
    // Child wartet, bis Parent register_broker(pid) erfolgreich ausgeführt hat.
    let (read_fd, write_fd) = pipe()?;

    match unsafe { fork()? } {
        ForkResult::Parent { child } => {
            let pid = child.as_raw();

            // Parent braucht das Lese-Ende nicht.
            close(read_fd)?;

            // Broker-PID beim Kernel registrieren.
            register_broker(pid)?;

            // Child freigeben.
            write(&write_fd, &[1])?;
            close(write_fd)?;

            Ok(())
        }

        ForkResult::Child => {
            // Child braucht das Schreib-Ende nicht.
            close(write_fd)?;

            // Warten, bis Parent die Registrierung abgeschlossen hat.
            let mut byte = [0u8; 1];
            read(&read_fd, &mut byte)?;
            close(read_fd)?;

            let path = CString::new("/usr/libexec/fbportscore")?;
            let argv: [&CStr; 1] = [path.as_c_str()];
            let env: [&CStr; 0] = [];

            execve(path.as_c_str(), &argv, &env)?;
            unreachable!();
        }
    }
}
```

---

## Intended usage

This crate is not meant to be a general-purpose Linux process inspection library.

It is specifically intended for FinchBerryOS components that interact with the `fbppid` kernel ABI, especially:

- boot components
- broker processes
- process-parent tracking logic

---

## Summary

`fbppid-rs` is a focused Rust wrapper around the FinchBerryOS `fbppid` kernel ABI.

It provides:

- broker registration via `register_broker`
- parent PID queries via `query_ppid`
- a shared central UAPI definition
- per-process backend caching for queries
- a procfs fallback path when the kernel interface is unavailable
- compatibility with musl and glibc builds
