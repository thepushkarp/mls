/// EXIF metadata extraction from image files.
///
/// Uses `kamadak-exif` to parse EXIF tags from JPEG, TIFF, PNG, WebP, etc.
/// All failures are silently swallowed — EXIF data is best-effort and many
/// images simply don't have it.
use crate::types::ExifInfo;
use exif::{Exif, In, Tag, Value};
use std::path::Path;

/// Read EXIF metadata from an image file.
///
/// Returns `None` on any failure: file not found, unsupported format, no EXIF
/// data, or a missing required tag. Never panics.
pub fn read_exif(path: &Path) -> Option<ExifInfo> {
    let file = std::fs::File::open(path).ok()?;
    let mut bufreader = std::io::BufReader::new(&file);
    let reader = exif::Reader::new();
    let exif = reader.read_from_container(&mut bufreader).ok()?;

    Some(ExifInfo {
        camera_make: get_string(&exif, Tag::Make),
        camera_model: get_string(&exif, Tag::Model),
        lens_model: get_string(&exif, Tag::LensModel),
        focal_length_mm: get_rational_f64(&exif, Tag::FocalLength),
        aperture: get_rational_f64(&exif, Tag::FNumber),
        exposure_time: get_exposure_time(&exif),
        iso: get_uint(&exif, Tag::PhotographicSensitivity),
        date_taken: get_string(&exif, Tag::DateTimeOriginal),
        gps_latitude: get_gps_coordinate(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, "S"),
        gps_longitude: get_gps_coordinate(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, "W"),
        orientation: get_uint(&exif, Tag::Orientation),
    })
}

/// Extract an ASCII string tag, stripping the surrounding double-quotes that
/// `display_value()` adds.
fn get_string(exif: &Exif, tag: Tag) -> Option<String> {
    let raw = exif
        .get_field(tag, In::PRIMARY)?
        .display_value()
        .to_string();
    // kamadak-exif wraps ASCII values in double-quotes: `"Canon"` → `Canon`
    Some(raw.trim_matches('"').to_owned())
}

/// Extract a Rational tag and return it as `f64` (first element only).
fn get_rational_f64(exif: &Exif, tag: Tag) -> Option<f64> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    match &field.value {
        Value::Rational(rats) if !rats.is_empty() => {
            let r = &rats[0];
            if r.denom == 0 {
                None
            } else {
                Some(f64::from(r.num) / f64::from(r.denom))
            }
        }
        _ => None,
    }
}

/// Extract an unsigned-integer tag (accepts Byte, Short, Long).
fn get_uint(exif: &Exif, tag: Tag) -> Option<u32> {
    exif.get_field(tag, In::PRIMARY)?.value.get_uint(0)
}

/// Format `ExposureTime` as "num/denom" string (e.g., "1/125").
fn get_exposure_time(exif: &Exif) -> Option<String> {
    let field = exif.get_field(Tag::ExposureTime, In::PRIMARY)?;
    match &field.value {
        Value::Rational(rats) if !rats.is_empty() => {
            let r = &rats[0];
            Some(format!("{}/{}", r.num, r.denom))
        }
        _ => None,
    }
}

/// Convert GPS DMS (degrees/minutes/seconds as three Rational values) to
/// decimal degrees. `negative_ref` is the hemisphere string that flips the
/// sign (e.g., `"S"` for latitude, `"W"` for longitude).
fn get_gps_coordinate(
    exif: &Exif,
    coord_tag: Tag,
    ref_tag: Tag,
    negative_ref: &str,
) -> Option<f64> {
    let coord_field = exif.get_field(coord_tag, In::PRIMARY)?;
    let rats = match &coord_field.value {
        Value::Rational(v) if v.len() >= 3 => v,
        _ => return None,
    };

    let degrees = rational_to_f64(rats[0])?;
    let minutes = rational_to_f64(rats[1])?;
    let seconds = rational_to_f64(rats[2])?;

    let decimal = degrees + minutes / 60.0 + seconds / 3600.0;

    // Check hemisphere ref to determine sign
    let ref_str = get_string(exif, ref_tag).unwrap_or_default();
    let sign = if ref_str.trim() == negative_ref {
        -1.0
    } else {
        1.0
    };

    Some(sign * decimal)
}

/// Convert a `Rational` to `f64`, returning `None` for zero denominators.
fn rational_to_f64(r: exif::Rational) -> Option<f64> {
    if r.denom == 0 {
        None
    } else {
        Some(f64::from(r.num) / f64::from(r.denom))
    }
}
