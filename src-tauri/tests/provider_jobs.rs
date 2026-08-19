use std::{
    collections::VecDeque,
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use aimeeting_lib::commands::jobs::{
    processing_job_status, retry_minutes, retry_or_enqueue, retry_transcription,
    ProcessingJobStatusRequest, RetryJobRequest,
};
use aimeeting_lib::commands::providers::{
    delete_provider, list_providers, save_provider, test_provider, EndpointFlavor, ProviderError,
    ProviderService, ProviderTester, ResolvedProviderProfile, SaveProviderRequest,
    SqliteProviderProfileRepository,
};
use aimeeting_lib::gateways::minutes::openai_compatible::{
    normalize_simplified_chinese, parse_structured_minutes, CancellationToken, MinutesClientError,
    MinutesInput, OpenAiCompatibleConfig, OpenAiCompatibleMinutesClient, TextEndpointFlavor,
};
use aimeeting_lib::jobs::{
    EnqueueDisposition, JobHandler, JobKind, JobOutput, JobRunDisposition, JobRunner, JobStatus,
    NewPersistentJob, PersistentJob, RetryPolicy, SqliteJobStore,
};
use aimeeting_lib::persistence::secrets::{MemorySecretStore, SecretStore, SecretString};
use aimeeting_lib::{
    domain::ProviderCapability,
    persistence::{DataPaths, MeetingRepository, NewMeetingRecord},
};
use futures_util::future::BoxFuture;
use tempfile::TempDir;

const NOW: &str = "2026-08-19T08:00:00Z";

fn test_database() -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("temporary directory");
    let paths = DataPaths::new(temp.path()).expect("data paths");
    let path = paths.database_path().to_path_buf();
    MeetingRepository::open(&path).expect("migrated repository");
    (temp, path)
}

fn create_meeting(path: &PathBuf, meeting_id: &str) {
    let repository = MeetingRepository::open(path).expect("repository");
    repository
        .create_meeting(&NewMeetingRecord {
            id: meeting_id.to_string(),
            title: "产品评审".to_string(),
            status: "processing".to_string(),
            transcription_status: "pending".to_string(),
            minutes_status: "pending".to_string(),
            created_at: NOW.to_string(),
        })
        .expect("meeting");
}

#[test]
fn provider_crud_persists_only_non_secret_configuration() {
    let (_temp, path) = test_database();
    let repository = SqliteProviderProfileRepository::open(&path).expect("provider repository");
    let secrets = MemorySecretStore::default();
    let mut service = ProviderService::new(repository, secrets.clone());

    let saved = save_provider(
        &mut service,
        SaveProviderRequest {
            id: "minutes-primary".to_string(),
            capability: ProviderCapability::Minutes,
            name: "主纪要模型".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            model: "qwen-plus".to_string(),
            endpoint_flavor: EndpointFlavor::ChatCompletions,
            is_default: true,
            api_key: Some(SecretString::new("sk-never-persist-this")),
        },
    )
    .expect("save provider");

    assert!(saved.has_secret);
    assert!(!serde_json::to_string(&saved)
        .unwrap()
        .contains("sk-never-persist-this"));
    assert_eq!(list_providers(&service).unwrap(), vec![saved.clone()]);

    let secret_reference = "aimeeting/provider/minutes-primary";
    assert_eq!(
        secrets
            .read(secret_reference)
            .unwrap()
            .expect("stored secret")
            .expose(),
        "sk-never-persist-this"
    );
    let database_bytes = std::fs::read(path).expect("database bytes");
    assert!(!String::from_utf8_lossy(&database_bytes).contains("sk-never-persist-this"));

    delete_provider(&mut service, "minutes-primary").expect("delete provider");
    assert!(list_providers(&service).unwrap().is_empty());
    assert!(secrets.read(secret_reference).unwrap().is_none());
}

#[test]
fn provider_default_is_unique_per_capability_and_secret_debug_is_redacted() {
    let (_temp, path) = test_database();
    let repository = SqliteProviderProfileRepository::open(&path).unwrap();
    let secrets = MemorySecretStore::default();
    let mut service = ProviderService::new(repository, secrets);

    for (id, is_default) in [("minutes-a", true), ("minutes-b", true)] {
        save_provider(
            &mut service,
            SaveProviderRequest {
                id: id.to_string(),
                capability: ProviderCapability::Minutes,
                name: id.to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                model: "model".to_string(),
                endpoint_flavor: EndpointFlavor::Responses,
                is_default,
                api_key: None,
            },
        )
        .unwrap();
    }

    let profiles = list_providers(&service).unwrap();
    assert_eq!(
        profiles.iter().filter(|profile| profile.is_default).count(),
        1
    );
    assert!(profiles
        .iter()
        .any(|profile| profile.id == "minutes-b" && profile.is_default));
    assert_eq!(format!("{:?}", SecretString::new("private")), "[REDACTED]");
}

#[test]
fn retry_command_enqueues_once_when_no_failed_job_exists() {
    let (_temp, path) = test_database();
    create_meeting(&path, "meeting-retry");
    let mut store = SqliteJobStore::open(&path).unwrap();
    let request = RetryJobRequest {
        meeting_id: "meeting-retry".to_string(),
        input_revision: Some(1),
        requested_at: NOW.to_string(),
    };

    let first = retry_or_enqueue(&mut store, JobKind::FileTranscription, request.clone()).unwrap();
    let second = retry_or_enqueue(&mut store, JobKind::FileTranscription, request).unwrap();

    assert_eq!(first.status, JobStatus::Queued);
    assert_eq!(second.id, first.id);
    assert_eq!(store.list_for_meeting("meeting-retry").unwrap().len(), 1);
}

#[tokio::test]
async fn provider_test_resolves_the_secret_only_inside_the_backend_probe() {
    let (_temp, path) = test_database();
    let repository = SqliteProviderProfileRepository::open(&path).unwrap();
    let secrets = MemorySecretStore::default();
    let mut service = ProviderService::new(repository, secrets);
    save_provider(
        &mut service,
        SaveProviderRequest {
            id: "asr-primary".to_string(),
            capability: ProviderCapability::LiveTranscription,
            name: "实时转写".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            model: "paraformer-realtime-v2".to_string(),
            endpoint_flavor: EndpointFlavor::RealtimeWebsocket,
            is_default: true,
            api_key: Some(SecretString::new("probe-only-secret")),
        },
    )
    .unwrap();

    let provider = service.resolve("asr-primary").unwrap();
    let result = test_provider(provider, &SecretCheckingProbe).await.unwrap();
    assert_eq!(result.detail, "连接成功");
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("probe-only-secret"));
}

#[test]
fn minutes_enqueue_coalesces_older_queued_revisions() {
    let (_temp, path) = test_database();
    create_meeting(&path, "meeting-1");
    let mut store = SqliteJobStore::open(&path).unwrap();

    assert_eq!(
        store
            .enqueue(NewPersistentJob::new(
                "minutes-1",
                "meeting-1",
                JobKind::Minutes,
                Some(1),
                NOW,
            ))
            .unwrap(),
        EnqueueDisposition::Enqueued
    );
    assert_eq!(
        store
            .enqueue(NewPersistentJob::new(
                "minutes-2",
                "meeting-1",
                JobKind::Minutes,
                Some(2),
                NOW,
            ))
            .unwrap(),
        EnqueueDisposition::Enqueued
    );

    assert_eq!(
        store.job("minutes-1").unwrap().unwrap().status,
        JobStatus::Superseded
    );
    let claimed = store.claim_next(JobKind::Minutes, NOW).unwrap().unwrap();
    assert_eq!(claimed.id, "minutes-2");
    assert!(store.claim_next(JobKind::Minutes, NOW).unwrap().is_none());
}

#[test]
fn file_transcription_enqueue_coalesces_an_active_job_for_the_same_meeting() {
    let (_temp, path) = test_database();
    create_meeting(&path, "meeting-1");
    let mut store = SqliteJobStore::open(&path).unwrap();

    assert_eq!(
        store
            .enqueue(NewPersistentJob::new(
                "transcribe-first",
                "meeting-1",
                JobKind::FileTranscription,
                Some(1),
                NOW,
            ))
            .unwrap(),
        EnqueueDisposition::Enqueued
    );
    assert_eq!(
        store
            .enqueue(NewPersistentJob::new(
                "transcribe-duplicate",
                "meeting-1",
                JobKind::FileTranscription,
                Some(1),
                NOW,
            ))
            .unwrap(),
        EnqueueDisposition::Coalesced
    );
    assert!(store.job("transcribe-duplicate").unwrap().is_none());
}

#[test]
fn startup_requeues_a_job_interrupted_while_running() {
    let (_temp, path) = test_database();
    create_meeting(&path, "meeting-1");
    let mut store = SqliteJobStore::open(&path).unwrap();
    store
        .enqueue(NewPersistentJob::new(
            "transcribe-interrupted",
            "meeting-1",
            JobKind::FileTranscription,
            Some(1),
            NOW,
        ))
        .unwrap();
    let running = store
        .claim_next(JobKind::FileTranscription, NOW)
        .unwrap()
        .unwrap();
    assert_eq!(running.status, JobStatus::Running);

    assert_eq!(store.recover_interrupted(NOW).unwrap(), 1);
    let recovered = store.job("transcribe-interrupted").unwrap().unwrap();
    assert_eq!(recovered.status, JobStatus::Queued);
    assert!(recovered.error_summary.is_some());
}

#[tokio::test]
async fn runner_retries_then_marks_a_persistent_failure() {
    let (_temp, path) = test_database();
    create_meeting(&path, "meeting-1");
    let mut store = SqliteJobStore::open(&path).unwrap();
    store
        .enqueue(NewPersistentJob::new(
            "transcribe-1",
            "meeting-1",
            JobKind::FileTranscription,
            Some(1),
            NOW,
        ))
        .unwrap();
    let handler = FakeHandler::with([
        Err("request failed at https://example.invalid?api_key=query-secret with Bearer sk-sensitive".to_string()),
        Err("provider still unavailable".to_string()),
    ]);
    let mut runner = JobRunner::new(&mut store, &handler, RetryPolicy::new(2));

    assert_eq!(
        runner
            .run_next(JobKind::FileTranscription, NOW)
            .await
            .unwrap(),
        JobRunDisposition::Retrying
    );
    assert_eq!(
        runner
            .run_next(JobKind::FileTranscription, NOW)
            .await
            .unwrap(),
        JobRunDisposition::Failed
    );

    let failed = store.job("transcribe-1").unwrap().unwrap();
    assert_eq!(failed.attempts, 2);
    assert_eq!(failed.status, JobStatus::Failed);
    let summary = failed.error_summary.unwrap();
    assert!(!summary.contains("sk-sensitive"));
    assert!(!summary.contains("query-secret"));

    assert_eq!(
        retry_transcription(
            &mut store,
            RetryJobRequest {
                meeting_id: "meeting-1".to_string(),
                input_revision: Some(1),
                requested_at: NOW.to_string(),
            },
        )
        .unwrap()
        .status,
        JobStatus::Queued
    );
}

#[tokio::test]
async fn successful_file_transcription_is_persisted_at_its_owned_revision() {
    let (_temp, path) = test_database();
    create_meeting(&path, "meeting-1");
    let repository = MeetingRepository::open(&path).unwrap();
    let mut store = SqliteJobStore::open(&path).unwrap();
    store
        .enqueue(NewPersistentJob::new(
            "transcribe-current",
            "meeting-1",
            JobKind::FileTranscription,
            Some(1),
            NOW,
        ))
        .unwrap();
    let handler = FakeHandler::with([Ok(JobOutput::Transcription {
        transcript_revision: 1,
        text: "确认下周发布。".to_string(),
    })]);
    let mut runner = JobRunner::new(&mut store, &handler, RetryPolicy::new(2));

    assert_eq!(
        runner
            .run_next(JobKind::FileTranscription, NOW)
            .await
            .unwrap(),
        JobRunDisposition::Completed
    );
    assert_eq!(
        repository.transcript_for_revision("meeting-1", 1).unwrap(),
        "确认下周发布。"
    );
}

#[tokio::test]
async fn stale_minutes_result_is_superseded_instead_of_overwriting_newer_revision() {
    let (_temp, path) = test_database();
    create_meeting(&path, "meeting-1");
    let repository = MeetingRepository::open(&path).unwrap();
    repository
        .append_transcript_segment("segment-1", "meeting-1", 1, 0, 1000, "旧讨论")
        .unwrap();
    repository
        .append_transcript_segment("segment-2", "meeting-1", 2, 1000, 2000, "新讨论")
        .unwrap();
    let mut store = SqliteJobStore::open(&path).unwrap();
    store
        .enqueue(NewPersistentJob::new(
            "minutes-stale",
            "meeting-1",
            JobKind::Minutes,
            Some(1),
            NOW,
        ))
        .unwrap();
    let handler = FakeHandler::with([Ok(JobOutput::Minutes {
        transcript_revision: 1,
        content: "旧纪要".to_string(),
        provider_label: "test".to_string(),
    })]);
    let mut runner = JobRunner::new(&mut store, &handler, RetryPolicy::new(1));

    assert_eq!(
        runner.run_next(JobKind::Minutes, NOW).await.unwrap(),
        JobRunDisposition::Superseded
    );
    assert!(repository.latest_minutes("meeting-1").unwrap().is_none());
    assert_eq!(
        store.job("minutes-stale").unwrap().unwrap().status,
        JobStatus::Superseded
    );
}

#[tokio::test]
async fn current_minutes_result_is_saved_and_commands_can_retry_a_failed_job() {
    let (_temp, path) = test_database();
    create_meeting(&path, "meeting-1");
    let repository = MeetingRepository::open(&path).unwrap();
    repository
        .append_transcript_segment("segment-1", "meeting-1", 1, 0, 1000, "确定负责人")
        .unwrap();
    let mut store = SqliteJobStore::open(&path).unwrap();
    store
        .enqueue(NewPersistentJob::new(
            "minutes-current",
            "meeting-1",
            JobKind::Minutes,
            Some(1),
            NOW,
        ))
        .unwrap();
    let handler = FakeHandler::with([Ok(JobOutput::Minutes {
        transcript_revision: 1,
        content: "负责人：小李".to_string(),
        provider_label: "test".to_string(),
    })]);
    let mut runner = JobRunner::new(&mut store, &handler, RetryPolicy::new(1));
    assert_eq!(
        runner.run_next(JobKind::Minutes, NOW).await.unwrap(),
        JobRunDisposition::Completed
    );
    assert_eq!(
        repository
            .latest_minutes("meeting-1")
            .unwrap()
            .unwrap()
            .content,
        "负责人：小李"
    );

    store
        .enqueue(NewPersistentJob::new(
            "minutes-failed",
            "meeting-1",
            JobKind::Minutes,
            Some(2),
            NOW,
        ))
        .unwrap();
    let claimed = store.claim_next(JobKind::Minutes, NOW).unwrap().unwrap();
    store
        .finish_failure(&claimed, 1, "temporary error", NOW)
        .unwrap();
    assert_eq!(
        retry_minutes(
            &mut store,
            RetryJobRequest {
                meeting_id: "meeting-1".to_string(),
                input_revision: Some(2),
                requested_at: NOW.to_string(),
            },
        )
        .unwrap()
        .status,
        JobStatus::Queued
    );
    let statuses = processing_job_status(
        &store,
        ProcessingJobStatusRequest {
            meeting_id: "meeting-1".to_string(),
        },
    )
    .unwrap();
    assert!(statuses.iter().any(|job| job.id == "minutes-failed"));
}

#[test]
fn minutes_response_is_structured_and_normalized_to_simplified_chinese() {
    let _supported_flavors = [
        TextEndpointFlavor::ChatCompletions,
        TextEndpointFlavor::Responses,
    ];
    let parsed =
        parse_structured_minutes(r#"{"minutes":"會議決策：負責人後續整理錄音與轉寫問題。"}"#)
            .unwrap();

    assert_eq!(
        normalize_simplified_chinese(&parsed),
        "会议决策：负责人后续整理录音与转写问题。"
    );
}

#[tokio::test]
async fn in_flight_minutes_request_can_be_cancelled_without_waiting_for_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let _connection = listener.accept().unwrap();
        thread::sleep(Duration::from_secs(2));
    });
    let client = OpenAiCompatibleMinutesClient::new(OpenAiCompatibleConfig {
        base_url: format!("http://{address}/v1"),
        model: "test-model".to_string(),
        api_key: SecretString::new("request-secret"),
        endpoint_flavor: TextEndpointFlavor::ChatCompletions,
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    let cancellation = CancellationToken::default();
    let cancel_signal = cancellation.clone();
    let input = MinutesInput {
        meeting_id: "meeting-1".to_string(),
        transcript_revision: 1,
        previous_minutes: String::new(),
        transcript: "讨论发布计划。".to_string(),
    };
    let started = Instant::now();

    let (result, ()) = tokio::join!(client.generate(&input, &cancellation), async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        cancel_signal.cancel();
    });

    assert!(matches!(result, Err(MinutesClientError::Cancelled)));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!format!("{client:?}").contains("request-secret"));
}

struct FakeHandler {
    outcomes: Arc<Mutex<VecDeque<Result<JobOutput, String>>>>,
}

impl FakeHandler {
    fn with(outcomes: impl IntoIterator<Item = Result<JobOutput, String>>) -> Self {
        Self {
            outcomes: Arc::new(Mutex::new(outcomes.into_iter().collect())),
        }
    }
}

impl JobHandler for FakeHandler {
    fn execute<'a>(&'a self, _job: &'a PersistentJob) -> BoxFuture<'a, Result<JobOutput, String>> {
        Box::pin(async move {
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .expect("configured outcome")
        })
    }
}

struct SecretCheckingProbe;

impl ProviderTester for SecretCheckingProbe {
    fn test<'a>(
        &'a self,
        provider: &'a ResolvedProviderProfile,
    ) -> BoxFuture<'a, Result<String, ProviderError>> {
        Box::pin(async move {
            assert_eq!(provider.profile.id, "asr-primary");
            assert_eq!(provider.api_key.expose(), "probe-only-secret");
            Ok("连接成功".to_string())
        })
    }
}
