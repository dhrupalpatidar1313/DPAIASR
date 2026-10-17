use std::time::Instant;
use std::process::Command;

#[derive(Debug, Clone, Copy, Eq)]
pub struct TargetSnapshot {
    pub window_id: isize,
    pub process_id: u32,
    pub captured_at: Instant,
}

impl PartialEq for TargetSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.window_id == other.window_id &&
        self.process_id == other.process_id
    }
}

impl TargetSnapshot {
    pub fn current() -> Option<Self> {
        let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
        
        if is_wayland {
            // On Wayland, we cannot determine the active window ID or PID 
            // due to strict security isolation.
            // We return a dummy snapshot because `wtype` automatically
            // types into the compositor's currently focused window anyway.
            return Some(Self {
                window_id: 0,
                process_id: 0,
                captured_at: Instant::now(),
            });
        }
        
        // On X11, try to get active window via xdotool
        if let Ok(output) = Command::new("xdotool").arg("getactivewindow").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Ok(window_id) = stdout.trim().parse::<isize>() {
                    
                    let mut process_id = 0;
                    if let Ok(pid_output) = Command::new("xdotool").args(&["getwindowpid", &window_id.to_string()]).output() {
                        if pid_output.status.success() {
                            let pid_stdout = String::from_utf8_lossy(&pid_output.stdout);
                            if let Ok(pid) = pid_stdout.trim().parse::<u32>() {
                                process_id = pid;
                            }
                        }
                    }

                    return Some(Self {
                        window_id,
                        process_id,
                        captured_at: Instant::now(),
                    });
                }
            }
        }

        // Fallback for X11 if xdotool is missing, or other errors
        Some(Self {
            window_id: 0,
            process_id: 0,
            captured_at: Instant::now(),
        })
    }

    pub fn is_alive(&self) -> bool {
        if self.window_id == 0 {
            // Wayland dummy snapshot or xdotool missing
            return true;
        }

        // On X11, verify window still exists
        if let Ok(output) = Command::new("xdotool").args(&["search", "--window", &self.window_id.to_string()]).output() {
            return output.status.success();
        }
        
        true
    }
}
