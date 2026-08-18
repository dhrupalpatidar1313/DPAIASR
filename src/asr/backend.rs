#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrBackendKind {
    Cpu,
    #[cfg(feature = "cuda")]
    Cuda,
}

/// Runtime backend selection preference from config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendPreference {
    Cpu,
    Gpu,
    Auto,
}

impl std::str::FromStr for BackendPreference {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "cpu" => Ok(Self::Cpu),
            "gpu" => Ok(Self::Gpu),
            "auto" => Ok(Self::Auto),
            other => Err(format!("unknown backend preference '{other}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_backend_preference_parsing() {
        assert_eq!(BackendPreference::from_str("cpu").unwrap(), BackendPreference::Cpu);
        assert_eq!(BackendPreference::from_str("GPU").unwrap(), BackendPreference::Gpu);
        assert_eq!(BackendPreference::from_str("Auto").unwrap(), BackendPreference::Auto);
        assert!(BackendPreference::from_str("invalid").is_err());
    }
}
