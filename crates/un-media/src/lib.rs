//! Frame sources for desktops: the webcam through nokhwa (AVFoundation on macOS, Media
//! Foundation on Windows, V4L2 on Linux) and image files for replay and tests. Phones
//! use their own camera APIs and hand frames to the engine through `un-ffi`.

use std::path::Path;
use std::time::Instant;

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType, Resolution};
use nokhwa::Camera;
use thiserror::Error;
use un_vision::RgbFrame;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("camera: {0}")]
    Camera(String),
    #[error("image: {0}")]
    Image(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub index: u32,
    pub name: String,
}

/// Usable cameras, in index order.
pub fn list_cameras() -> Result<Vec<CameraInfo>, MediaError> {
    let cameras = nokhwa::query(ApiBackend::Auto).map_err(|e| MediaError::Camera(e.to_string()))?;
    Ok(cameras
        .into_iter()
        .enumerate()
        .map(|(i, c)| CameraInfo {
            index: match c.index() {
                CameraIndex::Index(n) => *n,
                CameraIndex::String(_) => i as u32,
            },
            name: c.human_name(),
        })
        .collect())
}

/// Anything that yields frames with timestamps (seconds).
pub trait FrameSource {
    fn next_frame(&mut self) -> Result<Option<(RgbFrame, f64)>, MediaError>;
}

/// The webcam.
pub struct CameraSource {
    camera: Camera,
    started: Instant,
    pub rotate: Rotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    #[default]
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    pub fn from_degrees(degrees: u32) -> Option<Rotation> {
        match degrees % 360 {
            0 => Some(Rotation::None),
            90 => Some(Rotation::Cw90),
            180 => Some(Rotation::Cw180),
            270 => Some(Rotation::Cw270),
            _ => None,
        }
    }
}

pub fn rotate(frame: &RgbFrame, rotation: Rotation) -> RgbFrame {
    let (w, h) = (frame.width, frame.height);
    match rotation {
        Rotation::None => frame.clone(),
        Rotation::Cw180 => {
            let mut out = Vec::with_capacity(frame.data.len());
            for y in (0..h).rev() {
                for x in (0..w).rev() {
                    out.extend_from_slice(&frame.pixel(x, y));
                }
            }
            RgbFrame::new(w, h, out)
        }
        Rotation::Cw90 => {
            let mut out = Vec::with_capacity(frame.data.len());
            for y in 0..w {
                for x in 0..h {
                    out.extend_from_slice(&frame.pixel(y, h - 1 - x));
                }
            }
            RgbFrame::new(h, w, out)
        }
        Rotation::Cw270 => {
            let mut out = Vec::with_capacity(frame.data.len());
            for y in 0..w {
                for x in 0..h {
                    out.extend_from_slice(&frame.pixel(w - 1 - y, x));
                }
            }
            RgbFrame::new(h, w, out)
        }
    }
}

impl CameraSource {
    /// Opens camera `index`, asking for `width` x `height` (the closest supported size is used).
    pub fn open(index: u32, width: u32, height: u32) -> Result<Self, MediaError> {
        let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestResolution(Resolution::new(width, height)));
        let mut camera = Camera::new(CameraIndex::Index(index), format).map_err(|e| MediaError::Camera(hint(&e.to_string())))?;
        camera.open_stream().map_err(|e| MediaError::Camera(hint(&e.to_string())))?;
        Ok(Self { camera, started: Instant::now(), rotate: Rotation::None })
    }

    pub fn resolution(&self) -> (u32, u32) {
        let r = self.camera.resolution();
        (r.width(), r.height())
    }
}

impl FrameSource for CameraSource {
    fn next_frame(&mut self) -> Result<Option<(RgbFrame, f64)>, MediaError> {
        let buffer = self.camera.frame().map_err(|e| MediaError::Camera(e.to_string()))?;
        let image = buffer.decode_image::<RgbFormat>().map_err(|e| MediaError::Camera(e.to_string()))?;
        let frame = RgbFrame::new(image.width(), image.height(), image.into_raw());
        let frame = rotate(&frame, self.rotate);
        Ok(Some((frame, self.started.elapsed().as_secs_f64())))
    }
}

impl Drop for CameraSource {
    fn drop(&mut self) {
        let _ = self.camera.stop_stream();
    }
}

fn hint(error: &str) -> String {
    let platform = match std::env::consts::OS {
        "macos" => " On macOS, allow camera access for this app under System Settings > Privacy & Security > Camera.",
        "windows" => " On Windows, allow desktop apps to use the camera under Settings > Privacy & security > Camera, and close other apps using it.",
        "linux" => " On Linux, check that /dev/video* exists and your user is in the 'video' group.",
        _ => "",
    };
    format!("{error}.{platform}")
}

/// Replays one image repeatedly, or a sequence of image files, at 30 fps timestamps.
pub struct FileSource {
    frames: Vec<RgbFrame>,
    index: usize,
    repeat: usize,
    served: usize,
}

impl FileSource {
    pub fn open(paths: &[&Path], repeat: usize) -> Result<Self, MediaError> {
        let mut frames = Vec::new();
        for path in paths {
            frames.push(load_image(path)?);
        }
        if frames.is_empty() {
            return Err(MediaError::Image("no files".into()));
        }
        Ok(Self { frames, index: 0, repeat: repeat.max(1), served: 0 })
    }
}

impl FrameSource for FileSource {
    fn next_frame(&mut self) -> Result<Option<(RgbFrame, f64)>, MediaError> {
        if self.served >= self.frames.len() * self.repeat {
            return Ok(None);
        }
        let frame = self.frames[self.index % self.frames.len()].clone();
        self.index += 1;
        self.served += 1;
        Ok(Some((frame, self.served as f64 / 30.0)))
    }
}

pub fn load_image(path: &Path) -> Result<RgbFrame, MediaError> {
    let image = image::open(path).map_err(|e| MediaError::Image(format!("{}: {e}", path.display())))?.to_rgb8();
    Ok(RgbFrame::new(image.width(), image.height(), image.into_raw()))
}

pub fn save_image(path: &Path, frame: &RgbFrame) -> Result<(), MediaError> {
    let image = image::RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
        .ok_or_else(|| MediaError::Image("bad frame size".into()))?;
    image.save(path).map_err(|e| MediaError::Image(format!("{}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_change_shape() {
        let f = RgbFrame::new(2, 1, vec![1, 1, 1, 2, 2, 2]);
        let r = rotate(&f, Rotation::Cw90);
        assert_eq!((r.width, r.height), (1, 2));
        assert_eq!(r.pixel(0, 0), [1, 1, 1]);
        assert_eq!(rotate(&f, Rotation::Cw180).pixel(0, 0), [2, 2, 2]);
        assert_eq!(rotate(&f, Rotation::Cw270).pixel(0, 1), [1, 1, 1]);
    }
}
