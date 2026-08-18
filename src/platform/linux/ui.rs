use crate::platform::OverlayState;
use crate::session::SessionController;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::sync::{Arc, Mutex};

pub struct OverlayUI {
    state_tx: Sender<OverlayState>,
}

impl OverlayUI {
    pub fn new() -> anyhow::Result<Self> {
        let (tx, rx) = channel::<OverlayState>();
        
        thread::spawn(move || {
            if let Err(e) = run_ui_thread(rx) {
                tracing::error!("GTK Overlay UI thread exited with error: {:?}", e);
            }
        });
        
        Ok(Self { state_tx: tx })
    }

    pub fn set_state(&self, state: OverlayState) {
        let _ = self.state_tx.send(state);
    }

    pub fn update_state(&self, _session: &SessionController) {}
}

fn run_ui_thread(rx: Receiver<OverlayState>) -> anyhow::Result<()> {
    use gtk4::prelude::*;
    use gtk4::{Application, ApplicationWindow, DrawingArea};
    use gtk4_layer_shell::{Edge, Layer, LayerShell};
    
    let app = Application::builder()
        .application_id("com.dpaiasr.overlay")
        .build();

    let current_state = Arc::new(Mutex::new(OverlayState::Stopped));
    let rx = Arc::new(Mutex::new(rx));

    app.connect_activate(move |app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("DPAIASR Overlay")
            .default_width(80)
            .default_height(80)
            .decorated(false)
            .build();

        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Bottom, true);
        window.set_anchor(Edge::Right, true);
        window.set_margin(Edge::Bottom, 100);
        window.set_margin(Edge::Right, 40);

        let drawing_area = DrawingArea::new();
        let state_clone = Arc::clone(&current_state);
        
        drawing_area.set_draw_func(move |_, cr, width, height| {
            let state = *state_clone.lock().unwrap();
            
            let (r, g, b) = match state {
                OverlayState::Stopped => (80.0, 80.0, 80.0),
                OverlayState::Listening => (30.0, 220.0, 80.0),
                OverlayState::Speaking => (30.0, 180.0, 255.0),
                OverlayState::Transcribing => (230.0, 180.0, 30.0),
                OverlayState::Injecting => (255.0, 230.0, 0.0),
                OverlayState::WaitingForTarget => (255.0, 120.0, 0.0),
                OverlayState::Orphaned => (180.0, 0.0, 255.0),
                OverlayState::Stopping => (40.0, 40.0, 40.0),
                OverlayState::Error => (255.0, 30.0, 30.0),
                OverlayState::Muted => (120.0, 20.0, 20.0),
            };

            let cx = width as f64 / 2.0;
            let cy = height as f64 / 2.0;
            let radius = cx.min(cy) - 10.0;

            cr.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
            cr.set_source_rgba(r / 255.0, g / 255.0, b / 255.0, 1.0);
            let _ = cr.fill();
        });

        window.set_child(Some(&drawing_area));
        window.present();

        let state_for_timer = Arc::clone(&current_state);
        let rx_for_timer = Arc::clone(&rx);
        
        // Use glib::timeout_add_local to periodically check the channel and redraw if needed
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            if let Ok(rx) = rx_for_timer.lock() {
                if let Ok(new_state) = rx.try_recv() {
                    let mut current = state_for_timer.lock().unwrap();
                    if *current != new_state {
                        *current = new_state;
                        drawing_area.queue_draw();
                    }
                }
            }
            gtk4::glib::ControlFlow::Continue
        });
    });

    app.run();
    Ok(())
}
