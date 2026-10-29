//! Shared scalar domains used by explicit CLI/MCP input parsers and selected
//! Manifest consumption points, before their analysis or recording work.
//! These are mathematical/type constraints, not empirical watch operating ranges.
//! Other persisted/programmatic configurations are not automatically validated.

pub fn bph(value: u64) -> Result<u32, String> {
    u32::try_from(value)
        .ok()
        .filter(|&v| v > 0)
        .ok_or_else(|| "BPH must be an integer from 1 through 4294967295".into())
}

pub fn escape_teeth(value: u32) -> Result<u32, String> {
    if value == 0 {
        Err("escape teeth must be a positive integer".into())
    } else {
        Ok(value)
    }
}

/// The lift angles the app lets a person type in, degrees. An editing
/// range only: [`lift_deg`], which the command line, MCP and session files
/// use, still accepts any finite positive angle.
pub const LIFT_ENTRY_DEG: std::ops::RangeInclusive<f64> = 10.0..=90.0;

pub fn lift_deg(value: f64) -> Result<f64, String> {
    if !value.is_finite() || value <= 0.0 {
        Err("lift angle must be finite and positive".into())
    } else {
        Ok(value)
    }
}

/// A clock scale must advance time: correction divides by 1 + ppm * 1e-6.
pub fn card_ppm(value: f64) -> Result<f64, String> {
    let factor = 1.0 + value * 1e-6;
    if !value.is_finite() || !factor.is_finite() || factor <= 0.0 {
        Err("card ppm must be finite with 1 + ppm * 1e-6 greater than zero".into())
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_scalar_domains_have_no_empirical_operating_limits() {
        assert!(bph(0).is_err());
        assert!(bph(u32::MAX as u64 + 1).is_err());
        assert_eq!(bph(1).unwrap(), 1);
        assert_eq!(bph(u32::MAX as u64).unwrap(), u32::MAX);
        assert!(escape_teeth(0).is_err());
        assert_eq!(escape_teeth(u32::MAX).unwrap(), u32::MAX);
        for value in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(lift_deg(value).is_err());
        }
        for value in [f64::MIN_POSITIVE, f64::from_bits(1), 52.0, f64::MAX] {
            assert_eq!(lift_deg(value).unwrap().to_bits(), value.to_bits());
        }
        assert!(LIFT_ENTRY_DEG.contains(&crate::take::DEFAULT_LIFT_DEG));
        assert!(lift_deg(*LIFT_ENTRY_DEG.start()).is_ok());
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1_000_000.0,
            -1_000_001.0,
        ] {
            assert!(card_ppm(value).is_err());
        }
        for value in [-999_999.999_999_999_9, -12.5, -0.0, 0.0, 12.5, f64::MAX] {
            assert_eq!(card_ppm(value).unwrap().to_bits(), value.to_bits());
        }
    }
}
