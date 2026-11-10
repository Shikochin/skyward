//! The animation the recovery shows while it starts: the screen comes up
//! before the phone's partitions do, so that the wait for them is a mark
//! coming up out of the dark with a ring turning under it, rather than a
//! dark screen.

use anyhow::Result;
use skyward_config::ramdisk;
use skyward_ui::font::Fonts;
use skyward_ui::surface::Surface;
use skyward_ui::theme::Theme;
use skyward_ui::widgets;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a frame is held: some sixty frames a second, as the panel shows
/// them.
const FRAME: Duration = Duration::from_millis(16);
/// The light the animation lights the panel with, as a share of its normal
/// maximum: enough to see, dim enough not to glare in a dark room.
const LIGHT: f32 = 0.45;

/// The animation, running on its own thread until this is dropped.
pub(crate) struct Booting {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Start the animation, where the screen can be opened for it. A screen that
/// cannot be opened for it is said in the kernel log and the recovery goes
/// on without.
pub(crate) fn start(density: u32) -> Booting {
    let stop = Arc::new(AtomicBool::new(false));
    let running = stop.clone();
    let thread = std::thread::Builder::new()
        .name("boot-animation".into())
        .spawn(move || {
            if let Err(e) = animate(&running, density) {
                let _ = skyward_sys::kmsg(&format!("the animation of the start stops: {e:#}"));
            }
        })
        .ok();
    Booting { stop, thread }
}

impl Drop for Booting {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn animate(stop: &AtomicBool, density: u32) -> Result<()> {
    let mut surface = skyward_ui::surface::kms::KmsSurface::open("/dev/dri/card0")?;
    let (w, h) = surface.size();
    let mut backlight = crate::ui::Backlight::open()?;
    backlight.show(Some(LIGHT))?;
    // The wordmark is the one the About page shows, made once and drawn
    // frame after frame.
    let root = Path::new("/");
    let fonts = Fonts::load(root.join(ramdisk::FONT), 0, root.join(ramdisk::MONO))?;
    let theme = Theme::dark(w, h, density as f32 / 160.0);
    let mark = widgets::wordmark(&theme, &fonts, "Skyward", h as f32 * 0.40, w as f32 * 0.17);
    let began = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let frame = skyward_ui::boot::frame(w, h, began.elapsed().as_secs_f32(), &mark);
        surface.present(&frame)?;
        surface.settle()?;
        std::thread::sleep(FRAME);
    }
    Ok(())
}
