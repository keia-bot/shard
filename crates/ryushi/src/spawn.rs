use ryushi_protocol::command::CloseStyle;
use ryushi_protocol::info::ShardInfo;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use twilight_gateway::queue::Queue;
use twilight_gateway::{Config, ConfigBuilder, EventTypeFlags, Shard, ShardId};

use crate::data::DataLayer;
use crate::event_handler::EventHandler;
use crate::event_loop::EventLoop;
use crate::sidecar::SidecarRef;

/// Represents a collection of shards and their associated tasks.
pub struct Shards {
    close: Vec<oneshot::Sender<CloseStyle>>,
    tasks: JoinSet<()>
}

impl Shards {
    /// Spawns a collection of shards with their associated event handlers and
    /// sidecars.
    ///
    /// # Arguments
    ///
    /// * `shards` - A vector of tuples containing the shard and its associated
    ///   information.
    /// * `published_events` - The event types that the shards will publish.
    /// * `event_handler` - A function that returns an event handler for each
    ///   shard.
    /// * `sidecar_factory` - A function that creates a sidecar for each shard.
    ///
    /// # Returns
    ///
    /// A [`Shards`] instance containing the spawned tasks and channels to close
    /// them.
    pub fn spawn<Q, E, DL, EF, SF>(
        shards: Vec<(Shard<Q>, ShardInfo)>, published_events: EventTypeFlags, event_handler: EF,
        sidecar_factory: SF
    ) -> Self
    where
        Q: Queue + Unpin + Send + 'static,
        E: EventHandler + 'static,
        DL: DataLayer,
        EF: Fn(&Shard<Q>) -> E,
        SF: Fn(&Shard<Q>) -> SidecarRef<DL>
    {
        tracing::info!("Spawning {} shards", shards.len());

        let mut set = JoinSet::new();

        let mut txs = Vec::with_capacity(shards.len());

        for (shard, info) in shards {
            let (tx, rx) = oneshot::channel();

            txs.push(tx);

            set.spawn(
                EventLoop::new(
                    published_events,
                    event_handler(&shard),
                    sidecar_factory(&shard),
                    shard
                )
                .run(info, rx)
            );
        }

        Self { close: txs, tasks: set }
    }

    /// Creates a list of shards with their associated information.
    pub async fn create_list<Q, I, DL>(
        cluster_id: Option<String>, config: Config<Q>, shard_count: usize, shard_ids: I,
        data_layer: &DL
    ) -> Result<Vec<(Shard<Q>, ShardInfo)>, DL::Error>
    where
        Q: Queue + Clone,
        I: IntoIterator<Item = ShardId>,
        DL: DataLayer
    {
        let mut shards: Vec<(Shard<Q>, ShardInfo)> = Vec::with_capacity(shard_count);
        for shard_id in shard_ids {
            // fetch the session
            let info = data_layer.shard_load(shard_id).await?;

            // configure the shard.
            let mut shard_config = ConfigBuilder::<Q>::from(config.clone());
            if let Some(ShardInfo { session: Some(session), .. }) = &info {
                shard_config = shard_config.session(session.clone());
            }

            if let Some(ShardInfo { resume_url: Some(resume_url), .. }) = &info {
                shard_config = shard_config.resume_url(resume_url.clone());
            }

            let shard = Shard::<Q>::with_config(shard_id, shard_config.build());
            shards.push((
                shard,
                info.unwrap_or_else(|| ShardInfo::new(cluster_id.clone(), shard_id))
            ));
        }

        Ok(shards)
    }

    /// Closes all shards and waits for their tasks to finish.
    ///
    /// # Arguments
    ///
    /// * `style` - The style of closing the shards, which can be either
    ///   [`CloseStyle::Normal`] or [`CloseStyle::Resume`].
    pub async fn close(mut self, style: CloseStyle) {
        for tx in self.close {
            let _ = tx.send(style);
        }

        // wait for all shards to close
        while self.tasks.join_next().await.is_some() {}
    }
}
