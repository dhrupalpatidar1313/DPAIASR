use super::portal::PortalKeyboard;
use super::target::TargetSnapshot;
use enigo::{Enigo, Keyboard, Settings};
use std::io::Write;
use std::process::{Command, Stdio};
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
/// - Wayland: the XDG RemoteDesktop portal (GNOME, KDE; one-time permission dialog, no
///   root), then `wtype` (wlroots virtual-keyboard protocol). `enigo` is deliberately not
///   used here because its X11 backend only reaches XWayland clients and would report
///   success while native Wayland windows receive nothing.
/// - `ydotool` (kernel uinput, needs a running `ydotoold`): works on GNOME and any other
///   compositor, assuming a US keyboard layout.
/// - Fallback: put the text on the clipboard (and paste with Shift+Insert when a
///   keyboard backend exists). On compositors without synthetic input (e.g. GNOME)
///   the text is left on the clipboard and an error tells the user to paste manually.
pub struct Injector {
    wayland: bool,
    enigo: Option<Enigo>,
    portal: Option<PortalKeyboard>,
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

        let portal = if wayland {
            tracing::info!(
                "Wayland session: requesting keyboard access through the desktop portal (approve the dialog if one appears)"
            );
            match PortalKeyboard::connect() {
                Ok(p) => {
                    tracing::info!("desktop portal keyboard access granted");
                    Some(p)
                }
                Err(e) => {
                    tracing::warn!("desktop portal keyboard unavailable ({:#}); trying wtype/ydotool/clipboard", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            wayland,
            enigo,
            portal,
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

        if let Some(portal) = self.portal.as_ref() {
            match portal.type_text(&to_type) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!("portal typing failed ({:#}); falling back", e);
                    self.portal = None;
                }
            }
        }

        if Self::type_with_wtype(&to_type) {
            return Ok(());
        }

        if Self::type_with_ydotool(&to_type) {
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

    fn type_with_ydotool(text: &str) -> bool {
        // The text goes over stdin so arbitrary content (e.g. a leading '-') is never parsed as a flag.
        let child = Command::new("ydotool")
            .args(["type", "--key-delay", "2", "--file", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("ydotool not runnable ({}); is it installed?", e);
                return false;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            if stdin.write_all(text.as_bytes()).is_err() {
                let _ = child.kill();
                return false;
            }
        }
        match child.wait_with_output() {
            Ok(out) if out.status.success() => true,
            Ok(out) => {
                tracing::debug!(
                    "ydotool failed (is ydotoold running and /dev/uinput accessible?): {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
                false
            }
            Err(e) => {
                tracing::debug!("ydotool wait failed: {}", e);
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
                "this Wayland compositor does not allow synthetic typing (GNOME has no virtual-keyboard protocol; approve the desktop-portal keyboard permission, or install wtype on wlroots compositors)"
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
