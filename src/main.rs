use std::{fs::File, io::BufReader};

use exif::{In, Reader, Tag, Value};

#[derive(Debug)]
pub struct GpsInfo {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f64>,
    pub timestamp: Option<String>,
}

impl GpsInfo {
    pub fn from_file(path: &str) -> Option<Self> {
        let file = File::open(path).ok()?;
        let mut bufreader = BufReader::new(&file);
        let exif = Reader::new().read_from_container(&mut bufreader).ok()?;

        let mut gps_info = GpsInfo {
            latitude: None,
            longitude: None,
            altitude: None,
            timestamp: None,
        };

        // Extract GPS coordinates
        if let (Some(lat), Some(lat_ref), Some(lon), Some(lon_ref)) = (
            get_gps_rational(&exif, Tag::GPSLatitude),
            get_gps_ref(&exif, Tag::GPSLatitudeRef),
            get_gps_rational(&exif, Tag::GPSLongitude),
            get_gps_ref(&exif, Tag::GPSLongitudeRef),
        ) {
            gps_info.latitude = Some(convert_to_decimal_degree(lat, lat_ref));
            gps_info.longitude = Some(convert_to_decimal_degree(lon, lon_ref))
        }

        // Extract altitude
        if let Some(alt) = get_gps_altitude(&exif) {
            gps_info.altitude = Some(alt);
        }

        // Extract gps timestamp
        if let Some(time) = get_gps_timestamp(&exif) {
            gps_info.timestamp = Some(time)
        }

        Some(gps_info)
    }

    pub fn print_location(&self) {
        println!("\n GPS Information");
        match (self.latitude, self.longitude) {
            (Some(lat), Some(lon)) => {
                println!(
                    "  + Location: {:.6}°{}, {:.6}°{}",
                    lat.abs(),
                    if lat >= 0.0 { "N" } else { "S" },
                    lon.abs(),
                    if lon >= 0.0 { "E" } else { "W" }
                );

                println!(
                    " - Google Maps Link: https://www.google.com/maps?q={},{}",
                    lat, lon
                );
            }
            _ => println!(" - No GPS Coordinates found in the image"),
        }

        if let Some(alt) = &self.altitude {
            println!(" - Altitude: {:.1} meters", alt);
        }

        if let Some(time) = &self.timestamp {
            println!(" - GPS Timestamp: {}", time);
        }
    }
}

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

fn main () {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <image_path>", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];
    println!("[*] Reading image metadata from: {}", path); // ✅ Debugging log

    match GpsInfo::from_file(path) {
        Some(gps_info) => {
            println!("[+] Successfully extracted GPS metadata.");
            gps_info.print_location();
        }
        None => {
            eprintln!("[-] No GPS information found or failed to read metadata.");
        }
    }
}
