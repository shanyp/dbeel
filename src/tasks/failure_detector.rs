use std::{rc::Rc, time::Duration};

use glommio::{spawn_local, timer::sleep, Task};
use log::{error, info};
use rand::{
    seq::{IteratorRandom, SliceRandom},
    thread_rng,
};

use crate::{
    error::Result,
    gossip::GossipEvent,
    messages::{ShardEvent, ShardMessage},
    remote_shard_connection::RemoteShardConnection,
    shards::MyShard,
};

async fn run_failure_detector(my_shard: Rc<MyShard>) -> Result<()> {
    let interval =
        Duration::from_millis(my_shard.args.failure_detection_interval);

    loop {
        sleep(interval).await;

        let mut rng = thread_rng();
        let node = if let Some(node) = my_shard
            .nodes
            .borrow()
            .iter()
            .map(|(_, node)| node)
            .filter(|node| !node.shard_ports.is_empty())
            .choose(&mut rng)
        {
            node.clone()
        } else {
            continue;
        };

        let connection = RemoteShardConnection::new(
            format!(
                "{}:{}",
                node.ip,
                node.shard_ports.choose(&mut rng).unwrap()
            ),
            Duration::from_millis(my_shard.args.remote_shard_connect_timeout),
        );

        if let Err(e) = connection.ping().await {
            my_shard.handle_dead_node(&node.name).await;

            info!(
                "Notifying cluster that we failed to ping '{}': {}",
                connection.address, e
            );

            let gossip_event = GossipEvent::Dead(node.name);

            if let Err(e) = my_shard
                .clone()
                .broadcast_message_to_local_shards(&ShardMessage::Event(
                    ShardEvent::Gossip(gossip_event.clone()),
                ))
                .await
            {
                error!(
                    "Failed to broadcast to local shards, node death event: {}",
                    e
                );
            }

            if let Err(e) = my_shard.gossip(gossip_event).await {
                error!("Failed to gossip node death event: {}", e);
            }
        }
    }
}

pub fn spawn_failure_detector_task(my_shard: Rc<MyShard>) -> Task<Result<()>> {
    spawn_local(async move {
        let result = run_failure_detector(my_shard).await;
        if let Err(e) = &result {
            error!("Error starting failure detector: {}", e);
        }
        result
    })
}
