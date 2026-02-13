//! TUI dashboard for `codex team dashboard`.
//!
//! Three-pane layout rendered with ratatui + crossterm:
//!   ┌─────────────────────────────────────┐
//!   │ Agent Table                         │
//!   ├──────────────────────┬──────────────┤
//!   │ Task Board           │ Budget/Stats │
//!   └──────────────────────┴──────────────┘
//!
//! Keybindings: q=quit, ↑/↓=select agent, k=kill agent.

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct DashboardState {
    agents: Vec<AgentRow>,
    tasks: Vec<TaskRow>,
    total_cost: f64,
    budget_limit: f64,
    daemon_pid: Option<u32>,
    selected_agent: usize,
}

#[derive(Debug, Clone)]
struct AgentRow {
    id: String,
    status: String,
    task: String,
    sessions: u64,
    cost: f64,
    pid: u32,
}

#[derive(Debug, Clone)]
struct TaskRow {
    title: String,
    priority: String,
    status: String,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn run_dashboard(refresh_secs: u64) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = dashboard_loop(&mut terminal, Duration::from_secs(refresh_secs)).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async fn dashboard_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    refresh: Duration,
) -> anyhow::Result<()> {
    let mut state = DashboardState::default();
    let mut table_state = TableState::default();
    table_state.select(Some(0));

    loop {
        // Refresh data
        refresh_state(&mut state);
        state.selected_agent = state
            .selected_agent
            .min(state.agents.len().saturating_sub(1));
        table_state.select(Some(state.selected_agent));

        // Draw
        terminal.draw(|f| draw_ui(f, &state, &mut table_state))?;

        // Poll events with timeout = refresh interval
        if event::poll(refresh)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() && key.code == KeyCode::Up => {
                        state.selected_agent = state.selected_agent.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !state.agents.is_empty() {
                            state.selected_agent =
                                (state.selected_agent + 1).min(state.agents.len() - 1);
                        }
                    }
                    KeyCode::Char('K') => {
                        // Kill selected agent
                        if let Some(agent) = state.agents.get(state.selected_agent) {
                            unsafe { libc::kill(agent.pid as i32, libc::SIGKILL); }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Data refresh
// ---------------------------------------------------------------------------

fn refresh_state(state: &mut DashboardState) {
    state.agents.clear();
    state.tasks.clear();
    state.total_cost = 0.0;
    state.budget_limit = 0.0;
    state.daemon_pid = None;

    // Read daemon PID
    let pid_path = PathBuf::from(".codex-team/state/daemon.pid");
    if let Ok(s) = std::fs::read_to_string(&pid_path) {
        state.daemon_pid = s.trim().parse().ok();
    }

    // Read agent heartbeats
    let agents_dir = PathBuf::from(".codex-team/state/agents");
    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(hb) = serde_json::from_str::<codex_core::team::git_coordinator::AgentHeartbeat>(&content) {
                    state.total_cost += hb.total_cost_usd;
                    state.agents.push(AgentRow {
                        id: hb.agent_id.clone(),
                        status: hb.status.clone(),
                        task: hb.current_task.clone().unwrap_or_else(|| "-".to_string()),
                        sessions: hb.sessions_completed,
                        cost: hb.total_cost_usd,
                        pid: hb.pid,
                    });
                }
            }
        }
    }
    state.agents.sort_by(|a, b| a.id.cmp(&b.id));

    // Read team spec for budget
    let spec_path = PathBuf::from(".codex-team/team_spec.yaml");
    if let Ok(spec) = codex_core::team::team_spec::load_team_spec(&spec_path) {
        state.budget_limit = spec.budget.max_total_usd;
    }

    // Read task board
    let tasks_path = PathBuf::from(".codex-team/TASKS.md");
    if let Ok(content) = std::fs::read_to_string(&tasks_path) {
        let board = codex_core::team::task_board::parse_tasks_markdown(&content);
        for task in &board.tasks {
            let status_str = format!("{:?}", task.status);
            state.tasks.push(TaskRow {
                title: task.title.clone(),
                priority: format!("{:?}", task.priority),
                status: status_str,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// UI rendering
// ---------------------------------------------------------------------------

fn draw_ui(
    frame: &mut ratatui::Frame,
    state: &DashboardState,
    table_state: &mut TableState,
) {
    let size = frame.area();

    // Top: header, Middle: agent table, Bottom: tasks + budget
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(8),    // agent table
            Constraint::Length(12), // bottom pane
        ])
        .split(size);

    // Header
    draw_header(frame, vertical[0], state);

    // Agent table
    draw_agent_table(frame, vertical[1], state, table_state);

    // Bottom: tasks + budget side by side
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(vertical[2]);

    draw_task_board(frame, bottom[0], state);
    draw_budget(frame, bottom[1], state);
}

fn draw_header(frame: &mut ratatui::Frame, area: Rect, state: &DashboardState) {
    let daemon_status = match state.daemon_pid {
        Some(pid) => format!("Daemon PID: {pid}"),
        None => "Daemon: NOT RUNNING".to_string(),
    };
    let header_text = vec![Line::from(vec![
        Span::styled(
            " 🤖 Codex Team Dashboard ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  │  "),
        Span::styled(
            format!("{} agents", state.agents.len()),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  │  "),
        Span::styled(daemon_status, Style::default().fg(Color::Yellow)),
        Span::raw("  │  "),
        Span::styled(
            "q:quit  ↑↓:select  K:kill",
            Style::default().fg(Color::DarkGray),
        ),
    ])];

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));
    let p = Paragraph::new(header_text).block(block);
    frame.render_widget(p, area);
}

fn draw_agent_table(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &DashboardState,
    table_state: &mut TableState,
) {
    let header = Row::new(vec![
        Cell::from("AGENT").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("STATUS").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("TASK").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("SESSIONS").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("COST").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("PID").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ])
    .height(1);

    let rows: Vec<Row> = state
        .agents
        .iter()
        .map(|a| {
            let status_color = match a.status.as_str() {
                "idle" => Color::DarkGray,
                "working" | "coding" => Color::Green,
                "testing" => Color::Yellow,
                "merging" => Color::Magenta,
                _ => Color::White,
            };
            Row::new(vec![
                Cell::from(a.id.clone()),
                Cell::from(a.status.clone()).style(Style::default().fg(status_color)),
                Cell::from(a.task.clone()),
                Cell::from(a.sessions.to_string()),
                Cell::from(format!("${:.2}", a.cost)),
                Cell::from(a.pid.to_string()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Min(20),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(" Agents ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, table_state);
}

fn draw_task_board(frame: &mut ratatui::Frame, area: Rect, state: &DashboardState) {
    let mut lines = Vec::new();
    for t in &state.tasks {
        let status_color = match t.status.as_str() {
            "Pending" => Color::DarkGray,
            "InProgress" => Color::Yellow,
            "Done" => Color::Green,
            "Failed" => Color::Red,
            _ => Color::White,
        };
        let priority_color = match t.priority.as_str() {
            "High" => Color::Red,
            "Medium" => Color::Yellow,
            "Low" => Color::DarkGray,
            _ => Color::White,
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("[{:^10}]", t.status),
                Style::default().fg(status_color),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{:>6}", t.priority),
                Style::default().fg(priority_color),
            ),
            Span::raw("  "),
            Span::raw(&t.title),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No tasks loaded",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let block = Block::default()
        .title(" Task Board ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let p = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    frame.render_widget(p, area);
}

fn draw_budget(frame: &mut ratatui::Frame, area: Rect, state: &DashboardState) {
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(3)])
        .split(area);

    // Budget gauge
    let ratio = if state.budget_limit > 0.0 {
        (state.total_cost / state.budget_limit).min(1.0)
    } else {
        0.0
    };

    let gauge_color = if ratio > 0.9 {
        Color::Red
    } else if ratio > 0.7 {
        Color::Yellow
    } else {
        Color::Green
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Budget ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(gauge_color).bg(Color::Black))
        .ratio(ratio)
        .label(format!(
            "${:.2} / ${:.2}",
            state.total_cost, state.budget_limit
        ));
    frame.render_widget(gauge, inner[0]);

    // Stats
    let total_sessions: u64 = state.agents.iter().map(|a| a.sessions).sum();
    let active = state.agents.iter().filter(|a| a.status != "idle").count();
    let stats_text = vec![
        Line::from(vec![
            Span::styled("Sessions: ", Style::default().fg(Color::DarkGray)),
            Span::styled(total_sessions.to_string(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Active:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(active.to_string(), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Tasks:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(state.tasks.len().to_string(), Style::default().fg(Color::White)),
        ]),
    ];

    let block = Block::default()
        .title(" Stats ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let p = Paragraph::new(stats_text).block(block);
    frame.render_widget(p, inner[1]);
}
