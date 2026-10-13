//! A camera frame as plain RGB8, plus the resampling the models need.

/// An RGB8 image, row-major, no padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbFrame {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), (width * height * 3) as usize, "RGB8 buffer size");
        Self { width, height, data }
    }

    pub fn filled(width: u32, height: u32, rgb: [u8; 3]) -> Self {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..width * height {
            data.extend_from_slice(&rgb);
        }
        Self { width, height, data }
    }

    /// Converts BGR8 (OpenCV, some Windows cameras) in place.
    pub fn from_bgr(width: u32, height: u32, mut data: Vec<u8>) -> Self {
        for px in data.as_chunks_mut::<3>().0 {
            px.swap(0, 2);
        }
        Self::new(width, height, data)
    }

    /// Converts RGBA8 by dropping the alpha channel.
    pub fn from_rgba(width: u32, height: u32, data: &[u8]) -> Self {
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for px in data.as_chunks::<4>().0 {
            rgb.extend_from_slice(&px[..3]);
        }
        Self::new(width, height, rgb)
    }

    /// Converts NV12 (Y plane then interleaved UV), the native format of most phone cameras.
    pub fn from_nv12(width: u32, height: u32, y: &[u8], uv: &[u8], y_stride: usize, uv_stride: usize) -> Self {
        let (w, h) = (width as usize, height as usize);
        let mut rgb = Vec::with_capacity(w * h * 3);
        for row in 0..h {
            for col in 0..w {
                let yv = y[row * y_stride + col] as f32;
                let uv_index = (row / 2) * uv_stride + (col / 2) * 2;
                let u = uv[uv_index] as f32 - 128.0;
                let v = uv[uv_index + 1] as f32 - 128.0;
                let c = (yv - 16.0).max(0.0) * 1.164;
                rgb.push((c + 1.596 * v).round().clamp(0.0, 255.0) as u8);
                rgb.push((c - 0.392 * u - 0.813 * v).round().clamp(0.0, 255.0) as u8);
                rgb.push((c + 2.017 * u).round().clamp(0.0, 255.0) as u8);
            }
        }
        Self::new(width, height, rgb)
    }

    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 3) as usize;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    /// Mirrors left/right.
    pub fn flipped(&self) -> RgbFrame {
        let mut out = Vec::with_capacity(self.data.len());
        for y in 0..self.height {
            for x in (0..self.width).rev() {
                out.extend_from_slice(&self.pixel(x, y));
            }
        }
        RgbFrame::new(self.width, self.height, out)
    }

    /// Bilinear sample at continuous source coordinates; `pad` outside the frame.
    #[inline]
    pub fn sample(&self, sx: f32, sy: f32, pad: [u8; 3]) -> [u8; 3] {
        if sx < 0.0 || sy < 0.0 || sx > (self.width - 1) as f32 || sy > (self.height - 1) as f32 {
            return pad;
        }
        let x0 = sx.floor() as u32;
        let y0 = sy.floor() as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let fx = sx - x0 as f32;
        let fy = sy - y0 as f32;
        let (p00, p10, p01, p11) = (self.pixel(x0, y0), self.pixel(x1, y0), self.pixel(x0, y1), self.pixel(x1, y1));
        let mut out = [0u8; 3];
        for c in 0..3 {
            let top = p00[c] as f32 * (1.0 - fx) + p10[c] as f32 * fx;
            let bottom = p01[c] as f32 * (1.0 - fx) + p11[c] as f32 * fx;
            out[c] = (top * (1.0 - fy) + bottom * fy).round() as u8;
        }
        out
    }

    /// Writes the frame, scaled by `scale` and offset by (`pad_x`, `pad_y`) pixels, into a
    /// planar float NCHW tensor of `out_w` x `out_h` normalised to 0...1. Uncovered pixels get `pad`.
    #[allow(clippy::too_many_arguments)]
    pub fn write_planar(&self, out: &mut [f32], out_w: u32, out_h: u32, scale: f32, pad_x: f32, pad_y: f32, pad: [u8; 3]) {
        let plane = (out_w * out_h) as usize;
        debug_assert!(out.len() >= plane * 3);
        for oy in 0..out_h {
            let sy = (oy as f32 + 0.5 - pad_y) / scale - 0.5;
            for ox in 0..out_w {
                let sx = (ox as f32 + 0.5 - pad_x) / scale - 0.5;
                let px = self.sample(sx, sy, pad);
                let i = (oy * out_w + ox) as usize;
                out[i] = px[0] as f32 / 255.0;
                out[plane + i] = px[1] as f32 / 255.0;
                out[2 * plane + i] = px[2] as f32 / 255.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        let bgr = RgbFrame::from_bgr(1, 1, vec![1, 2, 3]);
        assert_eq!(bgr.pixel(0, 0), [3, 2, 1]);
        let rgba = RgbFrame::from_rgba(1, 1, &[9, 8, 7, 255]);
        assert_eq!(rgba.pixel(0, 0), [9, 8, 7]);
        let grey = RgbFrame::from_nv12(2, 2, &[128, 128, 128, 128], &[128, 128], 2, 2);
        let p = grey.pixel(0, 0);
        assert!(p.iter().all(|c| (125..=135).contains(c)), "{p:?}");
        let f = RgbFrame::new(2, 1, vec![1, 1, 1, 9, 9, 9]).flipped();
        assert_eq!(f.pixel(0, 0), [9, 9, 9]);
    }

    #[test]
    fn planar_letterbox_pads() {
        let frame = RgbFrame::filled(4, 2, [255, 0, 0]);
        let mut out = vec![0.0; 4 * 4 * 3];
        // Fit the 4x2 frame into 4x4: scale 1, pad one row top and bottom.
        frame.write_planar(&mut out, 4, 4, 1.0, 0.0, 1.0, [114, 114, 114]);
        let plane = 16;
        assert!((out[0] - 114.0 / 255.0).abs() < 1e-6); // top padding, red channel
        assert!((out[4] - 1.0).abs() < 1e-6); // first real row, red
        assert!((out[plane + 4]).abs() < 1e-6); // green of the real row is 0
    }
}
