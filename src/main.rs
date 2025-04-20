use std::{cell::RefCell, fs::{self, File}, io::BufReader, path::Path, rc::Rc};

use exif::{In, Reader, Tag, Value};
use fltk::{app::{self, redraw},
     button::Button,
     dialog::{FileDialog, FileDialogType}, 
     enums::ColorDepth, 
     frame:: Frame, 
     group::{Group, Pack, Scroll}, 
     image::RgbImage, 
     prelude::*, 
     window::Window};
use image::GenericImageView;



fn get_gps_rational(exif: &exif::Exif, tag: Tag) -> Option<Vec<exif::Rational>> {
    if let Some(field) = exif.get_field(tag, In::PRIMARY) {
        if let Value::Rational(rationals) = &field.value {
            return Some(rationals.to_vec());
        }
    }
    None
}

fn get_gps_ref(exif: &exif::Exif, tag: Tag) -> Option<String> {
    if let Some(field) = exif.get_field(tag, In::PRIMARY) {
        if let Value::Ascii(chars) = &field.value {
            if let Ok(s) = std::str::from_utf8(&chars[0]) {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn get_gps_altitude(exif: &exif::Exif) -> Option<f64> {
    let altitude = get_gps_rational(exif, Tag::GPSAltitude)?;
    if altitude.is_empty() {
        return None;
    }

    let alt_ref = exif
        .get_field(Tag::GPSAltitudeRef, In::PRIMARY)
        .and_then(|field| {
            if let Value::Byte(bytes) = &field.value {
                Some(bytes[0] != 0) // 0 = above sea level, 1 = below
            } else {
                None
            }
        })
        .unwrap_or(false);

    let altitude = altitude[0].to_f64();
    Some(if alt_ref { -altitude } else { altitude })
}

fn get_gps_timestamp(exif: &exif::Exif) -> Option<String> {
    let time = get_gps_rational(exif, Tag::GPSTimeStamp)?;
    if time.len() != 3 {
        return None;
    }

    let hour = time[0].to_f64() as u32;
    let minute = time[1].to_f64() as u32;
    let second = time[2].to_f64() as u32;

    let date = exif.get_field(Tag::GPSDateStamp, In::PRIMARY).and_then(|field| {
        if let Value::Ascii(chars) = &field.value {
            std::str::from_utf8(&chars[0]).ok().map(|s| s.to_string())
        } else {
            None
        }
    });

    match date {
        Some(date) => Some(format!("{} {:02}:{:02}:{:02}", date, hour, minute, second)),
        None => Some(format!("{:02}:{:02}:{:02}", hour, minute, second)),
    }
}

fn convert_to_decimal_degree(components: Vec<exif::Rational>, reference: String) -> f64 {
    if components.len() != 3 {
        return 0.0;
    }

    let degrees = components[0].to_f64();
    let minutes = components[1].to_f64();
    let seconds = components[2].to_f64();

    let mut decimal = degrees + minutes / 60.0 + seconds / 3600.0;

    if reference == "S" || reference == "W" {
        decimal = -decimal;
    }

    decimal
}

fn load_any_image(path: &str) -> Option<RgbImage> {
    match image::open(path) {
        Ok(img) => {
            let rgb_img = img.to_rgb8();
            let (width, height) = img.dimensions();
            RgbImage::new(&rgb_img, width as i32, height as i32, ColorDepth::Rgb8).ok()
        }
        Err(e) => {
            eprintln!("Failed to load image: {}", e);
            None
        }
    }
}

fn get_file_info(path: &Path) -> Vec<String> {
    let mut info = Vec::new();

    // Basic file info
    if let Ok(metadata) = fs::metadata(path) {
        let size_bytes = metadata.len();
        if size_bytes > 1_000_000 {
            info.push(format!("Size: {:.2} MB", size_bytes as f64 / 1_000_000.0));
        } else if size_bytes > 1_000 {
            info.push(format!("Size: {:.2} KB", size_bytes as f64 / 1_000.0));
        } else {
            info.push(format!("Size: {} bytes", size_bytes));
        }

        if let Ok(time) = metadata.modified() {
            if let Ok(time) = time.duration_since(std::time::UNIX_EPOCH) {
                use chrono::TimeZone;
                let datetime = chrono::Local.timestamp_opt(time.as_secs() as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string();
                info.push(format!("Modified: {}", datetime));
            }
        }
    }

    // GPS Information
    if let Ok(file) = File::open(path) {
        let mut reader = BufReader::new(file);
        if let Ok(exif) = Reader::new().read_from_container(&mut reader) {
            if let (Some(lat), Some(lat_ref), Some(lon), Some(lon_ref)) = (
                get_gps_rational(&exif, Tag::GPSLatitude),
                get_gps_ref(&exif, Tag::GPSLatitudeRef),
                get_gps_rational(&exif, Tag::GPSLongitude),
                get_gps_ref(&exif, Tag::GPSLongitudeRef),
            ) {
                let latitude = convert_to_decimal_degree(lat, lat_ref);
                let longitude = convert_to_decimal_degree(lon, lon_ref);

                info.push(format!("Location: {:.6}*{}, {:.6}*{}",
                    latitude.abs(),
                    if latitude >= 0.0 { "N" } else { "S" },
                    longitude.abs(),
                    if longitude >= 0.0 { "E" } else { "W" }
                ));

                info.push(format!("Google Maps Link: https://www.google.com/maps?q={},{}",
                    latitude, longitude
                ));
            }

            if let Some(alt) = get_gps_altitude(&exif) {
                info.push(format!("Altitude: {:.1} meters", alt));
            }

            if let Some(time) = get_gps_timestamp(&exif) {
                info.push(format!("GPS Timestamp: {}", time));
            }
        } 
    }

    // Format and additional information
    if let Some(extension) = path.extension() {
        if let Some(ext_str) = extension.to_str() {
            info.push(format!("Format: {}", ext_str.to_uppercase()));

            if ext_str.eq_ignore_ascii_case("tif") || ext_str.eq_ignore_ascii_case("tiff") {
                if let Ok(img) = image::open(path) {
                    info.push(format!("Color Type: {:?}", img.color()));
                    if let Some(depth) = match img.color() {
                        image::ColorType::L8 => Some(8),
                        image::ColorType::L16 => Some(16),
                        image::ColorType::Rgb8 => Some(24),
                        image::ColorType::Rgb16 => Some(48),
                        _ => None,
                    } {
                        info.push(format!("Bit Depth: {} bits", depth));
                    }
                }
            }
        }
    }
    info
}

fn main() {
    let app = app::App::default();
    let mut wind = Window::new(100, 100, 800, 600, "Image Viewer");

    // create toolbar
    let toolbar = Group::new(0, 0, 800, 40, "");
    let mut open_btn = Button::new(10, 5, 100, 30, "Open Image");
    let mut info_btn = Button::new(120, 5, 100, 30, "Image Info");
    toolbar.end();

    // create frame for image display
    let frame = Rc::new(RefCell::new(Frame::new(0, 40, 800, 560, "")));
    frame.borrow_mut().set_frame(fltk::enums::FrameType::FlatBox);
    frame.borrow_mut().set_color(fltk::enums::Color::White);

    // Store the current image path
    let current_path = Rc::new(RefCell::new(None::<std::path::PathBuf>));

    // clone frame and path for open button
    let frame_open = frame.clone();
    let path_open = current_path.clone();
    open_btn.set_callback(move |_| {
        let mut dialog = FileDialog::new(FileDialogType::BrowseFile);
        dialog.set_filter("Image files\t*.{jpg,jpeg,png,gif,bmp,tif,tiff}");
        dialog.show();

        if let Some(path) = dialog.filename().to_str() {
            if let Some(mut image) = load_any_image(path) {
                let mut frame = frame_open.borrow_mut();
                // Calculate scaling
                let scale_w = frame.width() as f64 / image.width() as f64;
                let scale_h = frame.height() as f64 / image.height() as f64;
                let scale = scale_w.min(scale_h);

                let new_w = (image.width() as f64 * scale) as i32;
                let new_h = (image.height() as f64 * scale) as i32;

               image.scale(new_w, new_h, true, true);
               frame.set_image(Some(image));
               frame.redraw();

                    // Store the path
                    *path_open.borrow_mut() = Some(std::path::PathBuf::from(path));
                
            }
        }
    });

    // clone frame and path for info button
    let frame_info = frame.clone();
    let path_info = current_path.clone();

    info_btn.set_callback(move |_| {
        if let Some(image) = frame_info.borrow().image() {
            let mut info_window = Window::new(200, 200, 450, 300, "Image Information");
            let scroll = Scroll::new(5, 5, 430, 290, "");
            let mut pack = Pack::new(5, 5, 420, 290, "");
            pack.set_spacing(5);

            let dimensions = format!("Dimensions: {}*{} pixels", image.width(), image.height());
            Frame::new(0, 0, 380, 30, &dimensions[..]);

            let aspect_ratio = image.width() as f64 / image.height() as f64;
            let aspect = format!("Aspect Ratio: {:.3}", aspect_ratio);
            Frame::new(0, 0, 400, 30, &aspect[..]);

            let print_size_inches = format!("Print Size at 300 DPI: {:.1}\" x {:.1}\"",
                image.width() as f64 / 300.0,
                image.height() as f64 / 300.0
            );
            Frame::new(0, 0, 400, 30, &print_size_inches[..]);

            if let Some(path) = path_info.borrow().as_ref() {
                for info_line in get_file_info(path) {
                    // check if this is a map link
                    if info_line.starts_with("Google Maps Link: ") {
                        let url = info_line.replace("Google Maps Link: ", "");
                        let mut link_btn = Button::new(0, 0, 430, 30, url.as_str());
                        link_btn.set_label_color(fltk::enums::Color::Blue);
                        let url_clone = url.clone();
                        link_btn.set_callback(move |_| {
                            if let Err(e) = open::that(&url_clone) {
                                eprintln!("Failed to open URL: {}", e);
                            }
                        });
                    } else {
                        Frame::new(0, 0, 400, 30, &info_line[..]);
                    }
                }
            }

            pack.end();
            scroll.end();
            info_window.end();
            info_window.show();
        }
    });

    wind.end();
    wind.show();

    app.run().unwrap();
}