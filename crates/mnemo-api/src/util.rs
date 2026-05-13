use std::time::SystemTime;
use std::time::UNIX_EPOCH;

pub fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

pub fn add_seconds_to_timestamp(timestamp: &str, seconds: u64) -> String {
    timestamp
        .parse::<u64>()
        .map(|value| value.saturating_add(seconds).to_string())
        .unwrap_or_else(|_| timestamp.to_string())
}

pub fn stable_hashed_id(prefix: &str, parts: &[&str]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0x1f;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{prefix}-{hash:016x}")
}
