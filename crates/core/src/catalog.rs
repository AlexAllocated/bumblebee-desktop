use crate::model::{Puppet, Voice};
use anyhow::{Result, bail};
use std::collections::HashSet;

pub fn puppets() -> Vec<Puppet> {
	serde_json::from_str(include_str!("../../../assets/catalog/puppets.json"))
		.expect("validated bundled puppet catalog")
}
pub fn voices() -> Vec<Voice> {
	serde_json::from_str(include_str!("../../../assets/catalog/voices.json"))
		.expect("validated bundled voice catalog")
}
pub fn resolve_voice<'a>(catalog: &'a [Voice], name: &str) -> Result<&'a Voice> {
	if let Some(voice) = catalog.iter().find(|v| v.id.eq_ignore_ascii_case(name)) {
		return Ok(voice);
	}
	let matches: Vec<_> = catalog
		.iter()
		.filter(|v| {
			v.name
				.as_deref()
				.is_some_and(|n| n.eq_ignore_ascii_case(name))
				|| (v.id.starts_with("azure:") && v.voice_name.eq_ignore_ascii_case(name))
		})
		.collect();
	match matches.as_slice() {
		[voice] => Ok(voice),
		[] => bail!("Unknown voice. Use !voices <search> to find a voice."),
		_ => bail!("That voice name is ambiguous. Use its exact ID from !voices."),
	}
}
pub fn eligible_voices(catalog: &[Voice]) -> Vec<Voice> {
	let mut seen = HashSet::new();
	// Distinct named presets remain selectable even when they happen to share
	// synthesis parameters. Discovered voices never replace curated presets.
	catalog
		.iter()
		.filter(|v| {
			v.role == "puppet" && v.provider == "azure_speech" && v.voice_name.starts_with("en-")
		})
		.filter(|v| seen.insert(v.id.to_ascii_lowercase()))
		.cloned()
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn curated_roles_stay_separate_and_preset_parameters_are_preserved() {
		let voices = voices();
		let available = eligible_voices(&voices);
		assert_eq!(voices.iter().filter(|v| v.role == "bumblebee").count(), 20);
		assert_eq!(available.len(), 22);
		assert!(!available.iter().any(|v| v.id == "ana"));
		let fawn = resolve_voice(&available, "fawn").unwrap();
		assert_eq!(fawn.expression, "hopeful");
		assert_eq!(resolve_voice(&available, "FiNaGlE").unwrap().pitch, "1.3");
	}
	#[test]
	fn voice_names_must_be_unambiguous_and_raw_names_select_default_catalog_voices() {
		let mut preset = voices().into_iter().find(|v| v.id == "alopex").unwrap();
		let mut discovered = preset.clone();
		discovered.id = format!("azure:{}", discovered.voice_name);
		preset.name = Some("Ashley".into());
		discovered.name = preset.name.clone();
		let catalog = vec![preset, discovered];
		assert_eq!(eligible_voices(&catalog).len(), 2);
		assert!(resolve_voice(&catalog, "Ashley").is_err());
		assert_eq!(
			resolve_voice(&catalog, "en-US-AshleyNeural").unwrap().id,
			"azure:en-US-AshleyNeural"
		);
		assert!(resolve_voice(&catalog, "unknown").is_err());
	}
}
pub fn random_index(length: usize) -> usize {
	assert!(length > 0);
	(uuid::Uuid::new_v4().as_u128() % length as u128) as usize
}
