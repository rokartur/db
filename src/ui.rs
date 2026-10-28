use crate::app::{App, Focus, Node, TableView, Tab};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Tabs};

const MAX_COLUMN_WIDTH: usize = 40;
const SELECTED: Style = Style::new().fg(Color::Black).bg(Color::Cyan);

const HELP: &str = "\
Tab        focus tree / right panel
j k ↑ ↓    move in tree and table
h l ← →    scroll table columns
Enter      expand group / open item in Records
] [        next / previous Records page
1 2        Records / Query tab
e i        edit query (Query tab), Esc leaves the editor
Ctrl-R F5  run query in the group selected in the tree
y          copy cell (OSC 52)
?          this help
q Ctrl-C   quit";

pub fn draw(frame: &mut Frame, app: &App) {
    let [main, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    match app.picker {
        Some(cursor) => draw_picker(frame, app, cursor, main),
        None => draw_main(frame, app, main),
    }
    let status_line = Line::from(vec![app.status.clone().into(), "  ? help | q quit".dark_gray()]);
    frame.render_widget(Paragraph::new(status_line), status);
    if app.help {
        let area = centered(frame.area(), 60, 14);
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(HELP).block(Block::bordered().title("Help")), area);
    }
}

fn draw_picker(frame: &mut Frame, app: &App, cursor: usize, area: Rect) {
    let block = Block::bordered().title("Connections");
    if app.connections.is_empty() {
        let hint = format!("No connections in {}.\nRun `dbtui <url>` or add [[connection]] entries.", crate::config::path().display());
        frame.render_widget(Paragraph::new(hint).block(block), area);
        return;
    }
    let items = app.connections.iter().map(|c| ListItem::new(c.name.clone()));
    let mut state = ListState::default().with_selected(Some(cursor));
    frame.render_stateful_widget(List::new(items).block(block).highlight_style(SELECTED), area, &mut state);
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
