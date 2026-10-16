//! Domains for explicit public CLI/MCP scalar inputs, before work or side effects.
//! These are mathematical/type constraints, not empirical watch operating ranges.
//! Persisted settings and programmatic analysis configurations are not validated here.

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
