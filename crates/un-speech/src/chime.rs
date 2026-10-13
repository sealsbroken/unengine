//! The "listen now" chime: a short rising two-tone, synthesised, no asset needed.

pub const RATE: u32 = 24_000;

/// Mono samples of the chime at [`RATE`].
pub fn samples() -> Vec<f32> {
    let (low, high) = (660.0f32, 880.0f32);
    let count = (RATE as f32 * 0.52) as usize;
    let mut out = Vec::with_capacity(count);
    let mut phase = 0.0f32;
    for i in 0..count {
        let t = i as f32 / RATE as f32;
        let frequency = if t < 0.42 {
            low
        } else if t < 0.52 {
            low + (high - low) * ((t - 0.42) / 0.10)
        } else {
            high
        };
        phase += 2.0 * std::f32::consts::PI * frequency / RATE as f32;
        let attack = (i as f32 / (RATE as f32 * 0.010)).min(1.0);
        let release = ((count - i) as f32 / (RATE as f32 * 0.060)).min(1.0);
        let dip = 1.0 - 0.35 * (1.0 - (t - 0.47).abs() / 0.05).max(0.0);
        out.push(phase.sin() * 0.55 * attack * release * dip);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn chime_is_half_a_second_and_bounded() {
        let s = super::samples();
        assert_eq!(s.len(), (super::RATE as f32 * 0.52) as usize);
        assert!(s.iter().all(|v| v.abs() <= 0.56));
        assert!(s[0].abs() < 0.01);
    }
}
