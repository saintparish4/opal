//! Rendering an install's progress, and only ever on a terminal.
//!
//! Two renderers behind one trait. On a TTY, a stage line and a bar that
//! advances in place. Everywhere else — a pipe, a CI log, the crash-safety
//! suite — plain newline-terminated lines, because `fault.rs` announces itself
//! on stderr and a test reads those announcements a line at a time. A bar
//! redrawing over that would append a marker mid-line and the scan would miss
//! it.
//!
//! The terminal look is "play of colour", after the gemstone: the download bar
//! is a thin line whose filled part runs through an opal's hues. The colours
//! are fixed to their position and nothing glows or sweeps, so the bar only
//! moves when a package lands; the elapsed time ticks on its own, so a single
//! large tarball still visibly makes progress. Colour is decoration only; every
//! glyph reads the same without it, so `NO_COLOR` and terminals without colour
//! lose nothing but the hue.
//!
//! Every line stops one column short of the terminal's width. A line that
//! fills the last column wraps early in some terminals, and indicatif's next
//! redraw then clears the row below it and leaves the old line on screen.
//!
//! Everything here writes to stderr. The summary `install_command` prints at
//! the end is the command's output and stays on stdout, so redirecting one does
//! not swallow the other.

use std::cell::RefCell;
use std::fmt;
use std::io::{self, IsTerminal};
use std::time::Duration;

use console::Term;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle, TermLike};
use opal_pm::progress::{Progress, Stage};
use opal_pm::resolve::PackageId;

pub fn reporter() -> Box<dyn Progress> {
    if std::io::stderr().is_terminal() {
        Box::new(Bar::new(ColourSupport::from_env()))
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
struct Bar {
    colour: ColourSupport,
    current: RefCell<Option<ProgressBar>>,
}

/// Redraw interval. Also the spinner's frame length, so it turns at an even
/// pace however irregularly packages arrive.
const TICK: Duration = Duration::from_millis(80);
/// A three-dot arc turning one step per tick. The single dot indicatif uses by
/// default hops between positions and reads as random; the arc reads as
/// rotation.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// The elapsed time is what shows a long resolve is still working: resolution
/// fetches registry metadata one package at a time, 25s on a Next.js app.
const SPINNER_TEMPLATE: &str = "{opal_spinner} {prefix:.bold}{msg:.dim}  {opal_elapsed}";
const DOWNLOAD_TEMPLATE: &str = "{prefix:.bold}  {opal_bar}  {opal_details}  {wide_msg:.dim}";

impl Bar {
    fn new(colour: ColourSupport) -> Self {
        Self {
            colour,
            current: RefCell::new(None),
        }
    }

    fn spinner(&self, label: &'static str, detail: String) -> ProgressBar {
        let colour = self.colour;
        let style = ProgressStyle::with_template(SPINNER_TEMPLATE)
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
            );
        // The `with_` builders set a field without drawing; `set_prefix` and
        // `set_message` each draw, and the first of them would show a line
        // missing the other.
        ProgressBar::with_draw_target(None, short_of_the_edge())
            .with_style(style)
            .with_prefix(label)
            .with_message(detail)
    }

    fn downloads(&self, packages: usize) -> ProgressBar {
        let colour = self.colour;
        let term = Term::stderr();
        let style = ProgressStyle::with_template(DOWNLOAD_TEMPLATE)
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .with_key(
                "opal_bar",
                move |state: &ProgressState, out: &mut dyn fmt::Write| {
                    // Measured on every frame, so a resized window gets a bar
                    // that fits it.
                    let cells = bar_cells(term.size().1);
                    draw_bar(out, f64::from(state.fraction()), cells, colour);
                },
            )
            .with_key(
                "opal_details",
                |state: &ProgressState, out: &mut dyn fmt::Write| {
                    let total = state.len().unwrap_or(0);
                    let elapsed = format_elapsed(state.elapsed());
                    let _ = write!(out, "{DIM}{}/{total}  {elapsed}{RESET}", state.pos());
                },
            );
        ProgressBar::with_draw_target(Some(packages as u64), short_of_the_edge())
            .with_style(style)
            .with_prefix("downloading")
    }
}

impl Progress for Bar {
    fn stage(&self, stage: Stage) {
        // A spinner draws the moment its steady tick starts, so the previous
        // bar has to be gone first. Cleared afterwards, the new line has
        // already wrapped below the old full-width one, the clear lands on the
        // new line, and the old bar stays on screen.
        self.finished();
        // Neither resolving nor linking knows its own size as it goes —
        // resolution discovers the tree, and the reconciler's work depends on
        // what it finds on disk — so a spinner is the honest shape for both.
        let next = match stage {
            Stage::Resolving => self.spinner("resolving", String::new()),
            Stage::Fetching { packages } => self.downloads(packages),
            Stage::Linking { packages } => {
                self.spinner("linking", format!("  {packages} packages"))
            }
        };
        next.enable_steady_tick(TICK);
        *self.current.borrow_mut() = Some(next);
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

fn short_of_the_edge() -> ProgressDrawTarget {
    ProgressDrawTarget::term_like_with_hz(Box::new(ShortOfTheEdge(Term::stderr())), 20)
}

/// Stderr's terminal, reported one column narrower than it is, so that
/// indicatif's `wide_` elements never fill the last column.
#[derive(Debug)]
struct ShortOfTheEdge(Term);

impl TermLike for ShortOfTheEdge {
    fn width(&self) -> u16 {
        self.0.size().1.saturating_sub(1)
    }

    fn height(&self) -> u16 {
        self.0.size().0
    }

    fn move_cursor_up(&self, n: usize) -> io::Result<()> {
        self.0.move_cursor_up(n)
    }

    fn move_cursor_down(&self, n: usize) -> io::Result<()> {
        self.0.move_cursor_down(n)
    }

    fn move_cursor_right(&self, n: usize) -> io::Result<()> {
        self.0.move_cursor_right(n)
    }

    fn move_cursor_left(&self, n: usize) -> io::Result<()> {
        self.0.move_cursor_left(n)
    }

    fn write_line(&self, s: &str) -> io::Result<()> {
        self.0.write_line(s)
    }

    fn write_str(&self, s: &str) -> io::Result<()> {
        self.0.write_str(s)
    }

    fn clear_line(&self) -> io::Result<()> {
        self.0.clear_line()
    }

    fn flush(&self) -> io::Result<()> {
        self.0.flush()
    }
}

type Rgb = (u8, u8, u8);

/// The hues an opal throws as it turns: sky, mint, lavender, rose, peach.
const OPAL: [Rgb; 5] = [
    (120, 196, 255),
    (128, 232, 204),
    (186, 168, 255),
    (255, 178, 204),
    (255, 214, 168),
];
/// The spinner holds one colour, the gradient's first, so the only motion on
/// the line is the rotation.
const SPINNER_COLOUR: Rgb = OPAL[0];

/// The bar is a thin line, filled and unfilled alike; colour tells them apart.
const LINE: char = '─';
/// Without colour the unfilled part is dashed, so the fill still shows.
const DASHED: char = '╌';

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

fn mix(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let channel =
        |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * amount).round() as u8;
    (
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

/// A colour along the gradient. Wraps, so the drift can run forever.
fn opal_at(position: f64) -> Rgb {
    let scaled = position.rem_euclid(1.0) * OPAL.len() as f64;
    let index = scaled.floor() as usize;
    mix(
        OPAL[index % OPAL.len()],
        OPAL[(index + 1) % OPAL.len()],
        scaled.fract(),
    )
}

/// The bar gives way to the package name in a narrow terminal and stops
/// growing in a wide one.
fn bar_cells(columns: u16) -> usize {
    usize::from(columns).saturating_sub(60).clamp(10, 40)
}

fn draw_bar(out: &mut dyn fmt::Write, fraction: f64, cells: usize, colour: ColourSupport) {
    let filled = filled_cells(fraction, cells);
    for cell in 0..filled {
        // Across 90% of the gradient, so the two ends of a full bar don't meet
        // in the same colour.
        colour.paint(opal_at(cell as f64 / cells as f64 * 0.9), out);
        let _ = out.write_char(LINE);
    }
    let rail = match colour {
        ColourSupport::Off => DASHED,
        ColourSupport::Palette256 | ColourSupport::TrueColour => LINE,
    };
    let unfilled: String = std::iter::repeat_n(rail, cells - filled).collect();
    let _ = write!(out, "{RESET}{DIM}{unfilled}{RESET}");
}

/// Whole cells only: a half-cell glyph softens the leading edge.
fn filled_cells(fraction: f64, cells: usize) -> usize {
    ((fraction.clamp(0.0, 1.0) * cells as f64).round() as usize).min(cells)
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

    fn bar(fraction: f64, cells: usize, colour: ColourSupport) -> String {
        let mut out = String::new();
        draw_bar(&mut out, fraction, cells, colour);
        out
    }

    fn glyphs(drawn: &str) -> String {
        drawn
            .chars()
            .filter(|c| matches!(c, '━' | '╸' | '─' | '╌'))
            .collect()
    }

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
    fn test_the_bar_is_exactly_its_cell_count_wide() {
        for colour in [
            ColourSupport::Off,
            ColourSupport::Palette256,
            ColourSupport::TrueColour,
        ] {
            for fraction in [0.0, 0.01, 0.35, 0.5, 0.99, 1.0] {
                assert_eq!(
                    glyphs(&bar(fraction, 24, colour)).chars().count(),
                    24,
                    "{colour:?} at {fraction}"
                );
            }
        }
    }

    #[test]
    fn test_the_fill_rounds_to_whole_cells() {
        assert_eq!(filled_cells(0.34, 10), 3);
        assert_eq!(filled_cells(0.36, 10), 4);
        assert_eq!(filled_cells(0.0, 10), 0);
        assert_eq!(filled_cells(1.0, 10), 10);
    }

    #[test]
    fn test_without_colour_the_unfilled_part_is_dashed() {
        assert_eq!(glyphs(&bar(0.34, 10, ColourSupport::Off)), "───╌╌╌╌╌╌╌");
    }

    #[test]
    fn test_a_full_bar_has_nothing_unfilled() {
        assert_eq!(glyphs(&bar(1.0, 8, ColourSupport::Off)), "────────");
    }

    #[test]
    fn test_a_cell_keeps_its_colour_as_the_bar_fills() {
        // The same cell is painted the same colour at any fill, so the filled
        // part never shifts or shimmers between frames.
        let first = bar(0.25, 20, ColourSupport::TrueColour);
        let later = bar(0.75, 20, ColourSupport::TrueColour);
        let head = |drawn: &str| drawn.split('─').take(5).collect::<Vec<_>>().join("─");
        assert_eq!(head(&first), head(&later));
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
    fn test_resolving_and_linking_show_their_elapsed_time() {
        assert!(SPINNER_TEMPLATE.ends_with("{opal_elapsed}"));
    }

    #[test]
    fn test_the_download_line_has_no_leading_glyph() {
        assert!(DOWNLOAD_TEMPLATE.starts_with("{prefix"));
    }

    #[test]
    fn test_the_line_is_thin_in_every_mode() {
        for colour in [
            ColourSupport::Off,
            ColourSupport::Palette256,
            ColourSupport::TrueColour,
        ] {
            let drawn = bar(0.5, 20, colour);
            assert!(
                !drawn.contains('━') && !drawn.contains('╸'),
                "{colour:?} drew a heavy segment: {drawn:?}"
            );
        }
    }

    #[test]
    fn test_colour_off_writes_no_colour_codes() {
        let drawn = bar(0.6, 20, ColourSupport::Off);
        assert!(!drawn.contains("\x1b[38;"), "{drawn:?}");
    }

    #[test]
    fn test_the_gradient_wraps_around() {
        assert_eq!(opal_at(0.0), OPAL[0]);
        assert_eq!(opal_at(1.0), OPAL[0]);
        assert_eq!(opal_at(-0.2), opal_at(0.8));
    }

    #[test]
    fn test_256_colour_maps_to_the_cube_corners() {
        assert_eq!(to_256((0, 0, 0)), 16);
        assert_eq!(to_256((255, 255, 255)), 231);
    }

    #[test]
    fn test_the_bar_gives_way_in_a_narrow_terminal() {
        assert_eq!(bar_cells(40), 10);
        assert_eq!(bar_cells(80), 20);
        assert_eq!(bar_cells(200), 40);
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
        assert!(ProgressStyle::with_template(DOWNLOAD_TEMPLATE).is_ok());
    }
}
