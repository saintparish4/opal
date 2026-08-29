//! Rendering an install's progress, and only ever on a terminal.
//!
//! Two renderers behind one trait. On a TTY, a stage line and a bar that
//! advances in place. Everywhere else — a pipe, a CI log, the crash-safety
//! suite — plain newline-terminated lines, because `fault.rs` announces itself
//! on stderr and a test reads those announcements a line at a time. A bar
//! redrawing over that would append a marker mid-line and the scan would miss
//! it.
//!
//! Everything here writes to stderr. The summary `install_command` prints at
//! the end is the command's output and stays on stdout, so redirecting one does
//! not swallow the other.

use std::cell::RefCell;
use std::io::IsTerminal;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use opal_pm::progress::{Progress, Stage};
use opal_pm::resolve::PackageId;

pub fn reporter() -> Box<dyn Progress> {
    if std::io::stderr().is_terminal() {
        Box::new(Bar::default())
    } else {
        Box::new(Lines)
    }
}

fn describe(stage: Stage) -> String {
    match stage {
        Stage::Resolving => "Resolving dependencies".to_string(),
        Stage::Fetching { packages } => format!("Installing {packages} packages"),
        Stage::Linking { packages } => format!("Linking {packages} packages"),
    }
}

/// The non-terminal renderer: one line per stage, nothing per package.
///
/// Per-package output is what a bar is for. A CI log does not want 440 lines of
/// it, and a fault-injection test wants as little interleaving as it can get.
struct Lines;

impl Progress for Lines {
    fn stage(&self, stage: Stage) {
        eprintln!("{}", describe(stage));
    }
}

/// One bar at a time, replaced as the pipeline moves between stages.
#[derive(Default)]
struct Bar {
    current: RefCell<Option<ProgressBar>>,
}

impl Progress for Bar {
    fn stage(&self, stage: Stage) {
        let next = match stage {
            // Neither stage knows its own size as it goes — resolution
            // discovers the tree, and the reconciler's work depends on what it
            // finds on disk — so a spinner is the honest shape for both. The
            // default spinner template renders `{msg}`, so the label goes
            // there; the fetch bar keeps its message free for package names
            // and carries the label in `{prefix}`.
            Stage::Resolving | Stage::Linking { .. } => {
                let bar = ProgressBar::new_spinner();
                bar.enable_steady_tick(Duration::from_millis(120));
                bar.set_message(describe(stage));
                bar
            }
            Stage::Fetching { packages } => {
                let bar = ProgressBar::new(packages as u64);
                if let Ok(style) =
                    ProgressStyle::with_template("{prefix} [{bar:24}] {pos}/{len} {wide_msg}")
                {
                    bar.set_style(style.progress_chars("=> "));
                }
                bar.set_prefix(describe(stage));
                bar
            }
        };
        if let Some(previous) = self.current.borrow_mut().replace(next) {
            previous.finish_and_clear();
        }
    }

    fn fetched(&self, id: &PackageId, _from_store: bool) {
        if let Some(bar) = self.current.borrow().as_ref() {
            bar.set_message(id.name.clone());
            bar.inc(1);
        }
    }

    fn finished(&self) {
        if let Some(bar) = self.current.borrow_mut().take() {
            bar.finish_and_clear();
        }
    }
}
