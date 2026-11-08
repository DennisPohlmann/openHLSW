//! Drawing. Everything sits where it is in the GUI: menu bar, server list, details on the
//! left, players and rules on the right, the tabs at the bottom, the status bar; below that
//! the function key bar of htop / Midnight Commander. Colors like Midnight Commander.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, Paragraph, Row, Sparkline, Table, TableState,
};

use crate::app::{
    App, Confirm, Field, Input, LineKind, MENUS, Mode, PLAYER_SORT, Pane, PlayerSort, PromptKind,
    SERVER_SORT, Server, SortKey, State, TABS, Tab,
};
use crate::net::RconState;

const BAR_BG: Color = Color::Cyan;
const ACCENT: Color = Color::Yellow;

fn bar() -> Style {
    Style::new().fg(Color::Black).bg(BAR_BG)
}

fn header() -> Style {
    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
}

fn selected(focused: bool) -> Style {
    if focused {
        Style::new().fg(Color::Black).bg(BAR_BG)
    } else {
        Style::new().bg(Color::DarkGray)
    }
}

/// A panel frame; the focused one has a double border and a highlighted title, like the
/// active panel in Midnight Commander.
fn panel(title: String, focused: bool) -> Block<'static> {
    let block = Block::bordered().title(Span::styled(
        format!(" {title} "),
        if focused {
            bar().bold()
        } else {
            Style::new().bold()
        },
    ));
    if focused {
        block
            .border_type(BorderType::Double)
            .border_style(Style::new().fg(Color::White).bold())
    } else {
        block.border_style(Style::new().fg(Color::Gray))
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let avail = area.height.saturating_sub(5);
    let list_h = (avail * 34 / 100).max(6);
    let tabs_h = (avail * 27 / 100).max(7);
    let [menu, top, list, middle, tabs, labels, status, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(list_h),
        Constraint::Min(6),
        Constraint::Length(tabs_h),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(middle);
    let [players, rules] =
        Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(right);

    draw_menu_bar(f, app, menu);
    draw_list_header(f, app, top);
    draw_servers(f, app, list);
    draw_details(f, app, left);
    draw_players(f, app, players);
    draw_rules(f, app, rules);
    draw_tab(f, app, tabs);
    draw_tab_labels(f, app, labels);
    draw_status(f, app, status);
    draw_keys(f, app, keys);
    match app.mode {
        Mode::Menu { menu, item } => draw_menu(f, app, menu, item),
        Mode::Sort(i) => {
            let body = if app.focus == Pane::Players {
                app.areas.players
            } else {
                app.areas.servers
            };
            draw_sort_menu(f, app, i, body);
        }
        Mode::Help => draw_help(f, area),
        Mode::About => draw_about(f, area),
        _ => {}
    }
}

// --------------------------------------------------------------------------
// Top
// --------------------------------------------------------------------------

fn draw_menu_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let open = match app.mode {
        Mode::Menu { menu, .. } => Some(menu),
        _ => None,
    };
    let mut spans = vec![Span::styled(" ", bar())];
    let mut x = area.x + 1;
    app.areas.menu_labels.clear();
    for (i, (name, _)) in MENUS.iter().enumerate() {
        let text = format!(" {name} ");
        let w = text.chars().count() as u16;
        app.areas.menu_labels.push(Rect::new(x, area.y, w, 1));
        x += w + 1;
        let style = if open == Some(i) {
            Style::new().fg(Color::White).bg(Color::Black)
        } else {
            bar()
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::styled(" ", bar()));
    }
    let version = format!("openHLSW {} ", env!("CARGO_PKG_VERSION"));
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let pad = usize::from(area.width).saturating_sub(used + version.len());
    spans.push(Span::styled(" ".repeat(pad), bar()));
    spans.push(Span::styled(version, bar()));
    f.render_widget(Paragraph::new(Line::from(spans)).style(bar()), area);
}

fn draw_list_header(f: &mut Frame, app: &App, area: Rect) {
    let shown = app.visible().len();
    let mut left = vec![
        Span::styled(" Server list: ", Style::new().fg(Color::Gray)),
        Span::styled(app.store.list_name(), Style::new().bold()),
    ];
    if !app.filter.is_empty() {
        left.push(Span::styled("   Filter: ", Style::new().fg(Color::Gray)));
        left.push(Span::styled(
            app.filter.clone(),
            Style::new().fg(ACCENT).bold(),
        ));
    }
    if app.hide_empty {
        left.push(Span::styled(
            "   [empty hidden]",
            Style::new().fg(Color::Gray),
        ));
    }
    let right = format!(
        "refresh {}s ({}s)   {shown} of {} servers ",
        app.delay.as_secs_f64(),
        app.next_refresh_in(),
        app.servers.len()
    );
    f.render_widget(Paragraph::new(Line::from(left)), area);
    f.render_widget(Paragraph::new(right).right_aligned().fg(Color::Gray), area);
}

fn status_symbol(s: &Server) -> Span<'static> {
    match s.state {
        State::Online => Span::styled("●", Style::new().fg(Color::Green)),
        State::Offline(_) => Span::styled("●", Style::new().fg(Color::Red)),
        State::Waiting => Span::styled("○", Style::new().fg(Color::DarkGray)),
    }
}

fn sort_title<K: PartialEq>(
    name: &str,
    key: Option<K>,
    current: K,
    reverse: bool,
) -> Cell<'static> {
    if key.is_some_and(|k| k == current) {
        Cell::from(format!("{name} {}", if reverse { "▼" } else { "▲" }))
            .style(header().fg(Color::White).add_modifier(Modifier::UNDERLINED))
    } else {
        Cell::from(name.to_string()).style(header())
    }
}

fn draw_servers(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Pane::Servers;
    let block = panel(
        format!("Servers ({}/{} online)", online(app), app.servers.len()),
        focused,
    );
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width;
    // The GUI's columns, as many as fit.
    let mut cols: Vec<(&str, Option<SortKey>, Constraint)> = vec![
        ("#", Some(SortKey::List), Constraint::Length(4)),
        ("Addr", Some(SortKey::Address), Constraint::Length(21)),
        ("Name", Some(SortKey::Name), Constraint::Min(16)),
    ];
    if w >= 100 {
        cols.push(("Game", Some(SortKey::Game), Constraint::Length(20)));
    }
    cols.push(("Map", Some(SortKey::Map), Constraint::Length(18)));
    cols.push(("Players", Some(SortKey::Players), Constraint::Length(8)));
    cols.push(("Ping", Some(SortKey::Ping), Constraint::Length(10)));
    if w >= 130 {
        cols.push(("Version", None, Constraint::Length(12)));
    }
    if w >= 150 {
        cols.push(("AddOn", None, Constraint::Length(24)));
    }
    if w >= 118 {
        cols.push(("Anti Cheat", None, Constraint::Length(10)));
    }
    if w >= 170 {
        cols.push(("Comment", None, Constraint::Length(20)));
    }
    let head = Row::new(
        cols.iter()
            .map(|(n, k, _)| sort_title(n, *k, app.sort, app.reverse)),
    );
    let rows: Vec<Row> = app
        .visible()
        .iter()
        .map(|s| {
            let (p, m) = s.counts();
            let info = s.info.as_ref();
            let players_style = if m > 0 && p >= m {
                Style::new().fg(Color::Red)
            } else if p > 0 {
                Style::new().fg(Color::Green)
            } else {
                Style::new()
            };
            let ping = match (s.ping_ms, s.ping_avg()) {
                (Some(p), Some(a)) => format!("{p}, Ø{a:.1}"),
                _ => String::new(),
            };
            let mut cells = Vec::new();
            for (name, _, _) in &cols {
                cells.push(match *name {
                    "#" => Cell::from(Line::from(vec![
                        status_symbol(s),
                        Span::raw(format!("{:>3}", app.index_of(s.id))),
                    ])),
                    "Addr" => Cell::from(s.address_text.clone()),
                    "Name" => Cell::from(s.name().to_string()),
                    "Game" => Cell::from(s.game_name()).fg(Color::LightRed),
                    "Map" => Cell::from(s.map().to_string()).fg(Color::LightRed),
                    "Players" => Cell::from(if info.is_some() {
                        format!("{p} / {m}")
                    } else {
                        String::new()
                    })
                    .style(players_style),
                    "Ping" => Cell::from(ping.clone()),
                    "Version" => Cell::from(info.map_or(String::new(), |i| i.version.clone())),
                    "AddOn" => Cell::from(s.addons()),
                    "Anti Cheat" => Cell::from(if info.is_some_and(|i| i.vac) {
                        "VAC"
                    } else {
                        ""
                    }),
                    "Comment" => Cell::from(s.comment.clone()).fg(Color::Gray),
                    _ => Cell::from(""),
                });
            }
            let row = Row::new(cells);
            if matches!(s.state, State::Offline(_)) {
                row.style(Style::new().fg(Color::DarkGray))
            } else {
                row
            }
        })
        .collect();
    let empty = rows.is_empty();
    let table = Table::new(rows, cols.iter().map(|(_, _, c)| *c))
        .header(head)
        .column_spacing(1)
        .row_highlight_style(selected(focused));
    let mut state = TableState::default()
        .with_selected(Some(app.selected))
        .with_offset(app.areas.servers_offset);
    f.render_stateful_widget(table, inner, &mut state);
    app.areas.servers = area;
    app.areas.servers_offset = state.offset();
    if empty {
        let text = if app.servers.is_empty() {
            "No servers yet: F7 adds one (IP:port)."
        } else {
            "No server matches the filter (Esc clears it)."
        };
        hint(f, inner, text);
    }
}

fn online(app: &App) -> usize {
    app.servers
        .iter()
        .filter(|s| s.state == State::Online)
        .count()
}

fn hint(f: &mut Frame, area: Rect, text: &str) {
    if area.height < 3 {
        return;
    }
    let a = Rect {
        y: area.y + 2,
        height: 1,
        ..area
    };
    f.render_widget(
        Paragraph::new(text.to_string()).fg(Color::Gray).centered(),
        a,
    );
}

// --------------------------------------------------------------------------
// Middle: details, players, rules
// --------------------------------------------------------------------------

fn ago(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s} s ago"),
        60..3600 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let block = panel("Details".into(), false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(s) = app.current() else {
        return hint(f, inner, "No server selected.");
    };
    let mut y = inner.y;
    let name = if s.name().is_empty() {
        &s.address_text
    } else {
        s.name()
    };
    f.render_widget(
        Paragraph::new(Span::styled(name.to_string(), Style::new().bold())),
        Rect {
            y,
            height: 1,
            ..inner
        },
    );
    y += 1;
    // The ping graph of the GUI.
    let graph_h: u16 = if inner.height >= 14 { 3 } else { 0 };
    if graph_h > 0 {
        let width = usize::from(inner.width);
        let data: Vec<u64> = s
            .pings
            .iter()
            .rev()
            .take(width)
            .rev()
            .map(|&p| u64::from(p.max(1)))
            .collect();
        let max = data.iter().copied().max().unwrap_or(1);
        f.render_widget(
            Sparkline::default()
                .data(&data)
                .max(max * 3 / 2 + 1)
                .style(Style::new().fg(Color::Green))
                .block(Block::new().style(Style::new().bg(Color::Black))),
            Rect {
                y,
                height: graph_h,
                ..inner
            },
        );
        y += graph_h;
    }
    let mut rows: Vec<(&str, Line)> = Vec::new();
    let plain = |t: String| Line::from(t);
    rows.push(("Address", plain(s.address_text.clone())));
    if let Some(p) = s.ping_ms {
        let avg = s
            .ping_avg()
            .map_or(String::new(), |a| format!(" (Ø {a:.1} ms)"));
        rows.push(("Ping", plain(format!("{p} ms{avg}"))));
    }
    if let State::Offline(e) = &s.state {
        rows.push((
            "Error",
            Line::styled(e.clone(), Style::new().fg(Color::Red)),
        ));
    }
    if let Some(i) = &s.info {
        let app_id = i.real_app_id();
        let game = if app_id > 0 {
            format!("{} ({}, AppID {app_id})", s.game_name(), i.folder)
        } else {
            s.game_name()
        };
        rows.push(("Game", plain(game)));
        rows.push((
            "Players",
            Line::styled(
                format!("{} / {}", i.players, i.max_players),
                Style::new().fg(Color::Green),
            ),
        ));
        if i.bots > 0 {
            rows.push(("Bots", plain(i.bots.to_string())));
        }
        rows.push(("Map", plain(i.map.clone())));
        if let Some(t) = s.rule("mp_timelimit") {
            rows.push(("Limits", plain(format!("{t} min"))));
        }
        if let Some(ff) = s.rule("mp_friendlyfire") {
            rows.push((
                "Friendly Fire",
                plain(if ff == "0" { "No" } else { "Yes" }.into()),
            ));
        }
        rows.push((
            "Server Type",
            plain(format!("{}, {}", i.server_type_name(), i.os_name())),
        ));
        rows.push(("Version", plain(i.version.clone())));
        let addons = s.addons();
        if !addons.is_empty() {
            rows.push(("AddOns", plain(addons)));
        }
        rows.push((
            "Anti-Cheat",
            plain(if i.vac { "Valve Anti-Cheat" } else { "None" }.into()),
        ));
        rows.push((
            "Password",
            plain(if i.passworded { "Yes" } else { "No" }.into()),
        ));
        if let Some(id) = i.steam_id {
            rows.push(("Steam ID", plain(id.to_string())));
        }
    }
    rows.push(("Protocol", plain(s.protocol_name().into())));
    if let Some(t) = s.last_seen {
        rows.push(("Last Seen", plain(ago(t.elapsed()))));
    }
    if !s.comment.is_empty() {
        rows.push(("Comment", plain(s.comment.clone())));
    }
    let area = Rect {
        y,
        height: inner.bottom().saturating_sub(y),
        ..inner
    };
    let table = Table::new(
        rows.into_iter().map(|(k, v)| {
            Row::new([
                Cell::from(Line::from(k).right_aligned()).fg(Color::Gray),
                Cell::from(v),
            ])
        }),
        [Constraint::Length(13), Constraint::Min(5)],
    );
    f.render_widget(table, area);
}

fn filter_title(filter: &str, placeholder: &str, total: usize) -> String {
    let f = if filter.is_empty() {
        placeholder.to_string()
    } else {
        format!("\"{filter}\"")
    };
    format!("Filter: {f} │ {total} total")
}

fn draw_players(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Pane::Players;
    let Some(s) = app.current() else {
        f.render_widget(panel("Players".into(), focused), area);
        app.areas.players = area;
        return;
    };
    let players = app.shown_players(s);
    let block = panel(
        filter_title(&app.player_filter, "name", players.len()),
        focused,
    );
    let inner = block.inner(area);
    f.render_widget(block, area);
    let deaths = players.iter().any(|(_, p)| p.deaths.is_some());
    let ping = players.iter().any(|(_, p)| p.ping.is_some());
    let mut cols: Vec<(&str, Option<PlayerSort>, Constraint)> = vec![
        ("#", Some(PlayerSort::Index), Constraint::Length(3)),
        ("Name", Some(PlayerSort::Name), Constraint::Min(14)),
        ("Frags", Some(PlayerSort::Frags), Constraint::Length(7)),
    ];
    if deaths {
        cols.push(("Deaths", None, Constraint::Length(7)));
    }
    cols.push(("Time", Some(PlayerSort::Time), Constraint::Length(9)));
    if ping {
        cols.push(("Ping", None, Constraint::Length(5)));
    }
    let head = Row::new(
        cols.iter()
            .map(|(n, k, _)| sort_title(n, *k, app.player_sort, app.player_reverse)),
    );
    let empty = players.is_empty();
    let rows: Vec<Row> = players
        .iter()
        .map(|(i, p)| {
            let mut cells = vec![
                Cell::from(format!("{i:02}")),
                Cell::from(p.name.clone()),
                Cell::from(p.score.to_string()),
            ];
            if deaths {
                cells.push(Cell::from(
                    p.deaths.map(|d| d.to_string()).unwrap_or_default(),
                ));
            }
            cells.push(Cell::from(duration(p.duration)));
            if ping {
                cells.push(Cell::from(
                    p.ping.map(|d| d.to_string()).unwrap_or_default(),
                ));
            }
            Row::new(cells)
        })
        .collect();
    let mut state = TableState::default()
        .with_selected(focused.then_some(app.player_sel))
        .with_offset(app.areas.players_offset);
    f.render_stateful_widget(
        Table::new(rows, cols.iter().map(|(_, _, c)| *c))
            .header(head)
            .row_highlight_style(selected(focused)),
        inner,
        &mut state,
    );
    app.areas.players = area;
    app.areas.players_offset = state.offset();
    if empty {
        hint(f, inner, "No players (or not answered yet).");
    }
}

fn duration(secs: f32) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn draw_rules(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Pane::Rules;
    let Some(s) = app.current() else {
        f.render_widget(panel("Rules".into(), focused), area);
        app.areas.rules = area;
        return;
    };
    let rules = app.shown_rules(s);
    let block = panel(
        filter_title(&app.rule_filter, "cvar or value", rules.len()),
        focused,
    );
    let inner = block.inner(area);
    f.render_widget(block, area);
    let empty = rules.is_empty();
    let rows: Vec<Row> = rules
        .iter()
        .enumerate()
        .map(|(i, r)| {
            Row::new([
                Cell::from(format!("{:02}", i + 1)),
                Cell::from(r.key.clone()),
                Cell::from(r.value.clone()),
            ])
        })
        .collect();
    let mut state = TableState::default()
        .with_selected(focused.then_some(app.rule_sel))
        .with_offset(app.areas.rules_offset);
    f.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Percentage(45),
                Constraint::Percentage(55),
            ],
        )
        .header(Row::new(["#", "Key", "Value"]).style(header()))
        .row_highlight_style(selected(focused)),
        inner,
        &mut state,
    );
    app.areas.rules = area;
    app.areas.rules_offset = state.offset();
    if empty {
        hint(f, inner, "No rules (or not answered yet).");
    }
}

// --------------------------------------------------------------------------
// Bottom tabs
// --------------------------------------------------------------------------

fn draw_tab_labels(f: &mut Frame, app: &mut App, area: Rect) {
    let mut spans = Vec::new();
    let mut x = area.x;
    app.areas.tab_labels.clear();
    for (i, (tab, name)) in TABS.iter().enumerate() {
        let text = format!(" {name} ");
        let w = text.chars().count() as u16;
        app.areas
            .tab_labels
            .push((Rect::new(x, area.y, w, 1), *tab));
        x += w + 1;
        let style = if *tab == app.tab {
            if app.focus == Pane::Tabs {
                bar().bold()
            } else {
                Style::new().fg(Color::White).bg(Color::DarkGray).bold()
            }
        } else {
            Style::new().fg(Color::Gray)
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::styled(
            if i + 1 < TABS.len() { "│" } else { "" },
            Style::new().fg(Color::DarkGray),
        ));
    }
    spans.push(Span::styled(
        "   Alt+1…5 / Shift+←→ / Esc",
        Style::new().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Pane::Tabs;
    let title = TABS
        .iter()
        .find(|(t, _)| *t == app.tab)
        .map_or("", |(_, n)| n);
    let block = panel(title.into(), focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.areas.tabs = area;
    if app.current().is_none() {
        return hint(f, inner, "No server selected.");
    }
    match app.tab {
        Tab::RconConfig => draw_rcon_config(f, app, inner, focused),
        Tab::Console => draw_console(f, app, inner, focused),
        Tab::Chat => draw_chat(f, app, inner, focused),
        Tab::Bans => draw_bans(f, app, inner, focused),
        Tab::ChangeLevel => draw_maps(f, app, inner, focused),
    }
}

fn rcon_state_text(state: &RconState) -> Span<'static> {
    match state {
        RconState::Unknown => Span::styled("Unknown", Style::new().fg(Color::Gray)),
        RconState::Connected => Span::styled("Connected", Style::new().fg(Color::Green).bold()),
        RconState::BadPassword => Span::styled("Bad password", Style::new().fg(Color::Red).bold()),
        RconState::Failed(e) => Span::styled(e.clone(), Style::new().fg(Color::Red)),
        RconState::Disconnected => Span::styled("Disconnected", Style::new().fg(Color::Gray)),
    }
}

/// `[ Test ]` buttons and `[x] Hide` check boxes; the selected one is highlighted.
fn button(label: &str, on: bool) -> Span<'static> {
    Span::styled(
        format!("[ {label} ]"),
        if on {
            bar().bold()
        } else {
            Style::new().bold()
        },
    )
}

fn checkbox(label: &str, checked: bool, on: bool) -> Span<'static> {
    Span::styled(
        format!("[{}] {label}", if checked { "x" } else { " " }),
        if on { bar() } else { Style::new() },
    )
}

/// The text of an input field, scrolled so the cursor stays visible, and the cursor column.
fn field_text(input: &Input, width: usize, masked: bool) -> (String, usize) {
    let width = width.max(1);
    let skip = input.cursor.saturating_sub(width - 1);
    let text: String = if masked {
        "*".repeat(input.text.chars().count())
    } else {
        input.text.clone()
    };
    let shown: String = text.chars().skip(skip).take(width).collect();
    (shown, input.cursor - skip)
}

fn draw_rcon_config(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let Some(s) = app.current() else { return };
    let [pw_box, log_box, rest] = Layout::horizontal([
        Constraint::Length(36),
        Constraint::Length(26),
        Constraint::Min(0),
    ])
    .areas(area);
    let on = |field: Field| focused && app.field == field;
    let b = Block::bordered()
        .title(" Rcon Password ")
        .border_style(Style::new().fg(Color::DarkGray));
    let inner = b.inner(pw_box);
    f.render_widget(b, pw_box);
    let field_w = 20usize;
    let (shown, cursor) = field_text(&app.password_input, field_w, s.hide_password);
    let field_style = if on(Field::Password) {
        Style::new().fg(Color::White).bg(Color::Blue)
    } else {
        Style::new().fg(Color::White).bg(Color::DarkGray)
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{shown:<field_w$}"), field_style),
            Span::raw(" "),
            button("Test", on(Field::Test)),
        ]),
        Line::from(vec![Span::raw("Status: "), rcon_state_text(&s.rcon.state)]),
        Line::from(button("Disconnect", on(Field::Disconnect))),
        Line::from(vec![
            checkbox("Hide", s.hide_password, on(Field::Hide)),
            Span::raw("   "),
            checkbox("Save", s.save_password, on(Field::Save)),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), inner);
    if focused && app.field == Field::Password && matches!(app.mode, Mode::Normal) {
        f.set_cursor_position(Position::new(inner.x + cursor as u16, inner.y));
    }

    let b = Block::bordered()
        .title(" Receive Server Log ")
        .border_style(Style::new().fg(Color::DarkGray));
    let inner = b.inner(log_box);
    f.render_widget(b, log_box);
    let gray = Style::new().fg(Color::DarkGray);
    f.render_widget(
        Paragraph::new(vec![
            Line::styled("[ Get Log (Internet) ]", gray),
            Line::styled("[ Get Log (LAN) ]", gray),
            Line::styled("not in the console", gray),
            Line::styled("tool yet", gray),
        ]),
        inner,
    );

    let help = vec![
        Line::from(vec![
            Span::styled("Up/Down", Style::new().fg(ACCENT)),
            Span::raw(" choose, "),
            Span::styled("Enter/Space", Style::new().fg(ACCENT)),
            Span::raw(" press or tick"),
        ]),
        Line::from("Typing only edits the password: the server is"),
        Line::from("asked with it on Test or Enter (failed logins"),
        Line::from("get your address banned)."),
        Line::styled(
            "Save stores it in plain text in config.json.",
            Style::new().fg(Color::Gray),
        ),
    ];
    f.render_widget(Paragraph::new(help), rest.inner(Margin::new(1, 0)));
}

fn input_row(f: &mut Frame, app: &App, area: Rect, label: &str, input: &Input, focused: bool) {
    let busy = app.current().is_some_and(|s| s.rcon.pending > 0);
    let prompt = format!("{}{label}", if busy { "… " } else { "" });
    let width = usize::from(area.width).saturating_sub(prompt.chars().count() + 1);
    let (shown, cursor) = field_text(input, width, false);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prompt.clone(), Style::new().fg(ACCENT).bold()),
            Span::raw(shown),
        ]))
        .style(Style::new().bg(Color::Black)),
        area,
    );
    if focused && matches!(app.mode, Mode::Normal) {
        let x = area.x + (prompt.chars().count() + cursor) as u16;
        f.set_cursor_position(Position::new(x.min(area.right().saturating_sub(1)), area.y));
    }
}

fn no_password(s: &Server) -> bool {
    s.password.is_none() && s.rcon.session.is_none()
}

const NO_PASSWORD: &str = "No RCON password yet: enter it in Rcon Config (Alt+1) and press Test.";

fn draw_console(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let Some(s) = app.current() else { return };
    let [out, input] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let h = usize::from(out.height);
    let r = &s.rcon;
    let end = r.console.len().saturating_sub(r.console_scroll);
    let start = end.saturating_sub(h);
    let lines: Vec<Line> = r
        .console
        .range(start..end)
        .map(|(text, kind)| match kind {
            LineKind::Command => Line::styled(text.clone(), Style::new().fg(ACCENT).bold()),
            LineKind::Error => Line::styled(text.clone(), Style::new().fg(Color::Red)),
            LineKind::Output => Line::raw(text.clone()),
        })
        .collect();
    if lines.is_empty() {
        let text = if no_password(s) {
            NO_PASSWORD
        } else {
            "Type a command (status, users …) and press Enter. Up/Down: history, PgUp/PgDn: scroll, Ctrl+L: clear."
        };
        f.render_widget(Paragraph::new(text).fg(Color::Gray), out);
    } else {
        f.render_widget(Paragraph::new(lines), out);
    }
    input_row(f, app, input, "] ", &app.console_input, focused);
}

fn draw_chat(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let Some(s) = app.current() else { return };
    let [out, input] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let h = usize::from(out.height);
    let r = &s.rcon;
    let end = r.chat.len().saturating_sub(r.chat_scroll);
    let start = end.saturating_sub(h);
    let lines: Vec<Line> = r
        .chat
        .range(start..end)
        .map(|c| {
            Line::from(vec![
                Span::styled(format!("{} ", c.time), Style::new().fg(Color::Gray)),
                Span::styled(
                    format!("{}: ", c.name),
                    Style::new().fg(Color::LightBlue).bold(),
                ),
                Span::raw(c.text.clone()),
            ])
        })
        .collect();
    if lines.is_empty() {
        let text = if no_password(s) {
            NO_PASSWORD
        } else {
            "Enter sends the text to all players (say). What players write needs the server log (Get Log), which the console tool cannot receive yet."
        };
        f.render_widget(
            Paragraph::new(text)
                .fg(Color::Gray)
                .wrap(ratatui::widgets::Wrap { trim: true }),
            out,
        );
    } else {
        f.render_widget(Paragraph::new(lines), out);
    }
    input_row(f, app, input, "Say: ", &app.chat_input, focused);
}

fn draw_bans(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let Some(s) = app.current() else { return };
    let [list, side] =
        Layout::horizontal([Constraint::Min(30), Constraint::Length(34)]).areas(area);
    let empty = s.rcon.bans.is_empty();
    let rows: Vec<Row> = s
        .rcon
        .bans
        .iter()
        .map(|b| {
            Row::new([
                Cell::from(if b.is_ip { "IP" } else { "Steam ID" }),
                Cell::from(b.ident.clone()),
                Cell::from(b.duration.clone()),
            ])
        })
        .collect();
    let no_pw = no_password(s);
    let mut state = TableState::default()
        .with_selected(focused.then_some(app.ban_sel))
        .with_offset(app.areas.bans_offset);
    f.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(9),
                Constraint::Min(16),
                Constraint::Length(14),
            ],
        )
        .header(Row::new(["Type", "Steam ID / IP", "Duration"]).style(header()))
        .row_highlight_style(selected(focused)),
        list,
        &mut state,
    );
    app.areas.bans_offset = state.offset();
    if empty {
        hint(
            f,
            list,
            if no_pw {
                NO_PASSWORD
            } else {
                "No bans (r: refresh)."
            },
        );
    }
    let key = |k: &str, t: &str| {
        Line::from(vec![
            Span::styled(format!("{k:>5} "), Style::new().fg(ACCENT).bold()),
            Span::raw(t.to_string()),
        ])
    };
    f.render_widget(
        Paragraph::new(vec![
            key("r", "Refresh (listid, listip)"),
            key("u Del", "Unban"),
            key("w", "Write (writeid, writeip)"),
            key("b Ins", "Ban Steam ID or IP..."),
        ])
        .block(
            Block::new()
                .borders(ratatui::widgets::Borders::LEFT)
                .border_style(Style::new().fg(Color::DarkGray)),
        ),
        side,
    );
}

fn draw_maps(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let Some(s) = app.current() else { return };
    let [left, side] =
        Layout::horizontal([Constraint::Min(30), Constraint::Length(26)]).areas(area);
    let [search, grid] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(left);
    let label = "Search (* ?): ";
    let width = usize::from(search.width).saturating_sub(label.len() + 1);
    let (shown, cursor) = field_text(&app.map_search, width, false);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(label, Style::new().fg(Color::Gray)),
            Span::styled(
                format!("{shown:<width$}"),
                Style::new().fg(Color::White).bg(Color::DarkGray),
            ),
        ])),
        search,
    );
    if focused && matches!(app.mode, Mode::Normal) {
        f.set_cursor_position(Position::new(
            search.x + (label.len() + cursor) as u16,
            search.y,
        ));
    }
    let maps = app.shown_maps(s);
    let current = s.map().to_string();
    // A grid of maps side by side, like the GUI.
    let col_w = maps
        .iter()
        .map(|m| m.chars().count())
        .max()
        .unwrap_or(10)
        .clamp(8, 30)
        + 2;
    let cols = (usize::from(grid.width) / col_w).max(1);
    let rows_shown = usize::from(grid.height).max(1);
    let sel_row = app.map_sel / cols;
    let first_row = sel_row.saturating_sub(rows_shown - 1);
    let mut lines = Vec::new();
    for row in first_row..first_row + rows_shown {
        let mut spans = Vec::new();
        for c in 0..cols {
            let i = row * cols + c;
            let Some(m) = maps.get(i) else { break };
            let mut style = if **m == current {
                Style::new().fg(Color::LightRed).bold()
            } else {
                Style::new()
            };
            if i == app.map_sel && focused {
                style = selected(true);
            }
            let name: String = m.chars().take(col_w - 2).collect();
            spans.push(Span::styled(format!("{name:<w$}", w = col_w - 2), style));
            spans.push(Span::raw("  "));
        }
        lines.push(Line::from(spans));
    }
    if maps.is_empty() {
        let text = if no_password(s) {
            NO_PASSWORD
        } else if s.rcon.maps.is_empty() {
            "No maps yet (Ctrl+R: update with maps *)."
        } else {
            "No map matches the search."
        };
        f.render_widget(Paragraph::new(text).fg(Color::Gray), grid);
    } else {
        f.render_widget(Paragraph::new(lines), grid);
    }
    let key = |k: &str, t: &str| {
        Line::from(vec![
            Span::styled(format!("{k:>6} "), Style::new().fg(ACCENT).bold()),
            Span::raw(t.to_string()),
        ])
    };
    f.render_widget(
        Paragraph::new(vec![
            key("Enter", "Change Level"),
            key("Ctrl+R", "Update (maps *)"),
            key("Arrows", "choose a map"),
            Line::raw(""),
            Line::from(vec![
                Span::styled(" Current: ", Style::new().fg(Color::Gray)),
                Span::styled(current, Style::new().fg(Color::LightRed).bold()),
            ]),
            Line::styled(
                format!(" {} maps", s.rcon.maps.len()),
                Style::new().fg(Color::Gray),
            ),
        ])
        .block(
            Block::new()
                .borders(ratatui::widgets::Borders::LEFT)
                .border_style(Style::new().fg(Color::DarkGray)),
        ),
        side,
    );
    app.areas.map_columns = cols;
}

// --------------------------------------------------------------------------
// Bottom lines and pop-ups
// --------------------------------------------------------------------------

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::raw(format!(" Server list: {} ", app.store.list_name())),
        Span::styled("│", Style::new().fg(Color::DarkGray)),
        Span::raw(format!(" {}/{} online ", online(app), app.servers.len())),
    ];
    if let Some(s) = app.current() {
        spans.push(Span::styled("│", Style::new().fg(Color::DarkGray)));
        spans.push(Span::raw(" Rcon: "));
        spans.push(rcon_state_text(&s.rcon.state));
        spans.push(Span::raw(" "));
    }
    if let Some((text, at)) = &app.message
        && at.elapsed() < Duration::from_secs(6)
    {
        spans.push(Span::styled("│ ", Style::new().fg(Color::DarkGray)));
        spans.push(Span::styled(text.clone(), Style::new().fg(ACCENT)));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(Color::Black)),
        area,
    );
}

fn fkeys(keys: &[(&str, &str)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (k, label) in keys {
        spans.push(Span::styled(
            k.to_string(),
            Style::new().fg(Color::White).bg(Color::Black),
        ));
        spans.push(Span::styled(format!("{label:<6} "), bar()));
    }
    spans
}

fn draw_keys(f: &mut Frame, app: &App, area: Rect) {
    let spans = match &app.mode {
        Mode::Prompt { kind, text } => {
            let label = kind.label();
            let mut spans = vec![Span::styled(label.to_string(), bar())];
            spans.push(Span::styled(text.clone(), Style::new().bold()));
            let x = area.x + (label.len() + text.chars().count()) as u16;
            f.set_cursor_position(Position::new(x.min(area.right().saturating_sub(1)), area.y));
            spans.push(Span::raw("  "));
            let tail: &[(&str, &str)] = match kind {
                PromptKind::Search => &[("F3", "Next"), ("Esc", "Done")],
                PromptKind::Filter(_) => &[("Enter", "Done"), ("Esc", "Clear")],
                _ => &[("Enter", "OK"), ("Esc", "Cancel")],
            };
            spans.extend(fkeys(tail));
            spans
        }
        Mode::Confirm(c) => {
            let text = match c {
                Confirm::Remove(id) => format!(
                    " Remove {} from the list? ",
                    app.server(*id)
                        .map_or(String::new(), |s| s.address_text.clone())
                ),
                Confirm::ChangeLevel(_, map) => {
                    format!(" Change level to '{map}'? (changelevel {map}) ")
                }
                Confirm::Unban(_, b) => format!(" Unban {}? ", b.ident),
            };
            let mut spans = vec![Span::styled(
                text,
                Style::new().fg(Color::White).bg(Color::Red).bold(),
            )];
            spans.push(Span::raw(" "));
            spans.extend(fkeys(&[("y", "Yes"), ("n", "No")]));
            spans
        }
        Mode::Sort(_) => fkeys(&[("Enter", "Sort"), ("Esc", "Cancel")]),
        Mode::Menu { .. } => fkeys(&[("Enter", "Select"), ("←→", "Menu"), ("Esc", "Close")]),
        Mode::Help | Mode::About => fkeys(&[("Esc", "Close")]),
        Mode::Normal => {
            let mut keys = vec![("F1", "Help"), ("F2", "Rcon")];
            match app.focus {
                Pane::Servers => keys.extend([
                    ("F3", "Search"),
                    ("F4", "Filter"),
                    ("F5", "Refrsh"),
                    ("F6", "SortBy"),
                    ("F7", "Add"),
                    ("F8", "Remove"),
                ]),
                Pane::Players => {
                    keys.extend([("F4", "Filter"), ("F5", "Refrsh"), ("F6", "SortBy")])
                }
                Pane::Rules => keys.extend([("F4", "Filter"), ("F5", "Refrsh")]),
                Pane::Tabs => match app.tab {
                    Tab::Bans => keys.extend([("F5", "Refrsh"), ("F8", "Unban")]),
                    Tab::ChangeLevel => keys.extend([("F5", "Update"), ("Enter", "Change")]),
                    _ => keys.push(("F5", "Refrsh")),
                },
            }
            if app.focus == Pane::Tabs {
                keys.push(("Esc", "Back"));
            }
            keys.extend([("F9", "Menu"), ("F10", "Quit"), ("Tab", "Panel")]);
            fkeys(&keys)
        }
    };
    f.render_widget(Paragraph::new(Line::from(spans)).style(bar()), area);
}

fn popup(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn draw_menu(f: &mut Frame, app: &App, menu: usize, item: usize) {
    let Some(label) = app.areas.menu_labels.get(menu) else {
        return;
    };
    let items = MENUS[menu].1;
    let w = items
        .iter()
        .map(|(n, k, _)| n.chars().count() + k.chars().count() + 4)
        .max()
        .unwrap_or(10) as u16
        + 2;
    let area = Rect {
        x: label.x,
        y: label.y + 1,
        width: w.min(f.area().width.saturating_sub(label.x)),
        height: (items.len() as u16 + 2).min(f.area().height.saturating_sub(1)),
    };
    f.render_widget(Clear, area);
    let inner_w = usize::from(w.saturating_sub(2));
    let lines: Vec<Line> = items
        .iter()
        .enumerate()
        .map(|(i, (name, key, _))| {
            let gap = inner_w.saturating_sub(name.chars().count() + key.chars().count() + 2);
            let text = format!(" {name}{}{key} ", " ".repeat(gap));
            let style = if i == item {
                Style::new().fg(Color::White).bg(Color::Black)
            } else {
                bar()
            };
            Line::styled(text, style)
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines)
            .style(bar())
            .block(Block::bordered().border_style(bar()).style(bar())),
        area,
    );
}

fn draw_sort_menu(f: &mut Frame, app: &App, selected_i: usize, body: Rect) {
    let names: Vec<&str> = if app.focus == Pane::Players {
        PLAYER_SORT.iter().map(|(_, n)| *n).collect()
    } else {
        SERVER_SORT.iter().map(|(_, n)| *n).collect()
    };
    let area = Rect {
        width: 16.min(body.width),
        height: (names.len() as u16 + 2).min(body.height),
        ..body
    };
    f.render_widget(Clear, area);
    let rows = names.iter().enumerate().map(|(i, n)| {
        let r = Row::new([*n]);
        if i == selected_i {
            r.style(selected(true))
        } else {
            r
        }
    });
    f.render_widget(
        Table::new(rows, [Constraint::Min(1)]).block(Block::bordered().title(" Sort by ")),
        area,
    );
}

const HELP: &[(&str, &str)] = &[
    (
        "Tab Shift+Tab",
        "next / previous panel (servers, players, rules, tabs)",
    ),
    (
        "Alt+1 … Alt+5",
        "tabs Rcon Config, Console, Chat, Ban List, Change Level",
    ),
    (
        "Shift+←→, Ctrl+PgUp/PgDn",
        "previous / next tab (in the tabs)",
    ),
    ("F9", "menu bar (arrows, Enter; first letter opens a menu)"),
    ("Esc", "back to the server list, clears a filter"),
    ("", ""),
    ("Up/Down j/k", "select (mouse: click, wheel)"),
    ("PgUp/PgDn Home/End", "page, first / last"),
    ("Enter", "server list: go to the tabs"),
    ("F2 c", "RCON console of the selected server"),
    ("F3 /  n N", "search a server; next / previous match"),
    ("F4 \\", "filter the focused list (servers, players, rules)"),
    (
        "F5 r",
        "refresh (Ban List / Change Level: fetch them again)",
    ),
    ("F6 < > I", "sort column; previous / next; invert"),
    ("P M T", "sort by players, map, ping"),
    ("F7 Ins a", "add a server (IP:port, host:port:queryport)"),
    ("F8 Del d", "remove the selected server"),
    ("e  y", "hide empty servers; copy the address (OSC 52)"),
    ("F10 q Ctrl+C", "quit"),
];

fn draw_help(f: &mut Frame, area: Rect) {
    let r = popup(area, 80, HELP.len() as u16 + 2);
    f.render_widget(Clear, r);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!("{k:>19}  "), Style::new().fg(ACCENT).bold()),
                Span::raw(v.to_string()),
            ])
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(" Keys ")
                .title_bottom(" any key closes "),
        ),
        r,
    );
}

fn draw_about(f: &mut Frame, area: Rect) {
    let r = popup(area, 64, 9);
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(vec![
            Line::styled(
                format!("openHLSW {} – console tool", env!("CARGO_PKG_VERSION")),
                Style::new().bold(),
            ),
            Line::raw("Game server browser and RCON admin tool, HLSW remake."),
            Line::raw("Free software under the GNU GPL v3.0 or later."),
            Line::raw("Built with ratatui and crossterm (MIT)."),
            Line::raw(""),
            Line::styled(
                "https://github.com/DennisPohlmann/openHLSW",
                Style::new().fg(Color::Cyan),
            ),
        ])
        .block(Block::bordered().title(" About ")),
        r,
    );
}
