use rsmq_async::{Rsmq, RsmqConnection};
use serde::{Serialize, de::DeserializeOwned};

use crate::errors::CloudError;

pub struct Queue {
    name: String,
    rsmq: Rsmq
}

impl Queue {
    pub async fn new(name: &str, url: &str) -> Result<Self, CloudError> {
        let client = redis::Client::open(url)
            .map_err(|err| CloudError::InternalError(err.to_string()))?;
        let connection = client.get_async_connection().await
            .map_err(|err| CloudError::InternalError(err.to_string()))?;
        
        let mut rsmq = Rsmq::new_with_connection(Default::default(), connection);
        
        let queues = rsmq.list_queues().await
            .map_err(|err| CloudError::InternalError(err.to_string()))?;

        if !queues.contains(&name.to_string()) {
            rsmq.create_queue(name, None, None, None)
                .await
                .map_err(|err| CloudError::InternalError(err.to_string()))?;
        }

        Ok(Queue {
            name: name.to_string(),
            rsmq
        })
    }

    pub async fn send<T: Serialize>(&mut self, item: T) -> Result<(), CloudError> {
        let message = serde_json::to_string(&item)
            .map_err(|err| CloudError::InternalError(err.to_string()))?;
        self.rsmq.send_message(&self.name, message, None)
            .await
            .map_err(|err| CloudError::InternalError(err.to_string()))?;
        Ok(())
    }

    pub async fn receive<T: DeserializeOwned>(&mut self) -> Result<Option<(String, T)>, CloudError> {
        let message = self.rsmq
            .receive_message::<String>(&self.name, None)
            .await
            .map_err(|err| CloudError::InternalError(err.to_string()))?;

        match message {
            Some(message) => {
                let id = message.id;
                let message: T = serde_json::from_str(&message.message)
                    .map_err(|err| CloudError::InternalError(err.to_string()))?;
                Ok(Some((id, message)))
            },
            None => Ok(None),    
        }
    }

    pub async fn delete(&mut self, id: &str) -> Result<(), CloudError> {
        self.rsmq.delete_message(&self.name, id)
            .await
            .map_err(|err| CloudError::InternalError(err.to_string()))?;
        Ok(())
    }
}