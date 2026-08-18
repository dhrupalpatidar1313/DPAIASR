use anyhow::Result;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::Com::*;

use crate::platform::OverlayState;

pub struct OverlayUI {
    state_tx: Sender<OverlayState>,
}

impl OverlayUI {
    pub fn new() -> Result<Self> {
        let (tx, rx) = channel::<OverlayState>();
        
        thread::spawn(move || {
            if let Err(e) = run_ui_thread(rx) {
                tracing::error!("Overlay UI thread exited with error: {:?}", e);
            }
        });
        
        Ok(Self { state_tx: tx })
    }

    pub fn set_state(&self, state: OverlayState) {
        let _ = self.state_tx.send(state);
    }
}

fn run_ui_thread(rx: Receiver<OverlayState>) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        
        let instance = GetModuleHandleW(None)?;
        
        let class_name = w!("DPAIASR_OverlayClass");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);
        
        // WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW
        let ex_style = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
        let style = WS_POPUP | WS_VISIBLE;
        
        // 100x100 window in the bottom right corner
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let size = 80;
        let x = screen_w - size - 40;
        let y = screen_h - size - 100;

        let hwnd = CreateWindowExW(
            ex_style,
            class_name,
            w!("DPAIASR Overlay"),
            style,
            x, y, size, size,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        // Use LWA_COLORKEY: black pixels become transparent, so the background
        // disappears and only the colored circle is visible.
        SetLayeredWindowAttributes(hwnd, COLORREF(0x00000000), 0, LWA_COLORKEY)?;
        
        // Setup Direct2D
        let d2d_factory: ID2D1Factory = D2D1CreateFactory(
            D2D1_FACTORY_TYPE_SINGLE_THREADED,
            None,
        )?;
        
        let mut rc: RECT = Default::default();
        GetClientRect(hwnd, &mut rc)?;
        
        let mut rt_props = D2D1_RENDER_TARGET_PROPERTIES::default();
        rt_props.pixelFormat.format = windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
        rt_props.pixelFormat.alphaMode = D2D1_ALPHA_MODE_PREMULTIPLIED;
        
        let mut hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES::default();
        hwnd_props.hwnd = hwnd;
        hwnd_props.pixelSize = D2D_SIZE_U { width: rc.right as u32, height: rc.bottom as u32 };
        
        let target = d2d_factory.CreateHwndRenderTarget(&rt_props as *const _, &hwnd_props as *const _)?;
        
        let mut current_state = OverlayState::Stopped;
        
        // Initial draw
        draw_overlay(&target, rc, current_state)?;

        let mut msg = MSG::default();
        loop {
            // Check message queue (non-blocking)
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                if msg.message == WM_QUIT {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            
            // Check state updates
            if let Ok(new_state) = rx.try_recv() {
                if new_state != current_state {
                    current_state = new_state;
                    draw_overlay(&target, rc, current_state)?;
                }
            }
            
            thread::sleep(std::time::Duration::from_millis(16)); // ~60fps poll
        }
    }
    
    Ok(())
}

unsafe fn draw_overlay(
    target: &ID2D1HwndRenderTarget,
    rc: RECT,
    state: OverlayState,
) -> Result<()> {
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        
        let (r, g, b) = match state {
            OverlayState::Stopped => (80u8, 80u8, 80u8),      // gray
            OverlayState::Listening => (30u8, 220u8, 80u8),   // green
            OverlayState::Speaking => (30u8, 180u8, 255u8),   // bright blue
            OverlayState::Transcribing => (230u8, 180u8, 30u8), // amber
            OverlayState::Injecting => (255u8, 230u8, 0u8),   // bright yellow
            OverlayState::WaitingForTarget => (255u8, 120u8, 0u8),     // orange
            OverlayState::Orphaned => (180u8, 0u8, 255u8),    // purple
            OverlayState::Stopping => (40u8, 40u8, 40u8),     // dark gray
            OverlayState::Error => (255u8, 30u8, 30u8),       // red
            OverlayState::Muted => (120u8, 20u8, 20u8),       // dim red
        };
        let (rf, gf, bf) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
        
        // The background is solid black which becomes transparent via LWA_COLORKEY
        target.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }));
        
        let center_x = (rc.right - rc.left) as f32 / 2.0;
        let center_y = (rc.bottom - rc.top) as f32 / 2.0;
        let radius = center_x.min(center_y) - 20.0;
        
        // Draw outer glow (layered circles)
        let glow_steps = 12;
        for i in (1..=glow_steps).rev() {
            let step = i as f32;
            let alpha = 0.6 * (1.0 - (step / glow_steps as f32));
            
            if let Ok(glow_brush) = target.CreateSolidColorBrush(&D2D1_COLOR_F { r: rf, g: gf, b: bf, a: alpha }, None) {
                let glow_ellipse = D2D1_ELLIPSE {
                    point: std::mem::transmute([center_x, center_y]),
                    radiusX: radius + (step * 1.5),
                    radiusY: radius + (step * 1.5),
                };
                target.FillEllipse(&glow_ellipse, &glow_brush);
            }
        }

        // Core circle
        if let Ok(core_brush) = target.CreateSolidColorBrush(&D2D1_COLOR_F { r: rf, g: gf, b: bf, a: 1.0 }, None) {
            let ellipse = D2D1_ELLIPSE {
                point: std::mem::transmute([center_x, center_y]),
                radiusX: radius,
                radiusY: radius,
            };
            target.FillEllipse(&ellipse, &core_brush);
            
            // Inner bright core
            if let Ok(bright_brush) = target.CreateSolidColorBrush(&D2D1_COLOR_F {
                r: (rf + 0.25).min(1.0), g: (gf + 0.25).min(1.0), b: (bf + 0.25).min(1.0), a: 1.0
            }, None) {
                let inner_ellipse = D2D1_ELLIPSE {
                    point: std::mem::transmute([center_x, center_y]),
                    radiusX: radius * 0.7,
                    radiusY: radius * 0.7,
                };
                target.FillEllipse(&inner_ellipse, &bright_brush);
            }
        }

        target.EndDraw(None, None)?;
    }
    
    Ok(())
}

unsafe extern "system" fn wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
