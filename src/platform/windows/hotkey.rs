use anyhow::Result;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    toggle_hotkey: HotKey,
    retry_hotkey: HotKey,
    copy_hotkey: HotKey,
}

impl HotkeyManager {
    pub fn new() -> Result<Self> {
        let manager = GlobalHotKeyManager::new().map_err(|e| anyhow::anyhow!("Failed to initialize hotkey manager: {:?}", e))?;
        
        let toggle_hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyM);
        let retry_hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyR);
        let copy_hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyC);
        
        manager.register(toggle_hotkey).map_err(|e| anyhow::anyhow!("Failed to register Alt+M hotkey: {:?}", e))?;
        manager.register(retry_hotkey).map_err(|e| anyhow::anyhow!("Failed to register Alt+R hotkey: {:?}", e))?;
        manager.register(copy_hotkey).map_err(|e| anyhow::anyhow!("Failed to register Alt+C hotkey: {:?}", e))?;
        
        Ok(Self { manager, toggle_hotkey, retry_hotkey, copy_hotkey })
    }
    
    pub fn is_toggle(&self, event: &global_hotkey::GlobalHotKeyEvent) -> bool {
        event.id == self.toggle_hotkey.id()
    }

    pub fn is_retry(&self, event: &global_hotkey::GlobalHotKeyEvent) -> bool {
        event.id == self.retry_hotkey.id()
    }

    pub fn is_copy(&self, event: &global_hotkey::GlobalHotKeyEvent) -> bool {
        event.id == self.copy_hotkey.id()
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        let _ = self.manager.unregister(self.toggle_hotkey);
        let _ = self.manager.unregister(self.retry_hotkey);
        let _ = self.manager.unregister(self.copy_hotkey);
    }
}
