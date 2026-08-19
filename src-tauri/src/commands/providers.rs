use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::future::BoxFuture;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::ProviderCapability;
use crate::persistence::secrets::{
    PlatformSecretStore, SecretStore, SecretStoreError, SecretString,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointFlavor {
    RealtimeWebsocket,
    AudioTranscriptions,
    ChatCompletions,
    Responses,
}

impl EndpointFlavor {
    fn as_str(self) -> &'static str {
        match self {
            Self::RealtimeWebsocket => "realtime-websocket",
            Self::AudioTranscriptions => "audio-transcriptions",
            Self::ChatCompletions => "chat-completions",
            Self::Responses => "responses",
        }
    }

    fn parse(value: &str) -> Result<Self, ProviderError> {
        match value {
            "realtime-websocket" => Ok(Self::RealtimeWebsocket),
            "audio-transcriptions" => Ok(Self::AudioTranscriptions),
            "chat-completions" => Ok(Self::ChatCompletions),
            "responses" => Ok(Self::Responses),
            _ => Err(ProviderError::InvalidEndpointFlavor(value.to_string())),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderRequest {
    pub id: String,
    pub capability: ProviderCapability,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub endpoint_flavor: EndpointFlavor,
    pub is_default: bool,
    pub api_key: Option<SecretString>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfileDto {
    pub id: String,
    pub capability: ProviderCapability,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub endpoint_flavor: EndpointFlavor,
    pub is_default: bool,
    pub has_secret: bool,
}

#[derive(Clone, Debug)]
pub struct ResolvedProviderProfile {
    pub profile: ProviderProfileDto,
    pub api_key: SecretString,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub provider_id: String,
    pub detail: String,
}

pub trait ProviderTester: Send + Sync {
    fn test<'a>(
        &'a self,
        provider: &'a ResolvedProviderProfile,
    ) -> BoxFuture<'a, Result<String, ProviderError>>;
}

#[derive(Clone, Debug)]
pub struct StoredProviderProfile {
    pub id: String,
    pub capability: ProviderCapability,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub endpoint_flavor: EndpointFlavor,
    pub secret_reference: Option<String>,
    pub is_default: bool,
}

pub trait ProviderProfileRepository {
    fn list(&self) -> Result<Vec<StoredProviderProfile>, ProviderError>;
    fn get(&self, id: &str) -> Result<Option<StoredProviderProfile>, ProviderError>;
    fn save(
        &mut self,
        profile: &StoredProviderProfile,
        updated_at: &str,
    ) -> Result<(), ProviderError>;
    fn delete(&mut self, id: &str) -> Result<Option<StoredProviderProfile>, ProviderError>;
}

pub struct SqliteProviderProfileRepository {
    connection: Connection,
}

impl SqliteProviderProfileRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProviderError> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(Self { connection })
    }
}

impl ProviderProfileRepository for SqliteProviderProfileRepository {
    fn list(&self) -> Result<Vec<StoredProviderProfile>, ProviderError> {
        let mut statement = self.connection.prepare(
            "SELECT id, capability, name, base_url, model, endpoint_flavor,
                    secret_reference, is_default
             FROM provider_profiles ORDER BY capability, name, id",
        )?;
        let rows = statement.query_map([], stored_profile_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn get(&self, id: &str) -> Result<Option<StoredProviderProfile>, ProviderError> {
        self.connection
            .query_row(
                "SELECT id, capability, name, base_url, model, endpoint_flavor,
                        secret_reference, is_default
                 FROM provider_profiles WHERE id = ?1",
                params![id],
                stored_profile_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    fn save(
        &mut self,
        profile: &StoredProviderProfile,
        updated_at: &str,
    ) -> Result<(), ProviderError> {
        let transaction = self.connection.transaction()?;
        if profile.is_default {
            transaction.execute(
                "UPDATE provider_profiles SET is_default = 0, updated_at = ?2
                 WHERE capability = ?1",
                params![capability_as_str(profile.capability), updated_at],
            )?;
        }
        transaction.execute(
            "INSERT INTO provider_profiles (
                id, capability, name, base_url, model, endpoint_flavor, secret_reference,
                is_default, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(id) DO UPDATE SET
                capability = excluded.capability,
                name = excluded.name,
                base_url = excluded.base_url,
                model = excluded.model,
                endpoint_flavor = excluded.endpoint_flavor,
                secret_reference = excluded.secret_reference,
                is_default = excluded.is_default,
                updated_at = excluded.updated_at",
            params![
                profile.id,
                capability_as_str(profile.capability),
                profile.name,
                profile.base_url,
                profile.model,
                profile.endpoint_flavor.as_str(),
                profile.secret_reference,
                profile.is_default,
                updated_at,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn delete(&mut self, id: &str) -> Result<Option<StoredProviderProfile>, ProviderError> {
        let existing = self.get(id)?;
        if existing.is_some() {
            self.connection
                .execute("DELETE FROM provider_profiles WHERE id = ?1", params![id])?;
        }
        Ok(existing)
    }
}

pub struct ProviderService<R, S> {
    repository: R,
    secrets: S,
}

impl<R, S> ProviderService<R, S>
where
    R: ProviderProfileRepository,
    S: SecretStore,
{
    pub fn new(repository: R, secrets: S) -> Self {
        Self {
            repository,
            secrets,
        }
    }

    pub fn list(&self) -> Result<Vec<ProviderProfileDto>, ProviderError> {
        self.repository
            .list()?
            .into_iter()
            .map(|profile| self.to_dto(profile))
            .collect()
    }

    pub fn save(
        &mut self,
        request: SaveProviderRequest,
    ) -> Result<ProviderProfileDto, ProviderError> {
        validate_request(&request)?;
        let existing = self.repository.get(&request.id)?;
        let reference = existing
            .as_ref()
            .and_then(|profile| profile.secret_reference.clone())
            .unwrap_or_else(|| secret_reference(&request.id));
        let previous_secret = self.secrets.read(&reference)?;
        let mut wrote_secret = false;
        if let Some(secret) = request.api_key {
            if secret.is_empty() {
                return Err(ProviderError::InvalidConfiguration(
                    "API key cannot be empty".to_string(),
                ));
            }
            self.secrets.write(&reference, secret)?;
            wrote_secret = true;
        }
        let has_secret = wrote_secret || previous_secret.is_some();
        let stored = StoredProviderProfile {
            id: request.id,
            capability: request.capability,
            name: request.name.trim().to_string(),
            base_url: request.base_url.trim_end_matches('/').to_string(),
            model: request.model.trim().to_string(),
            endpoint_flavor: request.endpoint_flavor,
            secret_reference: has_secret.then_some(reference.clone()),
            is_default: request.is_default,
        };
        if let Err(error) = self.repository.save(&stored, &current_timestamp()) {
            if wrote_secret {
                if let Some(previous) = previous_secret {
                    let _ = self.secrets.write(&reference, previous);
                } else {
                    let _ = self.secrets.delete(&reference);
                }
            }
            return Err(error);
        }
        self.to_dto(stored)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), ProviderError> {
        if let Some(profile) = self.repository.delete(id)? {
            if let Some(reference) = profile.secret_reference {
                self.secrets.delete(&reference)?;
            }
        }
        Ok(())
    }

    pub fn resolve(&self, id: &str) -> Result<ResolvedProviderProfile, ProviderError> {
        let stored = self
            .repository
            .get(id)?
            .ok_or_else(|| ProviderError::NotFound(id.to_string()))?;
        let reference = stored
            .secret_reference
            .as_deref()
            .ok_or(ProviderError::MissingSecret)?;
        let api_key = self
            .secrets
            .read(reference)?
            .ok_or(ProviderError::MissingSecret)?;
        let profile = self.to_dto(stored)?;
        Ok(ResolvedProviderProfile { profile, api_key })
    }

    fn to_dto(&self, profile: StoredProviderProfile) -> Result<ProviderProfileDto, ProviderError> {
        let has_secret = match profile.secret_reference.as_deref() {
            Some(reference) => self.secrets.read(reference)?.is_some(),
            None => false,
        };
        Ok(ProviderProfileDto {
            id: profile.id,
            capability: profile.capability,
            name: profile.name,
            base_url: profile.base_url,
            model: profile.model,
            endpoint_flavor: profile.endpoint_flavor,
            is_default: profile.is_default,
            has_secret,
        })
    }
}

pub fn list_providers<R, S>(
    service: &ProviderService<R, S>,
) -> Result<Vec<ProviderProfileDto>, ProviderError>
where
    R: ProviderProfileRepository,
    S: SecretStore,
{
    service.list()
}

pub fn save_provider<R, S>(
    service: &mut ProviderService<R, S>,
    request: SaveProviderRequest,
) -> Result<ProviderProfileDto, ProviderError>
where
    R: ProviderProfileRepository,
    S: SecretStore,
{
    service.save(request)
}

pub fn delete_provider<R, S>(
    service: &mut ProviderService<R, S>,
    id: &str,
) -> Result<(), ProviderError>
where
    R: ProviderProfileRepository,
    S: SecretStore,
{
    service.delete(id)
}

pub async fn test_provider<T>(
    provider: ResolvedProviderProfile,
    tester: &T,
) -> Result<ProviderTestResult, ProviderError>
where
    T: ProviderTester,
{
    let provider_id = provider.profile.id.clone();
    let detail = tester.test(&provider).await?;
    Ok(ProviderTestResult {
        provider_id,
        detail,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Secret(#[from] SecretStoreError),
    #[error("provider {0} was not found")]
    NotFound(String),
    #[error("provider has no API key")]
    MissingSecret,
    #[error("invalid provider endpoint flavor: {0}")]
    InvalidEndpointFlavor(String),
    #[error("invalid provider capability: {0}")]
    InvalidCapability(String),
    #[error("invalid provider configuration: {0}")]
    InvalidConfiguration(String),
}

fn stored_profile_from_row(row: &Row<'_>) -> rusqlite::Result<StoredProviderProfile> {
    let capability = row.get::<_, String>(1)?;
    let endpoint_flavor = row.get::<_, String>(5)?;
    Ok(StoredProviderProfile {
        id: row.get(0)?,
        capability: parse_capability(&capability).map_err(to_sql_conversion_error)?,
        name: row.get(2)?,
        base_url: row.get(3)?,
        model: row.get(4)?,
        endpoint_flavor: EndpointFlavor::parse(&endpoint_flavor)
            .map_err(to_sql_conversion_error)?,
        secret_reference: row.get(6)?,
        is_default: row.get(7)?,
    })
}

fn capability_as_str(capability: ProviderCapability) -> &'static str {
    match capability {
        ProviderCapability::LiveTranscription => "live_transcription",
        ProviderCapability::FileTranscription => "file_transcription",
        ProviderCapability::Minutes => "minutes",
    }
}

fn parse_capability(value: &str) -> Result<ProviderCapability, ProviderError> {
    match value {
        "live_transcription" => Ok(ProviderCapability::LiveTranscription),
        "file_transcription" => Ok(ProviderCapability::FileTranscription),
        "minutes" => Ok(ProviderCapability::Minutes),
        _ => Err(ProviderError::InvalidCapability(value.to_string())),
    }
}

fn to_sql_conversion_error(error: ProviderError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn validate_request(request: &SaveProviderRequest) -> Result<(), ProviderError> {
    let valid_id = !request.id.is_empty()
        && request.id.len() <= 120
        && request
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid_id {
        return Err(ProviderError::InvalidConfiguration(
            "provider id is invalid".to_string(),
        ));
    }
    if request.name.trim().is_empty()
        || request.base_url.trim().is_empty()
        || request.model.trim().is_empty()
    {
        return Err(ProviderError::InvalidConfiguration(
            "name, base URL, and model are required".to_string(),
        ));
    }
    if !request.base_url.starts_with("https://") && !request.base_url.starts_with("http://") {
        return Err(ProviderError::InvalidConfiguration(
            "base URL must use HTTP or HTTPS".to_string(),
        ));
    }
    Ok(())
}

fn secret_reference(provider_id: &str) -> String {
    format!("aimeeting/provider/{provider_id}")
}

fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

pub struct ProviderState {
    database_path: PathBuf,
}

impl ProviderState {
    pub fn new(database_path: PathBuf) -> Self {
        Self { database_path }
    }

    fn service(
        &self,
    ) -> Result<ProviderService<SqliteProviderProfileRepository, PlatformSecretStore>, ProviderError>
    {
        Ok(ProviderService::new(
            SqliteProviderProfileRepository::open(&self.database_path)?,
            PlatformSecretStore::default(),
        ))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdRequest {
    provider_id: String,
}

#[tauri::command]
pub fn list_provider_profiles(
    state: State<'_, ProviderState>,
) -> Result<Vec<ProviderProfileDto>, String> {
    state
        .service()
        .and_then(|service| list_providers(&service))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_provider_profile(
    state: State<'_, ProviderState>,
    request: SaveProviderRequest,
) -> Result<ProviderProfileDto, String> {
    state
        .service()
        .and_then(|mut service| save_provider(&mut service, request))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_provider_profile(
    state: State<'_, ProviderState>,
    request: ProviderIdRequest,
) -> Result<(), String> {
    state
        .service()
        .and_then(|mut service| delete_provider(&mut service, &request.provider_id))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn test_provider_profile(
    state: State<'_, ProviderState>,
    request: ProviderIdRequest,
) -> Result<ProviderTestResult, String> {
    let provider = state
        .service()
        .and_then(|service| service.resolve(&request.provider_id))
        .map_err(|error| error.to_string())?;
    test_provider(provider, &HttpProviderTester)
        .await
        .map_err(|error| error.to_string())
}

struct HttpProviderTester;

impl ProviderTester for HttpProviderTester {
    fn test<'a>(
        &'a self,
        provider: &'a ResolvedProviderProfile,
    ) -> BoxFuture<'a, Result<String, ProviderError>> {
        Box::pin(async move {
            let endpoint = format!("{}/models", provider.profile.base_url.trim_end_matches('/'));
            let response = reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .build()
                .map_err(|error| ProviderError::InvalidConfiguration(error.to_string()))?
                .get(endpoint)
                .bearer_auth(provider.api_key.expose())
                .send()
                .await
                .map_err(|error| ProviderError::InvalidConfiguration(error.to_string()))?;
            if response.status().is_success() {
                Ok("连接成功".to_string())
            } else {
                Err(ProviderError::InvalidConfiguration(format!(
                    "Provider 返回 {}",
                    response.status()
                )))
            }
        })
    }
}
