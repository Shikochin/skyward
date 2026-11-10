//! The animation the recovery shows while it starts: the wordmark coming up
//! out of the dark, and a bar filling under it while the phone's partitions
//! are mounted and its services started. The pages take the screen once
//! they are, and the animation goes.

use crate::canvas::Canvas;
use crate::color::Rgba;
use crate::sprite::Sprite;

/// The dark the wordmark comes up out of.
const DARK: Rgba = Rgba { r: 0x0a, g: 0x0b, b: 0x0f, a: 255 };
/// The wordmark itself.
const MARK: Rgba = Rgba { r: 0xf4, g: 0xf6, b: 0xfa, a: 255 };
/// The bar's track, and the part of it filled.
const TRACK: Rgba = Rgba { r: 0x24, g: 0x28, b: 0x32, a: 255 };
const FILL: Rgba = Rgba { r: 0x5b, g: 0x9b, b: 0xff, a: 255 };

/// How long the wordmark takes to come up, in seconds.
const RISE: f32 = 0.5;
/// How long the bar takes to come within a third of its end: it is never
/// full, since the pages take the screen rather than the bar reaching it.
const FILLING: f32 = 5.0;

/// The frame the animation shows `t` seconds after it began, `w` by `h`,
/// with the wordmark `mark` drawn as it was made.
pub fn frame(w: u32, h: u32, t: f32, mark: &Sprite) -> Canvas {
    let mut c = Canvas::filled(w, h, DARK);
    let (wf, hf) = (w as f32, h as f32);
    // The wordmark comes up out of the dark and settles, a little larger
    // than it started.
    let rise = (t / RISE).clamp(0.0, 1.0);
    let ease = 1.0 - (1.0 - rise).powi(3);
    mark.draw_scaled(&mut c, 0.0, (1.0 - ease) * hf * 0.015, 0.88 + 0.12 * ease, MARK, ease);
    // The bar under it fills while the phone starts.
    let bar_w = wf * 0.42;
    let bar_h = (hf * 0.0042).max(3.0);
    let (bx, by) = ((wf - bar_w) / 2.0, hf * 0.60);
    let r = bar_h / 2.0;
    c.fill_round_rect(bx, by, bar_w, bar_h, r, TRACK);
    let filled = (1.0 - (-t / FILLING).exp()).clamp(0.0, 1.0) * bar_w;
    if filled > bar_h {
        c.fill_round_rect(bx, by, filled, bar_h, r, FILL);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::Fonts;
    use crate::theme::Theme;
    use crate::widgets;

    fn mark(w: u32, h: u32) -> Sprite {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fonts = Fonts::load(root.join("resources/fonts/MiSansVF.ttf"), 0, root.join("resources/fonts/KirkMono-Regular.ttf")).unwrap();
        let t = Theme::dark(w, h, 3.0);
        widgets::wordmark(&t, &fonts, "Skyward", h as f32 * 0.40, w as f32 * 0.17)
    }

    #[test]
    fn the_wordmark_comes_up_and_the_bar_fills() {
        let (w, h) = (1216, 2640);
        let mark = mark(w, h);
        let first = frame(w, h, 0.0, &mark);
        let later = frame(w, h, 3.0, &mark);
        assert_eq!((first.w, first.h), (w, h));
        assert_ne!(first.data, later.data, "the frame moves on");
        // The bar's filled part is longer later on.
        let row = (h as f32 * 0.60) as usize + 1;
        let lit = |c: &Canvas| (0..w).filter(|&x| c.data[(row * w as usize + x as usize) * 4 + 2] > 0x80).count();
        assert!(lit(&later) > lit(&first), "the bar fills: {} then {}", lit(&first), lit(&later));
    }
}
