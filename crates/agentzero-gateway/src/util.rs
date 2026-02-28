use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn generate_pairing_code() -> String {
    let seed = now_epoch_secs() ^ u64::from(std::process::id());
    format!("{:06}", seed % 1_000_000)
}

pub(crate) fn generate_session_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    format!("aztok_{nanos:x}")
}

pub(crate) fn generate_base32_secret(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(len);
    let mut x = now_epoch_secs() ^ u64::from(std::process::id());
    for _ in 0..len {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = (x as usize) % ALPHABET.len();
        out.push(ALPHABET[idx] as char);
    }
    out
}

pub(crate) fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_secs()
}
