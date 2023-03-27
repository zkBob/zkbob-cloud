use std::{sync::Arc, collections::HashSet};

use tokio::sync::{RwLock, SemaphorePermit, Semaphore, TryAcquireError};

pub struct TaskSemaphore {
    in_progress: Arc<RwLock<HashSet<String>>>,
    semaphore: Semaphore
}

impl TaskSemaphore {
    pub fn new(permits: usize) -> TaskSemaphore {
        TaskSemaphore {
            in_progress: Arc::new(RwLock::new(HashSet::new())),
            semaphore: Semaphore::new(permits),
        }
    }

    pub async fn try_acquire(&self, id: &str) -> Result<TaskSemaphorePermit, TryAcquireError> {
        let mut in_progress = self.in_progress.write().await;
        if in_progress.contains(id) {
            return Err(TryAcquireError::NoPermits)
        }

        let permit = self.semaphore.try_acquire()?;
        in_progress.insert(id.to_string());

        Ok(TaskSemaphorePermit {
            id: id.to_string(),
            in_progress: self.in_progress.clone(),
            permit,
        })
    }
}

pub struct TaskSemaphorePermit<'a> {
    id: String,
    in_progress: Arc<RwLock<HashSet<String>>>,
    #[allow(dead_code)]
    permit: SemaphorePermit<'a>
}

impl Drop for TaskSemaphorePermit<'_> {
    fn drop(&mut self) {
        let id = self.id.clone();
        let in_progress = self.in_progress.clone();
        tokio::spawn(async move {
            in_progress.write().await.remove(&id);
        });
    }
}