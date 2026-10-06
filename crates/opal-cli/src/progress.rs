//! Rendering an install's progress, and only ever on a terminal.
//!
//! Two renderers behind one trait. On a TTY, lines that redraw in place.
//! Everywhere else — a pipe, a CI log, the crash-safety
//! suite — plain newline-terminated lines, because `fault.rs` announces itself
//! on stderr and a test reads those announcements a line at a time. A bar
//! redrawing over that would append a marker mid-line and the scan would miss
//! it.
//!
//! On a terminal, resolving and linking are each one spinner line. Fetching is
//! a spinner line counting packages, with a bar under it for every tarball in
//! flight: its name, a thirty-cell line, and bytes against the total. The
//! look is uv's. A bar exists for as long as its download does, so sixteen at
//! once is the most there can be, and small packages come and go faster than
//! they can be read; what holds still long enough to see is whatever is slow,
//! which is what a progress display is for.
//!
//! Colour is decoration only. Without it the bar's unfilled part is blank
//! instead of dim, so the fill still shows.
//!
//! Every line stops one column short of the terminal's width. A line that
//! fills the last column wraps early in some terminals, and indicatif's next
//! redraw then clears the row below it and leaves the old line on screen.
//!
//! Everything here writes to stderr. The summary `install_command` prints at
//! the end is the command's output and stays on stdout, so redirecting one does
//! not swallow the other.

use std::fmt;
use std::io::{self, IsTerminal};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use console::Term;
use indicatif::{
    MultiProgress, ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle, TermLike,
};
use opal_pm::progress::{Progress, Stage};
use opal_pm::resolve::PackageId;

use crate::style::Paint;

pub fn reporter() -> Box<dyn Progress> {
    if std::io::stderr().is_terminal() {
        // indicatif colours a template through `console`, which decides from
        // stdout. Everything drawn here goes to stderr, so `opal install >
        // result.txt` would otherwise lose the bars' colour with nothing on
        // the terminal having changed.
        console::set_colors_enabled(Paint::for_stderr().is_on());
        Box::new(Bar::new(ColourSupport::from_env()))
    } else {
        Box::new(Lines::default())
    }
}

/// The non-terminal renderer: one line for what was resolved, and nothing per
/// package.
///
/// Per-package output is what a bar is for. A CI log does not want 440 lines of
/// it, and a fault-injection test wants as little interleaving as it can get.
/// Fetching and linking get no line: the summary counts the packages and
/// gives each phase its time.
#[derive(Default)]
struct Lines {
    /// The latest counts resolution reported. A line can't be redrawn, so they
    /// are held until the resolve is over and printed once, as its result.
    resolved: Mutex<Option<(usize, usize)>>,
}

impl Progress for Lines {
    fn stage(&self, stage: Stage) {
        // Fetching is the first stage after a resolve, and so the first
        // moment its count is final.
        if !matches!(stage, Stage::Fetching { .. }) {
            return;
        }
        let resolved = self
            .resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some((settled, known)) = resolved {
            eprintln!("Resolving [{settled}/{known}]");
        }
    }

    fn resolving(&self, settled: usize, known: usize) {
        *self.resolved.lock().unwrap_or_else(PoisonError::into_inner) = Some((settled, known));
    }
}

/// What is on screen for the stage the pipeline is in.
struct Bar {
    colour: ColourSupport,
    current: Mutex<Option<Drawn>>,
}

enum Drawn {
    Spinner(ProgressBar),
    Downloads(Downloads),
}

/// Redraw interval. Also the spinner's frame length, so it turns at an even
/// pace however irregularly packages arrive.
const TICK: Duration = Duration::from_millis(80);
/// A three-dot arc turning one step per tick. The single dot indicatif uses by
/// default hops between positions and reads as random; the arc reads as
/// rotation.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// The elapsed time ticks on its own, so a line whose message has stopped
/// changing (one slow packument, a large link) still shows it is working.
const SPINNER_TEMPLATE: &str = "{opal_spinner} {prefix:.bold}{msg:.dim}  {opal_elapsed}";

/// A download whose size the server did not state has no fraction to draw.
const UNSIZED_TEMPLATE: &str = "{wide_msg:.dim} ....";

/// Names are padded to the longest one seen, and never to less than this, so
/// the bars start in one column and don't shift for every short name.
const NAME_WIDTH_MIN: usize = 20;
const BAR_CELLS: usize = 30;
/// `181.85 KiB/1023.99 KiB`: a side is eleven columns at its widest.
const BYTES_CELLS: usize = 11 + 1 + 11;
/// What is shown when the terminal's width can't be read, as in a test.
const COLUMNS_ASSUMED: usize = 100;

/// The widths of one download line's name and bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    name: usize,
    bar: usize,
}

impl Layout {
    /// A line has to fit the terminal: one that wraps takes two rows, and the
    /// rows under it are then redrawn in the wrong place. The bar gives way
    /// first, down to ten cells, and then the name is cut short.
    fn fitting(columns: usize, longest_name: usize) -> Self {
        let spaces = 2;
        let bar = columns
            .saturating_sub(NAME_WIDTH_MIN + spaces + BYTES_CELLS)
            .clamp(10, BAR_CELLS);
        let room = columns.saturating_sub(bar + spaces + BYTES_CELLS).max(1);
        Self {
            name: longest_name.max(NAME_WIDTH_MIN).min(room),
            bar,
        }
    }
}

/// One download's line: its name, a line that fills, and bytes of the total.
fn transfer_template(layout: Layout) -> String {
    let Layout { name, bar } = layout;
    format!(
        "{{msg:{name}!.dim}} {{bar:{bar}.green/black.dim}} \
         {{binary_bytes:>7}}/{{binary_total_bytes:7}}"
    )
}

/// The filled and unfilled parts are the same dash, told apart by colour.
/// Without colour the unfilled part is left blank.
fn transfer_chars(colour: ColourSupport) -> &'static str {
    match colour {
        ColourSupport::Off => "- ",
        ColourSupport::Palette256 | ColourSupport::TrueColour => "--",
    }
}

fn transfer_style(layout: Layout, sized: bool, colour: ColourSupport) -> ProgressStyle {
    if !sized {
        return ProgressStyle::with_template(UNSIZED_TEMPLATE)
            .unwrap_or_else(|_| ProgressStyle::default_bar());
    }
    ProgressStyle::with_template(&transfer_template(layout))
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars(transfer_chars(colour))
}

/// Where a download of `total` bytes goes among those already shown: smaller
/// above larger, so the long ones settle at the bottom and stay put, and one
/// of unstated size last.
fn slot_for(shown: &[Option<u64>], total: Option<u64>) -> usize {
    let size = |total: Option<u64>| total.unwrap_or(u64::MAX);
    shown.partition_point(|other| size(*other) <= size(total))
}

/// The fetching stage: a line counting packages, and a bar per download.
struct Downloads {
    multi: MultiProgress,
    root: ProgressBar,
    packages: usize,
    done: usize,
    /// In the order they are drawn, which is ascending by size.
    transfers: Vec<Transfer>,
    longest_name: usize,
    /// Absent when nothing is drawn, as in a test.
    term: Option<Term>,
    colour: ColourSupport,
}

struct Transfer {
    id: PackageId,
    total: Option<u64>,
    bar: ProgressBar,
}

impl Downloads {
    fn new(
        packages: usize,
        colour: ColourSupport,
        counting: ProgressStyle,
        term: Option<Term>,
    ) -> Self {
        let multi = MultiProgress::with_draw_target(match &term {
            Some(_) => short_of_the_edge(),
            None => ProgressDrawTarget::hidden(),
        });
        let root = multi.add(
            ProgressBar::with_draw_target(None, ProgressDrawTarget::hidden())
                .with_style(counting)
                .with_prefix("downloading"),
        );
        let downloads = Self {
            multi,
            root,
            packages,
            done: 0,
            transfers: Vec::new(),
            longest_name: 0,
            term,
            colour,
        };
        downloads.root.set_message(downloads.count());
        downloads.root.enable_steady_tick(TICK);
        downloads
    }

    fn count(&self) -> String {
        format!("  [{}/{}]", self.done, self.packages)
    }

    /// Measured each time it is asked, so a resized window is fitted from the
    /// next download on.
    fn layout(&self) -> Layout {
        let columns = self.term.as_ref().map_or(COLUMNS_ASSUMED, usable_columns);
        Layout::fitting(columns, self.longest_name)
    }

    fn start(&mut self, id: &PackageId, total: Option<u64>) {
        // A retried request starts over, on the line it already has.
        if let Some(retried) = self.transfers.iter().find(|transfer| &transfer.id == id) {
            retried.bar.set_position(0);
            if let Some(total) = total {
                retried.bar.set_length(total);
            }
            return;
        }

        let name = id.name.clone();
        let before = self.layout();
        self.longest_name = self.longest_name.max(console::measure_text_width(&name));
        let layout = self.layout();
        let bar = ProgressBar::with_draw_target(total, ProgressDrawTarget::hidden())
            .with_style(transfer_style(layout, total.is_some(), self.colour))
            .with_message(name);
        let shown: Vec<Option<u64>> = self.transfers.iter().map(|other| other.total).collect();
        let index = slot_for(&shown, total);
        // The counting line is the first, so every bar sits one below its
        // place in the list.
        let bar = self.multi.insert(index + 1, bar);
        self.transfers.insert(
            index,
            Transfer {
                id: id.clone(),
                total,
                bar,
            },
        );
        // A longer name moves the column every bar starts in.
        if layout != before {
            for transfer in &self.transfers {
                transfer.bar.set_style(transfer_style(
                    layout,
                    transfer.total.is_some(),
                    self.colour,
                ));
            }
        }
    }

    fn advance(&self, id: &PackageId, bytes: u64) {
        if let Some(transfer) = self.transfers.iter().find(|transfer| &transfer.id == id) {
            transfer.bar.inc(bytes);
        }
    }

    /// A package answered by the store never had a bar, and still counts.
    fn complete(&mut self, id: &PackageId) {
        if let Some(index) = self
            .transfers
            .iter()
            .position(|transfer| &transfer.id == id)
        {
            let transfer = self.transfers.remove(index);
            transfer.bar.finish_and_clear();
            self.multi.remove(&transfer.bar);
        }
        self.done += 1;
        self.root.set_message(self.count());
    }

    fn clear(self) {
        for transfer in &self.transfers {
            transfer.bar.finish_and_clear();
        }
        self.root.finish_and_clear();
    }
}

impl Bar {
    fn new(colour: ColourSupport) -> Self {
        Self {
            colour,
            current: Mutex::new(None),
        }
    }

    fn current(&self) -> MutexGuard<'_, Option<Drawn>> {
        // What is drawn is only ever swapped whole, so a poisoned lock still
        // holds something usable, and progress output is no reason to end an
        // install.
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn spinner_style(&self) -> ProgressStyle {
        let colour = self.colour;
        ProgressStyle::with_template(SPINNER_TEMPLATE)
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .with_key(
                "opal_spinner",
                move |state: &ProgressState, out: &mut dyn fmt::Write| {
                    draw_spinner(out, state.elapsed().as_secs_f64(), colour);
                },
            )
            .with_key(
                "opal_elapsed",
                |state: &ProgressState, out: &mut dyn fmt::Write| {
                    let _ = write!(out, "{DIM}{}{RESET}", format_elapsed(state.elapsed()));
                },
            )
    }

    fn spinner(&self, label: &'static str, detail: String) -> ProgressBar {
        // The `with_` builders set a field without drawing; `set_prefix` and
        // `set_message` each draw, and the first of them would show a line
        // missing the other.
        let spinner = ProgressBar::with_draw_target(None, short_of_the_edge())
            .with_style(self.spinner_style())
            .with_prefix(label)
            .with_message(detail);
        spinner.enable_steady_tick(TICK);
        spinner
    }

    fn downloads(&self, packages: usize) -> Downloads {
        Downloads::new(
            packages,
            self.colour,
            self.spinner_style(),
            Some(Term::stderr()),
        )
    }
}

impl Progress for Bar {
    fn stage(&self, stage: Stage) {
        // A spinner draws the moment its steady tick starts, so the previous
        // lines have to be gone first. Cleared afterwards, the new line has
        // already wrapped below the old full-width one, the clear lands on the
        // new line, and the old one stays on screen.
        self.finished();
        // Neither resolving nor linking knows its own size as it goes —
        // resolution discovers the tree, and the reconciler's work depends on
        // what it finds on disk — so a spinner is the honest shape for both.
        // Resolving adds a count beside it, whose total grows as it goes.
        let next = match stage {
            Stage::Resolving => Drawn::Spinner(self.spinner("resolving", String::new())),
            Stage::Fetching { packages } => Drawn::Downloads(self.downloads(packages)),
            Stage::Linking { packages } => {
                Drawn::Spinner(self.spinner("linking", format!("  {packages} packages")))
            }
        };
        *self.current() = Some(next);
    }

    fn resolving(&self, settled: usize, known: usize) {
        if let Some(Drawn::Spinner(spinner)) = self.current().as_ref() {
            spinner.set_message(format!("  [{settled}/{known}]"));
        }
    }

    fn download_started(&self, id: &PackageId, total: Option<u64>) {
        if let Some(Drawn::Downloads(downloads)) = self.current().as_mut() {
            downloads.start(id, total);
        }
    }

    fn downloaded(&self, id: &PackageId, bytes: u64) {
        if let Some(Drawn::Downloads(downloads)) = self.current().as_ref() {
            downloads.advance(id, bytes);
        }
    }

    fn fetched(&self, id: &PackageId, _from_store: bool) {
        if let Some(Drawn::Downloads(downloads)) = self.current().as_mut() {
            downloads.complete(id);
        }
    }

    fn finished(&self) {
        match self.current().take() {
            Some(Drawn::Spinner(spinner)) => spinner.finish_and_clear(),
            Some(Drawn::Downloads(downloads)) => downloads.clear(),
            None => {}
        }
    }
}

fn short_of_the_edge() -> ProgressDrawTarget {
    ProgressDrawTarget::term_like_with_hz(Box::new(ShortOfTheEdge::new(Term::stderr())), 20)
}

/// How many columns a line may use: one fewer than the terminal has.
fn usable_columns(term: &Term) -> usize {
    usize::from(term.size().1.saturating_sub(1))
}

/// Stderr's terminal, reported one column narrower than it is, so that
/// nothing indicatif draws ever fills the last column.
///
/// indicatif does not end a line itself. It pads each one to the width it was
/// told and counts on the terminal wrapping to the next row, which a line one
/// column short never does: a second line would start on the same row as the
/// first. So the break is made here, when text arrives and the row is full.
#[derive(Debug)]
struct ShortOfTheEdge {
    term: Term,
    /// Columns written on the row the cursor is on.
    column: AtomicUsize,
}

impl ShortOfTheEdge {
    fn new(term: Term) -> Self {
        Self {
            term,
            column: AtomicUsize::new(0),
        }
    }
}

impl TermLike for ShortOfTheEdge {
    fn width(&self) -> u16 {
        self.term.size().1.saturating_sub(1)
    }

    fn height(&self) -> u16 {
        self.term.size().0
    }

    fn move_cursor_up(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_up(n)
    }

    fn move_cursor_down(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_down(n)
    }

    fn move_cursor_right(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_right(n)
    }

    fn move_cursor_left(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_left(n)
    }

    fn write_line(&self, s: &str) -> io::Result<()> {
        self.column.store(0, Ordering::Relaxed);
        self.term.write_line(s)
    }

    fn write_str(&self, s: &str) -> io::Result<()> {
        if s == "\r" {
            self.column.store(0, Ordering::Relaxed);
            return self.term.write_str(s);
        }
        let width = console::measure_text_width(s);
        if width == 0 {
            return self.term.write_str(s);
        }
        if self.column.load(Ordering::Relaxed) >= usize::from(self.width()) {
            self.term.write_str("\r\n")?;
            self.column.store(0, Ordering::Relaxed);
        }
        self.column.fetch_add(width, Ordering::Relaxed);
        self.term.write_str(s)
    }

    fn clear_line(&self) -> io::Result<()> {
        // Clearing also returns the cursor to the first column.
        self.column.store(0, Ordering::Relaxed);
        self.term.clear_line()
    }

    fn flush(&self) -> io::Result<()> {
        self.term.flush()
    }
}

type Rgb = (u8, u8, u8);

/// Sky, the first of the hues an opal throws as it turns. The spinner holds
/// this one colour, so the only motion on its line is the rotation.
const SPINNER_COLOUR: Rgb = (120, 196, 255);

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// What the terminal can show. Detected once, from the environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColourSupport {
    Off,
    Palette256,
    TrueColour,
}

impl ColourSupport {
    fn from_env() -> Self {
        Self::detect(
            std::env::var("NO_COLOR").ok().as_deref(),
            std::env::var("COLORTERM").ok().as_deref(),
            std::env::var_os("WT_SESSION").is_some(),
            std::env::var("TERM").ok().as_deref(),
        )
    }

    /// `NO_COLOR` wins when set to anything non-empty (no-color.org). Windows
    /// Terminal renders 24-bit colour but doesn't set `COLORTERM`; it does set
    /// `WT_SESSION`, which it also forwards into WSL.
    fn detect(
        no_color: Option<&str>,
        colorterm: Option<&str>,
        windows_terminal: bool,
        term: Option<&str>,
    ) -> Self {
        if no_color.is_some_and(|value| !value.is_empty()) {
            return Self::Off;
        }
        if matches!(colorterm, Some("truecolor" | "24bit")) || windows_terminal {
            return Self::TrueColour;
        }
        if term.is_some_and(|term| term.contains("256color")) {
            return Self::Palette256;
        }
        Self::Off
    }

    fn paint(self, rgb: Rgb, out: &mut dyn fmt::Write) {
        let (r, g, b) = rgb;
        let _ = match self {
            Self::Off => Ok(()),
            Self::Palette256 => write!(out, "\x1b[38;5;{}m", to_256(rgb)),
            Self::TrueColour => write!(out, "\x1b[38;2;{r};{g};{b}m"),
        };
    }
}

/// The nearest colour in xterm's 6×6×6 cube.
fn to_256((r, g, b): Rgb) -> u8 {
    let level = |channel: u8| ((u16::from(channel) * 5 + 127) / 255) as u8;
    16 + 36 * level(r) + 6 * level(g) + level(b)
}

fn draw_spinner(out: &mut dyn fmt::Write, seconds: f64, colour: ColourSupport) {
    let frame = SPINNER[(seconds / TICK.as_secs_f64()) as usize % SPINNER.len()];
    colour.paint(SPINNER_COLOUR, out);
    let _ = write!(out, "{frame}{RESET}");
}

fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_color_turns_colour_off_whatever_the_terminal_supports() {
        let detected =
            ColourSupport::detect(Some("1"), Some("truecolor"), true, Some("xterm-256color"));
        assert_eq!(detected, ColourSupport::Off);
    }

    #[test]
    fn test_an_empty_no_color_is_ignored() {
        let detected = ColourSupport::detect(Some(""), Some("truecolor"), false, None);
        assert_eq!(detected, ColourSupport::TrueColour);
    }

    #[test]
    fn test_windows_terminal_gets_true_colour_without_colorterm() {
        let detected = ColourSupport::detect(None, None, true, Some("xterm-256color"));
        assert_eq!(detected, ColourSupport::TrueColour);
    }

    #[test]
    fn test_a_256_colour_terminal_gets_the_palette() {
        let detected = ColourSupport::detect(None, None, false, Some("xterm-256color"));
        assert_eq!(detected, ColourSupport::Palette256);
    }

    #[test]
    fn test_an_unknown_terminal_gets_no_colour() {
        assert_eq!(
            ColourSupport::detect(None, None, false, Some("dumb")),
            ColourSupport::Off
        );
        assert_eq!(
            ColourSupport::detect(None, None, false, None),
            ColourSupport::Off
        );
    }

    #[test]
    fn test_the_spinner_turns_one_step_per_tick() {
        let tick = TICK.as_secs_f64();
        for step in 0..SPINNER.len() * 2 {
            let mut out = String::new();
            // Mid-tick, so float rounding can't land on the previous frame.
            draw_spinner(&mut out, (step as f64 + 0.5) * tick, ColourSupport::Off);
            assert!(
                out.starts_with(SPINNER[step % SPINNER.len()]),
                "step {step}: {out:?}"
            );
        }
    }

    #[test]
    fn test_the_spinner_holds_one_colour() {
        let mut early = String::new();
        let mut late = String::new();
        draw_spinner(&mut early, 0.05, ColourSupport::TrueColour);
        draw_spinner(&mut late, 7.05, ColourSupport::TrueColour);
        let colour = |drawn: &str| drawn.split('m').next().unwrap_or_default().to_string();
        assert_eq!(colour(&early), colour(&late));
    }

    #[test]
    fn test_spinner_lines_show_their_elapsed_time() {
        assert!(SPINNER_TEMPLATE.ends_with("{opal_elapsed}"));
    }

    #[test]
    fn test_256_colour_maps_to_the_cube_corners() {
        assert_eq!(to_256((0, 0, 0)), 16);
        assert_eq!(to_256((255, 255, 255)), 231);
    }

    #[test]
    fn test_elapsed_reads_as_minutes_and_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(0)), "0s");
        assert_eq!(format_elapsed(Duration::from_secs(59)), "59s");
        assert_eq!(format_elapsed(Duration::from_secs(72)), "1m12s");
    }

    #[test]
    fn test_the_templates_parse() {
        assert!(ProgressStyle::with_template(SPINNER_TEMPLATE).is_ok());
        assert!(ProgressStyle::with_template(UNSIZED_TEMPLATE).is_ok());
        let layout = Layout::fitting(COLUMNS_ASSUMED, 0);
        assert!(ProgressStyle::with_template(&transfer_template(layout)).is_ok());
    }

    #[test]
    fn test_a_download_line_is_a_name_a_thirty_cell_bar_and_bytes() {
        assert_eq!(
            transfer_template(Layout { name: 24, bar: 30 }),
            "{msg:24!.dim} {bar:30.green/black.dim} {binary_bytes:>7}/{binary_total_bytes:7}"
        );
    }

    #[test]
    fn test_a_download_line_never_outgrows_the_terminal() {
        let width = |layout: Layout| layout.name + 1 + layout.bar + 1 + BYTES_CELLS;
        for columns in [40, 60, 79, 100, 200] {
            for longest in [3, 20, 37, 90] {
                let layout = Layout::fitting(columns, longest);
                assert!(width(layout) <= columns, "{columns} columns: {layout:?}");
            }
        }
    }

    #[test]
    fn test_the_bar_gives_way_before_the_name_is_cut() {
        assert_eq!(Layout::fitting(100, 37), Layout { name: 37, bar: 30 });
        assert_eq!(Layout::fitting(60, 12), Layout { name: 20, bar: 15 });
        assert_eq!(Layout::fitting(60, 37), Layout { name: 20, bar: 15 });
        assert_eq!(Layout::fitting(48, 37), Layout { name: 13, bar: 10 });
    }

    #[test]
    fn test_without_colour_the_unfilled_part_is_blank() {
        assert_eq!(transfer_chars(ColourSupport::Off), "- ");
        assert_eq!(transfer_chars(ColourSupport::TrueColour), "--");
    }

    #[test]
    fn test_downloads_are_listed_smallest_first() {
        let shown = [Some(10), Some(500), Some(9_000)];
        assert_eq!(slot_for(&shown, Some(1)), 0);
        assert_eq!(slot_for(&shown, Some(700)), 2);
        assert_eq!(slot_for(&shown, Some(50_000)), 3);
        // Equal sizes keep the order they arrived in.
        assert_eq!(slot_for(&shown, Some(500)), 2);
    }

    #[test]
    fn test_a_download_of_unstated_size_goes_last() {
        assert_eq!(slot_for(&[Some(10), None], Some(u64::MAX - 1)), 1);
        assert_eq!(slot_for(&[Some(10), Some(20)], None), 2);
    }

    #[test]
    fn test_a_finished_download_gives_up_its_line_and_is_counted() {
        let colour = ColourSupport::Off;
        let mut downloads = Downloads::new(3, colour, Bar::new(colour).spinner_style(), None);
        let id = |name: &str| {
            let version = opal_pm::semver::Version::parse("1.0.0").expect("a version");
            PackageId::new(name.to_string(), version)
        };

        downloads.start(&id("big"), Some(9_000));
        downloads.start(&id("a-package-with-a-rather-long-name"), Some(10));
        let names: Vec<&str> = downloads
            .transfers
            .iter()
            .map(|transfer| transfer.id.name.as_str())
            .collect();
        assert_eq!(names, ["a-package-with-a-rather-long-name", "big"]);
        assert_eq!(downloads.layout(), Layout { name: 33, bar: 30 });

        downloads.advance(&id("big"), 4_000);
        assert_eq!(downloads.transfers[1].bar.position(), 4_000);
        // A retry starts the same line over.
        downloads.start(&id("big"), Some(9_000));
        assert_eq!(downloads.transfers.len(), 2);
        assert_eq!(downloads.transfers[1].bar.position(), 0);

        downloads.complete(&id("big"));
        // Answered by the store: never had a line, still one of the three.
        downloads.complete(&id("stored"));
        assert_eq!(downloads.transfers.len(), 1);
        assert_eq!(downloads.count(), "  [2/3]");
    }
}
