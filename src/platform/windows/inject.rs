use super::target::TargetSnapshot;
use thiserror::Error;
use enigo::{Enigo, Keyboard, Settings};

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("Focus changed. Expected {expected:?}, got {actual:?}")]
    FocusLost {
        expected: TargetSnapshot,
        actual: Option<TargetSnapshot>,
    },
    #[error("Failed to inject text: {0}")]
    SystemError(String),
}

pub struct Injector {
    enigo: Enigo,
}

impl Injector {
    pub fn new() -> anyhow::Result<Self> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| anyhow::anyhow!("Failed to initialize enigo: {:?}", e))?;
        Ok(Self { enigo })
    }

    /// Verifies the current target matches the expected one, and if so, types the text.
    pub fn inject(&mut self, text: &str, expected_target: &TargetSnapshot) -> Result<(), InjectError> {
        let current = TargetSnapshot::current();
        if current.as_ref() != Some(expected_target) {
            return Err(InjectError::FocusLost {
                expected: *expected_target,
                actual: current,
            });
        }
        
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }

        // We append a trailing space since dictated sentences normally need separation
        let to_type = format!("{} ", text);
        
        self.enigo.text(&to_type)
            .map_err(|e| InjectError::SystemError(format!("{:?}", e)))?;
            
        Ok(())
    }
}
