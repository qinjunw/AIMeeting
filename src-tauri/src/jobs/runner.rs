use futures_util::future::BoxFuture;

use super::{
    JobError, JobKind, JobOutput, JobRunDisposition, JobStatus, PersistentJob, RetryPolicy,
    SqliteJobStore,
};

pub trait JobHandler: Send + Sync {
    fn execute<'a>(&'a self, job: &'a PersistentJob) -> BoxFuture<'a, Result<JobOutput, String>>;
}

pub struct JobRunner<'a, H> {
    store: &'a mut SqliteJobStore,
    handler: &'a H,
    retry_policy: RetryPolicy,
}

impl<'a, H: JobHandler> JobRunner<'a, H> {
    pub fn new(store: &'a mut SqliteJobStore, handler: &'a H, retry_policy: RetryPolicy) -> Self {
        Self {
            store,
            handler,
            retry_policy,
        }
    }

    pub async fn run_next(
        &mut self,
        kind: JobKind,
        updated_at: &str,
    ) -> Result<JobRunDisposition, JobError> {
        let Some(job) = self.store.claim_next(kind, updated_at)? else {
            return Ok(JobRunDisposition::Idle);
        };

        match self.handler.execute(&job).await {
            Ok(output) => self.store.finish_success(&job, output, updated_at),
            Err(error) => {
                let status = self.store.finish_failure(
                    &job,
                    self.retry_policy.max_attempts(),
                    &error,
                    updated_at,
                )?;
                Ok(if status == JobStatus::Queued {
                    JobRunDisposition::Retrying
                } else {
                    JobRunDisposition::Failed
                })
            }
        }
    }
}
