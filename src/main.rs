use aws_config::SdkConfig;
use aws_sdk_kms as kms;
use aws_sdk_rds as rds;
use clap::{Parser, ValueEnum};
use inquire::InquireError;
use inquire::Select;
use kms::model::AliasListEntry;
use kms::model::KeyListEntry;
use std::collections::HashMap;
use std::fmt;
use tokio::join;
use tokio_stream::StreamExt;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    db_identifier: Option<String>,

    #[arg(short, long)]
    kms_key_id: Option<String>,

    #[arg(value_enum, short = 't', long, default_value_t = DatabaseType::Database)]
    db_type: DatabaseType,

    #[arg()]
    account_ids: Option<Vec<String>>,
}

#[derive(ValueEnum, Clone)]
enum DatabaseType {
    Cluster,
    Database,
}

impl fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DatabaseType::Cluster => write!(f, "cluster"),
            DatabaseType::Database => write!(f, "database"),
        }
    }
}

struct RDS {
    client: rds::Client,
}

impl RDS {
    fn new(config: &SdkConfig) -> RDS {
        RDS {
            client: rds::Client::new(config),
        }
    }

    async fn describe_instances(&self) -> Result<Vec<String>, rds::Error> {
        let paginator = self
            .client
            .describe_db_instances()
            .into_paginator()
            .items()
            .send();

        let instances = paginator.collect::<Result<Vec<_>, _>>().await?;

        Ok(instances
            .iter()
            .filter(|db| db.db_cluster_identifier().is_none())
            .map(|db| db.db_instance_identifier().unwrap().to_string())
            .collect())
    }

    async fn describe_clusters(&self) -> Result<Vec<String>, rds::Error> {
        let paginator = self
            .client
            .describe_db_clusters()
            .into_paginator()
            .items()
            .send();

        let clusters = paginator.collect::<Result<Vec<_>, _>>().await?;

        Ok(clusters
            .iter()
            .map(|db| db.db_cluster_identifier().unwrap().to_string())
            .collect())
    }
}

enum KeyType {
    AWS,
    Custom,
}

struct Key {
    id: String,
    alias: Option<String>,
}

struct KMS {
    client: kms::Client,
}

impl KMS {
    fn new(config: &SdkConfig) -> KMS {
        KMS {
            client: kms::Client::new(config),
        }
    }

    async fn list_aliases(&self) -> Result<Vec<AliasListEntry>, kms::Error> {
        let paginator = self.client.list_aliases().into_paginator().items().send();

        Ok(paginator.collect::<Result<Vec<_>, _>>().await?)
    }

    async fn list_all_keys(&self) -> Result<Vec<KeyListEntry>, kms::Error> {
        let paginator = self.client.list_keys().into_paginator().items().send();

        Ok(paginator.collect::<Result<Vec<_>, _>>().await?)
    }

    async fn list_keys(&self) -> Result<Vec<Key>, kms::Error> {
        let aliases_future = self.list_aliases();
        let keys_future = self.list_all_keys();

        let (aliases, keys) = join!(aliases_future, keys_future);

        let aliases = aliases.unwrap();
        let alias_map: HashMap<_, _> = aliases
            .iter()
            .map(|al| {
                let id = al.target_key_id().unwrap_or_default().to_string();

                (id, al)
            })
            .collect();

        let mut customer_managed_keys: Vec<Key> = vec![];

        for key in keys.unwrap() {
            let id = key.key_id().unwrap();

            let (key_type, alias) = match alias_map.get(id) {
                Some(&key_alias) => {
                    let name = key_alias.alias_name().unwrap().to_string();

                    if name.starts_with("alias/aws") {
                        (KeyType::AWS, Some(name))
                    } else {
                        (KeyType::Custom, Some(name))
                    }
                }
                None => (KeyType::Custom, None),
            };

            match key_type {
                KeyType::Custom => customer_managed_keys.push(Key {
                    id: id.to_string(),
                    alias,
                }),
                _ => (),
            }
        }

        Ok(customer_managed_keys)
    }
}

fn select(prompt: &str, choices: Vec<String>) -> Result<String, InquireError> {
    let ans = Select::new(prompt, choices.clone()).prompt()?;

    let index = choices.iter().position(|c| c.to_string() == ans).unwrap();

    Ok(choices[index].clone())
}

fn select_rds(identifiers: Vec<String>) -> Result<String, InquireError> {
    select("Please choose an RDS", identifiers)
}

fn select_keys(keys: Vec<Key>) -> Result<String, InquireError> {
    let keys: HashMap<_, _> = keys
        .iter()
        .map(|key| {
            let id = &key.id;

            match &key.alias {
                Some(alias) => (alias.clone(), key),
                None => (id.clone(), key),
            }
        })
        .collect();

    let ans = select(
        &"Choose a KMS key to use for snapshot",
        keys.keys().cloned().collect(),
    )
    .unwrap();

    let key = keys.get(&ans);

    Ok(key.unwrap().id.clone())
}

#[tokio::main]
async fn main() -> Result<(), rds::Error> {
    let args = Args::parse();

    let config = aws_config::load_from_env().await;
    let rds = RDS::new(&config);
    let kms = KMS::new(&config);

    let identifier = match args.db_identifier {
        Some(id) => Ok(id),
        None => {
            let identifiers = match args.db_type {
                DatabaseType::Database => rds.describe_instances().await.unwrap(),
                DatabaseType::Cluster => rds.describe_clusters().await.unwrap(),
            };

            select_rds(identifiers)
        }
    };

    let kms_key_id = match args.kms_key_id {
        Some(kms_key_id) => Ok(kms_key_id),
        None => {
            let keys = kms.list_keys().await.unwrap();

            select_keys(keys)
        }
    };

    println!("{}", identifier.unwrap());
    println!("{}", kms_key_id.unwrap());

    // println!("\nPrinting databases\n");
    // let instances = describe_instances(&client).await.unwrap();
    //
    // for instance in instances {
    //     println!("{}", instance.db_instance_identifier().unwrap());
    // }

    Ok(())
}
