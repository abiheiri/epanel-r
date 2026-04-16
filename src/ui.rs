//! UI rendering logic for epanel.
//!
//! A TUI panel for managing links and notes.
//!
//! Author: Al Biheiri <al@forgottheaddress.com>

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, CurrentTab, FlatItemKind, Focus, Popup};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    draw_tabs(f, app, chunks[0]);
    match app.current_tab {
        CurrentTab::Links => draw_links(f, app, chunks[1]),
        CurrentTab::Notes => draw_notes(f, app, chunks[1]),
        CurrentTab::Settings => draw_settings(f, app, chunks[1]),
    }
    draw_status(f, app, chunks[2]);

    if let Some(ref popup) = app.popup {
        draw_popup(f, app, popup, area);
    }
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" epanel {} — Tabs (F1/F2/F3) ", crate::app::VERSION)),
        )
        .select(app.current_tab as usize)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, area);
}

fn draw_links(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let input_style = if app.focus == Focus::SearchInput {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let input = Paragraph::new(app.search_input.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Search / Add ")
                .border_style(input_style),
        );
    f.render_widget(input, chunks[0]);

    let items: Vec<ListItem> = app
        .flat_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut prefix = String::new();
            for _ in 0..item.depth {
                prefix.push_str("  ");
            }
            match item.kind {
                FlatItemKind::Folder => {
                    let icon = if item.is_collapsed { "> " } else { "v " };
                    prefix.push_str(icon);
                }
                FlatItemKind::Entry => {
                    prefix.push_str("• ");
                }
            }
            let text = format!("{}{}", prefix, item.name);

            let mut style = Style::default();
            let is_cursor = app.links_cursor == Some(i);
            let is_selected = app.selected_item_ids.contains(&item.id);

            if is_cursor && app.focus == Focus::LinksList {
                style = style.bg(Color::Yellow).fg(Color::Black);
            } else if is_selected {
                style = style.bg(Color::DarkGray);
            }

            ListItem::new(Text::from(text)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Links "));
    f.render_widget(list, chunks[1]);

    if app.focus == Focus::SearchInput {
        let x = chunks[0].x + app.search_input.chars().count() as u16 + 1;
        let y = chunks[0].y + 1;
        f.set_cursor_position(Position::new(x, y));
    }
}

fn draw_notes(f: &mut Frame, app: &App, area: Rect) {
    let para = Paragraph::new(app.notes_text.clone())
        .block(Block::default().borders(Borders::ALL).title(" Notes "));
    f.render_widget(para, area);

    let (cx, cy) = app.notes_cursor;
    f.set_cursor_position(Position::new(
        area.x + cx as u16 + 1,
        area.y + cy as u16 + 1,
    ));
}

fn draw_settings(f: &mut Frame, app: &App, area: Rect) {
    #[cfg(target_os = "macos")]
    let has_safari = true;
    #[cfg(not(target_os = "macos"))]
    let has_safari = false;

    let mut constraints = vec![
        Constraint::Length(3),
        Constraint::Length(3),
    ];
    if has_safari {
        constraints.push(Constraint::Length(3));
    }
    constraints.push(Constraint::Length(3));
    constraints.push(Constraint::Min(0));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
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

    #[cfg(target_os = "macos")]
    let save_idx = {
        let sync_label = if app.safari_sync_enabled {
            "[X] Safari Sync"
        } else {
            "[ ] Safari Sync"
        };
        let sync_style = if app.focus == Focus::SettingsSafariSync {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        };
        let sync_border = if app.focus == Focus::SettingsSafariSync {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let sync = Paragraph::new(sync_label)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(sync_border),
            )
            .style(sync_style)
            .alignment(Alignment::Center);
        f.render_widget(sync, chunks[2]);
        3
    };
    #[cfg(not(target_os = "macos"))]
    let save_idx = 2;

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
    f.render_widget(btn, chunks[save_idx]);

    let hint_idx = save_idx + 1;
    let hint = Paragraph::new("Tab to navigate • Enter to save • Ctrl+C to quit")
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(hint, chunks[hint_idx]);

    match app.focus {
        Focus::SettingsLinksPath => {
            let x = chunks[0].x + app.settings_links_path.chars().count() as u16 + 1;
            let y = chunks[0].y + 1;
            f.set_cursor_position(Position::new(x, y));
        }
        Focus::SettingsNotesPath => {
            let x = chunks[1].x + app.settings_notes_path.chars().count() as u16 + 1;
            let y = chunks[1].y + 1;
            f.set_cursor_position(Position::new(x, y));
        }
        _ => {}
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.current_tab {
        CurrentTab::Links => "↑↓ navigate • Enter open/add • Space select • n new • d delete • m move • r rename • e export • i import • ? help • Esc clear search • F1/F2/F3 tabs • Ctrl+C quit",
        CurrentTab::Notes => "Type to edit • ? help • F1/F2/F3 tabs • Ctrl+C quit",
        CurrentTab::Settings => "Tab navigate • Enter save • ? help • F1/F2/F3 tabs • Ctrl+C quit",
    };
    let text = if let Some(ref msg) = app.message {
        format!("{} | {}", msg, hints)
    } else {
        hints.to_string()
    };
    let para = Paragraph::new(text)
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Left);
    f.render_widget(para, area);
}

fn draw_popup(f: &mut Frame, app: &App, popup: &Popup, area: Rect) {
    let popup_area = centered_rect(60, 50, area);
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black));
    f.render_widget(block.clone(), popup_area);

    let inner = popup_area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });

    match popup {
        Popup::AddEntry { text, selected_folder } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let para = Paragraph::new(text.clone())
                .block(Block::default().borders(Borders::ALL).title(" Entry "));
            f.render_widget(para, chunks[0]);

            let folders = app.flattened_folder_choices(None);
            let items: Vec<ListItem> = folders
                .iter()
                .map(|(id, name, depth)| {
                    let indent = "  ".repeat(*depth);
                    let label = format!("{}{}", indent, name);
                    let style = if *id == *selected_folder {
                        Style::default().bg(Color::Yellow).fg(Color::Black)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Text::from(label)).style(style)
                })
                .collect();
            let list =
                List::new(items).block(Block::default().borders(Borders::ALL).title(" Folder "));
            f.render_widget(list, chunks[1]);

            let hint = Paragraph::new("↑↓ select • Enter confirm • Esc cancel")
                .alignment(Alignment::Center);
            f.render_widget(hint, chunks[2]);
        }
        Popup::NewFolder { name, selected_parent } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let para = Paragraph::new(name.clone())
                .block(Block::default().borders(Borders::ALL).title(" Folder Name "));
            f.render_widget(para, chunks[0]);

            let folders = app.flattened_folder_choices(None);
            let items: Vec<ListItem> = folders
                .iter()
                .map(|(id, fname, depth)| {
                    let indent = "  ".repeat(*depth);
                    let label = format!("{}{}", indent, fname);
                    let style = if *id == *selected_parent {
                        Style::default().bg(Color::Yellow).fg(Color::Black)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Text::from(label)).style(style)
                })
                .collect();
            let list =
                List::new(items).block(Block::default().borders(Borders::ALL).title(" Location "));
            f.render_widget(list, chunks[1]);

            let hint = Paragraph::new("↑↓ select • Enter confirm • Esc cancel")
                .alignment(Alignment::Center);
            f.render_widget(hint, chunks[2]);
        }
        Popup::RenameFolder { name, .. } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Length(1)])
                .split(inner);

            let para = Paragraph::new(name.clone())
                .block(Block::default().borders(Borders::ALL).title(" Rename Folder "));
            f.render_widget(para, chunks[0]);

            let hint = Paragraph::new("Enter confirm • Esc cancel").alignment(Alignment::Center);
            f.render_widget(hint, chunks[1]);
        }
        Popup::MoveItem { selected_folder, .. } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let folders = app.flattened_folder_choices(None);
            let items: Vec<ListItem> = folders
                .iter()
                .map(|(id, name, depth)| {
                    let indent = "  ".repeat(*depth);
                    let label = format!("{}{}", indent, name);
                    let style = if *id == *selected_folder {
                        Style::default().bg(Color::Yellow).fg(Color::Black)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Text::from(label)).style(style)
                })
                .collect();
            let list =
                List::new(items).block(Block::default().borders(Borders::ALL).title(" Move To "));
            f.render_widget(list, chunks[0]);

            let hint = Paragraph::new("↑↓ select • Enter confirm • Esc cancel")
                .alignment(Alignment::Center);
            f.render_widget(hint, chunks[1]);
        }
        Popup::ConfirmDeleteSelected {
            entry_count,
            folder_count,
            subfolder_count,
        } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let mut lines = vec![
                Line::from("Delete selected items?"),
                Line::from(""),
                Line::from(format!("  Folders:       {}", folder_count)),
                Line::from(format!("  Subfolders:    {}", subfolder_count)),
                Line::from(format!("  Entries:       {}", entry_count)),
            ];
            if *folder_count > 0 {
                lines.push(Line::from(""));
                lines.push(Line::from("This will delete all contained items."));
            }
            let para = Paragraph::new(Text::from(lines));
            f.render_widget(para, chunks[0]);

            let hint = Paragraph::new("y/Enter confirm • n/Esc cancel").alignment(Alignment::Center);
            f.render_widget(hint, chunks[1]);
        }
        Popup::Alert { message } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let para = Paragraph::new(message.clone());
            f.render_widget(para, chunks[0]);

            let hint = Paragraph::new("Enter/Esc/Space dismiss").alignment(Alignment::Center);
            f.render_widget(hint, chunks[1]);
        }
        Popup::ExportJSON { path } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Length(1)])
                .split(inner);

            let para = Paragraph::new(path.clone())
                .block(Block::default().borders(Borders::ALL).title(" Export JSON Path "));
            f.render_widget(para, chunks[0]);

            let hint = Paragraph::new("Enter confirm • Esc cancel").alignment(Alignment::Center);
            f.render_widget(hint, chunks[1]);
        }
        Popup::ImportJSON { path } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Length(1)])
                .split(inner);

            let para = Paragraph::new(path.clone())
                .block(Block::default().borders(Borders::ALL).title(" Import JSON Path "));
            f.render_widget(para, chunks[0]);

            let hint = Paragraph::new("Enter confirm • Esc cancel").alignment(Alignment::Center);
            f.render_widget(hint, chunks[1]);
        }
        Popup::ConfirmImportJSON { path } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let lines = vec![
                Line::from("Replace All Data?").style(Style::default().add_modifier(Modifier::BOLD)),
                Line::from(""),
                Line::from(format!("Importing '{}'", path)),
                Line::from(""),
                Line::from("This will replace all existing entries, folders, and notes."),
                Line::from("This cannot be undone."),
            ];
            let para = Paragraph::new(Text::from(lines));
            f.render_widget(para, chunks[0]);

            let hint = Paragraph::new("y/Enter confirm • n/Esc cancel").alignment(Alignment::Center);
            f.render_widget(hint, chunks[1]);
        }
        Popup::Help => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let mut lines: Vec<Line> = vec![];
            for (i, (title, items)) in app.help_sections().iter().enumerate() {
                if i > 0 {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(*title).style(Style::default().add_modifier(Modifier::BOLD)));
                for (key, desc) in items {
                    lines.push(Line::from(format!("  {:<14} {}", key, desc)));
                }
            }
            let para = Paragraph::new(Text::from(lines));
            f.render_widget(para, chunks[0]);

            let hint = Paragraph::new("Enter/Esc/q/Space dismiss").alignment(Alignment::Center);
            f.render_widget(hint, chunks[1]);
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
