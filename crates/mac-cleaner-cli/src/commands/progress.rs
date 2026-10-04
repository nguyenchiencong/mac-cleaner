use is_terminal::IsTerminal;
use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A terminal-only spinner used while scanner workers are running.
pub struct ScanProgress {
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ScanProgress {
    pub fn start(message: impl Into<String>) -> Self {
        if !std::io::stderr().is_terminal() {
            return Self {
                running: Arc::new(AtomicBool::new(false)),
                worker: None,
            };
        }

        let running = Arc::new(AtomicBool::new(true));
        let worker_running = Arc::clone(&running);
        let message = message.into();
        let worker = thread::spawn(move || {
            const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
            let mut frame = 0;
            while worker_running.load(Ordering::Relaxed) {
                let mut stderr = std::io::stderr().lock();
                let _ = write!(stderr, "\r{} {}", FRAMES[frame], message);
                let _ = stderr.flush();
                frame = (frame + 1) % FRAMES.len();
                thread::sleep(Duration::from_millis(100));
            }
        });

        Self {
            running,
            worker: Some(worker),
        }
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
            let mut stderr = std::io::stderr().lock();
            let _ = write!(stderr, "\r\x1b[2K");
            let _ = stderr.flush();
        }
    }
}

impl Drop for ScanProgress {
    fn drop(&mut self) {
        self.stop();
    }
}
