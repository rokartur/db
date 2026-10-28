use super::{Driver, Grid, Group, PAGE};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use rskafka::client::partition::{Compression, OffsetAt, PartitionClient, UnknownTopicHandling};
use rskafka::client::{Client, ClientBuilder};
use rskafka::record::{Record, RecordAndOffset};
use std::collections::BTreeMap;

const COLUMNS: [&str; 5] = ["partition", "offset", "timestamp", "key", "value"];
const HELP: &str = "commands: topics | create <topic> [partitions] | produce <topic> <value> | consume <topic> [partition] [offset]";

/// Kafka via rskafka: `kafka://broker:9092[,broker2:9092]`.
pub struct Kafka {
    client: Client,
}

impl Kafka {
    pub async fn connect(url: &str) -> Result<Self> {
        let brokers = url.split_once("://").map_or(url, |(_, rest)| rest).trim_end_matches('/');
        let client = ClientBuilder::new(brokers.split(',').map(String::from).collect()).build().await?;
        Ok(Kafka { client })
    }

    async fn partition(&self, topic: &str, partition: i32) -> Result<PartitionClient> {
        Ok(self.client.partition_client(topic, partition, UnknownTopicHandling::Error).await?)
    }

    async fn partitions(&self, topic: &str) -> Result<Vec<i32>> {
        let topics = self.client.list_topics().await?;
        let found = topics.into_iter().find(|t| t.name == topic).with_context(|| format!("no topic `{topic}`"))?;
        Ok(found.partitions.into_iter().collect())
    }

    /// Reads up to `limit` records of one partition, starting at `offset` (or the earliest one).
    async fn read(&self, topic: &str, partition: i32, offset: Option<i64>, limit: usize) -> Result<Vec<Vec<String>>> {
        let client = self.partition(topic, partition).await?;
        let mut offset = match offset {
            Some(offset) => offset,
            None => client.get_offset(OffsetAt::Earliest).await?,
        };
        let end = client.get_offset(OffsetAt::Latest).await?;
        let mut rows = Vec::new();
        while offset < end && rows.len() < limit {
            let (records, _) = client.fetch_records(offset, 1..1_000_000, 500).await?;
            let Some(last) = records.last() else { break };
            offset = last.offset + 1;
            rows.extend(records.iter().map(|r| row(partition, r)));
        }
        rows.truncate(limit);
        Ok(rows)
    }
}

#[async_trait]
impl Driver for Kafka {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let words: Vec<&str> = text.split_whitespace().collect();
        match words.as_slice() {
            ["topics"] => {
                let topics = self.client.list_topics().await?;
                let rows = topics.into_iter().map(|t| vec![t.name, t.partitions.len().to_string()]).collect();
                Ok(Grid { columns: vec!["topic".into(), "partitions".into()], rows })
            }
            ["create", topic, rest @ ..] => {
                let partitions = rest.first().map_or(Ok(1), |n| n.parse()).context("partitions must be a number")?;
                self.client.controller_client()?.create_topic(*topic, partitions, 1, 5000).await?;
                Ok(Grid::message(format!("created {topic}")))
            }
            ["produce", topic, _, ..] => {
                let value = super::after_words(text, 2);
                let record = Record { key: None, value: Some(value.as_bytes().to_vec()), headers: BTreeMap::new(), timestamp: chrono::Utc::now() };
                let offsets = self.partition(topic, 0).await?.produce(vec![record], Compression::NoCompression).await?;
                Ok(Grid::message(format!("offset {}", offsets[0])))
            }
            ["consume", topic, rest @ ..] => {
                let partition = rest.first().map_or(Ok(0), |p| p.parse()).context("partition must be a number")?;
                let offset = rest.get(1).map(|o| o.parse()).transpose().context("offset must be a number")?;
                Ok(Grid { columns: COLUMNS.map(String::from).to_vec(), rows: self.read(topic, partition, offset, PAGE).await? })
            }
            _ => bail!("{HELP}"),
        }
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let mut topics: Vec<String> = self.client.list_topics().await?.into_iter().map(|t| t.name).filter(|n| !n.starts_with("__")).collect();
        topics.sort();
        Ok(vec![Group { name: "topics".into(), items: topics }])
    }

    /// Partitions are read one after another from their earliest offset; a page is 200 records of that sequence.
    async fn browse(&self, _group: &str, topic: &str, page: usize) -> Result<Grid> {
        let wanted = (page + 1) * PAGE;
        let mut rows = Vec::new();
        for partition in self.partitions(topic).await? {
            rows.extend(self.read(topic, partition, None, wanted - rows.len()).await?);
            if rows.len() >= wanted {
                break;
            }
        }
        rows.drain(..rows.len().min(page * PAGE));
        Ok(Grid { columns: COLUMNS.map(String::from).to_vec(), rows })
    }
}

fn row(partition: i32, record: &RecordAndOffset) -> Vec<String> {
    let bytes = |b: &Option<Vec<u8>>| b.as_ref().map_or("NULL".into(), |b| String::from_utf8_lossy(b).into_owned());
    vec![
        partition.to_string(),
        record.offset.to_string(),
        record.record.timestamp.to_rfc3339(),
        bytes(&record.record.key),
        bytes(&record.record.value),
    ]
}
