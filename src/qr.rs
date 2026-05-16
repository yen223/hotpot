use image::ImageReader;
use rqrr::PreparedImage;

use crate::AppError;

/// Decode QR codes from an image file.
///
/// Pipeline: load image → upscale 3× with nearest-neighbor if either
/// dimension is under 1000px → convert to grayscale → detect with rqrr.
///
/// Returns the decoded content of the first QR code found.
pub fn decode_qr_from_image(image_path: &str) -> Result<String, AppError> {
    // Load and decode the image
    let img = ImageReader::open(image_path)
        .map_err(|e| AppError::new(format!("Failed to open image: {e}")))?
        .decode()
        .map_err(|e| AppError::new(format!("Failed to decode image: {e}")))?;

    // Convert to luma (grayscale) for QR code detection
    let luma_img = img.to_luma8();

    // Upscale small images so rqrr can detect QR patterns more reliably
    let luma_img = if luma_img.width() < 1000 || luma_img.height() < 1000 {
        image::imageops::resize(
            &luma_img,
            luma_img.width() * 3,
            luma_img.height() * 3,
            image::imageops::FilterType::Nearest,
        )
    } else {
        luma_img
    };

    // Prepare image and detect QR codes
    let mut prepared = PreparedImage::prepare(luma_img);
    let grids = prepared.detect_grids();

    if grids.is_empty() {
        return Err(AppError::new("No QR code found in image"));
    }

    // Decode the first QR code found
    let (_, content) = grids[0]
        .decode()
        .map_err(|e| AppError::new(format!("Failed to decode QR code: {e:?}")))?;

    Ok(content)
}
