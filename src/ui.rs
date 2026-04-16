//! UI rendering logic for epanel.
//!
//! A TUI panel for managing links and notes.
//!
//! Author: Al Biheiri <al@forgottheaddress.com>

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, CurrentTab, Focus};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let titles: Vec<Line> = vec!["Links", "Notes", "Settings"]
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == app.current_tab as usize {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(Span::styled(format!(" {} ", t), style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(format!(" epanel {} — Tabs (1/2/3) ", crate::app::VERSION)))
        .select(app.current_tab as usize)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, chunks[0]);

    match app.current_tab {
        CurrentTab::Links => draw_links(f, app, chunks[1]),
        CurrentTab::Notes => draw_notes(f, app, chunks[1]),
        CurrentTab::Settings => draw_settings(f, app, chunks[1]),
    }
}

fn draw_links(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(chunks[0]);

    let input_style = if app.focus == Focus::LinksInput {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let input = Paragraph::new(app.links_input.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Link ")
                .border_style(input_style),
        );
    f.render_widget(input, top[0]);

    let btn_style = if app.focus == Focus::LinksButton {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else {
        Style::default().fg(Color::White).bg(Color::DarkGray)
    };
    let btn_border_style = if app.focus == Focus::LinksButton {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let btn = Paragraph::new(" Add ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(btn_border_style),
        )
        .style(btn_style)
        .alignment(Alignment::Center);
    f.render_widget(btn, top[1]);

    let items: Vec<ListItem> = app
        .links_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if app.focus == Focus::LinksList && app.links_selected == Some(i) {
                Style::default().bg(Color::Yellow).fg(Color::Black)
            } else {
                Style::default()
            };
            ListItem::new(Text::from(item.clone())).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Saved Links "));
    f.render_widget(list, chunks[1]);

    if app.focus == Focus::LinksInput {
        let x = top[0].x + app.links_input.len() as u16 + 1;
        let y = top[0].y + 1;
        f.set_cursor_position(Position::new(x, y));
    }
}

fn draw_notes(f: &mut Frame, app: &App, area: Rect) {
    let para = Paragraph::new(app.notes_text.clone())
        .block(Block::default().borders(Borders::ALL).title(" Notes "));
    f.render_widget(para, area);

    let lines: Vec<&str> = app.notes_text.split('\n').collect();
    let cy = if app.notes_text.ends_with('\n') {
        lines.len()
    } else {
        lines.len().saturating_sub(1)
    } as u16;
    let cx = if app.notes_text.ends_with('\n') {
        0
    } else {
        lines.last().map(|l| l.len()).unwrap_or(0)
    } as u16;

    f.set_cursor_position(Position::new(area.x + cx + 1, area.y + cy + 1));
}

fn draw_settings(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let st1 = if app.focus == Focus::SettingsLinksPath {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let p1 = Paragraph::new(app.settings_links_path.clone()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Links Save Path ")
            .border_style(st1),
    );
    f.render_widget(p1, chunks[0]);

    let st2 = if app.focus == Focus::SettingsNotesPath {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let p2 = Paragraph::new(app.settings_notes_path.clone()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Notes Save Path ")
            .border_style(st2),
    );
    f.render_widget(p2, chunks[1]);

    let btn_style = if app.focus == Focus::SettingsSaveButton {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else {
        Style::default().fg(Color::White).bg(Color::DarkGray)
    };
    let btn_border_style = if app.focus == Focus::SettingsSaveButton {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let btn = Paragraph::new(" Save Settings ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(btn_border_style),
        )
        .style(btn_style)
        .alignment(Alignment::Center);
    f.render_widget(btn, chunks[2]);

    let hint = Paragraph::new("Tab/Shift+Tab to navigate, Enter to activate. Esc to quit.")
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(hint, chunks[3]);

    match app.focus {
        Focus::SettingsLinksPath => {
            let x = chunks[0].x + app.settings_links_path.len() as u16 + 1;
            let y = chunks[0].y + 1;
            f.set_cursor_position(Position::new(x, y));
        }
        Focus::SettingsNotesPath => {
            let x = chunks[1].x + app.settings_notes_path.len() as u16 + 1;
            let y = chunks[1].y + 1;
            f.set_cursor_position(Position::new(x, y));
        }
        _ => {}
    }
}
