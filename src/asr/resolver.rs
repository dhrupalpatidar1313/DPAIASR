use crate::asr::backend::{AsrBackendKind, BackendPreference};

/// Configuration fed into the resolver at startup.
#[derive(Debug, Clone)]
pub struct BackendResolverConfig {
    pub preference: BackendPreference,
    pub cpu_threads: u32,
    pub gpu_device: u32,
}

/// The resolver's output — tells the factory exactly what to build.
#[derive(Debug, Clone)]
pub struct BackendResolution {
    pub backend_kind: AsrBackendKind,
    pub cpu_threads: u32,
    pub gpu_device: Option<u32>,
    pub reason: String,
}

pub struct BackendResolver;

impl BackendResolver {
    pub fn resolve(config: &BackendResolverConfig) -> Result<BackendResolution, String> {
        match config.preference {
            BackendPreference::Cpu => Ok(BackendResolution {
                backend_kind: AsrBackendKind::Cpu,
                cpu_threads: config.cpu_threads,
                gpu_device: None,
                reason: "user requested CPU".to_string(),
            }),
            BackendPreference::Gpu => {
                #[cfg(feature = "cuda")]
                {
                    if Self::try_init_cuda_probe() {
                        Ok(BackendResolution {
                            backend_kind: AsrBackendKind::Cuda,
                            cpu_threads: config.cpu_threads,
                            gpu_device: Some(config.gpu_device),
                            reason: "user requested GPU, CUDA compiled and device found".to_string(),
                        })
                    } else {
                        // User specifically requested GPU, but no device found - error or fallback?
                        // Fallback is safer to prevent crashes.
                        tracing::warn!("GPU requested and CUDA compiled, but no active device found! Falling back to CPU.");
                        Ok(BackendResolution {
                            backend_kind: AsrBackendKind::Cpu,
                            cpu_threads: config.cpu_threads,
                            gpu_device: None,
                            reason: "user requested GPU but no active CUDA device found; fallback to CPU".to_string(),
                        })
                    }
                }
                #[cfg(not(feature = "cuda"))]
                {
                    Err("backend=gpu requested but no GPU backend was compiled in; rebuild with --features cuda".to_string())
                }
            }
            BackendPreference::Auto => {
                #[cfg(feature = "cuda")]
                {
                    if Self::try_init_cuda_probe() {
                        Ok(BackendResolution {
                            backend_kind: AsrBackendKind::Cuda,
                            cpu_threads: config.cpu_threads,
                            gpu_device: Some(config.gpu_device),
                            reason: "auto: CUDA compiled and active device found".to_string(),
                        })
                    } else {
                        tracing::warn!("auto: CUDA feature compiled, but no active device found! Falling back to CPU gracefully.");
                        Ok(BackendResolution {
                            backend_kind: AsrBackendKind::Cpu,
                            cpu_threads: config.cpu_threads,
                            gpu_device: None,
                            reason: "auto: CUDA compiled but no device found; fallback to CPU".to_string(),
                        })
                    }
                }
                #[cfg(not(feature = "cuda"))]
                {
                    tracing::debug!("GPU backend not compiled in; auto resolves to CPU");
                    Ok(BackendResolution {
                        backend_kind: AsrBackendKind::Cpu,
                        cpu_threads: config.cpu_threads,
                        gpu_device: None,
                        reason: "auto: no GPU backend compiled, selected CPU".to_string(),
                    })
                }
            }
        }
    }

    /// Simple check to see if an NVIDIA driver and device are visible at runtime.
    #[allow(dead_code)]
    fn try_init_cuda_probe() -> bool {
        std::process::Command::new("nvidia-smi")
            .arg("-L") // Query attached GPUs briefly
            .output()
            .map(|output| output.status.success() && !output.stdout.is_empty())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_resolves_to_cpu_when_no_gpu_features() {
        // Without cuda feature, Auto should resolve to Cpu
        #[cfg(not(feature = "cuda"))]
        {
            let config = BackendResolverConfig {
                preference: BackendPreference::Auto,
                cpu_threads: 4,
                gpu_device: 0,
            };
            let res = BackendResolver::resolve(&config).unwrap();
            assert_eq!(res.backend_kind, AsrBackendKind::Cpu);
        }
    }
}
