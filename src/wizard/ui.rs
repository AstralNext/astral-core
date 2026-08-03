//! TUI 绘制。

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::app::{App, ConfirmKind, Mode};

/// 绘制整帧。
pub fn draw(f: &mut Frame<'_>, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(8),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, root[0], app);
    match app.mode {
        Mode::Menu => draw_menu(f, root[1], app),
        Mode::Install => draw_install(f, root[1], app),
        Mode::Confirm => {
            draw_menu(f, root[1], app);
            draw_confirm(f, app);
        }
        Mode::Result => draw_result_main(f, root[1], app),
    }
    draw_log(f, root[2], app);
    draw_footer(f, root[3], app);
}

fn draw_header(f: &mut Frame<'_>, area: Rect, app: &App) {
    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " Astral Core ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" 本地部署 TUI"),
        ]),
        Line::from(Span::styled(
            format!(" {}", app.status_line),
            Style::default().fg(Color::Gray),
        )),
    ])
    .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(title, area);
}

fn draw_menu(f: &mut Frame<'_>, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .menu_items()
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let selected = i == app.menu_idx;
            let marker = if selected { "> " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker}{}", it.title()), style),
                Span::styled(
                    format!("  - {}", it.subtitle()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 主菜单 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(list, area);
}

fn draw_install(f: &mut Frame<'_>, area: Rect, app: &App) {
    let fields = app.install_fields();
    let items: Vec<ListItem> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let selected = i == app.install_focus;
            let marker = if selected { "> " } else { "  " };
            let label = App::field_label(*f);
            let value = app.field_value(*f);
            let display = if value.is_empty() {
                "(空)".to_string()
            } else {
                value
            };
            let style = if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker}{label:<10}"), style),
                Span::raw(" "),
                Span::styled(display, Style::default().fg(Color::White)),
            ]))
        })
        .collect();

    let focus = app.focused_field();
    let hint = App::field_hint(focus);
    let list = List::new(items).block(
        Block::default()
            .title(format!(" 安装配置 | {hint} | Enter 下一步 "))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(list, area);
}

fn draw_result_main(f: &mut Frame<'_>, area: Rect, app: &App) {
    let text = if app.log.len() <= 12 {
        app.log.join("\n")
    } else {
        app.log[app.log.len() - 12..].join("\n")
    };
    let p = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(" 结果（Enter 返回菜单） ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );
    f.render_widget(p, area);
}

fn draw_log(f: &mut Frame<'_>, area: Rect, app: &App) {
    let lines: Vec<Line> = app
        .log
        .iter()
        .rev()
        .take(area.height.saturating_sub(2) as usize)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|s| Line::from(Span::raw(s.as_str())))
        .collect();
    let p = Paragraph::new(lines).block(
        Block::default()
            .title(" 日志 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(p, area);
}

fn draw_footer(f: &mut Frame<'_>, area: Rect, app: &App) {
    let help = match app.mode {
        Mode::Menu => "Up/Down 选择  Enter 打开  q 退出",
        Mode::Install => "Up/Down/Tab 字段  输入编辑  Space 切换装后启动  Enter 确认  Esc 返回",
        Mode::Confirm => "Left/Right 切换  Y/Enter 确定  N/Esc 取消",
        Mode::Result => "Enter / Esc 返回菜单",
    };
    f.render_widget(
        Paragraph::new(Span::styled(help, Style::default().fg(Color::DarkGray))),
        area,
    );
}

fn draw_confirm(f: &mut Frame<'_>, app: &App) {
    let area = centered_rect(60, 7, f.area());
    f.render_widget(Clear, area);

    let kind = app.confirm.unwrap_or(ConfirmKind::Install);
    let title = match kind {
        ConfirmKind::Install => format!(
            "确认安装？\n服务 dev.astral.core-{}\n监听 {}",
            app.name, app.listen
        ),
        ConfirmKind::Uninstall => format!("确认卸载 dev.astral.core-{} ？", app.name),
        ConfirmKind::Update => "确认用当前程序执行 service update？".into(),
        ConfirmKind::Rollback => "确认回滚到上一版本？".into(),
    };

    let yes = if app.confirm_yes {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let no = if !app.confirm_yes {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };

    let body = Paragraph::new(vec![
        Line::from(title),
        Line::from(""),
        Line::from(vec![
            Span::styled("  是  ", yes),
            Span::raw("   "),
            Span::styled("  否  ", no),
        ]),
    ])
    .block(
        Block::default()
            .title(" 确认 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)),
    );
    f.render_widget(body, area);
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}
