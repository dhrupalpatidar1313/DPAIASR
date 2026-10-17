use sha2::Digest;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModelManagerConfig {
    pub model_filename: String,
    pub expected_sha256_hex: String,
    pub download_base_url: String,
    pub revision: String,
    pub cache_dir: Option<PathBuf>,
}

impl ModelManagerConfig {
    /// `ggml-small.en-q5_1` — 181 MB, fast, high accuracy, optimal for CPU and entry GPUs (e.g. MX450 / 2GB VRAM)
    pub fn default_small_en_q5_1() -> Self {
        Self {
            model_filename: "ggml-small.en-q5_1.bin".into(),
            expected_sha256_hex: "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
        }
    }

    /// `ggml-small-q5_1` — 181 MB multilingual
    pub fn default_small_q5_1() -> Self {
        Self {
            model_filename: "ggml-small-q5_1.bin".into(),
            expected_sha256_hex: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
        }
    }

    /// `ggml-small.en` — 466 MB FP16 unquantized
    pub fn default_small_en() -> Self {
        Self {
            model_filename: "ggml-small.en.bin".into(),
            expected_sha256_hex: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
        }
    }

    /// `ggml-base.en` — 142 MB, very fast
    pub fn default_base_en() -> Self {
        Self {
            model_filename: "ggml-base.en.bin".into(),
            expected_sha256_hex: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
        }
    }

    /// `ggml-tiny.en` — English-only, ~75 MB, fastest
    pub fn default_tiny_en() -> Self {
        Self {
            model_filename: "ggml-tiny.en.bin".into(),
            expected_sha256_hex: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
        }
    }

    /// `ggml-medium.en` — High accuracy, 1.53 GB (Requires GPU with ≥4GB-6GB VRAM)
    pub fn default_medium_en() -> Self {
        Self {
            model_filename: "ggml-medium.en.bin".into(),
            expected_sha256_hex: "cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
        }
    }

    /// Parse model name from CLI argument
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "tiny" | "tiny.en" | "ggml-tiny.en.bin" => Some(Self::default_tiny_en()),
            "base" | "base.en" | "ggml-base.en.bin" => Some(Self::default_base_en()),
            "small.en" | "small-en" | "ggml-small.en.bin" => Some(Self::default_small_en()),
            "small" | "small.en-q5_1" | "small-q5_1" | "small-en-q5_1" | "ggml-small.en-q5_1.bin" => Some(Self::default_small_en_q5_1()),
            "medium" | "medium.en" | "ggml-medium.en.bin" => Some(Self::default_medium_en()),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelManagerError {
    #[error("could not determine platform cache directory")]
    NoCacheDir,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("download failed: {0}")]
    Download(String),
    #[error("SHA-256 mismatch: expected {expected}, got {actual}")]
    ShaMismatch { expected: String, actual: String },
}

pub struct ModelManager;

impl ModelManager {
    pub async fn ensure_model(config: &ModelManagerConfig) -> Result<PathBuf, ModelManagerError> {
        debug_assert_eq!(
            config.expected_sha256_hex.len(),
            64,
            "expected_sha256_hex must be a 64-character SHA-256 hex string"
        );

        let cache_dir = if let Some(dir) = &config.cache_dir {
            dir.clone()
        } else {
            dirs::data_local_dir()
                .ok_or(ModelManagerError::NoCacheDir)?
                .join("dpaiasr")
                .join("models")
        };

        tokio::fs::create_dir_all(&cache_dir).await?;
        let model_path = cache_dir.join(&config.model_filename);

        if model_path.exists() {
            let sha = compute_sha256(&model_path).await?;
            if sha.eq_ignore_ascii_case(&config.expected_sha256_hex) {
                tracing::info!("model cache hit: {:?}", model_path);
                return Ok(model_path);
            } else {
                tracing::warn!("cached model SHA mismatch; re-downloading");
                tokio::fs::remove_file(&model_path).await?;
            }
        }

        let url = format!("{}/resolve/{}/{}", config.download_base_url, config.revision, config.model_filename);
        tracing::info!("downloading model from {}", url);
        
        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ModelManagerError::Download(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ModelManagerError::Download(format!("HTTP {}", response.status())));
        }

        let tmp_path = model_path.with_extension("tmp");
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        let mut hasher = sha2::Sha256::new();

        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ModelManagerError::Download(e.to_string()))?;
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
            sha2::Digest::update(&mut hasher, &chunk);
        }

        let actual_sha = hex::encode(hasher.finalize());
        if !actual_sha.eq_ignore_ascii_case(&config.expected_sha256_hex) {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(ModelManagerError::ShaMismatch {
                expected: config.expected_sha256_hex.clone(),
                actual: actual_sha,
            });
        }

        tokio::fs::rename(&tmp_path, &model_path).await?;
        tracing::info!("model download complete and verified: {:?}", model_path);
        
        Ok(model_path)
    }
}

async fn compute_sha256(path: &Path) -> Result<String, ModelManagerError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 64 * 1024]; // 64 KiB read buffer
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut file, &mut buf).await?;
        if n == 0 {
            break;
        }
        sha2::Digest::update(&mut hasher, &buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_has_a_well_formed_sha256() {
        for name in ["tiny", "base", "small.en", "small", "medium"] {
            let cfg = ModelManagerConfig::from_name(name).expect(name);
            assert_eq!(cfg.expected_sha256_hex.len(), 64, "{name}: not a SHA-256 length");
            assert!(
                cfg.expected_sha256_hex.chars().all(|c| c.is_ascii_hexdigit()),
                "{name}: non-hex characters"
            );
        }
        let q5 = ModelManagerConfig::default_small_q5_1();
        assert_eq!(q5.expected_sha256_hex.len(), 64);
    }

    #[test]
    fn unknown_model_name_is_rejected() {
        assert!(ModelManagerConfig::from_name("large-v9").is_none());
    }
}
