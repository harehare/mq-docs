use std::io;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

use mq_docs::{DocEntry, ModuleEntry, is_deprecated};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentTab {
    Functions,
    Selectors,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusedPane {
    Modules,
    Items,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Searching,
}

struct App {
    // module_names[0] = "All" (virtual), [1..] = real modules
    module_names: Vec<String>,
    // parallel vecs of functions/selectors per module (index 0 = All combined)
    module_functions: Vec<Vec<DocEntry>>,
    module_selectors: Vec<Vec<DocEntry>>,

    selected_module: usize,
    module_list_state: ListState,

    content_tab: ContentTab,

    // filtered indices into module_functions[selected_module] / module_selectors[selected_module]
    filtered_fn: Vec<usize>,
    filtered_sel: Vec<usize>,
    item_list_state: ListState,

    focused: FocusedPane,
    input_mode: InputMode,
    search_query: String,

    multi_module: bool,
}

impl App {
    fn new(modules: Vec<ModuleEntry>, initial_search: Option<String>) -> Self {
        let multi_module = modules.len() > 1;

        // "All" virtual module = concat of all real modules
        let all_fns: Vec<DocEntry> = modules
            .iter()
            .flat_map(|m| m.functions.iter().cloned())
            .collect();
        let all_sels: Vec<DocEntry> = modules
            .iter()
            .flat_map(|m| m.selectors.iter().cloned())
            .collect();

        let mut module_names = vec!["All".to_string()];
        let mut module_functions = vec![all_fns];
        let mut module_selectors = vec![all_sels];

        for m in modules {
            module_names.push(m.name);
            module_functions.push(m.functions);
            module_selectors.push(m.selectors);
        }

        let fn_count = module_functions[0].len();
        let sel_count = module_selectors[0].len();

        let mut module_state = ListState::default();
        module_state.select(Some(0));
        let mut item_state = ListState::default();
        if fn_count > 0 {
            item_state.select(Some(0));
        }

        let mut app = App {
            module_names,
            module_functions,
            module_selectors,
            selected_module: 0,
            module_list_state: module_state,
            content_tab: ContentTab::Functions,
            filtered_fn: (0..fn_count).collect(),
            filtered_sel: (0..sel_count).collect(),
            item_list_state: item_state,
            focused: if multi_module {
                FocusedPane::Modules
            } else {
                FocusedPane::Items
            },
            input_mode: InputMode::Normal,
            search_query: initial_search.unwrap_or_default(),
            multi_module,
        };

        if !app.search_query.is_empty() {
            app.apply_filter();
        }

        app
    }

    fn current_fns(&self) -> &Vec<DocEntry> {
        &self.module_functions[self.selected_module]
    }

    fn current_sels(&self) -> &Vec<DocEntry> {
        &self.module_selectors[self.selected_module]
    }

    fn apply_filter(&mut self) {
        let q = self.search_query.to_lowercase();

        self.filtered_fn = self
            .current_fns()
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                q.is_empty()
                    || e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();

        self.filtered_sel = self
            .current_sels()
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                q.is_empty()
                    || e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();

        let empty = match self.content_tab {
            ContentTab::Functions => self.filtered_fn.is_empty(),
            ContentTab::Selectors => self.filtered_sel.is_empty(),
        };
        self.item_list_state
            .select(if empty { None } else { Some(0) });
    }

    fn select_module(&mut self, idx: usize) {
        if idx >= self.module_names.len() {
            return;
        }
        self.selected_module = idx;
        self.module_list_state.select(Some(idx));
        self.apply_filter();
    }

    fn navigate_module(&mut self, delta: isize) {
        let count = self.module_names.len();
        if count == 0 {
            return;
        }
        let cur = self.module_list_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(count as isize) as usize;
        self.select_module(next);
    }

    fn navigate_item(&mut self, delta: isize) {
        let count = match self.content_tab {
            ContentTab::Functions => self.filtered_fn.len(),
            ContentTab::Selectors => self.filtered_sel.len(),
        };
        if count == 0 {
            return;
        }
        let cur = self.item_list_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(count as isize) as usize;
        self.item_list_state.select(Some(next));
    }

    fn switch_content_tab(&mut self) {
        self.content_tab = match self.content_tab {
            ContentTab::Functions => ContentTab::Selectors,
            ContentTab::Selectors => ContentTab::Functions,
        };
        let empty = match self.content_tab {
            ContentTab::Functions => self.filtered_fn.is_empty(),
            ContentTab::Selectors => self.filtered_sel.is_empty(),
        };
        self.item_list_state
            .select(if empty { None } else { Some(0) });
    }

    fn current_fn_entry(&self) -> Option<&DocEntry> {
        let idx = self.item_list_state.selected()?;
        let real = self.filtered_fn.get(idx)?;
        self.current_fns().get(*real)
    }

    fn current_sel_entry(&self) -> Option<&DocEntry> {
        let idx = self.item_list_state.selected()?;
        let real = self.filtered_sel.get(idx)?;
        self.current_sels().get(*real)
    }

    fn fn_count(&self) -> usize {
        self.filtered_fn.len()
    }

    fn sel_count(&self) -> usize {
        self.filtered_sel.len()
    }
}

pub fn run_tui(
    modules: Vec<ModuleEntry>,
    initial_search: Option<String>,
) -> miette::Result<()> {
    enable_raw_mode().map_err(|e| miette::miette!("Failed to enable raw mode: {e}"))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| miette::miette!("Failed to enter alternate screen: {e}"))?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal =
        Terminal::new(backend).map_err(|e| miette::miette!("Failed to create terminal: {e}"))?;

    let mut app = App::new(modules, initial_search);
    let result = run_event_loop(&mut terminal, &mut app);

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
                    KeyCode::Tab => app.switch_content_tab(),
                    // Focus switching between module pane and item pane
                    KeyCode::Char('h') | KeyCode::Left if app.multi_module => {
                        app.focused = FocusedPane::Modules;
                    }
                    KeyCode::Char('l') | KeyCode::Right if app.multi_module => {
                        app.focused = FocusedPane::Items;
                    }
                    KeyCode::Enter if app.focused == FocusedPane::Modules => {
                        app.focused = FocusedPane::Items;
                    }
                    KeyCode::Up | KeyCode::Char('k') => match app.focused {
                        FocusedPane::Modules => app.navigate_module(-1),
                        FocusedPane::Items => app.navigate_item(-1),
                    },
                    KeyCode::Down | KeyCode::Char('j') => match app.focused {
                        FocusedPane::Modules => app.navigate_module(1),
                        FocusedPane::Items => app.navigate_item(1),
                    },
                    KeyCode::PageUp => match app.focused {
                        FocusedPane::Modules => app.navigate_module(-10),
                        FocusedPane::Items => app.navigate_item(-10),
                    },
                    KeyCode::PageDown => match app.focused {
                        FocusedPane::Modules => app.navigate_module(10),
                        FocusedPane::Items => app.navigate_item(10),
                    },
                    _ => {}
                },
            }
        }
    }
}

fn focused_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style)
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    // Tab bar
    let fn_label = format!(" Functions ({}) ", app.fn_count());
    let sel_label = format!(" Selectors ({}) ", app.sel_count());
    let tabs = Tabs::new(vec![Line::from(fn_label), Line::from(sel_label)])
        .block(Block::default().borders(Borders::ALL).title(" mq docs "))
        .select(match app.content_tab {
            ContentTab::Functions => 0,
            ContentTab::Selectors => 1,
        })
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, outer[0]);

    // Main area
    if app.multi_module {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(30),
                Constraint::Percentage(50),
            ])
            .split(outer[1]);
        draw_module_list(f, app, body[0]);
        draw_item_list(f, app, body[1]);
        draw_detail(f, app, body[2]);
    } else {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(outer[1]);
        draw_item_list(f, app, body[0]);
        draw_detail(f, app, body[1]);
    }

    // Help bar
    let help_text = if app.input_mode == InputMode::Searching {
        Line::from(vec![
            Span::styled(
                " Search: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(app.search_query.as_str(), Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::Cyan)),
            Span::styled(
                "  Esc: cancel  Enter: confirm",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else {
        let hints = if app.multi_module {
            " /: search  Tab: switch  h/l ←/→: pane  j/k ↑/↓: navigate  Enter: select  q: quit"
        } else {
            " /: search  Tab: switch  j/k ↑/↓: navigate  PgUp/PgDn: page  q: quit"
        };
        Line::from(Span::styled(hints, Style::default().fg(Color::DarkGray)))
    };
    f.render_widget(Paragraph::new(help_text), outer[2]);
}

fn draw_module_list(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused == FocusedPane::Modules;
    let items: Vec<ListItem> = app
        .module_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let fn_count = app.module_functions[i].len();
            let label = format!("{name} ({fn_count})");
            ListItem::new(label)
        })
        .collect();

    let list = List::new(items)
        .block(focused_block(" Modules ", focused))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut app.module_list_state.clone());
}

fn draw_item_list(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.multi_module || app.focused == FocusedPane::Items;

    match app.content_tab {
        ContentTab::Functions => {
            let items: Vec<ListItem> = app
                .filtered_fn
                .iter()
                .map(|&i| {
                    let e = &app.current_fns()[i];
                    if is_deprecated(e) {
                        ListItem::new(Line::from(Span::styled(
                            e.name.as_str(),
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::DIM),
                        )))
                    } else {
                        ListItem::new(e.name.as_str())
                    }
                })
                .collect();

            let list = List::new(items)
                .block(focused_block(" Functions ", focused))
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");
            f.render_stateful_widget(list, area, &mut app.item_list_state.clone());
        }
        ContentTab::Selectors => {
            let items: Vec<ListItem> = app
                .filtered_sel
                .iter()
                .map(|&i| ListItem::new(app.current_sels()[i].name.as_str()))
                .collect();

            let list = List::new(items)
                .block(focused_block(" Selectors ", focused))
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");
            f.render_stateful_widget(list, area, &mut app.item_list_state.clone());
        }
    }
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Detail ")
        .border_style(Style::default().fg(Color::DarkGray));

    let entry = match app.content_tab {
        ContentTab::Functions => app.current_fn_entry(),
        ContentTab::Selectors => app.current_sel_entry(),
    };

    match entry {
        Some(e) => {
            let label = if e.kind == "selector" { "Selector:    " } else { "Function:    " };
            f.render_widget(
                Paragraph::new(entry_detail_lines(e, label))
                    .block(block)
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        None => f.render_widget(Paragraph::new("No entry selected.").block(block), area),
    }
}

fn entry_detail_lines<'a>(e: &'a DocEntry, name_label: &'static str) -> Vec<Line<'a>> {
    let label_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = vec![Line::from(vec![
        Span::styled(name_label, label_style),
        Span::styled(e.name.as_str(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ])];

    if is_deprecated(e) {
        lines.push(Line::from(Span::styled(
            "             ⚠ Deprecated",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Description", label_style)));
    if e.description.is_empty() {
        lines.push(Line::from("  —"));
    } else {
        for l in e.description.lines() {
            lines.push(Line::from(format!("  {l}")));
        }
    }

    lines.push(Line::from(""));
    let params = e.params.iter().map(|p| format!("{}: {}", p.name, p.type_name)).collect::<Vec<_>>().join(", ");
    lines.push(Line::from(vec![
        Span::styled("Parameters:  ", label_style),
        Span::raw(if params.is_empty() { "—".to_string() } else { params }),
    ]));
    lines.push(Line::from(vec![Span::styled("Returns:     ", label_style), Span::raw(e.returns.as_str())]));

    if let Some(module) = &e.related_module {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Module:      ", label_style),
            Span::raw(format!("import \"{module}\" | {module}::{}(...)", e.name)),
        ]));
    }

    if let Some(capability) = &e.capability {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Capability:  ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(format!("requires `{capability}` (not available via the hosted Web API/playground)")),
        ]));
    }

    if !e.examples.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Examples", label_style)));
        for example in &e.examples {
            for l in example.code.lines() {
                lines.push(Line::from(Span::styled(format!("  {l}"), Style::default().fg(Color::Green))));
            }
            for (i, l) in example.expected.lines().enumerate() {
                let prefix = if i == 0 { "  #=> " } else { "      " };
                lines.push(Line::from(Span::styled(
                    format!("{prefix}{l}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    lines
}
