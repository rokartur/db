use super::http::{json_to_grid, objects_to_grid};
use super::{Driver, Grid, Group, PAGE};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures::TryStreamExt;
use mongodb::Client;
use mongodb::bson::{Bson, Document, doc};
use serde_json::Value;

const SYSTEM_DATABASES: [&str; 3] = ["admin", "config", "local"];

/// MongoDB: the tree is databases and collections, a query is a database command as JSON.
pub struct Mongo {
    client: Client,
}

impl Mongo {
    pub async fn connect(url: &str) -> Result<Self> {
        Ok(Mongo { client: Client::with_uri_str(url).await? })
    }
}

#[async_trait]
impl Driver for Mongo {
    /// `{"find": "users", "filter": {"age": {"$gt": 30}}}` runs in the database selected in the tree.
    async fn query(&self, group: &str, text: &str) -> Result<Grid> {
        let json: Value = serde_json::from_str(text).context("a MongoDB query is a command document in JSON")?;
        let Bson::Document(command) = Bson::try_from(json)? else { bail!("the command must be a JSON object") };
        let database = if group.is_empty() { "admin" } else { group };
        let reply = self.client.database(database).run_command(command).await?;
        let reply = Bson::Document(reply).into_relaxed_extjson();
        match reply["cursor"]["firstBatch"].as_array() {
            Some(batch) => Ok(objects_to_grid(batch)),
            None => Ok(json_to_grid(&reply)),
        }
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let mut groups = Vec::new();
        for name in self.client.list_database_names().await? {
            if SYSTEM_DATABASES.contains(&name.as_str()) {
                continue;
            }
            let mut items = self.client.database(&name).list_collection_names().await?;
            items.sort();
            groups.push(Group { name, items });
        }
        Ok(groups)
    }

    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        let collection = self.client.database(group).collection::<Document>(item);
        let cursor = collection.find(doc! {}).skip((page * PAGE) as u64).limit(PAGE as i64).await?;
        let documents: Vec<Document> = cursor.try_collect().await?;
        let objects: Vec<Value> = documents.into_iter().map(|d| Bson::Document(d).into_relaxed_extjson()).collect();
        Ok(objects_to_grid(&objects))
    }
}
