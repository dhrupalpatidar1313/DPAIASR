use anyhow::Result;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};

pub struct HotkeyManager {
    manager: Option<GlobalHotKeyManager>,
    toggle_hotkey: HotKey,
    retry_hotkey: HotKey,
    copy_hotkey: HotKey,
}

impl HotkeyManager {
    pub fn new() -> Result<Self> {
        let toggle_hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyM);
        let retry_hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyR);
        let copy_hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyC);
        
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => {
                let _ = m.register(toggle_hotkey);
                let _ = m.register(retry_hotkey);
                let _ = m.register(copy_hotkey);
                Some(m)
            }
            Err(e) => {
                tracing::warn!("Failed to initialize GlobalHotKeyManager (Common on Wayland without XDG Portal): {:?}", e);
                tracing::warn!("Please use CLI arguments (e.g. `dpaiasr --toggle`) and bind them in your compositor settings.");
                None
            }
        };

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
        if let Some(ref mut m) = self.manager {
            let _ = m.unregister(self.toggle_hotkey);
            let _ = m.unregister(self.retry_hotkey);
            let _ = m.unregister(self.copy_hotkey);
        }
    }
}
