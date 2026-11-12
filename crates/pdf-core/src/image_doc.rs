//! Raster image documents (PNG, JPEG, GIF, WebP, BMP, TIFF) decoded with `image`.
//!
//! An image is exposed to the UI as a one-page document so the viewer's
//! virtual scrolling, zoom and caching work unchanged. One image pixel is one
//! CSS pixel at 100% zoom, i.e. 0.75 "points" (the viewer converts 1 pt = 4/3 px).

use std::path::Path;

use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader, RgbaImage};

use crate::{PageSize, PdfError, RenderedPage};

pub struct ImageDoc {
    rgba: RgbaImage,
}

/// File extensions the engine opens as images (lowercase, no dot).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];

impl ImageDoc {
    pub fn open(path: &Path) -> Result<Self, PdfError> {
        let err = |e: &dyn std::fmt::Display| PdfError::Image(e.to_string());
        let mut decoder = ImageReader::open(path)
            .map_err(|e| err(&e))?
            .with_guessed_format()
            .map_err(|e| err(&e))?
            .into_decoder()
            .map_err(|e| err(&e))?;
        let orientation = decoder.orientation().map_err(|e| err(&e))?;
        let mut img = DynamicImage::from_decoder(decoder).map_err(|e| err(&e))?;
        img.apply_orientation(orientation);
        Ok(Self { rgba: img.into_rgba8() })
    }

    #[cfg(test)]
    pub fn from_rgba(rgba: RgbaImage) -> Self {
        Self { rgba }
    }

    pub fn page_size(&self) -> PageSize {
        PageSize {
            width: self.rgba.width() as f32 * 0.75,
            height: self.rgba.height() as f32 * 0.75,
        }
    }

    /// Scale to `width_px` (never above the native width: the canvas is
    /// scaled by CSS anyway, upscaling would only waste memory).
    pub fn render(&self, width_px: u32) -> RenderedPage {
        let (w, h) = self.rgba.dimensions();
        let target_w = width_px.clamp(1, w);
        if target_w == w {
            return RenderedPage { width: w, height: h, rgba: self.rgba.as_raw().clone() };
        }
        let target_h = ((h as f64 * target_w as f64 / w as f64).round() as u32).max(1);
        let scaled = image::imageops::resize(&self.rgba, target_w, target_h, FilterType::Triangle);
        RenderedPage { width: target_w, height: target_h, rgba: scaled.into_raw() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn page_size_maps_one_pixel_to_three_quarter_point() {
        let d = ImageDoc::from_rgba(RgbaImage::new(400, 200));
        let s = d.page_size();
        assert_eq!((s.width, s.height), (300.0, 150.0));
    }

    #[test]
    fn render_downscales_keeping_aspect_and_caps_at_native() {
        let d = ImageDoc::from_rgba(RgbaImage::from_pixel(400, 200, Rgba([10, 20, 30, 255])));
        let r = d.render(100);
        assert_eq!((r.width, r.height), (100, 50));
        assert_eq!(r.rgba.len(), 100 * 50 * 4);
        assert_eq!(&r.rgba[..4], &[10, 20, 30, 255]);
        let r = d.render(5000);
        assert_eq!((r.width, r.height), (400, 200));
    }

    #[test]
    fn opens_png_from_disk() {
        let dir = std::env::temp_dir().join(format!("foarbyld-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.png");
        RgbaImage::from_pixel(8, 4, Rgba([1, 2, 3, 255])).save(&p).unwrap();
        let d = ImageDoc::open(&p).unwrap();
        assert_eq!(d.rgba.dimensions(), (8, 4));
        assert!(ImageDoc::open(&dir.join("missing.png")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
