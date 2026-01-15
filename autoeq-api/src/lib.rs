use std::collections::BTreeMap;

pub type Entries = BTreeMap<String, Vec<Entry>>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub form: Form,
    pub rig: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Form {
    Earbud,
    InEar,
    OverEar,
}

pub async fn entries(client: &reqwest::Client) -> reqwest::Result<Entries> {
    client
        .get("https://www.autoeq.app/entries")
        .send()
        .await?
        .error_for_status()?
        .json::<Entries>()
        .await
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub label: String,
    pub compatible: Vec<Entry>,
    pub recommended: Vec<Entry>,
    pub bass_boost: Filter,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Filter {
    pub fc: f64,
    pub q: f64,
    pub gain: f64,
}

pub async fn targets(client: &reqwest::Client) -> reqwest::Result<Vec<Target>> {
    let targets = client
        .get("https://www.autoeq.app/targets")
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<Target>>()
        .await?;

    Ok(targets)
}

#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Measurement {
    pub frequency: Vec<f32>,
    pub raw: Vec<f32>,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Serialize)]
pub struct ResponseRequirements {
    pub fr_f_step: f64,
    pub fr_fields: Vec<String>,
    pub base64fp16: bool,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParametricEq {
    #[serde(rename = "fs")]
    pub sample_rate: i64,
    pub filters: Vec<Filter>,
    pub preamp: f64,
}

pub struct EqualizeRequest {
    /// Target name, e.g. "Harman over-ear 2018"
    pub target: String,
    /// Headphone name, e.g. "Focal Clear"
    pub name: String,
    /// Measurement source, e.g. "oratory1990"
    pub source: String,
    /// Measurement rig, e.g. "GRAS 45BC-10"
    pub rig: Option<String>,
    pub sample_rate: u32,
}

pub async fn equalize(
    client: &reqwest::Client,
    request: &EqualizeRequest,
) -> reqwest::Result<ParametricEq> {
    // Full request structure accepted by the API, we expose a more controlled subset via EqualizeRequest
    #[derive(Debug, Clone, PartialEq, serde::Serialize)]
    pub struct Req {
        target: String,
        parametric_eq: bool,
        name: String,
        source: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        rig: Option<String>,
        response: ResponseRequirements,
        fs: u32,
        // The rest all have reasonable defaults on the server side.
        // Can be added to EqualizeRequest if needed.
        // sound_signature: Option<Measurement>,
        // sound_signature_smoothing_window_size: Option<i64>,
        // bass_boost_gain: i64,
        // bass_boost_fc: i64,
        // bass_boost_q: f64,
        // treble_boost_gain: i64,
        // treble_boost_fc: i64,
        // treble_boost_q: f64,
        // tilt: i64,
        // bit_depth: i64,
        // phase: String,
        // f_res: i64,
        // preamp: i64,
        // max_gain: Option<f32>,
        // max_slope: i64,
        // window_size: f64,
        // treble_window_size: i64,
        // treble_f_lower: i64,
        // treble_f_upper: i64,
        // treble_gain_k: i64,
        // graphic_eq: bool,
        // fixed_band_eq: bool,
        // convolution_eq: bool,
        // parametric_eq_config: String,
    }

    #[derive(Debug, serde::Deserialize)]
    struct Res {
        parametric_eq: ParametricEq,
    }

    let req = Req {
        target: request.target.clone(),
        name: request.name.clone(),
        source: request.source.clone(),
        rig: request.rig.clone(),
        parametric_eq: true,
        fs: request.sample_rate,
        response: ResponseRequirements {
            fr_f_step: 1.02,
            fr_fields: vec![],
            base64fp16: false,
        },
    };

    let res = client
        .post("https://www.autoeq.app/equalize")
        .json(&req)
        .send()
        .await?
        .error_for_status()?
        .json::<Res>()
        .await?;
    Ok(res.parametric_eq)
}
