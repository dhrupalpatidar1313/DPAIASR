//! Keyboard injection through the XDG RemoteDesktop portal.
//!
//! This is the sanctioned way to type on GNOME/KDE Wayland: no root, no daemon. The
//! compositor shows a one-time "allow keyboard control" dialog; the returned restore
//! token is cached on disk so later launches are silent. Text is sent as keysyms, so it
//! does not depend on the keyboard layout and handles non-ASCII characters.

use anyhow::{Context, Result, anyhow, bail};
use ashpd::desktop::PersistMode;
use ashpd::desktop::Session;
use ashpd::desktop::remote_desktop::{
    DeviceType, KeyState, NotifyKeyboardKeysymOptions, RemoteDesktop,
    SelectDevicesOptions, StartOptions,
};
use ashpd::enumflags2::BitFlags;
use std::path::PathBuf;
use std::time::Duration;

/// How long to wait for the user to answer the permission dialog.
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(120);

pub struct PortalKeyboard {
    // Multi-threaded (one worker) so zbus' background tasks keep running between calls.
    rt: tokio::runtime::Runtime,
    proxy: RemoteDesktop,
    session: Session<RemoteDesktop>,
}

impl PortalKeyboard {
    /// Opens a keyboard session, prompting the user if no saved permission exists.
    pub fn connect() -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .context("create portal runtime")?;

        let saved_token = read_token();
        let (proxy, session, new_token) = rt.block_on(async {
            tokio::time::timeout(PERMISSION_TIMEOUT, open_session(saved_token.as_deref()))
                .await
                .map_err(|_| anyhow!("timed out waiting for the keyboard permission dialog"))?
        })?;

        if let Some(token) = new_token {
            write_token(&token);
        }
        Ok(Self { rt, proxy, session })
    }

    pub fn type_text(&self, text: &str) -> Result<()> {
        self.rt.block_on(async {
            for ch in text.chars() {
                let keysym = keysym_for(ch);
                for state in [KeyState::Pressed, KeyState::Released] {
                    self.proxy
                        .notify_keyboard_keysym(&self.session, keysym, state, NotifyKeyboardKeysymOptions::default())
                        .await
                        .with_context(|| format!("portal rejected key {:?}", ch))?;
                }
            }
            Ok(())
        })
    }
}

async fn open_session(saved_token: Option<&str>) -> Result<(RemoteDesktop, Session<RemoteDesktop>, Option<String>)> {
    let proxy = RemoteDesktop::new().await.context("RemoteDesktop portal unavailable")?;
    let session = proxy
        .create_session(Default::default())
        .await
        .context("create portal session")?;

    proxy
        .select_devices(
            &session,
            SelectDevicesOptions::default()
                .set_devices(BitFlags::from(DeviceType::Keyboard))
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(saved_token),
        )
        .await
        .context("select devices")?
        .response()
        .context("select devices response")?;

    let started = proxy
        .start(&session, None, StartOptions::default())
        .await
        .context("start portal session")?
        .response()
        .context("keyboard permission was denied or cancelled")?;

    if !started.devices().contains(DeviceType::Keyboard) {
        bail!("portal session started without keyboard access");
    }
    let token = started.restore_token().map(str::to_owned);
    Ok((proxy, session, token))
}

/// X11 keysym for a character (Latin-1 maps directly, everything else uses the Unicode range).
fn keysym_for(ch: char) -> i32 {
    match ch {
        '\n' => 0xff0d, // Return
        '\t' => 0xff09, // Tab
        c if (0x20..=0x7e).contains(&(c as u32)) || (0xa0..=0xff).contains(&(c as u32)) => c as i32,
        c => 0x0100_0000 + c as i32,
    }
}

fn token_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("dpaiasr").join("portal-restore-token"))
}

fn read_token() -> Option<String> {
    let token = std::fs::read_to_string(token_path()?).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

fn write_token(token: &str) {
    let Some(path) = token_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, token) {
        tracing::warn!("could not save portal restore token to {:?}: {}", path, e);
    }
}

#[cfg(test)]
mod tests {
    use super::keysym_for;

    #[test]
    fn ascii_and_latin1_map_directly() {
        assert_eq!(keysym_for('a'), 0x61);
        assert_eq!(keysym_for(' '), 0x20);
        assert_eq!(keysym_for('é'), 0xe9);
    }

    #[test]
    fn other_unicode_uses_unicode_keysym_range() {
        assert_eq!(keysym_for('€'), 0x0100_20ac);
        assert_eq!(keysym_for('\n'), 0xff0d);
    }
}
