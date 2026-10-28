use crate::app::{Action, step};
use crate::config::{self, Connection};
use crate::driver;
use anyhow::{Result, bail};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const LAST_OPENED: usize = 5;
const LABEL_ORDER: [&str; 3] = ["local", "test", "prod"];
pub const MENU: [&str; 3] = ["Edit", "Copy connection string", "Remove"];

/// Date is the order of `config.toml`: new connections are appended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sort {
    Newest,
    Oldest,
    NameAsc,
    NameDesc,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grouping {
    Label,
    Kind,
    None,
}

#[derive(Debug, PartialEq)]
pub enum ListRow {
    LastOpened(usize),
    Group(String),
    Entry(usize),
}

#[derive(Debug, Default)]
pub struct Form {
    /// 0 name, 1 url, 2 label.
    pub field: usize,
    pub name: String,
    pub url: String,
    pub label: String,
    /// The connection being edited; `None` adds a new one.
    pub editing: Option<usize>,
}

impl Form {
    fn edit(connection: &Connection, i: usize) -> Form {
        Form {
            field: 0,
            name: connection.name.clone(),
            url: connection.url.clone(),
            label: connection.label.clone().unwrap_or_default(),
            editing: Some(i),
        }
    }

    fn text(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.name,
            1 => &mut self.url,
            _ => &mut self.label,
        }
    }
}

pub struct Picker {
    pub dir: PathBuf,
    pub connections: Vec<Connection>,
    pub recent: HashMap<String, u64>,
    pub sort: Sort,
    pub grouping: Grouping,
    pub filter: String,
    pub filtering: bool,
    /// Index among the connection rows, group headers are skipped.
    pub cursor: usize,
    pub form: Option<Form>,
    /// The selected item of the open `MENU`.
    pub menu: Option<usize>,
    /// The connection waiting for `y` to confirm its removal.
    pub deleting: Option<usize>,
}

impl Picker {
    pub fn load(dir: PathBuf) -> Result<Picker> {
        let connections = config::load(&dir)?.connections;
        let recent = config::load_recent(&dir)?;
        Ok(Picker::new(dir, connections, recent))
    }

    pub fn new(dir: PathBuf, connections: Vec<Connection>, recent: HashMap<String, u64>) -> Picker {
        Picker {
            dir,
            connections,
            recent,
            sort: Sort::Newest,
            grouping: Grouping::Label,
            filter: String::new(),
            filtering: false,
            cursor: 0,
            form: None,
            menu: None,
            deleting: None,
        }
    }

    /// Last opened first (hidden while filtering), then the connections, grouped unless `Grouping::None`.
    pub fn rows(&self) -> Vec<ListRow> {
        let mut matching: Vec<usize> = (0..self.connections.len()).filter(|&i| self.matches(i)).collect();
        matching.sort_by(|&a, &b| self.compare(a, b));

        let mut rows = Vec::new();
        if self.filter.is_empty() {
            let mut opened: Vec<usize> = matching.iter().copied().filter(|&i| self.last_used(i).is_some()).collect();
            opened.sort_by_key(|&i| Reverse(self.last_used(i)));
            opened.truncate(LAST_OPENED);
            rows.extend(opened.into_iter().map(ListRow::LastOpened));
        }
        if self.grouping == Grouping::None {
            rows.extend(matching.into_iter().map(ListRow::Entry));
            return rows;
        }

        let mut groups: Vec<String> = matching.iter().map(|&i| self.group_of(i)).collect();
        groups.sort_by_key(|group| (label_rank(self.grouping, group), group.clone()));
        groups.dedup();
        for group in groups {
            let members = matching.iter().copied().filter(|&i| self.group_of(i) == group);
            rows.push(ListRow::Group(group.clone()));
            rows.extend(members.map(ListRow::Entry));
        }
        rows
    }

    pub fn last_used(&self, i: usize) -> Option<u64> {
        self.recent.get(&self.connections[i].name).copied()
    }

    /// The connection under the cursor.
    pub fn selected(&self) -> Option<usize> {
        self.entries().get(self.cursor).copied()
    }

    fn group_of(&self, i: usize) -> String {
        let connection = &self.connections[i];
        match self.grouping {
            Grouping::Label => connection.label().to_string(),
            Grouping::Kind => connection.kind(),
            Grouping::None => String::new(),
        }
    }

    fn matches(&self, i: usize) -> bool {
        let needle = self.filter.to_lowercase();
        let connection = &self.connections[i];
        let fields = [connection.name.clone(), connection.label().to_string(), connection.kind(), connection.redacted_url()];
        fields.iter().any(|field| field.to_lowercase().contains(&needle))
    }

    fn compare(&self, a: usize, b: usize) -> std::cmp::Ordering {
        let name = |i: usize| self.connections[i].name.to_lowercase();
        match self.sort {
            Sort::Newest => b.cmp(&a),
            Sort::Oldest => a.cmp(&b),
            Sort::NameAsc => name(a).cmp(&name(b)),
            Sort::NameDesc => name(b).cmp(&name(a)),
        }
    }

    fn entries(&self) -> Vec<usize> {
        let mut entries = Vec::new();
        for row in self.rows() {
            if let ListRow::LastOpened(i) | ListRow::Entry(i) = row {
                entries.push(i);
            }
        }
        entries
    }

    /// Letters go into a text field, the menu or a y/n prompt instead of being commands.
    pub fn is_typing(&self) -> bool {
        self.filtering || self.form.is_some() || self.menu.is_some() || self.deleting.is_some()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<Action> {
        if let Some(i) = self.deleting.take() {
            if key.code == KeyCode::Char('y') {
                self.delete(i)?;
            }
            return Ok(Action::None);
        }
        if let Some(item) = self.menu {
            return self.menu_key(key, item);
        }
        if self.form.is_some() {
            return self.form_key(key);
        }
        if self.filtering {
            match key.code {
                KeyCode::Esc if self.filter.is_empty() => return Ok(Action::Quit),
                KeyCode::Esc => {
                    self.filter.clear();
                    self.filtering = false;
                }
                KeyCode::Enter => return self.open(),
                KeyCode::Backspace => _ = self.filter.pop(),
                KeyCode::Char(c) => self.filter.push(c),
                KeyCode::Down => self.move_cursor(1),
                KeyCode::Up => self.move_cursor(-1),
                _ => return Ok(Action::None),
            }
            if !matches!(key.code, KeyCode::Down | KeyCode::Up) {
                self.cursor = 0;
            }
            return Ok(Action::None);
        }
        match key.code {
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.cursor = 0;
            }
            KeyCode::Char('q') | KeyCode::Esc => return Ok(Action::Quit),
            KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
            KeyCode::Char('/') => self.filtering = true,
            KeyCode::Char('n') => self.form = Some(Form::default()),
            KeyCode::Char('m' | '.') if self.selected().is_some() => self.menu = Some(0),
            KeyCode::Char('d') => self.deleting = self.selected(),
            KeyCode::Char('x') => self.forget()?,
            KeyCode::Char('s') => {
                self.sort = match self.sort {
                    Sort::Newest => Sort::Oldest,
                    Sort::Oldest => Sort::NameAsc,
                    Sort::NameAsc => Sort::NameDesc,
                    Sort::NameDesc => Sort::Newest,
                }
            }
            KeyCode::Char('g') => {
                self.grouping = match self.grouping {
                    Grouping::Label => Grouping::Kind,
                    Grouping::Kind => Grouping::None,
                    Grouping::None => Grouping::Label,
                }
            }
            KeyCode::Enter => return self.open(),
            _ => {}
        }
        Ok(Action::None)
    }

    /// Ctrl-P: the list comes up ready for typing, like a command palette.
    pub fn open_palette(&mut self) {
        self.filter.clear();
        self.filtering = true;
        self.cursor = 0;
        self.form = None;
        self.menu = None;
    }

    fn move_cursor(&mut self, delta: isize) {
        self.cursor = step(self.cursor, delta, self.entries().len());
    }

    fn open(&mut self) -> Result<Action> {
        let Some(i) = self.selected() else { return Ok(Action::None) };
        let connection = &self.connections[i];
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        self.recent.insert(connection.name.clone(), now);
        config::save_recent(&self.dir, &self.recent)?;
        Ok(Action::Connect(connection.url.clone()))
    }

    fn forget(&mut self) -> Result<()> {
        let Some(i) = self.selected() else { return Ok(()) };
        if self.recent.remove(&self.connections[i].name).is_some() {
            config::save_recent(&self.dir, &self.recent)?;
        }
        self.cursor = self.cursor.min(self.entries().len().saturating_sub(1));
        Ok(())
    }

    fn menu_key(&mut self, key: KeyEvent, item: usize) -> Result<Action> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.menu = Some(step(item, 1, MENU.len())),
            KeyCode::Char('k') | KeyCode::Up => self.menu = Some(step(item, -1, MENU.len())),
            KeyCode::Enter => {
                self.menu = None;
                let Some(i) = self.selected() else { return Ok(Action::None) };
                match item {
                    0 => self.form = Some(Form::edit(&self.connections[i], i)),
                    1 => return Ok(Action::Copy(self.connections[i].url.clone())),
                    _ => self.deleting = Some(i),
                }
            }
            KeyCode::Esc | KeyCode::Char('q' | 'm' | '.') => self.menu = None,
            _ => {}
        }
        Ok(Action::None)
    }

    fn delete(&mut self, i: usize) -> Result<()> {
        let mut remaining = self.connections.clone();
        let removed = remaining.remove(i);
        config::save(&self.dir, &remaining)?;
        self.connections = remaining;
        if self.recent.remove(&removed.name).is_some() {
            config::save_recent(&self.dir, &self.recent)?;
        }
        self.cursor = self.cursor.min(self.entries().len().saturating_sub(1));
        Ok(())
    }

    fn form_key(&mut self, key: KeyEvent) -> Result<Action> {
        let Some(form) = &mut self.form else { return Ok(Action::None) };
        match key.code {
            KeyCode::Esc => self.form = None,
            KeyCode::Enter => self.save()?,
            KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % 3,
            KeyCode::BackTab | KeyCode::Up => form.field = (form.field + 2) % 3,
            KeyCode::Backspace => _ = form.text().pop(),
            KeyCode::Char(c) => form.text().push(c),
            _ => {}
        }
        Ok(Action::None)
    }

    fn save(&mut self) -> Result<()> {
        let Some(form) = &self.form else { return Ok(()) };
        let (name, url, label, editing) = (form.name.trim(), form.url.trim(), form.label.trim(), form.editing);
        if name.is_empty() || url.is_empty() {
            bail!("name and url are required");
        }
        let taken = self.connections.iter().enumerate().any(|(i, c)| c.name == name && Some(i) != editing);
        if taken {
            bail!("a connection named `{name}` already exists");
        }
        driver::family(url)?;
        let connection = Connection {
            name: name.to_string(),
            url: url.to_string(),
            label: (!label.is_empty()).then(|| label.to_string()),
        };

        let Some(i) = editing else {
            config::append(&self.dir, &connection)?;
            self.connections.push(connection);
            self.form = None;
            return Ok(());
        };
        let mut updated = self.connections.clone();
        let old = std::mem::replace(&mut updated[i], connection);
        config::save(&self.dir, &updated)?;
        self.connections = updated;
        if let Some(at) = self.recent.remove(&old.name) {
            self.recent.insert(self.connections[i].name.clone(), at);
            config::save_recent(&self.dir, &self.recent)?;
        }
        self.form = None;
        Ok(())
    }
}

/// Labels local, test, prod first, others alphabetical after them; types alphabetical.
fn label_rank(grouping: Grouping, group: &str) -> usize {
    match grouping {
        Grouping::Label => LABEL_ORDER.iter().position(|label| *label == group).unwrap_or(LABEL_ORDER.len()),
        Grouping::Kind | Grouping::None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn connection(name: &str, url: &str, label: Option<&str>) -> Connection {
        Connection { name: name.into(), url: url.into(), label: label.map(String::from) }
    }

    fn picker(test: &str) -> Picker {
        let connections = vec![
            connection("prod-db", "postgres://u:secret@db.example.com/app", Some("prod")),
            connection("cache", "redis://localhost:6379/0", None),
            connection("ledger", "main.beancount", None),
            connection("stage", "mysql://stage.example.com/app", Some("test")),
            connection("misc", "clickhouse://ch.example.com", None),
        ];
        let recent = HashMap::from([("ledger".to_string(), 10), ("prod-db".to_string(), 20)]);
        Picker::new(std::env::temp_dir().join(format!("dbtui-{test}-{}", std::process::id())), connections, recent)
    }

    fn press(picker: &mut Picker, code: KeyCode) -> Action {
        picker.handle_key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap()
    }

    fn type_text(picker: &mut Picker, text: &str) {
        for c in text.chars() {
            press(picker, KeyCode::Char(c));
        }
    }

    fn groups(picker: &Picker) -> Vec<String> {
        let mut groups = Vec::new();
        for row in picker.rows() {
            if let ListRow::Group(name) = row {
                groups.push(name);
            }
        }
        groups
    }

    #[test]
    fn last_opened_first_then_labels_in_fixed_order_newest_first() {
        let picker = picker("order");
        assert_eq!(groups(&picker), ["local", "test", "prod", "other"]);
        assert_eq!(picker.entries(), [0, 2, 2, 1, 3, 0, 4]);
    }

    #[test]
    fn grouping_cycles_and_sort_by_name() {
        let mut picker = picker("grouping");
        press(&mut picker, KeyCode::Char('g'));
        assert_eq!(groups(&picker), ["beancount", "clickhouse", "mysql", "postgres", "redis"]);
        press(&mut picker, KeyCode::Char('g'));
        press(&mut picker, KeyCode::Char('s'));
        press(&mut picker, KeyCode::Char('s'));
        assert_eq!(picker.sort, Sort::NameAsc);
        assert!(groups(&picker).is_empty());
        assert_eq!(picker.entries(), [0, 2, 1, 2, 4, 0, 3]);
    }

    #[test]
    fn filter_hides_last_opened_and_never_matches_passwords() {
        let mut picker = picker("filter");
        press(&mut picker, KeyCode::Char('/'));
        type_text(&mut picker, "secret");
        assert!(picker.rows().is_empty());
        press(&mut picker, KeyCode::Esc);
        press(&mut picker, KeyCode::Char('/'));
        type_text(&mut picker, "example");
        assert_eq!(groups(&picker), ["test", "prod", "other"]);
        assert_eq!(press(&mut picker, KeyCode::Char('q')), Action::None);
        assert_eq!(picker.filter, "exampleq");
    }

    #[test]
    fn opening_records_last_use_and_x_forgets_it() {
        let mut picker = picker("open");
        press(&mut picker, KeyCode::Char('j'));
        assert_eq!(press(&mut picker, KeyCode::Enter), Action::Connect("main.beancount".into()));
        assert!(picker.last_used(2) > Some(20));
        assert_eq!(picker.entries()[0], 2);

        press(&mut picker, KeyCode::Char('k'));
        press(&mut picker, KeyCode::Char('x'));
        assert_eq!(picker.last_used(2), None);
        assert!(!config::load_recent(&picker.dir).unwrap().contains_key("ledger"));
        std::fs::remove_dir_all(&picker.dir).unwrap();
    }

    #[test]
    fn new_form_validates_and_saves() {
        let mut picker = picker("add");
        press(&mut picker, KeyCode::Char('n'));
        type_text(&mut picker, "cache");
        press(&mut picker, KeyCode::Tab);
        type_text(&mut picker, "sqlite://x.db");
        let duplicate = picker.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(duplicate.unwrap_err().to_string().contains("already exists"));

        press(&mut picker, KeyCode::BackTab);
        type_text(&mut picker, "2");
        press(&mut picker, KeyCode::Up);
        type_text(&mut picker, "staging");
        press(&mut picker, KeyCode::Enter);
        assert!(picker.form.is_none());
        assert_eq!(picker.connections.last(), Some(&connection("cache2", "sqlite://x.db", Some("staging"))));
        assert_eq!(config::load(&picker.dir).unwrap().connections.last(), picker.connections.last());
        std::fs::remove_dir_all(&picker.dir).unwrap();
    }

    #[test]
    fn menu_edit_renames_and_keeps_last_use() {
        let mut picker = picker("edit");
        press(&mut picker, KeyCode::Char('m'));
        press(&mut picker, KeyCode::Enter);
        assert_eq!(picker.form.as_ref().unwrap().editing, Some(0));
        type_text(&mut picker, "2");
        press(&mut picker, KeyCode::Enter);
        assert_eq!(picker.connections[0].name, "prod-db2");
        assert_eq!(picker.connections.len(), 5);
        assert_eq!(picker.last_used(0), Some(20));
        assert_eq!(config::load(&picker.dir).unwrap().connections, picker.connections);
        std::fs::remove_dir_all(&picker.dir).unwrap();
    }

    #[test]
    fn menu_copies_full_url() {
        let mut picker = picker("copy");
        press(&mut picker, KeyCode::Char('m'));
        press(&mut picker, KeyCode::Char('j'));
        assert_eq!(press(&mut picker, KeyCode::Enter), Action::Copy("postgres://u:secret@db.example.com/app".into()));
        assert!(!picker.is_typing());
    }

    #[test]
    fn remove_asks_first_and_forgets_last_use() {
        let mut picker = picker("delete");
        press(&mut picker, KeyCode::Char('d'));
        press(&mut picker, KeyCode::Char('n'));
        assert_eq!(picker.connections.len(), 5);

        press(&mut picker, KeyCode::Char('m'));
        press(&mut picker, KeyCode::Char('j'));
        press(&mut picker, KeyCode::Char('j'));
        press(&mut picker, KeyCode::Enter);
        assert!(picker.is_typing());
        press(&mut picker, KeyCode::Char('y'));
        assert!(picker.connections.iter().all(|c| c.name != "prod-db"));
        assert_eq!(config::load(&picker.dir).unwrap().connections, picker.connections);
        assert!(!config::load_recent(&picker.dir).unwrap().contains_key("prod-db"));
        std::fs::remove_dir_all(&picker.dir).unwrap();
    }
}
