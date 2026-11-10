//! The torch: the phone's white flash LED, lit at a share of its brightness
//! while the side button has it on.

use anyhow::{Context, Result};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};

/// The white flash LED's brightness, as the kernel's LED class gives it.
const LED: &str = "/sys/class/leds/white:flash-1/brightness";
const MAX: &str = "/sys/class/leds/white:flash-1/max_brightness";
/// The share of the LED's brightness the torch is lit at: bright enough to
/// see by, short of the flash a photograph uses.
const SHARE: u32 = 4;
const OF: u32 = 5;

/// Whether the torch is on now, as this program last set it.
static ON: AtomicBool = AtomicBool::new(false);

/// Light the torch, or put it out, and give whether it is on now.
pub fn toggle() -> Result<bool> {
    let on = !ON.load(Ordering::Relaxed);
    set(on)?;
    Ok(on)
}

/// Light the torch, or put it out.
pub fn set(on: bool) -> Result<()> {
    let level = match on {
        true => {
            let max: u32 = fs::read_to_string(MAX).with_context(|| format!("read {MAX}"))?.trim().parse().with_context(|| format!("parse {MAX}"))?;
            (max / SHARE * OF / SHARE).max(1).min(max)
        }
        false => 0,
    };
    fs::write(LED, level.to_string()).with_context(|| format!("write {LED}"))?;
    ON.store(on, Ordering::Relaxed);
    Ok(())
}

/// Whether the torch is on, as this program last set it.
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}
