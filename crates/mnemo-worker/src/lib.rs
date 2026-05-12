use mnemo_application::MnemoApp;

#[derive(Clone)]
pub struct WorkerRuntime {
    _app: MnemoApp,
}

impl WorkerRuntime {
    pub fn new(app: MnemoApp) -> Self {
        Self { _app: app }
    }

    pub async fn run_once(&self) -> anyhow::Result<usize> {
        let jobs = self._app.run_jobs_once(20).await?;
        let outbox = self._app.run_outbox_once(100).await?;
        let processed = jobs + outbox;
        if processed > 0 {
            tracing::debug!(jobs, outbox, processed, "processed worker tasks");
        }
        Ok(processed)
    }

    pub async fn run_loop(self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            if let Err(error) = self.run_once().await {
                tracing::warn!(%error, "worker run_once failed");
            }
        }
    }
}
