use std::io;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs};

use crate::types::{BodyRecord, MessageRecord};

pub struct MailItem {
    pub subject: String,
    pub from: String,
    pub date: String,
    pub folder: String,
    pub is_read: bool,
    pub preview: String,
    pub body: String,
}

pub struct TuiState {
    pub mail_items: Vec<MailItem>,
    pub updates: Option<Receiver<TuiEvent>>,
}

struct App {
    updates: Option<Receiver<TuiEvent>>,
    tabs: Vec<&'static str>,
    selected_tab: usize,
    selected_mail: usize,
    mail_items: Vec<MailItem>,
    sync_in_progress: bool,
    spinner_index: usize,
    last_tick: Instant,
}

pub enum TuiEvent {
    SyncStarted,
    SyncFinished,
    MailItems(Vec<MailItem>),
}

const SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

impl App {
    fn new(mail_items: Vec<MailItem>, updates: Option<Receiver<TuiEvent>>) -> Self {
        Self {
            updates,
            tabs: vec!["Calendar", "Mail", "Notes", "Projects"],
            selected_tab: 1, // Mail
            selected_mail: 0,
            mail_items,
            sync_in_progress: false,
            spinner_index: 0,
            last_tick: Instant::now(),
        }
    }

    fn next_mail(&mut self) {
        if self.mail_items.is_empty() {
            return;
        }
        self.selected_mail = (self.selected_mail + 1).min(self.mail_items.len() - 1);
    }

    fn prev_mail(&mut self) {
        if self.mail_items.is_empty() {
            return;
        }
        if self.selected_mail > 0 {
            self.selected_mail -= 1;
        }
    }

    fn drain_updates(&mut self) {
        if let Some(rx) = self.updates.take() {
            while let Ok(event) = rx.try_recv() {
                self.apply_event(event);
            }
            self.updates = Some(rx);
        }
    }

    fn apply_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::SyncStarted => {
                self.sync_in_progress = true;
            }
            TuiEvent::SyncFinished => {
                self.sync_in_progress = false;
            }
            TuiEvent::MailItems(items) => {
                self.mail_items = items;
                if self.mail_items.is_empty() {
                    self.selected_mail = 0;
                } else if self.selected_mail >= self.mail_items.len() {
                    self.selected_mail = self.mail_items.len() - 1;
                }
            }
        }
    }

    fn advance_spinner(&mut self) {
        if self.sync_in_progress {
            self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
        }
    }

    fn spinner_frame(&self) -> &str {
        SPINNER_FRAMES[self.spinner_index % SPINNER_FRAMES.len()]
    }
}

pub fn run(state: TuiState) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, state);

    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    res
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: TuiState,
) -> Result<()> {
    let mut app = App::new(state.mail_items, state.updates);
    let tick_rate = Duration::from_millis(200);

    loop {
        app.drain_updates();
        terminal.draw(|f| draw(f, &app))?;

        let timeout = tick_rate
            .checked_sub(app.last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && handle_key(&mut app, key)?
        {
            break;
        }

        if app.last_tick.elapsed() >= tick_rate {
            app.last_tick = Instant::now();
            app.advance_spinner();
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Ok(true);
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            app.next_mail();
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            app.prev_mail();
        }
        (KeyCode::Left, _) => {
            if app.selected_tab > 0 {
                app.selected_tab -= 1;
            }
        }
        (KeyCode::Right, _) => {
            if app.selected_tab + 1 < app.tabs.len() {
                app.selected_tab += 1;
            }
        }
        _ => {}
    }
    Ok(false)
}

fn draw(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
        .split(size);

    draw_top_bar(f, app, chunks[0]);
    draw_body(f, app, chunks[1]);
}

fn draw_top_bar(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = app.tabs.iter().map(|t| Line::from(Span::raw(*t))).collect();

    let title_text = if app.sync_in_progress {
        format!("Otto | Syncing {}", app.spinner_frame())
    } else {
        "Otto".to_string()
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Line::from(title_text)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .select(app.selected_tab);

    f.render_widget(tabs, area);
}

fn draw_body(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
        .split(area);

    draw_mail_area(f, app, chunks[0]);
    draw_agent_panel(f, chunks[1]);
}

fn draw_mail_area(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Min(5),    // list + detail
                Constraint::Length(3), // action bar
            ]
            .as_ref(),
        )
        .split(area);

    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)].as_ref())
        .split(chunks[0]);

    draw_mail_list(f, app, inner[0]);
    draw_mail_detail(f, app, inner[1]);
    draw_action_bar(f, chunks[1]);
}

fn draw_mail_list(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .mail_items
        .iter()
        .map(|m| {
            let status = if m.is_read { "R" } else { "U" };
            let line = format!("[{}] {} — {}", status, m.from, m.subject);
            ListItem::new(Line::from(line))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Mail"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut make_list_state(app));
}

fn make_list_state(app: &App) -> ratatui::widgets::ListState {
    let mut state = ratatui::widgets::ListState::default();
    if !app.mail_items.is_empty() {
        state.select(Some(app.selected_mail));
    }
    state
}

fn draw_mail_detail(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let content = if app.mail_items.is_empty() {
        "No messages loaded yet.\n\nRun sync first to populate the cache.".to_string()
    } else {
        let current = &app.mail_items[app.selected_mail];
        format!(
            "From: {}\nFolder: {}\nDate: {}\n\n{}",
            current.from, current.folder, current.date, current.body
        )
    };

    let paragraph = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title("Body"))
        .wrap(ratatui::widgets::Wrap { trim: true });

    f.render_widget(paragraph, area);
}

fn draw_action_bar(f: &mut ratatui::Frame, area: Rect) {
    let line = Line::from(vec![
        Span::raw("[j/k] move  "),
        Span::raw("[←/→] switch tab  "),
        Span::raw("[q] quit"),
    ]);

    let paragraph =
        Paragraph::new(line).block(Block::default().borders(Borders::ALL).title("Actions"));

    f.render_widget(paragraph, area);
}

fn draw_agent_panel(f: &mut ratatui::Frame, area: Rect) {
    let text = "Agent chat (future)\n\nThis panel will host conversations with coding agents (Codex, Claude, etc). For now it is a read-only placeholder.";
    let paragraph =
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Agent"));
    f.render_widget(paragraph, area);
}

pub fn build_mail_items(messages: &[(MessageRecord, Option<BodyRecord>)]) -> Vec<MailItem> {
    messages
        .iter()
        .map(|(msg, body)| {
            let date = msg
                .internal_date
                .map(|ts| {
                    DateTime::<Utc>::from_timestamp(ts, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "Unknown".to_string())
                })
                .unwrap_or_else(|| "Unknown".to_string());

            let from = msg.from.clone().unwrap_or_else(|| "Unknown".to_string());
            let subject = msg
                .subject
                .clone()
                .unwrap_or_else(|| "(No Subject)".to_string());
            let is_read = msg.flags.iter().any(|f| f.eq("Seen") || f.eq("\\Seen"));

            let body_text = body
                .as_ref()
                .and_then(|b| b.sanitized_text.as_deref())
                .map(|s| s.to_string())
                .unwrap_or_else(String::new);

            let preview = body_text
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .to_string();

            MailItem {
                subject,
                from,
                date,
                folder: msg.folder.clone(),
                is_read,
                preview,
                body: body_text,
            }
        })
        .collect()
}
