use egui::{Color32, ColorImage};

const MAX_PIXELS: usize = 16 * 1024 * 1024;
const MAX_SIDE: usize = 8192;
const MAX_DIRTY_REGIONS: usize = 32;

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

    pub(crate) fn union(self, other: Self) -> Self {
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

    pub(crate) fn intersects(self, other: Self) -> bool {
        let [right_a, bottom_a] = self.end().unwrap();
        let [right_b, bottom_b] = other.end().unwrap();
        self.origin[0] < right_b
            && other.origin[0] < right_a
            && self.origin[1] < bottom_b
            && other.origin[1] < bottom_a
    }
}

#[derive(Default)]
pub(crate) struct Framebuffer {
    size: [usize; 2],
    pixels: Vec<Color32>,
    dirty: Vec<Region>,
    pub(super) revision: u64,
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
            self.revision = self.revision.wrapping_add(1);
            self.dirty.clear();
            self.dirty.push(Region {
                origin: [0, 0],
                size,
            });
            self.resized = true;
        }
        Ok(())
    }

    pub(crate) fn write_rgba(&mut self, region: Region, rgba: &[u8]) -> Result<(), &'static str> {
        let packed_stride = region.size[0]
            .checked_mul(4)
            .ok_or("Invalid pixel stride.")?;
        if packed_stride.checked_mul(region.size[1]) != Some(rgba.len()) {
            return Err("Invalid remote framebuffer pixel length.");
        }
        self.write_strided_rgba(region, rgba, packed_stride)
    }

    pub(crate) fn write_strided_rgba(
        &mut self,
        region: Region,
        rgba: &[u8],
        source_stride: usize,
    ) -> Result<(), &'static str> {
        let [x, y] = region.origin;
        let [w, h] = region.size;
        let [right, bottom] = region
            .end()
            .ok_or("Invalid remote framebuffer rectangle.")?;
        if w == 0
            || h == 0
            || right > self.size[0]
            || bottom > self.size[1]
            || source_stride < w * 4
            || (h - 1)
                .checked_mul(source_stride)
                .and_then(|offset| offset.checked_add(w * 4))
                .is_none_or(|end| end > rgba.len())
        {
            return Err("Invalid remote framebuffer rectangle or pixel length.");
        }
        let stride = self.size[0];
        for row in 0..h {
            let offset = row * source_stride;
            let source = &rgba[offset..offset + w * 4];
            let start = (y + row) * stride + x;
            for (target, pixel) in self.pixels[start..start + w]
                .iter_mut()
                .zip(source.chunks_exact(4))
            {
                // Desktop pixels are opaque. Several servers leave the fourth byte unset.
                *target = Color32::from_rgb(pixel[0], pixel[1], pixel[2]);
            }
        }
        self.revision = self.revision.wrapping_add(1);
        self.mark_dirty(region);
        Ok(())
    }

    pub(crate) fn write_colors(
        &mut self,
        region: Region,
        pixels: &[Color32],
    ) -> Result<(), &'static str> {
        let [x, y] = region.origin;
        let [w, h] = region.size;
        let [right, bottom] = region.end().ok_or("Invalid framebuffer rectangle.")?;
        if w == 0
            || h == 0
            || right > self.size[0]
            || bottom > self.size[1]
            || w.checked_mul(h) != Some(pixels.len())
        {
            return Err("Invalid framebuffer rectangle or pixel length.");
        }
        for row in 0..h {
            let source = row * w;
            let target = (y + row) * self.size[0] + x;
            self.pixels[target..target + w].copy_from_slice(&pixels[source..source + w]);
        }
        self.revision = self.revision.wrapping_add(1);
        self.mark_dirty(region);
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
        self.revision = self.revision.wrapping_add(1);
        self.mark_dirty(destination);
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> Option<ColorImage> {
        (!self.pixels.is_empty()).then(|| ColorImage {
            size: self.size,
            pixels: self.pixels.clone(),
        })
    }

    fn mark_dirty(&mut self, mut region: Region) {
        let mut index = 0;
        while index < self.dirty.len() {
            if region.intersects(self.dirty[index]) {
                region = region.union(self.dirty.swap_remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }

        if self.dirty.len() < MAX_DIRTY_REGIONS {
            self.dirty.push(region);
            return;
        }

        self.dirty.push(region);
        let mut closest = (0, 1);
        let mut least_expansion = usize::MAX;
        for left in 0..self.dirty.len() {
            for right in (left + 1)..self.dirty.len() {
                let a = self.dirty[left];
                let b = self.dirty[right];
                let union = a.union(b);
                let area = |region: Region| region.size[0] * region.size[1];
                let expansion = area(union).saturating_sub(area(a) + area(b));
                if expansion < least_expansion {
                    closest = (left, right);
                    least_expansion = expansion;
                }
            }
        }

        let (left, right) = closest;
        region = self.dirty[left].union(self.dirty[right]);
        self.dirty.swap_remove(right);
        self.dirty.swap_remove(left);
        let mut index = 0;
        while index < self.dirty.len() {
            if region.intersects(self.dirty[index]) {
                region = region.union(self.dirty.swap_remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }
        self.dirty.push(region);
    }

    pub(crate) fn take_updates(&mut self) -> Vec<Update> {
        let regions = std::mem::take(&mut self.dirty);
        regions
            .into_iter()
            .enumerate()
            .map(|(index, region)| {
                let [x, y] = region.origin;
                let [w, h] = region.size;
                let mut pixels = Vec::with_capacity(w * h);
                for row in y..y + h {
                    let start = row * self.size[0] + x;
                    pixels.extend_from_slice(&self.pixels[start..start + w]);
                }
                Update {
                    origin: region.origin,
                    image: ColorImage {
                        size: region.size,
                        pixels,
                    },
                    resized: index == 0 && std::mem::take(&mut self.resized),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consuming_viewer_damage_does_not_hide_changes_from_recorder() {
        let mut framebuffer = Framebuffer::default();
        framebuffer.resize([1, 1]).unwrap();
        let revision = framebuffer.revision;
        framebuffer.take_updates().pop().unwrap();
        assert_eq!(framebuffer.revision, revision);
        framebuffer
            .write_rgba(
                Region {
                    origin: [0, 0],
                    size: [1, 1],
                },
                &[1, 2, 3, 255],
            )
            .unwrap();
        assert_ne!(framebuffer.revision, revision);
    }

    #[test]
    fn strided_damage_skips_source_padding() {
        let mut fb = Framebuffer::default();
        fb.resize([1, 2]).unwrap();
        let region = Region {
            origin: [0, 0],
            size: [1, 2],
        };
        let source = [255, 0, 0, 0, 99, 99, 99, 99, 0, 255, 0, 0];
        fb.write_strided_rgba(region, &source, 8).unwrap();
        assert_eq!(fb.pixels, [Color32::RED, Color32::GREEN]);
    }

    #[test]
    fn writes_prepared_color_rows_into_the_framebuffer() {
        let mut fb = Framebuffer::default();
        fb.resize([3, 2]).unwrap();
        fb.write_colors(
            Region {
                origin: [1, 0],
                size: [2, 2],
            },
            &[Color32::RED, Color32::GREEN, Color32::BLUE, Color32::WHITE],
        )
        .unwrap();
        assert_eq!(fb.pixels[1..3], [Color32::RED, Color32::GREEN]);
        assert_eq!(fb.pixels[4..6], [Color32::BLUE, Color32::WHITE]);
    }

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
        fb.take_updates();
        let destination = Region {
            origin: [1, 1],
            size: [2, 2],
        };
        fb.copy_rect([0, 0], destination).unwrap();
        let update = fb.take_updates().pop().unwrap();
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
        assert!(fb.take_updates().is_empty());
    }

    #[test]
    fn merges_overlapping_damage_but_keeps_distant_regions() {
        let mut fb = Framebuffer::default();
        fb.resize([4, 4]).unwrap();
        assert!(fb.take_updates().pop().unwrap().resized);
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
        let mut updates = fb.take_updates();
        updates.sort_by_key(|update| update.origin);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].origin, [1, 1]);
        assert_eq!(updates[0].image.pixels, [Color32::BLUE]);
        assert_eq!(updates[1].origin, [2, 2]);
        assert_eq!(updates[1].image.pixels, [Color32::GREEN]);
        assert!(updates.iter().all(|update| !update.resized));
        assert!(fb.take_updates().is_empty());
    }

    #[test]
    fn caps_damage_region_count_by_merging_the_least_extra_area() {
        let mut fb = Framebuffer::default();
        fb.resize([100, 1]).unwrap();
        fb.take_updates();
        for x in 0..=MAX_DIRTY_REGIONS {
            fb.write_rgba(
                Region {
                    origin: [x, 0],
                    size: [1, 1],
                },
                &[0, 0, 0, 255],
            )
            .unwrap();
        }
        let updates = fb.take_updates();
        assert_eq!(updates.len(), MAX_DIRTY_REGIONS);
        assert!(updates.iter().all(|update| update.image.size[0] <= 2));
        assert_eq!(
            updates
                .iter()
                .map(|update| update.image.size[0] * update.image.size[1])
                .sum::<usize>(),
            MAX_DIRTY_REGIONS + 1
        );
    }

    #[test]
    fn rejects_invalid_frames_without_marking_damage() {
        let mut fb = Framebuffer::default();
        assert!(fb.resize([8192, 8192]).is_err());
        fb.resize([2, 2]).unwrap();
        fb.take_updates();
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
        assert!(fb.take_updates().is_empty());
    }
}
