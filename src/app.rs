use crate::driver::{self, Driver, Grid, Group, PAGE};
use crate::editor::Editor;
use crate::picker::Picker;
use crate::ui;
use anyhow::Result;
use base64::Engine;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Tree,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    Records,
    Query,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Node {
    Group(usize),
    Item(usize, usize),
}

#[derive(Debug, Default)]
pub struct TableView {
    pub grid: Grid,
    pub row: usize,
    pub col: usize,
}

impl TableView {
    fn show(&mut self, grid: Grid) {
        *self = TableView { grid, row: 0, col: 0 };
    }

    fn cell(&self) -> Option<&String> {
        self.grid.rows.get(self.row)?.get(self.col)
    }
}

#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Quit,
    Connect(String),
    Browse,
    Query,
    Copy(String),
}

pub struct App {
    pub picker: Picker,
    /// The connection list is on screen: full screen until a database opens, a popup (Ctrl-P) after.
    pub palette: bool,
    pub connected: bool,
    pub groups: Vec<Group>,
    pub expanded: Vec<bool>,
    pub tree_cursor: usize,
    pub focus: Focus,
    pub tab: Tab,
    pub records: TableView,
    pub opened: Option<(usize, usize)>,
    pub page: usize,
    pub result: TableView,
    pub editor: Editor,
    pub editing: bool,
    pub status: String,
    pub help: bool,
    secrets: Vec<String>,
}

impl App {
    pub fn new(picker: Picker) -> Self {
        App {
            picker,
            palette: true,
            connected: false,
            groups: Vec::new(),
            expanded: Vec::new(),
            tree_cursor: 0,
            focus: Focus::Tree,
            tab: Tab::Records,
            records: TableView::default(),
            opened: None,
            page: 0,
            result: TableView::default(),
            editor: Editor::new(),
            editing: false,
            status: String::new(),
            help: false,
            secrets: Vec::new(),
        }
    }

    pub fn nodes(&self) -> Vec<Node> {
        let mut nodes = Vec::new();
        for (g, group) in self.groups.iter().enumerate() {
            nodes.push(Node::Group(g));
            if self.expanded[g] {
                nodes.extend((0..group.items.len()).map(|i| Node::Item(g, i)));
            }
        }
        nodes
    }

    /// The group of the tree selection: the context a query runs in.
    pub fn context(&self) -> &str {
        match self.nodes().get(self.tree_cursor) {
            Some(Node::Group(g) | Node::Item(g, _)) => &self.groups[*g].name,
            None => "",
        }
    }

    pub fn opened_names(&self) -> Option<(&str, &str)> {
        let (g, i) = self.opened?;
        let group = self.groups.get(g)?;
        Some((&group.name, group.items.get(i)?))
    }

    pub fn connected(&mut self, url: &str, groups: Vec<Group>) {
        self.secrets = driver::secrets_in(url);
        self.expanded = vec![groups.len() == 1; groups.len()];
        self.groups = groups;
        self.tree_cursor = 0;
        self.opened = None;
        self.page = 0;
        self.records = TableView::default();
        self.result = TableView::default();
        self.palette = false;
        self.connected = true;
        self.status = format!("connected: {} groups", self.groups.len());
    }

    pub fn fail(&mut self, error: anyhow::Error) {
        self.status = driver::redact(&format!("error: {error:#}"), &self.secrets).replace('\n', " ");
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        if self.help {
            self.help = false;
            return Action::None;
        }
        if self.palette && !self.picker.is_typing() && key.code == KeyCode::Char('?') {
            self.help = true;
            return Action::None;
        }
        if self.palette {
            return match self.picker.handle_key(key) {
                Ok(Action::Quit) if self.connected => {
                    self.palette = false;
                    Action::None
                }
                Ok(action) => action,
                Err(error) => {
                    self.fail(error);
                    Action::None
                }
            };
        }
        if ctrl && key.code == KeyCode::Char('p') {
            self.picker.open_palette();
            self.palette = true;
            return Action::None;
        }
        let run = (ctrl && key.code == KeyCode::Char('r')) || key.code == KeyCode::F(5);
        if run {
            self.tab = Tab::Query;
            return Action::Query;
        }
        if self.editing {
            if key.code == KeyCode::Esc {
                self.editing = false;
            } else {
                self.editor.handle(key);
            }
            return Action::None;
        }
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => self.focus = if self.focus == Focus::Tree { Focus::Right } else { Focus::Tree },
            KeyCode::Char('1') => self.tab = Tab::Records,
            KeyCode::Char('2') => self.tab = Tab::Query,
            KeyCode::Char('e' | 'i') if self.tab == Tab::Query => {
                self.editing = true;
                self.focus = Focus::Right;
            }
            KeyCode::Char('y') => {
                if let Some(cell) = self.table().cell() {
                    return Action::Copy(cell.clone());
                }
            }
            KeyCode::Char(']') if self.opened.is_some() && self.records.grid.rows.len() == PAGE => {
                self.page += 1;
                return Action::Browse;
            }
            KeyCode::Char('[') if self.opened.is_some() && self.page > 0 => {
                self.page -= 1;
                return Action::Browse;
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_vertical(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_vertical(-1),
            KeyCode::Char('l') | KeyCode::Right => self.move_column(1),
            KeyCode::Char('h') | KeyCode::Left => self.move_column(-1),
            KeyCode::Enter if self.focus == Focus::Tree => return self.open_node(),
            _ => {}
        }
        Action::None
    }

    fn open_node(&mut self) -> Action {
        match self.nodes().get(self.tree_cursor) {
            Some(Node::Group(g)) => {
                self.expanded[*g] = !self.expanded[*g];
                Action::None
            }
            Some(Node::Item(g, i)) => {
                self.opened = Some((*g, *i));
                self.page = 0;
                self.tab = Tab::Records;
                Action::Browse
            }
            None => Action::None,
        }
    }

    pub fn table(&self) -> &TableView {
        match self.tab {
            Tab::Records => &self.records,
            Tab::Query => &self.result,
        }
    }

    fn table_mut(&mut self) -> &mut TableView {
        match self.tab {
            Tab::Records => &mut self.records,
            Tab::Query => &mut self.result,
        }
    }

    fn move_vertical(&mut self, delta: isize) {
        if self.focus == Focus::Tree {
            self.tree_cursor = step(self.tree_cursor, delta, self.nodes().len());
            return;
        }
        let table = self.table_mut();
        table.row = step(table.row, delta, table.grid.rows.len());
    }

    fn move_column(&mut self, delta: isize) {
        if self.focus == Focus::Right {
            let table = self.table_mut();
            table.col = step(table.col, delta, table.grid.columns.len());
        }
    }

    pub fn show_records(&mut self, grid: Grid) {
        self.status = format!("page {} | {} rows", self.page + 1, grid.rows.len());
        self.records.show(grid);
    }

    pub fn show_result(&mut self, grid: Grid) {
        self.status = format!("{} rows", grid.rows.len());
        self.result.show(grid);
    }
}

pub(crate) fn step(value: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    value.saturating_add_signed(delta).min(len - 1)
}

pub async fn run(url: Option<String>, picker: Picker) -> Result<()> {
    let mut terminal = ratatui::init();
    let outcome = event_loop(&mut terminal, url, picker).await;
    ratatui::restore();
    outcome
}

async fn event_loop(terminal: &mut DefaultTerminal, url: Option<String>, picker: Picker) -> Result<()> {
    let mut app = App::new(picker);
    let mut db: Option<Box<dyn Driver>> = None;
    if let Some(url) = url {
        connect(terminal, &mut app, &mut db, &url).await?;
    }
    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match app.handle_key(key) {
            Action::None => {}
            Action::Quit => return Ok(()),
            Action::Connect(url) => connect(terminal, &mut app, &mut db, &url).await?,
            Action::Copy(text) => copy(&mut app, &text)?,
            Action::Browse => {
                let Some(db) = &db else { continue };
                let Some((group, item)) = app.opened_names() else { continue };
                let (group, item) = (group.to_string(), item.to_string());
                busy(terminal, &mut app, &format!("loading {item}..."))?;
                match db.browse(&group, &item, app.page).await {
                    Ok(grid) => app.show_records(grid),
                    Err(error) => app.fail(error),
                }
            }
            Action::Query => {
                let Some(db) = &db else { continue };
                let (context, text) = (app.context().to_string(), app.editor.text());
                busy(terminal, &mut app, "running...")?;
                match db.query(&context, &text).await {
                    Ok(grid) => app.show_result(grid),
                    Err(error) => app.fail(error),
                }
            }
        }
    }
}

async fn connect(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    db: &mut Option<Box<dyn Driver>>,
    url: &str,
) -> Result<()> {
    app.secrets = driver::secrets_in(url);
    busy(terminal, app, "connecting...")?;
    let opened = async {
        let driver = driver::connect(url).await?;
        let groups = driver.groups().await?;
        anyhow::Ok((driver, groups))
    };
    match opened.await {
        Ok((driver, groups)) => {
            *db = Some(driver);
            app.connected(url, groups);
        }
        Err(error) => app.fail(error),
    }
    Ok(())
}

fn busy(terminal: &mut DefaultTerminal, app: &mut App, status: &str) -> Result<()> {
    app.status = status.to_string();
    terminal.draw(|frame| ui::draw(frame, app))?;
    Ok(())
}

/// OSC 52 puts the text on the clipboard of the terminal, also over ssh.
fn copy(app: &mut App, text: &str) -> Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut stdout = std::io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()?;
    app.status = format!("copied {} chars", text.chars().count());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new(Picker::new(std::path::PathBuf::new(), Vec::new(), Default::default()))
    }

    fn press(app: &mut App, code: KeyCode) -> Action {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn tree_opens_items_and_pages_records() {
        let mut app = app();
        app.connected("sqlite://x.db", vec![Group { name: "main".into(), items: vec!["a".into(), "b".into()] }]);
        assert_eq!(app.nodes().len(), 3);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(press(&mut app, KeyCode::Enter), Action::Browse);
        assert_eq!(app.opened_names(), Some(("main", "b")));
        app.show_records(Grid { columns: vec!["x".into()], rows: vec![vec!["1".into()]; PAGE] });
        assert_eq!(press(&mut app, KeyCode::Char(']')), Action::Browse);
        assert_eq!(app.page, 1);
        app.show_records(Grid::default());
        assert_eq!(press(&mut app, KeyCode::Char(']')), Action::None);
        assert_eq!(press(&mut app, KeyCode::Char('[')), Action::Browse);
        assert_eq!(app.page, 0);
    }

    #[test]
    fn q_types_into_the_editor_instead_of_quitting() {
        let mut app = app();
        app.connected("sqlite://x.db", Vec::new());
        press(&mut app, KeyCode::Char('2'));
        press(&mut app, KeyCode::Char('e'));
        assert_eq!(press(&mut app, KeyCode::Char('q')), Action::None);
        assert_eq!(app.editor.text(), "q");
        press(&mut app, KeyCode::Esc);
        assert_eq!(press(&mut app, KeyCode::Char('q')), Action::Quit);
    }

    #[test]
    fn ctrl_p_opens_the_palette_and_esc_closes_it_once_connected() {
        let mut app = app();
        assert!(app.palette);
        press(&mut app, KeyCode::Char('?'));
        assert!(app.help);
        press(&mut app, KeyCode::Esc);
        app.connected("sqlite://x.db", Vec::new());
        assert!(!app.palette);
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert!(app.palette && app.picker.filtering);
        assert_eq!(press(&mut app, KeyCode::Char('q')), Action::None);
        assert_eq!(app.picker.filter, "q");
        press(&mut app, KeyCode::Esc);
        assert_eq!(press(&mut app, KeyCode::Esc), Action::None);
        assert!(!app.palette);
        assert_eq!(press(&mut app, KeyCode::Char('q')), Action::Quit);
    }

    #[test]
    fn errors_hide_secrets() {
        let mut app = app();
        app.connected("d1://acct/db?token=abc123", Vec::new());
        app.fail(anyhow::anyhow!("rejected token abc123"));
        assert_eq!(app.status, "error: rejected token ***");
    }
}
