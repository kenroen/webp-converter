//! Parallel Rayon batch conversion worker and progress event stream.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use rayon::prelude::*;

use crate::config::ConversionConfig;
use crate::converter::{ConvertOutcome, convert_file};
use crate::scanner::{DiscoveredFile, resolve_output_dir};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerEvent {
    FileStarted {
        index: usize,
    },
    FileCompleted {
        index: usize,
        outputs: Vec<PathBuf>,
        was_animated: bool,
        duration_ms: u64,
    },
    FileSkipped {
        index: usize,
        path: PathBuf,
        reason: String,
    },
    FileFailed {
        index: usize,
        error: String,
    },
    BatchFinished {
        converted: usize,
        skipped: usize,
        failed: usize,
        cancelled: bool,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchSummary {
    pub converted: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: bool,
}

pub fn run_batch_sync(
    items: &[DiscoveredFile],
    config: &ConversionConfig,
    cancel: &AtomicBool,
    on_event: impl Fn(WorkerEvent) + Send + Sync,
) -> BatchSummary {
    let converted = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    for item in items {
        let dir = resolve_output_dir(item, config.output_dir.as_deref());
        if !dir.as_os_str().is_empty() && !dir.is_dir() {
            let _ = std::fs::create_dir_all(&dir);
        }
    }

    items.par_iter().enumerate().for_each(|(index, item)| {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        on_event(WorkerEvent::FileStarted { index });
        let started = Instant::now();

        match convert_file(item, config, Some(cancel)) {
            Ok(ConvertOutcome::Converted {
                outputs,
                was_animated,
            }) => {
                converted.fetch_add(1, Ordering::Relaxed);
                let duration_ms = started.elapsed().as_millis() as u64;
                on_event(WorkerEvent::FileCompleted {
                    index,
                    outputs,
                    was_animated,
                    duration_ms,
                });
            }
            Ok(ConvertOutcome::Skipped { path, reason }) => {
                skipped.fetch_add(1, Ordering::Relaxed);
                on_event(WorkerEvent::FileSkipped {
                    index,
                    path,
                    reason,
                });
            }
            Err(err) => {
                if cancel.load(Ordering::Relaxed) && err == "Cancelled" {
                    return;
                }
                failed.fetch_add(1, Ordering::Relaxed);
                on_event(WorkerEvent::FileFailed { index, error: err });
            }
        }
    });

    let summary = BatchSummary {
        converted: converted.load(Ordering::Relaxed),
        skipped: skipped.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
        cancelled: cancel.load(Ordering::Relaxed),
    };

    on_event(WorkerEvent::BatchFinished {
        converted: summary.converted,
        skipped: summary.skipped,
        failed: summary.failed,
        cancelled: summary.cancelled,
    });

    summary
}

pub fn spawn_batch(
    items: Vec<DiscoveredFile>,
    config: ConversionConfig,
    on_wakeup: Option<Box<dyn Fn() + Send + Sync>>,
) -> (Receiver<WorkerEvent>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = Arc::clone(&cancel);

    std::thread::spawn(move || {
        run_batch_sync(&items, &config, &cancel_worker, |ev| {
            let _ = tx.send(ev);
            if let Some(wake) = &on_wakeup {
                wake();
            }
        });
    });

    (rx, cancel)
}
