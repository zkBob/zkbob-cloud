use rsmq_async::{Rsmq, RsmqConnection};
use serde::{de::DeserializeOwned, Serialize};
use zkbob_utils_rs::tracing;

use crate::errors::CloudError;

pub struct Queue {
    name: String,
    redis_url: String,
    rsmq: Rsmq,
}

impl Queue {
    pub async fn new(name: &str, url: &str) -> Result<Self, CloudError> {
        let mut rsmq = Self::init_rsmq(url).await?;

        let queues = rsmq.list_queues().await.map_err(|err| {
            tracing::error!("failed to list redis queues: {}", err);
            CloudError::InternalError(format!("failed to list redis queues"))
        })?;

        if !queues.contains(&name.to_string()) {
            rsmq.create_queue(name, None, None, None)
                .await
                .map_err(|err| {
                    tracing::error!("failed to create {} queue: {}", name, err);
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
            CloudError::InternalError(format!("failed to serialize task"))
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
            CloudError::InternalError(format!("failed to connect to redis"))
        })?;

        let connection = client.get_async_connection().await.map_err(|err| {
            tracing::error!("failed to connect to redis: {}", err);
            CloudError::InternalError(format!("failed to connect to redis"))
        })?;

        Ok(Rsmq::new_with_connection(Default::default(), connection))
    }
}
