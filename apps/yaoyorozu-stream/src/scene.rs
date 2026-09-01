use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct OverlaySource {
    pub enabled: bool,
    pub name: String,
    pub x_percent: f32,
    pub y_percent: f32,
    pub width_percent: f32,
    pub bounce: bool,
}

impl Default for OverlaySource {
    fn default() -> Self {
        Self {
            enabled: false,
            name: "跳ねるししゃも（テスト）".to_owned(),
            x_percent: 78.0,
            y_percent: 70.0,
            width_percent: 16.0,
            bounce: true,
        }
    }
}

impl OverlaySource {
    const FISH_ASPECT: f32 = 0.42;
    const MIN_WIDTH_PERCENT: f32 = 5.0;
    const MAX_WIDTH_PERCENT: f32 = 45.0;

    pub fn move_by_preview_delta(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        preview_width: f32,
        preview_height: f32,
        source_width: u32,
        source_height: u32,
    ) {
        move_transform(
            &mut self.x_percent,
            &mut self.y_percent,
            delta_x,
            delta_y,
            preview_width,
            preview_height,
        );
        self.clamp_to_frame(source_width, source_height);
    }

    pub fn resize_by_preview_delta(
        &mut self,
        delta_x: f32,
        preview_width: f32,
        source_width: u32,
        source_height: u32,
    ) {
        resize_width(
            &mut self.width_percent,
            delta_x,
            preview_width,
            Self::MIN_WIDTH_PERCENT,
            Self::MAX_WIDTH_PERCENT,
        );
        self.clamp_to_frame(source_width, source_height);
    }

    pub fn clamp_to_frame(&mut self, source_width: u32, source_height: u32) {
        self.width_percent = self
            .width_percent
            .clamp(Self::MIN_WIDTH_PERCENT, Self::MAX_WIDTH_PERCENT);

        let aspect = Self::FISH_ASPECT;
        clamp_transform_to_frame(
            &mut self.x_percent,
            &mut self.y_percent,
            self.width_percent,
            aspect,
            source_width,
            source_height,
        );
    }

    pub fn compose_test_overlay(
        &self,
        rgba: &mut [u8],
        width: u32,
        height: u32,
        time_seconds: f32,
    ) {
        if !self.enabled || width == 0 || height == 0 {
            return;
        }
        let expected = width as usize * height as usize * 4;
        if rgba.len() < expected {
            return;
        }

        let fish_w = ((width as f32 * (self.width_percent / 100.0)).round() as i32).max(24);
        let fish_h = (fish_w as f32 * Self::FISH_ASPECT).round() as i32;
        let bounce = if self.bounce {
            (time_seconds * 4.2).sin().abs() * height as f32 * 0.055
        } else {
            0.0
        };
        let cx = (width as f32 * (self.x_percent / 100.0)).round() as i32;
        let cy = (height as f32 * (self.y_percent / 100.0) - bounce).round() as i32;

        let body_rx = fish_w / 2;
        let body_ry = fish_h / 2;
        for yy in -body_ry..=body_ry {
            for xx in -body_rx..=body_rx {
                let nx = xx as f32 / body_rx.max(1) as f32;
                let ny = yy as f32 / body_ry.max(1) as f32;
                if nx * nx + ny * ny <= 1.0 {
                    blend_pixel(rgba, width, height, cx + xx, cy + yy, [232, 150, 76, 235]);
                }
            }
        }

        let tail_x = cx - body_rx;
        for dx in 0..(fish_w / 3).max(1) {
            let half =
                ((fish_h as f32 * 0.65) * (1.0 - dx as f32 / (fish_w / 3).max(1) as f32)) as i32;
            for dy in -half..=half {
                blend_pixel(
                    rgba,
                    width,
                    height,
                    tail_x - dx,
                    cy + dy,
                    [220, 126, 63, 225],
                );
            }
        }

        let eye_x = cx + body_rx / 2;
        let eye_y = cy - body_ry / 4;
        let eye_r = (fish_h / 10).max(2);
        for yy in -eye_r..=eye_r {
            for xx in -eye_r..=eye_r {
                if xx * xx + yy * yy <= eye_r * eye_r {
                    blend_pixel(
                        rgba,
                        width,
                        height,
                        eye_x + xx,
                        eye_y + yy,
                        [25, 25, 25, 255],
                    );
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageOverlaySource {
    pub enabled: bool,
    pub name: String,
    pub path: PathBuf,
    pub x_percent: f32,
    pub y_percent: f32,
    pub width_percent: f32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    rgba: Vec<u8>,
}

impl ImageOverlaySource {
    const MIN_WIDTH_PERCENT: f32 = 5.0;
    const MAX_WIDTH_PERCENT: f32 = 80.0;

    pub fn load(path: &Path) -> Result<Self, String> {
        let decoded = image::open(path)
            .map_err(|err| format!("画像を開けませんでした: {err}"))?
            .to_rgba8();
        let (pixel_width, pixel_height) = decoded.dimensions();
        if pixel_width == 0 || pixel_height == 0 {
            return Err("画像サイズが0です".to_owned());
        }

        Ok(Self {
            enabled: true,
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("画像")
                .to_owned(),
            path: path.to_path_buf(),
            x_percent: 50.0,
            y_percent: 50.0,
            width_percent: 24.0,
            pixel_width,
            pixel_height,
            rgba: decoded.into_raw(),
        })
    }

    pub fn aspect(&self) -> f32 {
        self.pixel_height as f32 / self.pixel_width.max(1) as f32
    }

    pub fn move_by_preview_delta(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        preview_width: f32,
        preview_height: f32,
        source_width: u32,
        source_height: u32,
    ) {
        move_transform(
            &mut self.x_percent,
            &mut self.y_percent,
            delta_x,
            delta_y,
            preview_width,
            preview_height,
        );
        self.clamp_to_frame(source_width, source_height);
    }

    pub fn resize_by_preview_delta(
        &mut self,
        delta_x: f32,
        preview_width: f32,
        source_width: u32,
        source_height: u32,
    ) {
        resize_width(
            &mut self.width_percent,
            delta_x,
            preview_width,
            Self::MIN_WIDTH_PERCENT,
            Self::MAX_WIDTH_PERCENT,
        );
        self.clamp_to_frame(source_width, source_height);
    }

    pub fn clamp_to_frame(&mut self, source_width: u32, source_height: u32) {
        self.width_percent = self
            .width_percent
            .clamp(Self::MIN_WIDTH_PERCENT, Self::MAX_WIDTH_PERCENT);
        let aspect = self.aspect();
        clamp_transform_to_frame(
            &mut self.x_percent,
            &mut self.y_percent,
            self.width_percent,
            aspect,
            source_width,
            source_height,
        );
    }

    pub fn compose(&self, target_rgba: &mut [u8], target_width: u32, target_height: u32) {
        if !self.enabled || target_width == 0 || target_height == 0 {
            return;
        }
        let expected = target_width as usize * target_height as usize * 4;
        if target_rgba.len() < expected
            || self.rgba.len() < self.pixel_width as usize * self.pixel_height as usize * 4
        {
            return;
        }

        let draw_w = ((target_width as f32 * self.width_percent / 100.0).round() as i32).max(1);
        let draw_h = ((draw_w as f32 * self.aspect()).round() as i32).max(1);
        let center_x = (target_width as f32 * self.x_percent / 100.0).round() as i32;
        let center_y = (target_height as f32 * self.y_percent / 100.0).round() as i32;
        let left = center_x - draw_w / 2;
        let top = center_y - draw_h / 2;

        for dy in 0..draw_h {
            let ty = top + dy;
            if ty < 0 || ty >= target_height as i32 {
                continue;
            }
            let sy = ((dy as i64 * self.pixel_height as i64) / draw_h as i64)
                .clamp(0, self.pixel_height.saturating_sub(1) as i64) as u32;
            for dx in 0..draw_w {
                let tx = left + dx;
                if tx < 0 || tx >= target_width as i32 {
                    continue;
                }
                let sx = ((dx as i64 * self.pixel_width as i64) / draw_w as i64)
                    .clamp(0, self.pixel_width.saturating_sub(1) as i64)
                    as u32;
                let src_idx = ((sy * self.pixel_width + sx) * 4) as usize;
                let src = [
                    self.rgba[src_idx],
                    self.rgba[src_idx + 1],
                    self.rgba[src_idx + 2],
                    self.rgba[src_idx + 3],
                ];
                if src[3] == 0 {
                    continue;
                }
                blend_pixel(target_rgba, target_width, target_height, tx, ty, src);
            }
        }
    }
}

fn move_transform(
    x_percent: &mut f32,
    y_percent: &mut f32,
    delta_x: f32,
    delta_y: f32,
    preview_width: f32,
    preview_height: f32,
) {
    if preview_width <= 0.0 || preview_height <= 0.0 {
        return;
    }
    *x_percent += delta_x / preview_width * 100.0;
    *y_percent += delta_y / preview_height * 100.0;
}

fn resize_width(
    width_percent: &mut f32,
    delta_x: f32,
    preview_width: f32,
    min_width: f32,
    max_width: f32,
) {
    if preview_width <= 0.0 {
        return;
    }
    *width_percent = (*width_percent + delta_x / preview_width * 100.0).clamp(min_width, max_width);
}

fn clamp_transform_to_frame(
    x_percent: &mut f32,
    y_percent: &mut f32,
    width_percent: f32,
    aspect: f32,
    source_width: u32,
    source_height: u32,
) {
    if source_width == 0 || source_height == 0 {
        *x_percent = (*x_percent).clamp(0.0, 100.0);
        *y_percent = (*y_percent).clamp(0.0, 100.0);
        return;
    }

    let half_w = width_percent * 0.5;
    let height_percent =
        width_percent * source_width as f32 / source_height as f32 * aspect.max(0.0001);
    let half_h = (height_percent * 0.5).min(50.0);

    *x_percent = (*x_percent).clamp(half_w.min(50.0), (100.0 - half_w).max(50.0));
    *y_percent = (*y_percent).clamp(half_h, 100.0 - half_h);
}

fn blend_pixel(rgba: &mut [u8], width: u32, height: u32, x: i32, y: i32, src: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let idx = ((y as u32 * width + x as u32) * 4) as usize;
    let alpha = src[3] as f32 / 255.0;
    let inv = 1.0 - alpha;
    rgba[idx] = (src[0] as f32 * alpha + rgba[idx] as f32 * inv).round() as u8;
    rgba[idx + 1] = (src[1] as f32 * alpha + rgba[idx + 1] as f32 * inv).round() as u8;
    rgba[idx + 2] = (src[2] as f32 * alpha + rgba[idx + 2] as f32 * inv).round() as u8;
    rgba[idx + 3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_overlay_alpha_blends_into_target() {
        let source = ImageOverlaySource {
            enabled: true,
            name: "test".to_owned(),
            path: PathBuf::from("test.png"),
            x_percent: 50.0,
            y_percent: 50.0,
            width_percent: 50.0,
            pixel_width: 1,
            pixel_height: 1,
            rgba: vec![255, 0, 0, 255],
        };
        let mut target = vec![0u8; 4 * 4 * 4];
        source.compose(&mut target, 4, 4);
        assert!(target
            .chunks_exact(4)
            .any(|px| px[0] == 255 && px[3] == 255));
    }

    #[test]
    fn image_overlay_keeps_center_inside_frame() {
        let mut source = ImageOverlaySource {
            enabled: true,
            name: "test".to_owned(),
            path: PathBuf::from("test.png"),
            x_percent: -100.0,
            y_percent: 200.0,
            width_percent: 20.0,
            pixel_width: 100,
            pixel_height: 50,
            rgba: vec![255; 100 * 50 * 4],
        };
        source.clamp_to_frame(1920, 1080);
        assert!(source.x_percent >= 10.0);
        assert!(source.y_percent <= 100.0);
    }
}
