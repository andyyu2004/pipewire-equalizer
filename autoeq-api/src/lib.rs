use std::collections::BTreeMap;

pub type Entries = BTreeMap<String, Vec<Entry>>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Entry {
    pub form: Form,
    pub rig: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Form {
    Earbud,
    InEar,
    OverEar,
}

pub async fn entries() -> anyhow::Result<Entries> {
    let entries = reqwest::get("https://www.autoeq.app/entries")
        .await?
        .json::<Entries>()
        .await?;

    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub label: String,
    pub compatible: Vec<Entry>,
    pub recommended: Vec<Entry>,
    pub bass_boost: Filter,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Filter {
    pub fc: f32,
    pub q: f32,
    pub gain: f32,
}

pub async fn targets() -> anyhow::Result<Vec<Target>> {
    let targets = reqwest::get("https://www.autoeq.app/targets")
        .await?
        .json::<Vec<Target>>()
        .await?;

    Ok(targets)
}
