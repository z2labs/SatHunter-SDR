//! Decoder plugins and the satellite catalog.
//!
//! A plugin is a folder:
//!
//! ```text
//! plugins/iss-aprs/
//!   plugin.toml      what it is, which satellites / modes it decodes
//!   decoder.lua      the decoder (Luau), see docs/PLUGINS.md
//!   test/            a recording and the frames it must produce (CI runs it)
//! ```
//!
//! The catalog (`catalog/index.json`) lists satellite profiles (frequencies, modes, which
//! plugin decodes them) and the plugins, so the app can offer them with one tap.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("{0}: {1}")]
    Io(String, String),
    #[error("{0}: {1}")]
    Parse(String, String),
    #[error("{0}: {1}")]
    Invalid(String, String),
}

/// Native signal chain the plugin builds on (the heavy DSP stays in the core).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    /// Audio after the demodulator (SSTV, voice, tones)
    Audio,
    /// AFSK 1200 Bd (Bell 202) -> AX.25 / HDLC frames
    Afsk1200,
    /// G3RUH 9600 Bd FSK -> AX.25 frames
    Fsk9600,
    /// GMSK at `baud` -> raw bits / AX.25
    Gmsk,
    /// BPSK at `baud`
    Bpsk,
    /// CW: keyed carrier -> characters
    Cw,
    /// Raw IQ of the channel (advanced, WASM plugins)
    Iq,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Easy / medium / advanced, shown in the catalog for kids and teachers
    #[serde(default = "default_level")]
    pub level: String,
    pub chain: Chain,
    #[serde(default)]
    pub baud: Option<f64>,
    /// Satellites it decodes (NORAD numbers); empty = any satellite with a matching mode
    #[serde(default)]
    pub norad: Vec<u64>,
    /// SatNOGS modes it decodes, e.g. ["AFSK", "APRS"]
    #[serde(default)]
    pub modes: Vec<String>,
    /// Entry script, default decoder.lua
    #[serde(default = "default_entry")]
    pub entry: String,
}

fn default_level() -> String {
    "easy".into()
}
fn default_entry() -> String {
    "decoder.lua".into()
}

impl Manifest {
    pub fn parse(text: &str, origin: &str) -> Result<Manifest, PluginError> {
        let m: Manifest = toml::from_str(text).map_err(|e| PluginError::Parse(origin.into(), e.to_string()))?;
        m.validate(origin)?;
        Ok(m)
    }

    pub fn validate(&self, origin: &str) -> Result<(), PluginError> {
        let bad = |w: &str| Err(PluginError::Invalid(origin.into(), w.into()));
        if self.id.is_empty() || !self.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return bad("id: lower case letters, digits and '-' only");
        }
        if self.name.trim().is_empty() {
            return bad("name is empty");
        }
        if matches!(self.chain, Chain::Gmsk | Chain::Bpsk) && self.baud.unwrap_or(0.0) <= 0.0 {
            return bad("gmsk / bpsk chains need a baud rate");
        }
        if !self.entry.ends_with(".lua") && !self.entry.ends_with(".wasm") {
            return bad("entry must be a .lua or .wasm file");
        }
        if !matches!(self.level.as_str(), "easy" | "medium" | "advanced") {
            return bad("level: easy, medium or advanced");
        }
        Ok(())
    }

    /// Does this plugin decode a downlink of this satellite?
    pub fn applies(&self, norad: u64, mode: &str) -> bool {
        let sat_ok = self.norad.is_empty() || self.norad.contains(&norad);
        let mode_ok = self.modes.is_empty() || self.modes.iter().any(|m| m.eq_ignore_ascii_case(mode));
        sat_ok && mode_ok
    }
}

/// Load a plugin folder (manifest + entry file present).
pub fn load_dir(dir: &Path) -> Result<Manifest, PluginError> {
    let origin = dir.display().to_string();
    let text = std::fs::read_to_string(dir.join("plugin.toml")).map_err(|e| PluginError::Io(origin.clone(), e.to_string()))?;
    let m = Manifest::parse(&text, &origin)?;
    if !dir.join(&m.entry).is_file() {
        return Err(PluginError::Invalid(origin, format!("missing {}", m.entry)));
    }
    Ok(m)
}

/// One downlink of a catalog profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Downlink {
    pub label: String,
    pub freq_hz: f64,
    pub mode: String,
    #[serde(default)]
    pub baud: Option<f64>,
    /// Plugin id that decodes it, if any
    #[serde(default)]
    pub plugin: Option<String>,
}

/// A ready-made satellite profile (ISS APRS, SSTV events, popular cubesats ...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub norad: u64,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub downlinks: Vec<Downlink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogPlugin {
    pub id: String,
    /// Folder in the repository, e.g. "plugins/iss-aprs"
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub profiles: Vec<Profile>,
    pub plugins: Vec<CatalogPlugin>,
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog, PluginError> {
        serde_json::from_str(text).map_err(|e| PluginError::Parse("catalog".into(), e.to_string()))
    }

    /// Check the catalog against the plugin folders under `root` (used by CI).
    pub fn validate(&self, root: &Path) -> Vec<PluginError> {
        let mut errs = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for p in &self.plugins {
            match load_dir(&root.join(&p.path)) {
                Ok(m) if m.id != p.id => errs.push(PluginError::Invalid(p.path.clone(), format!("id {} != catalog id {}", m.id, p.id))),
                Ok(_) => {
                    ids.insert(p.id.clone());
                }
                Err(e) => errs.push(e),
            }
        }
        for pr in &self.profiles {
            if pr.downlinks.is_empty() {
                errs.push(PluginError::Invalid(pr.id.clone(), "no downlinks".into()));
            }
            for d in &pr.downlinks {
                if d.freq_hz < 1e6 {
                    errs.push(PluginError::Invalid(pr.id.clone(), format!("{}: frequency in Hz", d.label)));
                }
                if let Some(pl) = &d.plugin {
                    if !ids.contains(pl) {
                        errs.push(PluginError::Invalid(pr.id.clone(), format!("unknown plugin {pl}")));
                    }
                }
            }
        }
        errs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rules() {
        let ok = r#"
            id = "iss-aprs"
            name = "ISS APRS"
            version = "1.0"
            author = "Mezőberény 7.b"
            chain = "afsk1200"
            norad = [25544]
            modes = ["AFSK"]
        "#;
        let m = Manifest::parse(ok, "t").unwrap();
        assert!(m.applies(25544, "afsk"));
        assert!(!m.applies(25544, "FM"));
        assert_eq!(m.entry, "decoder.lua");
        assert!(Manifest::parse(&ok.replace("iss-aprs", "ISS APRS"), "t").is_err());
        assert!(Manifest::parse(&ok.replace("afsk1200", "gmsk"), "t").is_err());
    }

    #[test]
    fn repository_catalog_is_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let text = std::fs::read_to_string(root.join("catalog/index.json")).expect("catalog/index.json");
        let c = Catalog::parse(&text).unwrap();
        let errs = c.validate(&root);
        assert!(errs.is_empty(), "{errs:?}");
        assert!(c.profiles.iter().any(|p| p.norad == 25544));
    }
}
