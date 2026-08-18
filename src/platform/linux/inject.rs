use super::target::TargetSnapshot;

pub struct TextInjector;

impl TextInjector {
    pub fn new() -> Self {
        Self
    }

    pub fn inject(&self, _target: &TargetSnapshot, text: &str) -> anyhow::Result<()> {
        use std::process::Command;
        
        let status = Command::new("wtype")
            .arg(text)
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to execute wtype (is it installed?): {}", e))?;
            
        if !status.success() {
            anyhow::bail!("wtype failed to inject text (compositor may not support wlr-virtual-keyboard)");
        }
        
        Ok(())
    }
}
