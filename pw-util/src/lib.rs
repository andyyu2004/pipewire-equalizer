mod serde_ex;
pub use pipewire;

pub mod api;

pub mod apo;
pub mod module;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::process::Command;

use self::serde_ex::KeyValuePairs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PwDumpObject {
    pub id: u32,
    #[serde(rename = "type")]
    pub object_type: PwObjectType,
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub permissions: Option<Vec<String>>,
    #[serde(default)]
    pub info: PwObjectInfo,
    #[serde(default)]
    pub props: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PwObjectType {
    #[serde(rename = "PipeWire:Interface:Core")]
    Core,
    #[serde(rename = "PipeWire:Interface:Module")]
    Module,
    #[serde(rename = "PipeWire:Interface:Client")]
    Client,
    #[serde(rename = "PipeWire:Interface:SecurityContext")]
    SecurityContext,
    #[serde(rename = "PipeWire:Interface:Profiler")]
    Profiler,
    #[serde(rename = "PipeWire:Interface:Factory")]
    Factory,
    #[serde(rename = "PipeWire:Interface:Device")]
    Device,
    #[serde(rename = "PipeWire:Interface:Metadata")]
    Metadata,
    #[serde(rename = "PipeWire:Interface:Node")]
    Node,
    #[serde(rename = "PipeWire:Interface:Port")]
    Port,
    #[serde(rename = "PipeWire:Interface:Link")]
    Link,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PwObjectInfo {
    #[serde(default)]
    pub props: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub params: PwParams,
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PwParams {
    #[serde(default)]
    pub enum_format: Vec<serde_json::Value>,
    #[serde(default)]
    pub prop_info: Vec<PwPropInfo>,
    #[serde(default)]
    pub props: Vec<Prop>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prop {
    #[serde(default)]
    pub params: KeyValuePairs<HashMap<String, serde_json::Value>>,
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PwPropInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "type")]
    pub type_: serde_json::Value,
}

pub async fn dump() -> Result<Vec<PwDumpObject>> {
    let output = Command::new("pw-dump")
        .output()
        .await
        .context("failed to execute pw-dump")?;

    if !output.status.success() {
        anyhow::bail!(
            "pw-dump failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let json_str = String::from_utf8(output.stdout).context("pw-dump output is not valid UTF-8")?;

    let objects: Vec<PwDumpObject> =
        serde_json::from_str(&json_str).context("Failed to parse pw-dump JSON")?;

    Ok(objects)
}

pub async fn set_default(node_id: u32) -> Result<()> {
    let output = Command::new("wpctl")
        .arg("set-default")
        .arg(node_id.to_string())
        .output()
        .await
        .context("Failed to execute wpctl")?;

    if !output.status.success() {
        anyhow::bail!("wpctl failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(())
}

pub async fn get_default_audio_sink_node_id() -> Result<u32> {
    let output = Command::new("wpctl")
        .arg("inspect")
        .arg("@DEFAULT_AUDIO_SINK@")
        .output()
        .await
        .context("Failed to execute wpctl")?;

    if !output.status.success() {
        anyhow::bail!("wpctl failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    // Parse output to extract id from first line (e.g., "id 123, type PipeWire:Interface:Node")
    let stdout = String::from_utf8(output.stdout).context("wpctl output is not valid UTF-8")?;
    let first_line = stdout.lines().next().context("wpctl output is empty")?;

    // Extract id from "id <number>,"
    let id_str = first_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.trim_end_matches(',').parse::<u32>().ok())
        .context("Failed to parse node id from wpctl output")?;

    Ok(id_str)
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: u32,
    pub node_name: String,
    pub object_serial: i64,
}

pub async fn get_default_audio_sink() -> Result<NodeInfo> {
    let node_id = get_default_audio_sink_node_id().await?;
    let objects = dump().await?;

    let node = objects
        .into_iter()
        .find(|obj| obj.id == node_id)
        .context("Default sink node not found in pw-dump")?;

    let node_name = node
        .info
        .props
        .get("node.name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .context("node.name not found for default sink")?;

    let object_serial = node
        .info
        .props
        .get("object.serial")
        .and_then(|v| v.as_i64())
        .context("object.serial not found for default sink")?;

    Ok(NodeInfo {
        node_id,
        node_name,
        object_serial,
    })
}

pub fn to_spa_json<T: serde::Serialize>(value: &T) -> String {
    let json_value = serde_json::to_value(value).expect("Failed to serialize to JSON value");
    self::module::SpaJson::new(&json_value).to_string()
}
