use crate::models::{AnonymousSummary, AreaSummary, RelayBundle, SummaryCount, SyncEnvelope};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::types::Json;
use sqlx::Row;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct EnvelopeStore {
    envelopes: Mutex<HashMap<String, SyncEnvelope>>,
    storage_path: Option<PathBuf>,
}

#[derive(Default)]
pub struct RelayBundleStore {
    bundles: Mutex<HashMap<String, RelayBundle>>,
    storage_path: Option<PathBuf>,
}

pub struct GatewayStore {
    envelope_store: EnvelopeStore,
    relay_bundle_store: RelayBundleStore,
    postgres: Option<PostgresStore>,
}

#[derive(Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl EnvelopeStore {
    pub fn from_env() -> Self {
        env::var("JIRANI_STORE_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .map(Self::from_path)
            .unwrap_or_default()
    }

    pub fn from_path(path: PathBuf) -> Self {
        let envelopes = load_envelopes(&path).unwrap_or_default();
        Self {
            envelopes: Mutex::new(envelopes),
            storage_path: Some(path),
        }
    }

    pub fn upsert(&self, envelope: SyncEnvelope) -> StoreWrite {
        let mut envelopes = self.envelopes.lock().expect("envelope store lock poisoned");
        if envelopes.contains_key(&envelope.envelope_id) {
            return StoreWrite::AlreadyStored;
        }
        let envelope_id = envelope.envelope_id.clone();
        envelopes.insert(envelope_id.clone(), envelope);

        if let Some(path) = &self.storage_path {
            if let Err(error) = persist_envelopes(path, &envelopes) {
                envelopes.remove(&envelope_id);
                return StoreWrite::PersistFailed(error.to_string());
            }
        }

        StoreWrite::Created
    }

    pub fn list(&self) -> Vec<SyncEnvelope> {
        let envelopes = self.envelopes.lock().expect("envelope store lock poisoned");
        let mut values = envelopes.values().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .payload
                .submitted_at_epoch_seconds
                .cmp(&left.payload.submitted_at_epoch_seconds)
        });
        values
    }

    pub fn summary(&self) -> AnonymousSummary {
        let envelopes = self.envelopes.lock().expect("envelope store lock poisoned");
        summarize_envelopes(envelopes.values())
    }
}

impl RelayBundleStore {
    pub fn from_env() -> Self {
        env::var("JIRANI_RELAY_STORE_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .map(Self::from_path)
            .unwrap_or_default()
    }

    pub fn from_path(path: PathBuf) -> Self {
        let bundles = load_relay_bundles(&path).unwrap_or_default();
        Self {
            bundles: Mutex::new(bundles),
            storage_path: Some(path),
        }
    }

    pub fn upsert(&self, bundle: RelayBundle) -> StoreWrite {
        let mut bundles = self
            .bundles
            .lock()
            .expect("relay bundle store lock poisoned");
        if bundles.contains_key(&bundle.bundle_id) {
            return StoreWrite::AlreadyStored;
        }
        let bundle_id = bundle.bundle_id.clone();
        bundles.insert(bundle_id.clone(), bundle);

        if let Some(path) = &self.storage_path {
            if let Err(error) = persist_relay_bundles(path, &bundles) {
                bundles.remove(&bundle_id);
                return StoreWrite::PersistFailed(error.to_string());
            }
        }

        StoreWrite::Created
    }

    pub fn list(&self) -> Vec<RelayBundle> {
        let bundles = self
            .bundles
            .lock()
            .expect("relay bundle store lock poisoned");
        let mut values = bundles.values().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .expires_at_epoch_seconds
                .cmp(&left.expires_at_epoch_seconds)
                .then_with(|| left.bundle_id.cmp(&right.bundle_id))
        });
        values
    }
}

impl GatewayStore {
    pub async fn from_env() -> Result<Self, sqlx::Error> {
        let envelope_store = EnvelopeStore::from_env();
        let relay_bundle_store = RelayBundleStore::from_env();
        let postgres = PostgresStore::from_env().await?;
        Ok(Self {
            envelope_store,
            relay_bundle_store,
            postgres,
        })
    }

    #[allow(dead_code)]
    pub fn from_memory() -> Self {
        Self {
            envelope_store: EnvelopeStore::default(),
            relay_bundle_store: RelayBundleStore::default(),
            postgres: None,
        }
    }

    #[allow(dead_code)]
    pub fn from_file_stores(
        envelope_store: EnvelopeStore,
        relay_bundle_store: RelayBundleStore,
    ) -> Self {
        Self {
            envelope_store,
            relay_bundle_store,
            postgres: None,
        }
    }

    pub async fn upsert_envelope(&self, envelope: SyncEnvelope) -> StoreWrite {
        if let Some(postgres) = &self.postgres {
            return postgres.upsert_envelope(envelope).await;
        }
        self.envelope_store.upsert(envelope)
    }

    pub async fn list_envelopes(&self) -> Result<Vec<SyncEnvelope>, String> {
        if let Some(postgres) = &self.postgres {
            return postgres
                .list_envelopes()
                .await
                .map_err(|error| error.to_string());
        }
        Ok(self.envelope_store.list())
    }

    pub async fn summary(&self) -> Result<AnonymousSummary, String> {
        if let Some(postgres) = &self.postgres {
            return postgres.summary().await.map_err(|error| error.to_string());
        }
        Ok(self.envelope_store.summary())
    }

    pub async fn upsert_relay_bundle(&self, bundle: RelayBundle) -> StoreWrite {
        if let Some(postgres) = &self.postgres {
            return postgres.upsert_relay_bundle(bundle).await;
        }
        self.relay_bundle_store.upsert(bundle)
    }

    pub async fn list_relay_bundles(&self) -> Result<Vec<RelayBundle>, String> {
        if let Some(postgres) = &self.postgres {
            return postgres
                .list_relay_bundles()
                .await
                .map_err(|error| error.to_string());
        }
        Ok(self.relay_bundle_store.list())
    }
}

impl PostgresStore {
    pub async fn from_env() -> Result<Option<Self>, sqlx::Error> {
        let Some(database_url) = env::var("JIRANI_DATABASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(Some(store))
    }

    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sync_envelopes (
                envelope_id TEXT PRIMARY KEY,
                submitted_at_epoch_seconds BIGINT NOT NULL,
                envelope JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS relay_bundles (
                bundle_id TEXT PRIMARY KEY,
                expires_at_epoch_seconds BIGINT NOT NULL,
                bundle JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn upsert_envelope(&self, envelope: SyncEnvelope) -> StoreWrite {
        let envelope_id = envelope.envelope_id.clone();
        let submitted_at_epoch_seconds = envelope.payload.submitted_at_epoch_seconds;
        let result = sqlx::query(
            r#"
            INSERT INTO sync_envelopes (
                envelope_id,
                submitted_at_epoch_seconds,
                envelope
            )
            VALUES ($1, $2, $3)
            ON CONFLICT (envelope_id) DO NOTHING
            "#,
        )
        .bind(envelope_id)
        .bind(submitted_at_epoch_seconds)
        .bind(Json(envelope))
        .execute(&self.pool)
        .await;

        match result {
            Ok(done) if done.rows_affected() == 1 => StoreWrite::Created,
            Ok(_) => StoreWrite::AlreadyStored,
            Err(error) => StoreWrite::PersistFailed(error.to_string()),
        }
    }

    async fn list_envelopes(&self) -> Result<Vec<SyncEnvelope>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT envelope
            FROM sync_envelopes
            ORDER BY submitted_at_epoch_seconds DESC, envelope_id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let Json(envelope): Json<SyncEnvelope> = row.try_get("envelope")?;
                Ok(envelope)
            })
            .collect()
    }

    async fn summary(&self) -> Result<AnonymousSummary, sqlx::Error> {
        let envelopes = self.list_envelopes().await?;
        Ok(summarize_envelopes(envelopes.iter()))
    }

    async fn upsert_relay_bundle(&self, bundle: RelayBundle) -> StoreWrite {
        let bundle_id = bundle.bundle_id.clone();
        let expires_at_epoch_seconds = bundle.expires_at_epoch_seconds;
        let result = sqlx::query(
            r#"
            INSERT INTO relay_bundles (
                bundle_id,
                expires_at_epoch_seconds,
                bundle
            )
            VALUES ($1, $2, $3)
            ON CONFLICT (bundle_id) DO NOTHING
            "#,
        )
        .bind(bundle_id)
        .bind(expires_at_epoch_seconds)
        .bind(Json(bundle))
        .execute(&self.pool)
        .await;

        match result {
            Ok(done) if done.rows_affected() == 1 => StoreWrite::Created,
            Ok(_) => StoreWrite::AlreadyStored,
            Err(error) => StoreWrite::PersistFailed(error.to_string()),
        }
    }

    async fn list_relay_bundles(&self) -> Result<Vec<RelayBundle>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT bundle
            FROM relay_bundles
            ORDER BY expires_at_epoch_seconds DESC, bundle_id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let Json(bundle): Json<RelayBundle> = row.try_get("bundle")?;
                Ok(bundle)
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreWrite {
    Created,
    AlreadyStored,
    PersistFailed(String),
}

fn load_envelopes(path: &Path) -> io::Result<HashMap<String, SyncEnvelope>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let body = fs::read_to_string(path)?;
    let envelopes: Vec<SyncEnvelope> = serde_json::from_str(&body).unwrap_or_default();
    Ok(envelopes
        .into_iter()
        .map(|envelope| (envelope.envelope_id.clone(), envelope))
        .collect())
}

fn persist_envelopes(path: &Path, envelopes: &HashMap<String, SyncEnvelope>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut values = envelopes.values().cloned().collect::<Vec<_>>();
    values.sort_by(|left, right| left.envelope_id.cmp(&right.envelope_id));
    let body = serde_json::to_string_pretty(&values)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, body)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn load_relay_bundles(path: &Path) -> io::Result<HashMap<String, RelayBundle>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let body = fs::read_to_string(path)?;
    let bundles: Vec<RelayBundle> = serde_json::from_str(&body).unwrap_or_default();
    Ok(bundles
        .into_iter()
        .map(|bundle| (bundle.bundle_id.clone(), bundle))
        .collect())
}

fn persist_relay_bundles(path: &Path, bundles: &HashMap<String, RelayBundle>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut values = bundles.values().cloned().collect::<Vec<_>>();
    values.sort_by(|left, right| left.bundle_id.cmp(&right.bundle_id));
    let body = serde_json::to_string_pretty(&values)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, body)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn summarize_envelopes<'a>(envelopes: impl Iterator<Item = &'a SyncEnvelope>) -> AnonymousSummary {
    let mut total_envelopes = 0;
    let mut by_sensitivity = BTreeMap::<String, usize>::new();
    let mut by_verification_status = BTreeMap::<String, usize>::new();
    let mut by_general_area = BTreeMap::<String, usize>::new();

    for envelope in envelopes {
        total_envelopes += 1;
        *by_sensitivity
            .entry(envelope.payload.sensitivity.clone())
            .or_insert(0) += 1;
        *by_verification_status
            .entry(envelope.payload.verification_status.clone())
            .or_insert(0) += 1;
        *by_general_area
            .entry(envelope.payload.general_area.clone())
            .or_insert(0) += 1;
    }

    AnonymousSummary {
        total_envelopes,
        by_sensitivity: to_counts(by_sensitivity),
        by_verification_status: to_counts(by_verification_status),
        top_areas: to_area_counts(by_general_area),
    }
}

fn to_counts(values: BTreeMap<String, usize>) -> Vec<SummaryCount> {
    values
        .into_iter()
        .map(|(key, count)| SummaryCount { key, count })
        .collect()
}

fn to_area_counts(values: BTreeMap<String, usize>) -> Vec<AreaSummary> {
    let mut counts = values
        .into_iter()
        .map(|(general_area, count)| AreaSummary {
            general_area,
            count,
        })
        .collect::<Vec<_>>();
    counts.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.general_area.cmp(&right.general_area))
            .then(Ordering::Equal)
    });
    counts.truncate(8);
    counts
}
