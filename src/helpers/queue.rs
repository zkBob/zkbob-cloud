use std::{time::Duration, sync::Arc};

use rsmq_async::{Rsmq, RsmqConnection};
use serde::{de::DeserializeOwned, Serialize};
use tokio::{time, sync::RwLock};
use zkbob_utils_rs::tracing;

use crate::errors::CloudError;

pub struct Queue {
    name: String,
    redis_url: String,
    rsmq: Rsmq,
}

impl Queue {
    pub async fn new(name: &str, url: &str, delay: u32, hidden: u32) -> Result<Self, CloudError> {
        let mut rsmq = Self::init_rsmq(url).await?;

        let queues = rsmq.list_queues().await.map_err(|err| {
            tracing::error!("failed to list redis queues: {}", err);
            CloudError::InternalError("failed to list redis queues".to_string())
        })?;

        if !queues.contains(&name.to_string()) {
            rsmq.create_queue(name, Some(hidden), Some(delay), None)
                .await
                .map_err(|err| {
                    tracing::error!("failed to create {} queue: {}", name, err);
                    CloudError::InternalError(format!("failed to create {} queue", name))
                })?;
        } else {
            rsmq.set_queue_attributes(name, Some(hidden as u64), Some(delay as u64), None)
                .await
                .map_err(|err| {
                    tracing::error!("failed to update {} queue attributes: {}", name, err);
                    CloudError::InternalError(format!("failed to create {} queue", name))
                })?;
        }

        Ok(Queue {
            name: name.to_string(),
            redis_url: url.to_string(),
            rsmq,
        })
    }

    pub async fn reconnect(&mut self) -> Result<(), CloudError> {
        self.rsmq = Self::init_rsmq(&self.redis_url).await?;
        Ok(())
    }

    pub async fn send<T: Serialize>(&mut self, item: T) -> Result<(), CloudError> {
        let message = serde_json::to_string(&item).map_err(|err| {
            tracing::error!("failed to serialize task: {}", err);
            CloudError::InternalError("failed to serialize task".to_string())
        })?;
        self.rsmq
            .send_message(&self.name, message, None)
            .await
            .map_err(|err| {
                tracing::error!("failed to send message to {} queue: {}", &self.name, err);
                CloudError::InternalError(format!("failed to send message to {} queue", &self.name))
            })?;
        Ok(())
    }

    pub async fn receive<T: DeserializeOwned>(
        &mut self,
    ) -> Result<Option<(String, T)>, CloudError> {
        let message = self
            .rsmq
            .receive_message::<String>(&self.name, None)
            .await
            .map_err(|err| {
                tracing::error!("failed to receive message from {} queue: {}", &self.name, err);
                CloudError::InternalError(format!("failed to receive message from {} queue", &self.name))
            })?;

        match message {
            Some(message) => {
                let id = message.id;
                let message: T = serde_json::from_str(&message.message)
                    .map_err(|err| {
                        tracing::error!("failed to deserialize message from {} queue: {}", &self.name, err);
                        CloudError::InternalError(format!("failed to deserialize message from {} queue", &self.name))
                    })?;
                Ok(Some((id, message)))
            }
            None => Ok(None),
        }
    }

    pub async fn delete(&mut self, id: &str) -> Result<(), CloudError> {
        self.rsmq
            .delete_message(&self.name, id)
            .await
            .map_err(|err| {
                tracing::error!("failed to delete message from {} queue: {}", &self.name, err);
                CloudError::InternalError(format!("failed to delete message from {} queue", &self.name))
            })?;
        Ok(())
    }

    async fn init_rsmq(url: &str) -> Result<Rsmq, CloudError> {
        let client = redis::Client::open(url).map_err(|err| {
            tracing::error!("failed to connect to redis: {}", err);
            CloudError::InternalError("failed to connect to redis".to_string())
        })?;

        let connection = client.get_async_connection().await.map_err(|err| {
            tracing::error!("failed to connect to redis: {}", err);
            CloudError::InternalError("failed to connect to redis".to_string())
        })?;

        Ok(Rsmq::new_with_connection(Default::default(), connection))
    }
}

pub async fn receive_blocking<T: DeserializeOwned>(
    queue: Arc<RwLock<Queue>>,
) -> (String, T) {
    loop {
        let task = {
            queue.write().await.receive::<T>().await
        };
        match task {
            Ok(Some(task)) => {
                return task;
            },
            Ok(None) => {
                time::sleep(Duration::from_millis(500)).await;
            },
            Err(_) => {
                match queue.write().await.reconnect().await {
                    Ok(_) => tracing::info!("connection to redis reestablished"),
                    Err(_) => {
                        time::sleep(Duration::from_millis(5000)).await;
                    }
                }
            }
        };
    }
}
