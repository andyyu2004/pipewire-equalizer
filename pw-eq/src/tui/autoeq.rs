use crate::filter::Filter;
use anyhow;
use pw_util::module::FilterType;
use std::path::PathBuf;
use tokio::sync::mpsc;

pub use autoeq_api::ParametricEq;

use super::Notif;

#[derive(Debug, Default)]
pub struct AutoEqBrowser {
    pub filter_query: String,
    pub entries: Option<autoeq_api::Entries>,
    pub targets: Option<Vec<autoeq_api::Target>>,
    pub filtered_results: Vec<(String, autoeq_api::Entry)>,
    pub selected_index: usize,
    pub selected_target_index: usize,
    pub loading: bool,
}

impl AutoEqBrowser {
    pub fn update_filtered_results(&mut self) {
        let Some(entries) = &self.entries else {
            self.filtered_results.clear();
            return;
        };

        let query = self.filter_query.to_lowercase();
        self.filtered_results = entries
            .iter()
            .flat_map(|(name, entries)| {
                entries
                    .iter()
                    .map(move |entry| (name.clone(), entry.clone()))
            })
            .filter(|(name, _)| {
                if query.is_empty() {
                    true
                } else {
                    name.to_lowercase().contains(&query)
                }
            })
            .collect();

        // Reset selection if out of bounds
        if self.selected_index >= self.filtered_results.len() {
            self.selected_index = 0;
        }
    }

    pub fn selected_entry(&self) -> Option<&(String, autoeq_api::Entry)> {
        self.filtered_results.get(self.selected_index)
    }

    pub fn selected_target(&self) -> Option<&autoeq_api::Target> {
        self.targets.as_ref()?.get(self.selected_target_index)
    }

    pub fn load_data(&mut self, http_client: reqwest::Client, notifs_tx: mpsc::Sender<Notif>) {
        if self.entries.is_some() && self.targets.is_some() {
            // Already loaded
            return;
        }

        self.loading = true;

        tokio::spawn(async move {
            let (entries, targets) = match AutoEqCache::load().await {
                Ok(Some(cache)) => {
                    tracing::info!("Loaded AutoEQ data from cache");
                    (cache.entries, cache.targets)
                }
                Ok(None) => {
                    tracing::info!("Cache miss or expired, fetching from API");
                    // Fetch from API
                    match tokio::try_join!(
                        autoeq_api::entries(&http_client),
                        autoeq_api::targets(&http_client)
                    ) {
                        Ok((entries, targets)) => {
                            // Save to cache
                            if let Err(err) =
                                AutoEqCache::save(entries.clone(), targets.clone()).await
                            {
                                tracing::warn!(error = &*err, "Failed to save cache");
                            }
                            (entries, targets)
                        }
                        Err(err) => {
                            let _ = notifs_tx.send(Notif::Error(err.into())).await;
                            return;
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = &*err, "Failed to load cache");
                    // Try fetching from API if cache load fails
                    match tokio::try_join!(
                        autoeq_api::entries(&http_client),
                        autoeq_api::targets(&http_client)
                    ) {
                        Ok((entries, targets)) => {
                            if let Err(err) =
                                AutoEqCache::save(entries.clone(), targets.clone()).await
                            {
                                tracing::warn!(error = &*err, "Failed to save cache");
                            }
                            (entries, targets)
                        }
                        Err(err) => {
                            let _ = notifs_tx.send(Notif::Error(err.into())).await;
                            return;
                        }
                    }
                }
            };

            let _ = notifs_tx
                .send(Notif::AutoEqDbLoaded { entries, targets })
                .await;
        });
    }

    pub fn apply_selected(
        &self,
        http_client: reqwest::Client,
        notifs_tx: mpsc::Sender<Notif>,
        sample_rate: u32,
    ) -> Option<Result<String, String>> {
        let (name, entry) = self.selected_entry()?.clone();
        let target = self.selected_target()?;

        let target_label = target.label.clone();
        let source = entry.source.clone();
        let rig = entry.rig.clone();

        let display_name = name.clone();
        let display_source = entry.source.clone();

        tokio::spawn(async move {
            let request = autoeq_api::EqualizeRequest {
                target: target_label.clone(),
                name: name.clone(),
                source,
                rig,
                sample_rate,
            };

            match autoeq_api::equalize(&http_client, &request).await {
                Ok(response) => {
                    let _ = notifs_tx.send(Notif::AutoEqLoaded { name, response }).await;
                }
                Err(err) => {
                    let _ = notifs_tx.send(Notif::Error(err.into())).await;
                }
            }
        });

        Some(Ok(format!(
            "Fetching EQ for {} from {}...",
            display_name, display_source
        )))
    }

    pub fn on_data_loaded(
        &mut self,
        entries: autoeq_api::Entries,
        targets: Vec<autoeq_api::Target>,
    ) {
        self.entries = Some(entries);
        self.targets = Some(targets);
        self.loading = false;
        self.update_filtered_results();

        // Select default target (Harman over-ear 2018 if available)
        if let Some(targets) = &self.targets
            && let Some(idx) = targets
                .iter()
                .position(|t| t.label.eq_ignore_ascii_case("harman over-ear 2018"))
        {
            self.selected_target_index = idx;
            tracing::error!("Selected default target: {}", targets[idx].label);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AutoEqCache {
    entries: autoeq_api::Entries,
    targets: Vec<autoeq_api::Target>,
    timestamp: u64,
}

impl AutoEqCache {
    const CACHE_DURATION_SECS: u64 = 24 * 60 * 60; // 24 hours

    fn cache_path() -> anyhow::Result<PathBuf> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find cache directory"))?
            .join("pw-eq");
        std::fs::create_dir_all(&cache_dir)?;
        Ok(cache_dir.join("autoeq-cache.json"))
    }

    async fn load() -> anyhow::Result<Option<Self>> {
        let path = Self::cache_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let data = tokio::fs::read_to_string(&path).await?;
        let cache: Self = serde_json::from_str(&data)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        if now - cache.timestamp > Self::CACHE_DURATION_SECS {
            return Ok(None);
        }

        Ok(Some(cache))
    }

    async fn save(
        entries: autoeq_api::Entries,
        targets: Vec<autoeq_api::Target>,
    ) -> anyhow::Result<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let cache = Self {
            entries,
            targets,
            timestamp,
        };

        let path = Self::cache_path()?;
        let data = serde_json::to_string_pretty(&cache)?;
        tokio::fs::write(&path, data).await?;

        Ok(())
    }
}

pub fn param_eq_to_filters(response: autoeq_api::ParametricEq) -> Vec<Filter> {
    let num_filters = response.filters.len();
    response
        .filters
        .into_iter()
        .enumerate()
        .map(|(idx, f)| {
            // First filter might be low shelf, last might be high shelf
            // Everything else is peaking
            let filter_type = if idx == 0 && f.fc < 100.0 {
                FilterType::LowShelf
            } else if idx == num_filters - 1 && f.fc > 8000.0 {
                FilterType::HighShelf
            } else {
                FilterType::Peaking
            };

            Filter {
                frequency: f.fc,
                gain: f.gain,
                q: f.q,
                filter_type,
                muted: false,
            }
        })
        .collect()
}
