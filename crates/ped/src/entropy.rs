//! Bootstrap and maintain kernel entropy before/while AWS-LC/rustls runs.
//!
//! Kindle kernels often report low `entropy_avail`. AWS-LC blocks on
//! `RNDGETENTCNT` until it sees >= 256 bits. Writing bytes to `/dev/random`
//! alone does not credit the counter; `RNDADDENTROPY` does.
//!
//! CPU jitter (`rand_jitter`) is used only as a userspace harvester to feed the
//! kernel. It is not treated as a general-purpose application CSPRNG.

use std::fs;
use std::io::{self, Write};
use std::os::fd::RawFd;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use libc::{O_RDWR, c_int, close, ioctl, open};
use rand_core::RngCore;
use rand_jitter::JitterRng;

/// Linux `RNDADDENTROPY`: `_IOW('R', 0x03, int[2])`.
const RNDADDENTROPY: libc::c_ulong = 0x4008_5203;
/// Linux `RNDGETENTCNT`: `_IOR('R', 0x00, int)`.
const RNDGETENTCNT: libc::c_ulong = 0x8004_5200;

/// AWS-LC waits until the kernel reports at least this many bits.
const TARGET_ENTROPY_BITS: i32 = 256;
/// Keep a cushion above the AWS-LC floor; Kindle drains the pool quickly.
const MAINTAIN_ENTROPY_BITS: i32 = 512;
const BYTES_PER_ROUND: usize = 64;
const STARTUP_MAX_ROUNDS: usize = 64;

pub struct EntropyGuard {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl EntropyGuard {
    pub fn start() -> Self {
        bootstrap_once();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let thread = thread::Builder::new()
            .name("ped-entropy".to_owned())
            .spawn(move || maintain_loop(stop_thread))
            .ok();
        Self { stop, thread }
    }
}

impl Drop for EntropyGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn bootstrap_once() {
    let before = entropy_avail().unwrap_or(-1);
    if before >= MAINTAIN_ENTROPY_BITS {
        println!("ped: kernel entropy already sufficient ({before} bits)");
        return;
    }
    let started = Instant::now();
    match credit_until(MAINTAIN_ENTROPY_BITS, STARTUP_MAX_ROUNDS) {
        Ok(after) => println!(
            "ped: seeded kernel entropy {} -> {} bits in {:.2?}",
            before.max(0),
            after,
            started.elapsed()
        ),
        Err(error) => {
            eprintln!("ped: kernel entropy seeding failed: {error}");
            let _ = mix_without_credit();
        }
    }
}

fn maintain_loop(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        match entropy_avail() {
            Ok(value) if value >= TARGET_ENTROPY_BITS => {
                thread::sleep(Duration::from_millis(500));
            }
            Ok(_) | Err(_) => {
                let _ = credit_until(MAINTAIN_ENTROPY_BITS, 8);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn credit_until(goal_bits: i32, max_rounds: usize) -> io::Result<i32> {
    let mut rng = JitterRng::new().map_err(|error| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("jitter timer quality check failed: {error:?}"),
        )
    })?;
    let _ = rng.next_u64();

    let mut after = entropy_avail().unwrap_or(0);
    for _ in 0..max_rounds {
        if after >= goal_bits {
            break;
        }
        let mut block = [0u8; BYTES_PER_ROUND];
        fill_from_jitter(&mut rng, &mut block);
        add_entropy_credit(&block, (block.len() * 8) as i32)?;
        after = entropy_avail().unwrap_or(after);
    }
    if after < TARGET_ENTROPY_BITS {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("entropy_avail stayed at {after} bits (goal {goal_bits})"),
        ));
    }
    Ok(after)
}

fn add_entropy_credit(data: &[u8], entropy_bits: i32) -> io::Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let fd = open_dev_random()?;
    let result = (|| {
        let mut payload = Vec::with_capacity(8 + data.len());
        payload.extend_from_slice(&entropy_bits.to_ne_bytes());
        payload.extend_from_slice(&(data.len() as i32).to_ne_bytes());
        payload.extend_from_slice(data);
        let rc = unsafe { ioctl(fd, RNDADDENTROPY as _, payload.as_ptr()) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    })();
    unsafe {
        close(fd);
    }
    result
}

fn entropy_avail() -> io::Result<i32> {
    if let Ok(text) = fs::read_to_string("/proc/sys/kernel/random/entropy_avail") {
        if let Ok(value) = text.trim().parse::<i32>() {
            return Ok(value);
        }
    }
    let fd = open_dev_random()?;
    let mut value: c_int = 0;
    let rc = unsafe { ioctl(fd, RNDGETENTCNT as _, &mut value as *mut c_int) };
    unsafe {
        close(fd);
    }
    if rc == 0 {
        Ok(value)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn open_dev_random() -> io::Result<RawFd> {
    let fd = unsafe { open(c"/dev/random".as_ptr(), O_RDWR) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

fn mix_without_credit() -> io::Result<()> {
    let mut rng = JitterRng::new().map_err(|error| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("jitter timer quality check failed: {error:?}"),
        )
    })?;
    let mut block = [0u8; 64];
    fill_from_jitter(&mut rng, &mut block);
    if Path::new("/dev/random").exists() {
        let mut file = fs::OpenOptions::new().write(true).open("/dev/random")?;
        file.write_all(&block)?;
    }
    Ok(())
}

fn fill_from_jitter<R: RngCore>(rng: &mut R, dest: &mut [u8]) {
    let mut offset = 0;
    while offset < dest.len() {
        let value = rng.next_u64().to_le_bytes();
        let take = (dest.len() - offset).min(value.len());
        dest[offset..offset + take].copy_from_slice(&value[..take]);
        offset += take;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_linux_random_ioctls() {
        assert_eq!(RNDADDENTROPY, 0x4008_5203);
        assert_eq!(RNDGETENTCNT, 0x8004_5200);
    }
}
