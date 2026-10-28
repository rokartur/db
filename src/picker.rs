use crate::app::{Action, step};
use crate::config::{self, Connection};
use crate::driver;
use anyhow::{Result, bail};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const RECENT: usize = 5;
/// Env choices in the add form; `auto` leaves `env` unset so localhost and files become `local`.
pub const ENVS: [&str; 3] = ["auto", "test", "prod"];
const ENV_ORDER: [&str; 4] = ["local", "test", "prod", "other"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sort {
    LastUsed,
    Name,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grouping {
    Env,
    Kind,
}

#[derive(Debug, PartialEq)]
pub enum ListRow {
    Header(String, usize),
    Entry(usize),
}

#[derive(Debug, Default)]
pub struct Form {
    /// 0 name, 1 url, 2 env.
    pub field: usize,
    pub name: String,
    pub url: String,
    pub env: usize,
}

pub struct Picker {
    pub dir: PathBuf,
    pub connections: Vec<Connection>,
    pub recent: HashMap<String, u64>,
    pub sort: Sort,
    pub grouping: Grouping,
    pub filter: String,
    pub filtering: bool,
    /// Index among the `ListRow::Entry` rows, headers are skipped.
    pub cursor: usize,
    pub form: Option<Form>,
    /// The connection waiting for `y` to confirm its deletion.
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
            sort: Sort::LastUsed,
            grouping: Grouping::Env,
            filter: String::new(),
            filtering: false,
            cursor: 0,
            form: None,
            deleting: None,
        }
    }

    /// Recent first (hidden while filtering), then one section per group.
    pub fn rows(&self) -> Vec<ListRow> {
        let mut matching: Vec<usize> = (0..self.connections.len()).filter(|&i| self.matches(i)).collect();
        matching.sort_by(|&a, &b| self.compare(a, b));

        let mut rows = Vec::new();
        if self.filter.is_empty() {
            let mut recent: Vec<usize> = matching.iter().copied().filter(|&i| self.last_used(i).is_some()).collect();
            recent.sort_by_key(|&i| Reverse(self.last_used(i)));
            recent.truncate(RECENT);
            if !recent.is_empty() {
                rows.push(ListRow::Header("Recent".into(), recent.len()));
                rows.extend(recent.into_iter().map(ListRow::Entry));
            }
        }

        let mut groups: Vec<String> = matching.iter().map(|&i| self.group_of(i)).collect();
        groups.sort_by_key(|group| (group_rank(self.grouping, group), group.clone()));
        groups.dedup();
        for group in groups {
            let members: Vec<usize> = matching.iter().copied().filter(|&i| self.group_of(i) == group).collect();
            rows.push(ListRow::Header(group, members.len()));
            rows.extend(members.into_iter().map(ListRow::Entry));
        }
        rows
    }

    pub fn last_used(&self, i: usize) -> Option<u64> {
        self.recent.get(&self.connections[i].name).copied()
    }

    fn group_of(&self, i: usize) -> String {
        let connection = &self.connections[i];
        match self.grouping {
            Grouping::Env => connection.env().to_string(),
            Grouping::Kind => connection.kind(),
        }
    }

    fn matches(&self, i: usize) -> bool {
        let needle = self.filter.to_lowercase();
        let connection = &self.connections[i];
        let fields = [connection.name.clone(), connection.env().to_string(), connection.kind(), connection.redacted_url()];
        fields.iter().any(|field| field.to_lowercase().contains(&needle))
    }

    fn compare(&self, a: usize, b: usize) -> std::cmp::Ordering {
        let name = |i: usize| self.connections[i].name.to_lowercase();
        match self.sort {
            Sort::LastUsed => self.last_used(b).cmp(&self.last_used(a)).then_with(|| name(a).cmp(&name(b))),
            Sort::Name => name(a).cmp(&name(b)),
            Sort::Kind => self.connections[a].kind().cmp(&self.connections[b].kind()).then_with(|| name(a).cmp(&name(b))),
        }
    }

    fn entries(&self) -> Vec<usize> {
        let rows = self.rows();
        let mut entries = Vec::new();
        for row in rows {
            if let ListRow::Entry(i) = row {
                entries.push(i);
            }
        }
        entries
    }

    /// Letters go into a text field or a y/n prompt instead of being commands.
    pub fn is_typing(&self) -> bool {
        self.filtering || self.form.is_some() || self.deleting.is_some()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<Action> {
        if let Some(i) = self.deleting.take() {
            if key.code == KeyCode::Char('y') {
                self.delete(i)?;
            }
            return Ok(Action::None);
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
            KeyCode::Char('a') => self.form = Some(Form::default()),
            KeyCode::Char('d') => self.deleting = self.entries().get(self.cursor).copied(),
            KeyCode::Char('s') => {
                self.sort = match self.sort {
                    Sort::LastUsed => Sort::Name,
                    Sort::Name => Sort::Kind,
                    Sort::Kind => Sort::LastUsed,
                }
            }
            KeyCode::Char('g') => {
                self.grouping = match self.grouping {
                    Grouping::Env => Grouping::Kind,
                    Grouping::Kind => Grouping::Env,
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
    }

    fn move_cursor(&mut self, delta: isize) {
        self.cursor = step(self.cursor, delta, self.entries().len());
    }

    fn open(&mut self) -> Result<Action> {
        let Some(&i) = self.entries().get(self.cursor) else { return Ok(Action::None) };
        let connection = &self.connections[i];
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        self.recent.insert(connection.name.clone(), now);
        config::save_recent(&self.dir, &self.recent)?;
        Ok(Action::Connect(connection.url.clone()))
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
            KeyCode::Enter => return self.save(),
            KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % 3,
            KeyCode::BackTab | KeyCode::Up => form.field = (form.field + 2) % 3,
            KeyCode::Right | KeyCode::Char(' ') if form.field == 2 => form.env = (form.env + 1) % ENVS.len(),
            KeyCode::Left if form.field == 2 => form.env = (form.env + ENVS.len() - 1) % ENVS.len(),
            KeyCode::Backspace if form.field == 0 => _ = form.name.pop(),
            KeyCode::Backspace if form.field == 1 => _ = form.url.pop(),
            KeyCode::Char(c) if form.field == 0 => form.name.push(c),
            KeyCode::Char(c) if form.field == 1 => form.url.push(c),
            _ => {}
        }
        Ok(Action::None)
    }

    fn save(&mut self) -> Result<Action> {
        let Some(form) = &self.form else { return Ok(Action::None) };
        let (name, url) = (form.name.trim(), form.url.trim());
        if name.is_empty() || url.is_empty() {
            bail!("name and url are required");
        }
        if self.connections.iter().any(|c| c.name == name) {
            bail!("a connection named `{name}` already exists");
        }
        driver::family(url)?;
        let env = (form.env > 0).then(|| ENVS[form.env].to_string());
        let connection = Connection { name: name.to_string(), url: url.to_string(), env };
        config::append(&self.dir, &connection)?;
        self.connections.push(connection);
        self.form = None;
        Ok(Action::None)
    }
}

/// Envs in a fixed local, test, prod order with unknown ones after; types alphabetical.
fn group_rank(grouping: Grouping, group: &str) -> usize {
    match grouping {
        Grouping::Env => ENV_ORDER.iter().position(|env| *env == group).unwrap_or(ENV_ORDER.len() - 1),
        Grouping::Kind => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn connection(name: &str, url: &str, env: Option<&str>) -> Connection {
        Connection { name: name.into(), url: url.into(), env: env.map(String::from) }
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

    fn headers(picker: &Picker) -> Vec<String> {
        let mut headers = Vec::new();
        for row in picker.rows() {
            if let ListRow::Header(name, _) = row {
                headers.push(name);
            }
        }
        headers
    }

    #[test]
    fn recent_comes_first_then_envs_in_fixed_order() {
        let picker = picker("order");
        assert_eq!(headers(&picker), ["Recent", "local", "test", "prod", "other"]);
        assert_eq!(picker.entries(), [0, 2, 2, 1, 3, 0, 4]);
    }

    #[test]
    fn grouping_by_type_and_sorting_by_name() {
        let mut picker = picker("grouping");
        press(&mut picker, KeyCode::Char('g'));
        press(&mut picker, KeyCode::Char('s'));
        assert_eq!(headers(&picker), ["Recent", "beancount", "clickhouse", "mysql", "postgres", "redis"]);
        assert_eq!(picker.sort, Sort::Name);
    }

    #[test]
    fn filter_hides_recent_and_never_matches_passwords() {
        let mut picker = picker("filter");
        press(&mut picker, KeyCode::Char('/'));
        type_text(&mut picker, "secret");
        assert!(picker.rows().is_empty());
        press(&mut picker, KeyCode::Esc);
        press(&mut picker, KeyCode::Char('/'));
        type_text(&mut picker, "example");
        assert_eq!(headers(&picker), ["test", "prod", "other"]);
        assert_eq!(press(&mut picker, KeyCode::Char('q')), Action::None);
        assert_eq!(picker.filter, "exampleq");
    }

    #[test]
    fn opening_records_last_use() {
        let mut picker = picker("open");
        press(&mut picker, KeyCode::Char('j'));
        assert_eq!(press(&mut picker, KeyCode::Enter), Action::Connect("main.beancount".into()));
        assert!(picker.last_used(2) > Some(20));
        assert_eq!(picker.entries()[0], 2);
        std::fs::remove_dir_all(&picker.dir).unwrap();
    }

    #[test]
    fn add_form_validates_and_saves() {
        let mut picker = picker("add");
        press(&mut picker, KeyCode::Char('a'));
        type_text(&mut picker, "cache");
        press(&mut picker, KeyCode::Tab);
        type_text(&mut picker, "sqlite://x.db");
        let duplicate = picker.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(duplicate.unwrap_err().to_string().contains("already exists"));

        press(&mut picker, KeyCode::BackTab);
        type_text(&mut picker, "2");
        press(&mut picker, KeyCode::Up);
        press(&mut picker, KeyCode::Right);
        press(&mut picker, KeyCode::Right);
        press(&mut picker, KeyCode::Enter);
        assert!(picker.form.is_none());
        assert_eq!(picker.connections.last(), Some(&connection("cache2", "sqlite://x.db", Some("prod"))));
        assert_eq!(config::load(&picker.dir).unwrap().connections.last(), picker.connections.last());
        std::fs::remove_dir_all(&picker.dir).unwrap();
    }

    #[test]
    fn delete_asks_first_and_forgets_last_use() {
        let mut picker = picker("delete");
        press(&mut picker, KeyCode::Char('d'));
        press(&mut picker, KeyCode::Char('n'));
        assert_eq!(picker.connections.len(), 5);

        press(&mut picker, KeyCode::Char('d'));
        assert!(picker.is_typing());
        press(&mut picker, KeyCode::Char('y'));
        assert!(picker.connections.iter().all(|c| c.name != "prod-db"));
        assert_eq!(config::load(&picker.dir).unwrap().connections, picker.connections);
        assert!(!config::load_recent(&picker.dir).unwrap().contains_key("prod-db"));
        std::fs::remove_dir_all(&picker.dir).unwrap();
    }
}
