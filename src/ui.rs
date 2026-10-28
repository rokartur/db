use crate::app::{App, Focus, Node, TableView, Tab};
use crate::config;
use crate::picker::{ENVS, Grouping, ListRow, Picker, Sort};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Tabs};

const MAX_COLUMN_WIDTH: usize = 40;
const SELECTED: Style = Style::new().fg(Color::Black).bg(Color::Cyan);

const HELP: &str = "\
Connections
Enter      open the database
/          filter by name, env, type or url
a d        add / delete a connection
s g        sort (last used, name, type) / group (env, type)

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
        draw_picker(frame, &app.picker, "q quit", main);
    } else {
        draw_main(frame, app, main);
        if app.palette {
            let popup = centered(main, main.width * 4 / 5, main.height * 7 / 10);
            frame.render_widget(Clear, popup);
            draw_picker(frame, &app.picker, "Esc close", popup);
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

fn draw_picker(frame: &mut Frame, picker: &Picker, exit_hint: &str, area: Rect) {
    let sort = match picker.sort {
        Sort::LastUsed => "last used",
        Sort::Name => "name",
        Sort::Kind => "type",
    };
    let grouping = match picker.grouping {
        Grouping::Env => "env",
        Grouping::Kind => "type",
    };
    let block = Block::bordered()
        .title("Connections")
        .title(Line::from(format!("sort: {sort} | group: {grouping}")).right_aligned())
        .title_bottom(format!("Enter open  / filter  a add  d delete  s sort  g group  ? help  {exit_hint}").dark_gray());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [filter_area, list_area] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);

    if let Some(i) = picker.deleting {
        let prompt = format!("delete {}? y / any key cancels", picker.connections[i].name);
        frame.render_widget(Paragraph::new(prompt.red().bold()), filter_area);
    } else if picker.filtering || !picker.filter.is_empty() {
        frame.render_widget(Paragraph::new(format!("/ {}", picker.filter).yellow()), filter_area);
        if picker.filtering && picker.form.is_none() {
            frame.set_cursor_position(Position::new(filter_area.x + 2 + picker.filter.chars().count() as u16, filter_area.y));
        }
    }

    if picker.connections.is_empty() {
        let hint = format!("No connections in {}.\nPress a to add one, or run `dbtui <url>`.", config::config_file(&picker.dir).display());
        frame.render_widget(Paragraph::new(hint), list_area);
    } else {
        draw_connection_rows(frame, picker, list_area);
    }

    if picker.form.is_some() {
        draw_form(frame, picker);
    }
}

fn draw_connection_rows(frame: &mut Frame, picker: &Picker, area: Rect) {
    let rows = picker.rows();
    let name_width = picker.connections.iter().map(|c| c.name.chars().count()).max().unwrap_or(0).min(24);
    let kind_width = picker.connections.iter().map(|c| c.kind().len()).max().unwrap_or(0).min(12);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock after 1970").as_secs();

    let mut items = Vec::new();
    let mut selected = None;
    let mut entry = 0;
    for row in &rows {
        match row {
            ListRow::Header(name, count) => {
                items.push(ListItem::new(Line::from(vec![Span::from(name.clone()).bold(), format!(" ({count})").dark_gray()])));
            }
            ListRow::Entry(i) => {
                if entry == picker.cursor {
                    selected = Some(items.len());
                }
                entry += 1;
                let connection = &picker.connections[*i];
                let used = picker.last_used(*i).map_or("never".to_string(), |at| ago(now.saturating_sub(at)));
                let fixed = 2 + name_width + 1 + kind_width + 1 + 1 + 12;
                let url_width = (area.width as usize).saturating_sub(fixed).max(10);
                let line = format!(
                    "  {:name_width$} {:kind_width$} {:url_width$} {used:>12}",
                    truncate(&connection.name, name_width),
                    truncate(&connection.kind(), kind_width),
                    truncate(&connection.redacted_url(), url_width),
                );
                items.push(ListItem::new(line));
            }
        }
    }
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(List::new(items).highlight_style(SELECTED), area, &mut state);
}

fn draw_form(frame: &mut Frame, picker: &Picker) {
    let Some(form) = &picker.form else { return };
    let area = centered(frame.area(), 70, 7);
    let block = Block::bordered().title("Add connection").border_style(border(true));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    let field = |index: usize, label: &str, value: String| {
        let style = if form.field == index { SELECTED } else { Style::new() };
        Line::from(vec![Span::from(format!("{label:<6}")), Span::styled(value, style)])
    };
    let lines = vec![
        field(0, "name", form.name.clone()),
        field(1, "url", form.url.clone()),
        field(2, "env", format!("< {} >", ENVS[form.env])),
        Line::raw(""),
        "Tab next field  ← → env  Enter save  Esc cancel".dark_gray().into(),
    ];
    frame.render_widget(Paragraph::new(lines).block(block), area);
    let typed = match form.field {
        0 => Some(form.name.chars().count()),
        1 => Some(form.url.chars().count()),
        _ => None,
    };
    if let Some(typed) = typed {
        let x = inner.x + (6 + typed).min(inner.width.saturating_sub(1) as usize) as u16;
        frame.set_cursor_position(Position::new(x, inner.y + form.field as u16));
    }
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
