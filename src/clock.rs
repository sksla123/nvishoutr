use chrono::{Local, Offset};

pub fn stamp() -> String {
    let now = Local::now();
    let offset = now.offset().fix().local_minus_utc();
    let zone = match offset {
        32400 => "KST".to_string(),
        _ => now.format("%:z").to_string(),
    };
    format!("{} {}", now.format("%Y-%m-%d %H:%M:%S"), zone)
}
