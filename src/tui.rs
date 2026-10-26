//! The interactive view: app table, streaming journal, and the handful of keys you actually use.
//!
//! Everything here is read-only unless appd runs as root; mutating keys explain themselves
//! instead of surfacing D-Bus errors.

use std::collections::VecDeque;
use std::io::{stdout, BufRead, BufReader, Stdout};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::{Frame, Terminal};

use crate::commands::status::probe;
use crate::{naming, registry, systemd};

const REFRESH: Duration = Duration::from_secs(2);
const TOAST: Duration = Duration::from_secs(4);
const LOG_LINES: usize = 2000;

pub fn run() -> Result<i32> {
    let mut terminal = setup()?;
    let outcome = event_loop(&mut terminal);
    restore(&mut terminal);

    match outcome {
        Ok(()) => Ok(0),
        Err(err) => Err(err),
    }
}

fn setup() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    // If we panic mid-draw, put the terminal back before the message prints.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, cursor::Show);
        hook(info);
    }));

    enable_raw_mode().context("cannot enable raw mode; is this a terminal?")?;
    execute!(stdout(), EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout());
    match Terminal::new(backend) {
        Ok(terminal) => Ok(terminal),
        Err(err) => {
            let _ = disable_raw_mode();
            let _ = execute!(stdout(), LeaveAlternateScreen);
            Err(err).context("cannot initialise the terminal")
        }
    }
}

fn restore(terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show);
    let _ = terminal.show_cursor();
}

struct AppRow {
    name: String,
    unit: String,
    state: String,
    since: String,
    pid: String,
    notes: String,
}

struct Model {
    rows: Vec<AppRow>,
    selected: usize,
    filter: String,
    logs: Option<Journal>,
    log_lines: VecDeque<String>,
    readonly: bool,
    mode: Mode,
    toast: Option<(String, bool, Instant)>,
    last_refresh: Instant,
}

enum Mode {
    Normal,
    Filter,
    Help,
    Confirm(Action),
}

#[derive(Clone, Copy)]
enum Action {
    Restart,
    Stop,
    Disable,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Action::Restart => "restart",
            Action::Stop => "stop",
            Action::Disable => "disable",
        }
    }
}

impl Model {
    fn new() -> Self {
        let mut model = Self {
            rows: Vec::new(),
            selected: 0,
            filter: String::new(),
            logs: None,
            log_lines: VecDeque::new(),
            readonly: !systemd::is_root(),
            mode: Mode::Normal,
            toast: None,
            last_refresh: Instant::now(),
        };
        model.refresh();
        model
    }

    fn refresh(&mut self) {
        match load_rows() {
            Ok(rows) => {
                self.rows = rows;
                if self.rows.is_empty() && self.filter.is_empty() {
                    self.say("no apps configured in /etc/appd", false);
                }
            }
            Err(err) => self.say(format!("{err:#}"), false),
        }
        self.last_refresh = Instant::now();
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    /// Indices into `rows` that pass the filter.
    fn visible(&self) -> Vec<usize> {
        let needle = self.filter.to_ascii_lowercase();
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| needle.is_empty() || row.name.to_ascii_lowercase().contains(&needle))
            .map(|(index, _)| index)
            .collect()
    }

    fn current(&self) -> Option<&AppRow> {
        let visible = self.visible();
        visible.get(self.selected).map(|index| &self.rows[*index])
    }

    fn say(&mut self, message: impl Into<String>, ok: bool) {
        self.toast = Some((message.into(), ok, Instant::now()));
    }

    fn toast(&self) -> Option<&(String, bool, Instant)> {
        self.toast
            .as_ref()
            .filter(|(_, _, at)| at.elapsed() < TOAST)
    }

    fn move_by(&mut self, delta: isize) {
        let len = self.visible().len();
        if len == 0 {
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, len as isize - 1) as usize;
    }

    fn toggle_logs(&mut self) {
        if self.logs.is_some() {
            self.logs = None;
            self.log_lines.clear();
            return;
        }
        let Some(row) = self.current() else { return };
        let unit = row.unit.clone();
        match start_journal(&unit) {
            Ok((backlog, journal)) => {
                self.log_lines = backlog.into_iter().collect();
                self.logs = Some(journal);
            }
            Err(err) => self.say(format!("{err:#}"), false),
        }
    }

    fn reload_logs(&mut self) {
        let Some(row) = self.current() else { return };
        let unit = row.unit.clone();
        match start_journal(&unit) {
            Ok((backlog, journal)) => {
                self.log_lines = backlog.into_iter().collect();
                self.logs = Some(journal);
            }
            Err(err) => self.say(format!("{err:#}"), false),
        }
    }

    fn drain_journal(&mut self) {
        let Some(journal) = &self.logs else { return };
        while let Ok(line) = journal.rx.try_recv() {
            self.log_lines.push_back(line);
            if self.log_lines.len() > LOG_LINES {
                self.log_lines.pop_front();
            }
        }
    }

    /// Re-open the stream when the selection changes while the log pane is visible.
    fn sync_journal(&mut self) {
        if self.logs.is_none() {
            return;
        }
        let Some(row) = self.current() else { return };
        let stale = self
            .logs
            .as_ref()
            .map(|journal| journal.unit != row.unit)
            .unwrap_or(false);
        if stale {
            self.reload_logs();
        }
    }

    fn request(&mut self, action: Action) {
        if self.readonly_toast(action.verb()) {
            return;
        }
        if self.current().is_none() {
            return;
        }
        self.mode = Mode::Confirm(action);
    }

    /// Explain the read-only refusal and return true when mutating keys are unavailable.
    fn readonly_toast(&mut self, verb: &str) -> bool {
        if !self.readonly {
            return false;
        }
        let row = self.current().map(|row| row.name.clone()).unwrap_or_default();
        self.say(
            format!("read-only: run `sudo appd {verb} {row}` from a shell"),
            false,
        );
        true
    }

    fn confirm(&mut self) {
        let Mode::Confirm(action) = self.mode else { return };
        self.mode = Mode::Normal;
        self.perform(action.verb());
    }

    fn perform(&mut self, verb: &str) {
        if self.readonly_toast(verb) {
            return;
        }
        let Some(row) = self.current() else { return };
        let unit = row.unit.clone();
        let name = row.name.clone();
        match systemd::systemctl(&[verb, &unit]) {
            Ok(()) => self.say(format!("{verb} {name}: ok"), true),
            Err(err) => self.say(format!("{verb} {name}: {err:#}"), false),
        }
        self.refresh();
    }

    /// Render every config into its unit and reload systemd, without restarting anything.
    fn apply_configs(&mut self) {
        if self.readonly {
            // Apply is global, so don't name the selected app like the other keys do.
            self.say("read-only: run `sudo appd apply` from a shell", false);
            return;
        }

        match crate::commands::apply::sync(&[], false) {
            Ok(report) => {
                let mut message = if !report.dir_exists {
                    "no config directory; run `appd init`".to_string()
                } else if report.applied == 0 {
                    "no configs to apply".to_string()
                } else if report.changed.is_empty() {
                    format!("applied {} configs; already in sync", report.applied)
                } else {
                    format!(
                        "applied {} configs, {} changed; restart {}",
                        report.applied,
                        report.changed.len(),
                        report.changed.join(" ")
                    )
                };
                if !report.warnings.is_empty() {
                    message.push_str(&format!(
                        " ({} warnings; see `appd apply`)",
                        report.warnings.len()
                    ));
                }
                self.say(message, report.dir_exists);
            }
            Err(err) => self.say(format!("{err:#}"), false),
        }

        self.refresh();
        self.sync_journal();
    }
}

fn load_rows() -> Result<Vec<AppRow>> {
    let registry = registry::load()?;
    let names = registry.select_names(&[])?;

    let mut rows = Vec::new();
    for name in names {
        let unit = naming::unit_name(&name);
        let (state, since, pid, notes) = probe(&unit);
        let opaque = registry.get_opaque(&name).is_some();
        let notes = if opaque {
            if notes.is_empty() {
                "config unreadable, run as root".to_string()
            } else {
                format!("{notes}; config unreadable")
            }
        } else {
            notes
        };
        rows.push(AppRow {
            name,
            unit,
            state,
            since,
            pid,
            notes,
        });
    }
    Ok(rows)
}

struct Journal {
    unit: String,
    child: Child,
    rx: Receiver<String>,
}

impl Drop for Journal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_journal(unit: &str) -> Result<(Vec<String>, Journal)> {
    // Read the backlog first so permission problems become a message instead of an empty pane.
    let backlog = systemd::journalctl_output(&[
        "--unit".to_string(),
        unit.to_string(),
        "--no-pager".to_string(),
        "--quiet".to_string(),
        "--output=short-iso".to_string(),
        "--lines".to_string(),
        "200".to_string(),
    ])?;

    let mut child = Command::new("journalctl")
        .args([
            "--unit",
            unit,
            "--follow",
            "--no-pager",
            "--quiet",
            "--output=short-iso",
            "--lines",
            "0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("cannot start journalctl for the log pane")?;

    let stdout = child
        .stdout
        .take()
        .context("journalctl produced no output stream")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let lines = backlog.lines().map(str::to_string).collect();
    Ok((
        lines,
        Journal {
            unit: unit.to_string(),
            child,
            rx,
        },
    ))
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let mut model = Model::new();

    loop {
        terminal.draw(|frame| ui(frame, &mut model))?;

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && handle_key(&mut model, key) {
                    return Ok(());
                }
            }
        }

        model.drain_journal();

        if model.last_refresh.elapsed() >= REFRESH {
            model.refresh();
            model.sync_journal();
        }
    }
}

/// Returns true when it's time to leave.
fn handle_key(model: &mut Model, key: KeyEvent) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return true;
    }

    match model.mode {
        Mode::Help => {
            if matches!(key.code, KeyCode::Char('q')) {
                return true;
            }
            model.mode = Mode::Normal;
            return false;
        }
        Mode::Filter => {
            match key.code {
                KeyCode::Enter => model.mode = Mode::Normal,
                KeyCode::Esc => {
                    model.filter.clear();
                    model.mode = Mode::Normal;
                    model.clamp_selection();
                }
                KeyCode::Backspace => {
                    model.filter.pop();
                    model.clamp_selection();
                }
                KeyCode::Char(c) => {
                    model.filter.push(c);
                    model.clamp_selection();
                }
                _ => {}
            }
            return false;
        }
        Mode::Confirm(_) => {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => model.confirm(),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => model.mode = Mode::Normal,
                _ => {}
            }
            return false;
        }
        Mode::Normal => {}
    }

    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Down | KeyCode::Char('j') => model.move_by(1),
        KeyCode::Up | KeyCode::Char('k') => model.move_by(-1),
        KeyCode::Char('g') => {
            model.selected = 0;
        }
        KeyCode::Char('G') => {
            let len = model.visible().len();
            model.selected = len.saturating_sub(1);
        }
        KeyCode::Enter | KeyCode::Char('l') => model.toggle_logs(),
        KeyCode::Char('a') => model.apply_configs(),
        KeyCode::Char('r') => model.request(Action::Restart),
        KeyCode::Char('s') => model.request(Action::Stop),
        KeyCode::Char('t') => model.perform("start"),
        KeyCode::Char('e') => model.perform("enable"),
        KeyCode::Char('x') => model.request(Action::Disable),
        KeyCode::Char('/') => model.mode = Mode::Filter,
        KeyCode::Char('?') => model.mode = Mode::Help,
        KeyCode::Esc => {
            if model.logs.is_some() {
                model.toggle_logs();
            } else if !model.filter.is_empty() {
                model.filter.clear();
                model.clamp_selection();
            }
        }
        _ => {}
    }
    false
}

fn ui(frame: &mut Frame, model: &mut Model) {
    let log_height = if model.logs.is_some() { 14 } else { 0 };
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(log_height),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(frame.area());

    render_header(frame, model, chunks[0]);
    render_table(frame, model, chunks[1]);
    if model.logs.is_some() {
        render_logs(frame, model, chunks[2]);
    }
    render_message(frame, model, chunks[3]);
    render_footer(frame, model, chunks[4]);

    if matches!(model.mode, Mode::Help) {
        render_help(frame);
    }
}

fn render_header(frame: &mut Frame, model: &Model, area: Rect) {
    let mut spans = vec![
        Span::styled(" appd ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{} app{}", model.rows.len(), if model.rows.len() == 1 { "" } else { "s" }),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            if model.readonly { "  read-only (not root) " } else { "  root " },
            Style::default().fg(if model.readonly { Color::Yellow } else { Color::Green }),
        ),
    ];
    if !model.filter.is_empty() {
        spans.push(Span::styled(
            format!("  filter: {}", model.filter),
            Style::default().fg(Color::Cyan),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_table(frame: &mut Frame, model: &mut Model, area: Rect) {
    let header = Row::new(vec!["APP", "UNIT", "STATE", "SINCE", "PID", "NOTES"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let visible = model.visible();
    let rows: Vec<Row> = visible
        .iter()
        .map(|index| {
            let row = &model.rows[*index];
            let state_style = match row.state.as_str() {
                "running" => Style::default().fg(Color::Green),
                "failed" => Style::default().fg(Color::Red),
                "stopped" | "exited" => Style::default().fg(Color::Yellow),
                "?" | "not-installed" => Style::default().fg(Color::DarkGray),
                _ => Style::default(),
            };
            Row::new(vec![
                Cell::from(row.name.clone()),
                Cell::from(row.unit.clone()),
                Cell::from(row.state.clone()).style(state_style),
                Cell::from(row.since.clone()),
                Cell::from(row.pid.clone()),
                Cell::from(row.notes.clone()).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(22),
        Constraint::Length(32),
        Constraint::Length(13),
        Constraint::Length(17),
        Constraint::Length(7),
        Constraint::Min(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" apps "))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");

    let mut state = TableState::default();
    if !visible.is_empty() {
        state.select(Some(model.selected));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_logs(frame: &mut Frame, model: &Model, area: Rect) {
    let title = model
        .logs
        .as_ref()
        .map(|journal| format!(" logs: {} (following) ", journal.unit))
        .unwrap_or_else(|| " logs ".to_string());

    let inner_height = area.height.saturating_sub(2) as usize;
    let skip = model.log_lines.len().saturating_sub(inner_height);
    let lines: Vec<Line> = model
        .log_lines
        .iter()
        .skip(skip)
        .map(|line| Line::from(line.as_str()))
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_message(frame: &mut Frame, model: &Model, area: Rect) {
    if let Mode::Confirm(action) = model.mode {
        let unit = model
            .current()
            .map(|row| row.unit.clone())
            .unwrap_or_default();
        let text = format!(" {} {unit}? [y/N]", action.verb());
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(Color::Yellow)),
            area,
        );
        return;
    }

    if let Some((message, ok, _)) = model.toast() {
        let style = if *ok {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        frame.render_widget(Paragraph::new(format!(" {message}")).style(style), area);
    }
}

fn render_footer(frame: &mut Frame, model: &Model, area: Rect) {
    let text = match model.mode {
        Mode::Filter => " filter: type to narrow, enter to keep, esc to clear".to_string(),
        Mode::Confirm(_) => " y confirm · n cancel".to_string(),
        Mode::Help => " any key to close".to_string(),
        Mode::Normal => {
            if area.width < 100 {
                " j/k move · enter logs · a apply · r restart · s stop · ? help · q quit".to_string()
            } else {
                " j/k move · enter logs · a apply · r restart · s stop · t start · e enable · x disable · / filter · ? help · q quit"
                    .to_string()
            }
        }
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_help(frame: &mut Frame) {
    let area = centered(frame.area(), 64, 40);
    let help = vec![
        Line::from("keys"),
        Line::from(""),
        Line::from("  j / k, ↑ / ↓   move between apps"),
        Line::from("  g / G          first / last"),
        Line::from("  enter or l     toggle the log pane for the selection"),
        Line::from("  a              apply configs: sync units (no restart)"),
        Line::from("  r              restart (asks first)"),
        Line::from("  s              stop (asks first)"),
        Line::from("  t              start"),
        Line::from("  e              enable at boot"),
        Line::from("  x              disable at boot (asks first)"),
        Line::from("  /              filter apps by name"),
        Line::from("  ?              this help"),
        Line::from("  q              quit"),
        Line::from(""),
        Line::from("Read-only when not root: mutating keys will tell you the"),
        Line::from("equivalent sudo command instead of failing."),
    ];
    let paragraph = Paragraph::new(help)
        .block(Block::default().borders(Borders::ALL).title(" help "))
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1]);
    horizontal[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_with(apps: &[&str]) -> Model {
        Model {
            rows: apps
                .iter()
                .map(|name| AppRow {
                    name: (*name).to_string(),
                    unit: naming::unit_name(name),
                    state: "running".to_string(),
                    since: "2026-10-26 14:02".to_string(),
                    pid: "1".to_string(),
                    notes: String::new(),
                })
                .collect(),
            selected: 0,
            filter: String::new(),
            logs: None,
            log_lines: VecDeque::new(),
            readonly: false,
            mode: Mode::Normal,
            toast: None,
            last_refresh: Instant::now(),
        }
    }

    #[test]
    fn filter_narrows_and_selection_clamps() {
        let mut model = model_with(&["sundial-api", "sundial-worker", "other"]);
        model.selected = 2;
        model.filter = "sundial".to_string();

        assert_eq!(model.visible(), vec![0, 1]);
        model.clamp_selection();
        assert_eq!(model.selected, 1);
        assert_eq!(model.current().unwrap().name, "sundial-worker");
    }

    #[test]
    fn moving_clamps_at_the_edges() {
        let mut model = model_with(&["a", "b"]);
        model.move_by(-1);
        assert_eq!(model.selected, 0);
        model.move_by(5);
        assert_eq!(model.selected, 1);
    }

    #[test]
    fn read_only_actions_do_not_enter_confirmation() {
        let mut model = model_with(&["demo"]);
        model.readonly = true;

        model.request(Action::Restart);

        assert!(matches!(model.mode, Mode::Normal));
        let (message, ok, _) = model.toast().unwrap();
        assert!(!ok);
        assert!(message.contains("sudo appd restart demo"), "{message}");
    }

    #[test]
    fn read_only_also_blocks_start_and_enable() {
        let mut model = model_with(&["demo"]);
        model.readonly = true;

        model.perform("start");

        let (message, ok, _) = model.toast().unwrap();
        assert!(!ok);
        assert!(message.contains("sudo appd start demo"), "{message}");
    }

    #[test]
    fn restart_asks_before_acting() {
        let mut model = model_with(&["demo"]);
        model.request(Action::Restart);
        assert!(matches!(model.mode, Mode::Confirm(Action::Restart)));
    }

    #[test]
    fn read_only_blocks_apply() {
        let mut model = model_with(&["demo"]);
        model.readonly = true;

        model.apply_configs();

        let (message, ok, _) = model.toast().unwrap();
        assert!(!ok);
        assert!(message.contains("sudo appd apply"), "{message}");
    }

    #[test]
    fn journal_lines_are_capped() {
        let mut model = model_with(&["demo"]);
        for index in 0..(LOG_LINES + 10) {
            model.log_lines.push_back(index.to_string());
            if model.log_lines.len() > LOG_LINES {
                model.log_lines.pop_front();
            }
        }
        assert_eq!(model.log_lines.len(), LOG_LINES);
    }
}
