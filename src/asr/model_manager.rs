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
    /// `ggml-small-q5_1` — higher accuracy, ~53s RTF on i5-9400F (slowest)
    pub fn default_small_q5_1() -> Self {
        Self {
            model_filename: "ggml-small-q5_1.bin".into(),
            expected_sha256_hex: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
        }
    }

    /// `ggml-tiny.en` — English-only, ~6x faster than small, best for real-time dictation
    pub fn default_tiny_en() -> Self {
        Self {
            model_filename: "ggml-tiny.en.bin".into(),
            expected_sha256_hex: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
        }
    }

    /// `ggml-medium.en` — High accuracy, English-only, intended for GPU inference.
    pub fn default_medium_en() -> Self {
        Self {
            model_filename: "ggml-medium.en.bin".into(),
            expected_sha256_hex: "cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356".into(),
            download_base_url: "https://huggingface.co/ggerganov/whisper.cpp".into(),
            revision: "main".into(),
            cache_dir: Some(std::env::current_dir().unwrap_or_default().join("assets").join("models")),
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
