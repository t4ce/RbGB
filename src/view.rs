pub const WIDTH: u32 = 160;
pub const HEIGHT: u32 = 144;

/// Integer pixel scaling with centered, transparent margins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub scale: u32,
}

impl View {
    pub fn fit(width: u32, height: u32) -> Self {
        let scale = (width / WIDTH).min(height / HEIGHT);
        let game_width = WIDTH * scale;
        let game_height = HEIGHT * scale;
        Self {
            x: (width - game_width) / 2,
            y: (height - game_height) / 2,
            width: game_width,
            height: game_height,
            scale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_integer_scales_and_centers_unused_space() {
        assert_eq!(
            View::fit(320, 288),
            View {
                x: 0,
                y: 0,
                width: 320,
                height: 288,
                scale: 2
            }
        );
        assert_eq!(View::fit(480, 432).scale, 3);
        assert_eq!(
            View::fit(1000, 700),
            View {
                x: 180,
                y: 62,
                width: 640,
                height: 576,
                scale: 4
            }
        );
        assert_eq!(View::fit(479, 431).scale, 2);
        assert_eq!(View::fit(320, 143).scale, 0);
    }
}
