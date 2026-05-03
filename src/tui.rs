use std::io;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs},
};

use mq_docs::{DocEntry, SelectorEntry};

/// Which top-level tab is active.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Functions,
    Selectors,
}

/// Whether the user is currently typing a search query.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Searching,
}

/// Full TUI application state.
struct App {
    tab: Tab,
    input_mode: InputMode,
    search_query: String,

    // full data
    all_functions: Vec<DocEntry>,
    all_selectors: Vec<SelectorEntry>,

    // filtered views (indices into all_*)
    filtered_fn_indices: Vec<usize>,
    filtered_sel_indices: Vec<usize>,

    fn_list_state: ListState,
    sel_list_state: ListState,
}

impl App {
    fn new(
        functions: Vec<DocEntry>,
        selectors: Vec<SelectorEntry>,
        initial_search: Option<String>,
    ) -> Self {
        let fn_count = functions.len();
        let sel_count = selectors.len();

        let mut app = App {
            tab: Tab::Functions,
            input_mode: InputMode::Normal,
            search_query: initial_search.unwrap_or_default(),
            all_functions: functions,
            all_selectors: selectors,
            filtered_fn_indices: (0..fn_count).collect(),
            filtered_sel_indices: (0..sel_count).collect(),
            fn_list_state: ListState::default(),
            sel_list_state: ListState::default(),
        };

        // Apply initial search if provided
        if !app.search_query.is_empty() {
            app.apply_filter();
        }

        // Select first item in each list
        if !app.filtered_fn_indices.is_empty() {
            app.fn_list_state.select(Some(0));
        }
        if !app.filtered_sel_indices.is_empty() {
            app.sel_list_state.select(Some(0));
        }

        app
    }

    fn apply_filter(&mut self) {
        let q = self.search_query.to_lowercase();

        self.filtered_fn_indices = self
            .all_functions
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();

        self.filtered_sel_indices = self
            .all_selectors
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();

        // Reset selection to first visible item
        self.fn_list_state
            .select(if self.filtered_fn_indices.is_empty() {
                None
            } else {
                Some(0)
            });
        self.sel_list_state
            .select(if self.filtered_sel_indices.is_empty() {
                None
            } else {
                Some(0)
            });
    }

    fn current_fn_entry(&self) -> Option<&DocEntry> {
        let idx = self.fn_list_state.selected()?;
        let real = self.filtered_fn_indices.get(idx)?;
        self.all_functions.get(*real)
    }

    fn current_sel_entry(&self) -> Option<&SelectorEntry> {
        let idx = self.sel_list_state.selected()?;
        let real = self.filtered_sel_indices.get(idx)?;
        self.all_selectors.get(*real)
    }

    fn fn_count(&self) -> usize {
        self.filtered_fn_indices.len()
    }

    fn sel_count(&self) -> usize {
        self.filtered_sel_indices.len()
    }

    fn navigate(&mut self, delta: isize) {
        match self.tab {
            Tab::Functions => {
                let count = self.fn_count();
                if count == 0 {
                    return;
                }
                let cur = self.fn_list_state.selected().unwrap_or(0) as isize;
                let next = (cur + delta).rem_euclid(count as isize) as usize;
                self.fn_list_state.select(Some(next));
            }
            Tab::Selectors => {
                let count = self.sel_count();
                if count == 0 {
                    return;
                }
                let cur = self.sel_list_state.selected().unwrap_or(0) as isize;
                let next = (cur + delta).rem_euclid(count as isize) as usize;
                self.sel_list_state.select(Some(next));
            }
        }
    }

    fn page_jump(&mut self, delta: isize) {
        self.navigate(delta * 10);
    }

    fn switch_tab(&mut self) {
        self.tab = match self.tab {
            Tab::Functions => Tab::Selectors,
            Tab::Selectors => Tab::Functions,
        };
    }
}

/// Run the interactive TUI. Returns when the user quits.
pub fn run_tui(
    functions: Vec<DocEntry>,
    selectors: Vec<SelectorEntry>,
    initial_search: Option<String>,
) -> miette::Result<()> {
    // Set up terminal
    enable_raw_mode().map_err(|e| miette::miette!("Failed to enable raw mode: {e}"))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| miette::miette!("Failed to enter alternate screen: {e}"))?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal =
        Terminal::new(backend).map_err(|e| miette::miette!("Failed to create terminal: {e}"))?;

    let mut app = App::new(functions, selectors, initial_search);
    let result = run_event_loop(&mut terminal, &mut app);

    // Always restore terminal
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();

    result.map_err(|e| miette::miette!("TUI error: {e}"))
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match app.input_mode {
                InputMode::Searching => match key.code {
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                        app.search_query.clear();
                        app.apply_filter();
                    }
                    KeyCode::Enter => {
                        app.input_mode = InputMode::Normal;
                    }
                    KeyCode::Backspace => {
                        app.search_query.pop();
                        app.apply_filter();
                    }
                    KeyCode::Char(c) => {
                        app.search_query.push(c);
                        app.apply_filter();
                    }
                    _ => {}
                },
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('/') => {
                        app.input_mode = InputMode::Searching;
                    }
                    KeyCode::Tab => app.switch_tab(),
                    KeyCode::Up | KeyCode::Char('k') => app.navigate(-1),
                    KeyCode::Down | KeyCode::Char('j') => app.navigate(1),
                    KeyCode::PageUp => app.page_jump(-1),
                    KeyCode::PageDown => app.page_jump(1),
                    _ => {}
                },
            }
        }
    }
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Outer vertical layout: tabs / body / help bar
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(0),    // main area
            Constraint::Length(1), // help bar
        ])
        .split(area);

    // --- Tab bar ---
    let fn_label = format!(" Functions ({}) ", app.fn_count());
    let sel_label = format!(" Selectors ({}) ", app.sel_count());
    let tab_titles: Vec<Line> = vec![Line::from(fn_label), Line::from(sel_label)];
    let selected_tab = match app.tab {
        Tab::Functions => 0,
        Tab::Selectors => 1,
    };
    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title(" mq docs "))
        .select(selected_tab)
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, outer[0]);

    // --- Main area: list | detail ---
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(outer[1]);

    match app.tab {
        Tab::Functions => {
            draw_fn_list(f, app, body[0]);
            draw_fn_detail(f, app, body[1]);
        }
        Tab::Selectors => {
            draw_sel_list(f, app, body[0]);
            draw_sel_detail(f, app, body[1]);
        }
    }

    // --- Help / search bar ---
    let help_text = if app.input_mode == InputMode::Searching {
        Line::from(vec![
            Span::styled(
                " Search: ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                app.search_query.as_str(),
                Style::default().fg(Color::White),
            ),
            Span::styled("_", Style::default().fg(Color::Cyan)),
            Span::styled(
                "  Esc: cancel  Enter: confirm",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else {
        Line::from(vec![Span::styled(
            " /: search  Tab: switch  ↑/↓ j/k: navigate  PgUp/PgDn: page  q: quit",
            Style::default().fg(Color::DarkGray),
        )])
    };

    let help = Paragraph::new(help_text)
        .style(Style::default().bg(Color::Reset));
    f.render_widget(help, outer[2]);
}

fn draw_fn_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .filtered_fn_indices
        .iter()
        .map(|&i| {
            let entry = &app.all_functions[i];
            if entry.is_deprecated {
                ListItem::new(Line::from(vec![Span::styled(
                    entry.name.as_str(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                )]))
            } else {
                ListItem::new(entry.name.as_str())
            }
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Functions "))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut app.fn_list_state.clone());
}

fn draw_fn_detail(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Detail ");

    if let Some(entry) = app.current_fn_entry() {
        let mut lines: Vec<Line> = Vec::new();

        // Function name (yellow + bold)
        lines.push(Line::from(vec![
            Span::styled(
                "Function:    ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                entry.name.as_str(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        // Deprecated warning
        if entry.is_deprecated {
            lines.push(Line::from(vec![Span::styled(
                "             ⚠ Deprecated",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            )]));
        }

        lines.push(Line::from(""));

        // Description
        lines.push(Line::from(vec![Span::styled(
            "Description",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));
        for desc_line in entry.description.lines() {
            lines.push(Line::from(format!("  {desc_line}")));
        }
        if entry.description.is_empty() {
            lines.push(Line::from("  —"));
        }

        lines.push(Line::from(""));

        // Parameters
        lines.push(Line::from(vec![
            Span::styled(
                "Parameters:  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(if entry.params.is_empty() {
                "—"
            } else {
                entry.params.as_str()
            }),
        ]));

        lines.push(Line::from(""));

        // Example
        lines.push(Line::from(vec![
            Span::styled(
                "Example:     ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                entry.example.as_str(),
                Style::default().fg(Color::Green),
            ),
        ]));

        let para = Paragraph::new(lines)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(para, area);
    } else {
        let para = Paragraph::new("No entry selected.").block(block);
        f.render_widget(para, area);
    }
}

fn draw_sel_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .filtered_sel_indices
        .iter()
        .map(|&i| {
            let entry = &app.all_selectors[i];
            ListItem::new(entry.name.as_str())
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Selectors "))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut app.sel_list_state.clone());
}

fn draw_sel_detail(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Detail ");

    if let Some(entry) = app.current_sel_entry() {
        let lines: Vec<Line> = vec![
            Line::from(vec![
                Span::styled(
                    "Selector:    ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    entry.name.as_str(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Description",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(format!(
                "  {}",
                if entry.description.is_empty() {
                    "—"
                } else {
                    entry.description.as_str()
                }
            )),
        ];

        let para = Paragraph::new(lines)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(para, area);
    } else {
        let para = Paragraph::new("No entry selected.").block(block);
        f.render_widget(para, area);
    }
}
