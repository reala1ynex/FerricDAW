//! Privacy-safe Host DNA seed from coarse machine traits (never shown raw).

/// Stable FNV-1a style mix — no crypto, just a host seed.
fn mix_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

fn mix_str(h: u64, s: &str) -> u64 {
    mix_bytes(h, s.as_bytes())
}

fn cpu_trait() -> String {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    format!("cores:{cores}")
}

fn hostname_trait() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown-host".into())
}

/// Collect coarse traits and fold into a 64-bit Host Seed (hex-displayable).
/// Never stores/shows serials, MACs, or disk IDs — only a hashed Host Seed.
pub fn compute_host_seed() -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    h = mix_str(h, &cpu_trait());
    h = mix_str(h, &hostname_trait());
    if let Ok(arch) = std::env::var("PROCESSOR_ARCHITECTURE") {
        h = mix_str(h, &arch);
    }
    if let Ok(id) = std::env::var("PROCESSOR_IDENTIFIER") {
        h = mix_str(h, &id.chars().take(48).collect::<String>());
    }
    if let Ok(level) = std::env::var("PROCESSOR_LEVEL") {
        h = mix_str(h, &level);
    }
    // Coarse user profile path length bucket (not the path itself).
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let bucket = (profile.len() / 8).saturating_mul(8);
        h = mix_bytes(h, &(bucket as u64).to_le_bytes());
    }
    // Never return zero — reserve 0 as "unset" on FxSlot.
    if h == 0 {
        0xF351_CDA0_0000_0001
    } else {
        h
    }
}

/// Hex display for Host Seed (UI never shows raw hardware IDs).
pub fn format_host_seed(seed: u64) -> String {
    format!("{:016X}", seed)
}

/// Parse pasted Host Seed hex (with optional separators).
pub fn parse_host_seed(text: &str) -> Option<u64> {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    if cleaned.is_empty() || cleaned.len() > 16 {
        return None;
    }
    u64::from_str_radix(&cleaned, 16).ok()
}

pub fn random_host_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xDEAD_BEEF);
    let mut h = t ^ 0xA5A5_5A5A_C3C3_3C3C;
    h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    if h == 0 {
        0xCAFE_BABE_F00D_FADE
    } else {
        h
    }
}
