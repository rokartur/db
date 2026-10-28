use crate::app::{App, Focus, Node, TableView, Tab};
use crate::config;
use crate::picker::{Grouping, ListRow, MENU, Picker, Sort};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Tabs};

const MAX_COLUMN_WIDTH: usize = 40;
const PICKER_WIDTH: u16 = 90;
const SELECTED: Style = Style::new().fg(Color::Black).bg(Color::Cyan);

const HELP: &str = "\
Connections
Enter      open the database
/          filter by name, label, type or url
n          new connection
m .        menu: edit, copy connection string, remove
d          remove a connection
x          forget from last opened
s          sort: date newest / oldest, name A-Z / Z-A
g          group: label, type, none

Database
Tab        focus tree / right panel
j k ↑ ↓    move in tree and table
h l ← →    scroll table columns
Enter      expand group / open item in Records
] [        next / previous Records page
1 2        Records / Query tab
e i        edit query (Query tab), Esc leaves the editor
Ctrl-R F5  run query in the group selected in the tree
y          copy cell (OSC 52)
Ctrl-P     switch database (type to filter, Esc closes)
?          this help
q Ctrl-C   quit (Esc closes the connection list)";

pub fn draw(frame: &mut Frame, app: &App) {
    let [main, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    if !app.connected {
        draw_picker(frame, &app.picker, false, main);
    } else {
        draw_main(frame, app, main);
        if app.palette {
            let popup = centered(main, main.width * 4 / 5, main.height * 7 / 10);
            frame.render_widget(Clear, popup);
            draw_picker(frame, &app.picker, true, popup);
        }
    }
    let status_line = Line::from(vec![app.status.clone().into(), "  Ctrl-P databases | ? help | q quit".dark_gray()]);
    frame.render_widget(Paragraph::new(status_line), status);
    if app.help {
        let area = centered(frame.area(), 62, HELP.lines().count() as u16 + 2);
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(HELP).block(Block::bordered().title("Help")), area);
    }
}

fn draw_picker(frame: &mut Frame, picker: &Picker, palette: bool, area: Rect) {
    let area = if palette {
        let block = Block::bordered().title("Databases").border_style(border(true));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    } else {
        area
    };
    let [body, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let [column] = Layout::horizontal([Constraint::Max(PICKER_WIDTH)]).flex(Flex::Center).areas(body);
    let [top, _, list, details] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(3)]).areas(column);

    draw_filter(frame, picker, top);
    if picker.connections.is_empty() {
        let hint = format!("No connections in {}.\nPress n to add one, or run `dbtui <url>`.", config::config_file(&picker.dir).display());
        frame.render_widget(Paragraph::new(hint), list);
    } else {
        draw_connection_rows(frame, picker, list);
    }
    draw_details(frame, picker, details);
    let exit = if palette { "Esc close" } else { "q quit" };
    let keys = format!("Enter open  m menu  n new  / filter  g group  s sort  x forget  ? help  {exit}");
    frame.render_widget(Paragraph::new(keys.dark_gray()), footer);

    if picker.form.is_some() {
        draw_form(frame, picker);
    }
    if let Some(i) = picker.deleting {
        draw_remove(frame, &picker.connections[i].name);
    }
}

fn draw_filter(frame: &mut Frame, picker: &Picker, area: Rect) {
    frame.render_widget(Paragraph::new("dbtui".bold()), area);
    let typed = picker.filtering || !picker.filter.is_empty();
    let filter = if typed { format!("/ {} ", picker.filter).yellow() } else { "/ filter connections".dark_gray() };
    frame.render_widget(Paragraph::new(filter).right_aligned(), area);
    if picker.filtering && picker.form.is_none() {
        frame.set_cursor_position(Position::new(area.right().saturating_sub(1), area.y));
    }
}

fn draw_connection_rows(frame: &mut Frame, picker: &Picker, area: Rect) {
    let rows = picker.rows();
    let locations: Vec<(String, String)> = picker.connections.iter().map(|c| c.location()).collect();
    let name_width = picker.connections.iter().map(|c| c.name.chars().count()).max().unwrap_or(0).min(24);
    let host_width = locations.iter().map(|(host, _)| host.chars().count()).max().unwrap_or(0).min(28);
    let sort = match picker.sort {
        Sort::Newest => "date (newest first)",
        Sort::Oldest => "date (oldest first)",
        Sort::NameAsc => "name (A-Z)",
        Sort::NameDesc => "name (Z-A)",
    };
    let grouping = match picker.grouping {
        Grouping::Label => "label",
        Grouping::Kind => "type",
        Grouping::None => "no grouping",
    };

    let mut items = Vec::new();
    if matches!(rows.first(), Some(ListRow::LastOpened(_))) {
        items.push(ListItem::new("LAST OPENED".dark_gray()));
    }
    let mut in_connections = false;
    let mut selected = None;
    let mut entry = 0;
    for row in &rows {
        if !in_connections && !matches!(row, ListRow::LastOpened(_)) {
            if !items.is_empty() {
                items.push(ListItem::new(""));
            }
            items.push(ListItem::new(format!("CONNECTIONS · {grouping} · {sort}").dark_gray()));
            in_connections = true;
        }
        let line = match row {
            ListRow::Group(name) => Line::from(Span::from(name.clone()).bold()),
            ListRow::LastOpened(i) => {
                let connection = &picker.connections[*i];
                let (_, database) = &locations[*i];
                let target = if database.is_empty() { connection.name.clone() } else { format!("{} / {database}", connection.name) };
                Line::from(vec!["  ".into(), badge(&connection.kind()), " ".into(), target.into()])
            }
            ListRow::Entry(i) => {
                let connection = &picker.connections[*i];
                let (host, database) = &locations[*i];
                let text = format!(
                    "{:name_width$}  {:host_width$}  {database}",
                    truncate(&connection.name, name_width),
                    truncate(host, host_width),
                );
                Line::from(vec!["  ".into(), badge(&connection.kind()), " ".into(), text.into()])
            }
        };
        if !matches!(row, ListRow::Group(_)) {
            if entry == picker.cursor {
                selected = Some(items.len());
            }
            entry += 1;
        }
        items.push(ListItem::new(line));
    }
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(List::new(items).highlight_style(SELECTED), area, &mut state);
    if let (Some(item), Some(row)) = (picker.menu, selected) {
        draw_menu(frame, item, area, (row - state.offset()) as u16);
    }
}

/// Two-letter type tag in the type's color, like the icons in Tamery.
fn badge(kind: &str) -> Span<'static> {
    let (tag, color) = match kind {
        "postgres" | "postgresql" => ("pg", Color::Blue),
        "mysql" => ("my", Color::LightYellow),
        "mariadb" => ("ma", Color::LightYellow),
        "sqlite" => ("sq", Color::Green),
        "duckdb" => ("dk", Color::Yellow),
        "redis" | "rediss" => ("rd", Color::Red),
        "mongodb" | "mongodb+srv" => ("mg", Color::LightGreen),
        "clickhouse" => ("ch", Color::Yellow),
        _ => (kind.get(..2).unwrap_or(kind), Color::Gray),
    };
    Span::styled(tag.to_string(), Style::new().fg(color))
}

fn draw_menu(frame: &mut Frame, item: usize, list: Rect, row: u16) {
    let (width, height) = (28, MENU.len() as u16 + 2);
    let y = (list.y + row + 1).min(frame.area().bottom().saturating_sub(height));
    let area = Rect::new(list.right().saturating_sub(width), y, width, height).intersection(frame.area());
    let items = MENU.iter().enumerate().map(|(index, label)| {
        let style = if index == MENU.len() - 1 { Style::new().fg(Color::Red) } else { Style::new() };
        ListItem::new(Span::styled(*label, style))
    });
    let list = List::new(items).block(Block::bordered().border_style(border(true))).highlight_style(SELECTED);
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut ListState::default().with_selected(Some(item)));
}

fn draw_details(frame: &mut Frame, picker: &Picker, area: Rect) {
    let Some(i) = picker.selected() else { return };
    let connection = &picker.connections[i];
    let auto = if connection.label.is_none() { " (auto)" } else { "" };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock after 1970").as_secs();
    let opened = picker.last_used(i).map_or("never".to_string(), |at| ago(now.saturating_sub(at)));
    let lines = vec![
        "─".repeat(area.width as usize).dark_gray().into(),
        Line::from(vec!["url    ".dark_gray(), connection.redacted_url().into()]),
        Line::from(vec![
            "label  ".dark_gray(),
            format!("{}{auto}", connection.label()).into(),
            "    opened  ".dark_gray(),
            opened.into(),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_form(frame: &mut Frame, picker: &Picker) {
    let Some(form) = &picker.form else { return };
    let area = centered(frame.area(), 70, 8);
    let title = if form.editing.is_some() { "Edit connection" } else { "New connection" };
    let block = Block::bordered().title(title).border_style(border(true));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    let field = |index: usize, label: &str, value: &str| {
        let style = if form.field == index { SELECTED } else { Style::new() };
        Line::from(vec![Span::from(format!("{label:<6}")), Span::styled(value.to_string(), style)])
    };
    let lines = vec![
        field(0, "name", &form.name),
        field(1, "url", &form.url),
        field(2, "label", &form.label),
        "      empty: local for localhost and files; or test, prod...".dark_gray().into(),
        Line::raw(""),
        "Tab next field  Enter save  Esc cancel".dark_gray().into(),
    ];
    frame.render_widget(Paragraph::new(lines).block(block), area);
    let typed = [&form.name, &form.url, &form.label][form.field].chars().count();
    let x = inner.x + (6 + typed).min(inner.width.saturating_sub(1) as usize) as u16;
    frame.set_cursor_position(Position::new(x, inner.y + form.field as u16));
}

fn draw_remove(frame: &mut Frame, name: &str) {
    let area = centered(frame.area(), 50, 4);
    let lines = vec![Line::from(format!("Remove {name}?")).bold(), "y removes, any other key cancels".dark_gray().into()];
    let block = Block::bordered().title("Remove connection").border_style(Style::new().fg(Color::Red));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(width.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86400),
    }
}

fn draw_main(frame: &mut Frame, app: &App, area: Rect) {
    let [tree, right] = Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)]).areas(area);
    draw_tree(frame, app, tree);
    let [tabs, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(right);
    let selected_tab = if app.tab == Tab::Records { 0 } else { 1 };
    frame.render_widget(Tabs::new(["1 Records", "2 Query"]).select(selected_tab).highlight_style(SELECTED), tabs);
    let focused = app.focus == Focus::Right;
    match app.tab {
        Tab::Records => {
            let title = match app.opened_names() {
                Some((group, item)) => format!("{group}.{item}"),
                None => "Records".into(),
            };
            draw_table(frame, &app.records, &title, Some(app.page), focused, body);
        }
        Tab::Query => {
            let [editor, result] = Layout::vertical([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)]).areas(body);
            draw_editor(frame, app, editor);
            draw_table(frame, &app.result, "Result", None, focused && !app.editing, result);
        }
    }
}

fn draw_tree(frame: &mut Frame, app: &App, area: Rect) {
    let items = app.nodes().into_iter().map(|node| match node {
        Node::Group(g) => {
            let marker = if app.expanded[g] { "v" } else { ">" };
            let name = &app.groups[g].name;
            ListItem::new(format!("{marker} {}", if name.is_empty() { "(default)" } else { name }))
        }
        Node::Item(g, i) => ListItem::new(format!("    {}", app.groups[g].items[i])),
    });
    let mut state = ListState::default().with_selected(Some(app.tree_cursor));
    let block = Block::bordered().title("Tree").border_style(border(app.focus == Focus::Tree));
    frame.render_stateful_widget(List::new(items).block(block).highlight_style(SELECTED), area, &mut state);
}

fn draw_editor(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title("Query (e edit, Ctrl-R run)").border_style(border(app.editing));
    let inner = block.inner(area);
    let editor = &app.editor;
    let scroll = editor.row.saturating_sub(inner.height.saturating_sub(1) as usize);
    let lines: Vec<Line> = editor.lines.iter().skip(scroll).map(|l| Line::raw(l.as_str())).collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);
    if app.editing {
        let x = inner.x + editor.col.min(inner.width.saturating_sub(1) as usize) as u16;
        let y = inner.y + (editor.row - scroll) as u16;
        frame.set_cursor_position(Position::new(x, y));
    }
}

fn draw_table(frame: &mut Frame, view: &TableView, title: &str, page: Option<usize>, focused: bool, area: Rect) {
    let block = Block::bordered().title(title.to_string()).border_style(border(focused));
    let grid = &view.grid;
    let visible = view.col.min(grid.columns.len());
    let widths: Vec<Constraint> = (visible..grid.columns.len())
        .map(|c| {
            let longest = grid.rows.iter().map(|row| row[c].chars().count()).max().unwrap_or(0);
            Constraint::Length(longest.max(grid.columns[c].chars().count()).clamp(3, MAX_COLUMN_WIDTH) as u16)
        })
        .collect();
    let header = Row::new(grid.columns[visible..].iter().map(|c| Cell::from(c.as_str()))).bold();
    let rows = grid.rows.iter().map(|row| Row::new(row[visible..].iter().map(|v| Cell::from(v.replace('\n', " ")))));
    let mut footer = format!("rows {} | col {}/{}", grid.rows.len(), view.col + 1, grid.columns.len());
    if let Some(page) = page {
        footer = format!("page {} | {footer}", page + 1);
    }
    let table = Table::new(rows, widths)
        .header(header)
        .block(block.title_bottom(footer))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = TableState::default().with_selected(focused.then_some(view.row));
    frame.render_stateful_widget(table, area, &mut state);
}

fn border(focused: bool) -> Style {
    if focused { Style::new().fg(Color::Cyan) } else { Style::new() }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height)
}
