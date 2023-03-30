use aws_sdk_rds as rds;
use inquire::InquireError;
use inquire::Select;
use rds::model::DbCluster;
// use rds::model::DbInstance;
use rds::Client;
use rds::Error;
use tokio_stream::StreamExt;

async fn describe_clusters(client: &Client) -> Result<Vec<DbCluster>, Error> {
    let paginator = client
        .describe_db_clusters()
        .into_paginator()
        .items()
        .send();

    let clusters = paginator.collect::<Result<Vec<_>, _>>().await?;

    Ok(clusters)
}

fn choose_cluster(clusters: Vec<DbCluster>) -> Result<DbCluster, InquireError> {
    let identifiers = clusters
        .iter()
        .map(|db| db.db_cluster_identifier().unwrap())
        .collect();

    let ans = Select::new("Choose a cluster to snapshot", identifiers).prompt()?;

    Ok(clusters
        .iter()
        .find(|db| db.db_cluster_identifier().unwrap() == ans)
        .unwrap()
        .to_owned())
}

// async fn describe_instances(client: &Client) -> Result<Vec<DbInstance>, Error> {
//     let paginator = client
//         .describe_db_instances()
//         .into_paginator()
//         .items()
//         .send();
//
//     let instances = paginator.collect::<Result<Vec<_>, _>>().await?;
//
//     Ok(instances)
// }

#[tokio::main]
async fn main() -> Result<(), rds::Error> {
    let config = aws_config::load_from_env().await;
    let client = rds::Client::new(&config);

    let clusters = describe_clusters(&client).await.unwrap();

    let cluster = choose_cluster(clusters).unwrap();

    println!("{}", cluster.db_cluster_identifier().unwrap());

    // println!("\nPrinting databases\n");
    // let instances = describe_instances(&client).await.unwrap();
    //
    // for instance in instances {
    //     println!("{}", instance.db_instance_identifier().unwrap());
    // }

    Ok(())
}
