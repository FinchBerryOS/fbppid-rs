use std::fs;
use std::io::{self, Write};

fn fallback_klog(msg: &str) {
    let line = format!("[FBPPID] {}\n", msg);

    if let Ok(mut f) = fs::OpenOptions::new().write(true).open("/dev/kmsg") {
        let _ = f.write_all(line.as_bytes());
    }

    eprint!("{}", line);
}

pub fn register_broker_fallback(pid: i32) -> Result<(), io::Error> {
    fallback_klog(&format!(
        "fbppid kernel interface unavailable, skipping broker registration for pid {}",
        pid
    ));
    Ok(())
}

pub fn query_ppid_fallback(pid: i32) -> Result<i32, io::Error> {
    if pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid pid: {}", pid),
        ));
    }

    let path = format!("/proc/{}/status", pid);
    let content = fs::read_to_string(&path)?;

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            let ppid = rest.trim().parse::<i32>().map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("failed to parse PPid for pid {} from {}: {}", pid, path, e),
                )
            })?;

            return Ok(ppid);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("PPid field not found for pid {} in {}", pid, path),
    ))
}