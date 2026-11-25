use egui::{Color32, ColorImage};

const MAX_PIXELS: usize = 16 * 1024 * 1024;
const MAX_SIDE: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub origin: [usize; 2],
    pub size: [usize; 2],
}

impl Region {
    fn end(self) -> Option<[usize; 2]> {
        Some([
            self.origin[0].checked_add(self.size[0])?,
            self.origin[1].checked_add(self.size[1])?,
        ])
    }

    fn union(self, other: Self) -> Self {
        let origin = [
            self.origin[0].min(other.origin[0]),
            self.origin[1].min(other.origin[1]),
        ];
        let end = [
            (self.origin[0] + self.size[0]).max(other.origin[0] + other.size[0]),
            (self.origin[1] + self.size[1]).max(other.origin[1] + other.size[1]),
        ];
        Self {
            origin,
            size: [end[0] - origin[0], end[1] - origin[1]],
        }
    }
}

#[derive(Default)]
pub(crate) struct Framebuffer {
    size: [usize; 2],
    pixels: Vec<Color32>,
    dirty: Option<Region>,
    resized: bool,
}

pub(crate) struct Update {
    pub origin: [usize; 2],
    pub image: ColorImage,
    pub resized: bool,
}

impl Framebuffer {
    pub(crate) fn resize(&mut self, size: [usize; 2]) -> Result<(), &'static str> {
        let [w, h] = size;
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || w * h > MAX_PIXELS {
            return Err("Remote framebuffer dimensions exceed the viewer limit.");
        }
        if size != self.size {
            self.size = size;
            self.pixels = vec![Color32::BLACK; w * h];
            self.dirty = Some(Region {
                origin: [0, 0],
                size,
            });
            self.resized = true;
        }
        Ok(())
    }

    pub(crate) fn write_rgba(&mut self, region: Region, rgba: &[u8]) -> Result<(), &'static str> {
        let [x, y] = region.origin;
        let [w, h] = region.size;
        let [right, bottom] = region
            .end()
            .ok_or("Invalid remote framebuffer rectangle.")?;
        if w == 0
            || h == 0
            || right > self.size[0]
            || bottom > self.size[1]
            || rgba.len() != w * h * 4
        {
            return Err("Invalid remote framebuffer rectangle or pixel length.");
        }
        let stride = self.size[0];
        for (row, source) in rgba.chunks_exact(w * 4).enumerate() {
            let start = (y + row) * stride + x;
            for (target, pixel) in self.pixels[start..start + w]
                .iter_mut()
                .zip(source.chunks_exact(4))
            {
                // Desktop pixels are opaque. Several servers leave the fourth byte unset.
                *target = Color32::from_rgb(pixel[0], pixel[1], pixel[2]);
            }
        }
        self.dirty = Some(self.dirty.map_or(region, |dirty| dirty.union(region)));
        Ok(())
    }

    pub(crate) fn copy_rect(
        &mut self,
        source: [usize; 2],
        destination: Region,
    ) -> Result<(), &'static str> {
        let [w, h] = destination.size;
        let source_region = Region {
            origin: source,
            size: destination.size,
        };
        let fits = |region: Region| {
            region
                .end()
                .is_some_and(|[right, bottom]| right <= self.size[0] && bottom <= self.size[1])
        };
        if w == 0 || h == 0 || !fits(source_region) || !fits(destination) {
            return Err("Invalid framebuffer copy rectangle.");
        }
        let [x, y] = destination.origin;
        let stride = self.size[0];
        for index in 0..h {
            let row = if y > source[1] { h - index - 1 } else { index };
            let from = (source[1] + row) * stride + source[0];
            let to = (y + row) * stride + x;
            self.pixels.copy_within(from..from + w, to);
        }
        self.dirty = Some(
            self.dirty
                .map_or(destination, |dirty| dirty.union(destination)),
        );
        Ok(())
    }

    pub(crate) fn take_update(&mut self) -> Option<Update> {
        let region = self.dirty.take()?;
        let [x, y] = region.origin;
        let [w, h] = region.size;
        let mut pixels = Vec::with_capacity(w * h);
        for row in y..y + h {
            let start = row * self.size[0] + x;
            pixels.extend_from_slice(&self.pixels[start..start + w]);
        }
        Some(Update {
            origin: region.origin,
            image: ColorImage {
                size: region.size,
                pixels,
            },
            resized: std::mem::take(&mut self.resized),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_overlapping_rows_and_columns_without_overwriting_the_source_early() {
        let mut fb = Framebuffer::default();
        fb.resize([3, 3]).unwrap();
        let pixels: Vec<u8> = (0..9).flat_map(|red| [red, 0, 0, 0]).collect();
        fb.write_rgba(
            Region {
                origin: [0, 0],
                size: [3, 3],
            },
            &pixels,
        )
        .unwrap();
        fb.take_update();
        let destination = Region {
            origin: [1, 1],
            size: [2, 2],
        };
        fb.copy_rect([0, 0], destination).unwrap();
        let update = fb.take_update().unwrap();
        assert_eq!(update.origin, [1, 1]);
        assert_eq!(
            update
                .image
                .pixels
                .iter()
                .map(|pixel| pixel.r())
                .collect::<Vec<_>>(),
            [0, 1, 3, 4]
        );
        assert!(fb.copy_rect([usize::MAX, 0], destination).is_err());
        assert!(fb.take_update().is_none());
    }

    #[test]
    fn coalesces_damage_and_keeps_the_latest_pixels() {
        let mut fb = Framebuffer::default();
        fb.resize([4, 4]).unwrap();
        assert!(fb.take_update().unwrap().resized);
        let a = Region {
            origin: [1, 1],
            size: [1, 1],
        };
        let b = Region {
            origin: [2, 2],
            size: [1, 1],
        };
        fb.write_rgba(a, &[255, 0, 0, 0]).unwrap();
        fb.write_rgba(b, &[0, 255, 0, 0]).unwrap();
        fb.write_rgba(a, &[0, 0, 255, 0]).unwrap();
        let update = fb.take_update().unwrap();
        assert_eq!(update.origin, [1, 1]);
        assert_eq!(update.image.size, [2, 2]);
        assert_eq!(
            update.image.pixels,
            [
                Color32::BLUE,
                Color32::BLACK,
                Color32::BLACK,
                Color32::GREEN
            ]
        );
        assert!(!update.resized);
        assert!(fb.take_update().is_none());
    }

    #[test]
    fn rejects_invalid_frames_without_marking_damage() {
        let mut fb = Framebuffer::default();
        assert!(fb.resize([8192, 8192]).is_err());
        fb.resize([2, 2]).unwrap();
        fb.take_update();
        assert!(fb
            .write_rgba(
                Region {
                    origin: [usize::MAX, 0],
                    size: [1, 1]
                },
                &[0; 4]
            )
            .is_err());
        assert!(fb
            .write_rgba(
                Region {
                    origin: [0, 0],
                    size: [2, 2]
                },
                &[0; 4]
            )
            .is_err());
        assert!(fb.take_update().is_none());
    }
}
