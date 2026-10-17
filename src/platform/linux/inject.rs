use super::target::TargetSnapshot;
use enigo::{Enigo, Keyboard, Settings};
use std::process::Command;
use thiserror::Error;

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

/// Types text into the focused window.
///
/// Backend choice depends on the session type:
/// - X11: `enigo` (XTest) types directly.
/// - Wayland: `wtype` (wlroots virtual-keyboard protocol). `enigo` is deliberately not
///   used here because its X11 backend only reaches XWayland clients and would report
///   success while native Wayland windows receive nothing.
/// - Fallback: put the text on the clipboard (and paste with Shift+Insert when a
///   keyboard backend exists). On compositors without synthetic input (e.g. GNOME)
///   the text is left on the clipboard and an error tells the user to paste manually.
pub struct Injector {
    wayland: bool,
    enigo: Option<Enigo>,
    /// Kept alive so the clipboard contents keep being served after `set_text`.
    clipboard: Option<arboard::Clipboard>,
}

impl Injector {
    pub fn new() -> anyhow::Result<Self> {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();

        let enigo = if wayland {
            None
        } else {
            match Enigo::new(&Settings::default()) {
                Ok(enigo) => Some(enigo),
                Err(e) => {
                    tracing::warn!("enigo unavailable ({:?}); using wtype/clipboard fallbacks", e);
                    None
                }
            }
        };

        if wayland {
            tracing::info!("Wayland session: injecting via wtype, falling back to clipboard");
        }

        Ok(Self {
            wayland,
            enigo,
            clipboard: None,
        })
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

        if let Some(enigo) = self.enigo.as_mut() {
            match enigo.text(&to_type) {
                Ok(()) => return Ok(()),
                Err(e) => tracing::debug!("enigo failed to type ({:?}), trying fallbacks", e),
            }
        }

        if Self::type_with_wtype(&to_type) {
            return Ok(());
        }

        self.paste_via_clipboard(&to_type)
    }

    fn type_with_wtype(text: &str) -> bool {
        match Command::new("wtype").arg("--").arg(text).output() {
            Ok(out) if out.status.success() => true,
            Ok(out) => {
                tracing::debug!("wtype failed: {}", String::from_utf8_lossy(&out.stderr).trim());
                false
            }
            Err(e) => {
                tracing::debug!("wtype not runnable ({}); is it installed?", e);
                false
            }
        }
    }

    fn paste_via_clipboard(&mut self, text: &str) -> Result<(), InjectError> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let clipboard = self
            .clipboard
            .as_mut()
            .ok_or_else(|| InjectError::SystemError("All injection methods failed and the clipboard is unavailable.".into()))?;

        clipboard
            .set_text(text)
            .map_err(|e| InjectError::SystemError(format!("Clipboard copy failed: {}", e)))?;

        let Some(enigo) = self.enigo.as_mut() else {
            let hint = if self.wayland {
                "this Wayland compositor does not allow synthetic typing (GNOME has no virtual-keyboard protocol; install wtype on wlroots compositors)"
            } else {
                "no keyboard backend is available"
            };
            return Err(InjectError::SystemError(format!(
                "Could not type text: {}. The transcript was copied to the clipboard; press Ctrl+V to paste it.",
                hint
            )));
        };

        // Shift+Insert pastes in most toolkits and terminals, unlike Ctrl+V.
        let _ = enigo.key(enigo::Key::Shift, enigo::Direction::Press);
        let paste = enigo.key(enigo::Key::Insert, enigo::Direction::Click);
        let _ = enigo.key(enigo::Key::Shift, enigo::Direction::Release);
        paste.map_err(|e| InjectError::SystemError(format!("All injection methods failed. Last error: {:?}", e)))
    }
}
