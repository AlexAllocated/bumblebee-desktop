//! Retained overlay layout. Shared validation covers desktop commands and agent edits.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const ANCHORS: &[&str] = &[
	"top-left",
	"top-right",
	"bottom-left",
	"bottom-right",
	"center",
];
fn bounds(key: &str) -> Option<(f64, f64)> {
	Some(match key {
		"horizontalPercent" | "verticalPercent" => (0., 100.),
		"scalePercentage" | "textSizePercentage" | "chatBubbleTextSizePercentage" => (0.05, 1.),
		"occlusionPercentage" => (0.1, 0.5),
		"chatBubbleMaxWidthPercent" => (15., 70.),
		"chatBubbleMaxHeightPercent" => (10., 70.),
		"subjectRadiusPx" => (8., 600.),
		"anchorRatioX" | "anchorRatioY" => (0., 1.),
		"vectorX" | "vectorY" => (-1000., 1000.),
		_ => return None,
	})
}
impl OverlaySettings {
	pub fn validate(&self) -> Result<()> {
		fn visit(key: &str, value: &Value) -> Result<()> {
			if let Some(fields) = value.as_object() {
				for (key, value) in fields {
					visit(key, value)?;
				}
			} else if let Some((min, max)) = bounds(key) {
				let n = value.as_f64().unwrap_or(f64::NAN);
				ensure!(
					n.is_finite() && (min..=max).contains(&n),
					"Overlay {key} must be between {min} and {max}"
				);
			} else if key == "anchor" {
				ensure!(
					ANCHORS.contains(&value.as_str().unwrap_or("")),
					"Unknown overlay anchor"
				);
			} else if key == "chatBubbleStyleId" {
				let id = value.as_str().unwrap_or("");
				ensure!(
					!id.is_empty()
						&& id.len() <= 80
						&& id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
					"Invalid bubble style ID"
				);
			}
			Ok(())
		}
		visit("", &serde_json::to_value(self)?)
	}
}
/// Strict provider tool schema: every leaf is nullable, where null means unchanged.
pub fn patch_schema() -> Value {
	fn schema(key: &str, value: &Value, nullable: bool) -> Value {
		let base = if let Some(fields) = value.as_object() {
			let properties: serde_json::Map<String, Value> = fields
				.iter()
				.map(|(k, v)| (k.clone(), schema(k, v, true)))
				.collect();
			json!({"type":"object", "additionalProperties":false, "required":fields.keys().collect::<Vec<_>>(), "properties":properties})
		} else if value.is_boolean() {
			json!({"type":"boolean"})
		} else if let Some((min, max)) = bounds(key) {
			json!({"type":"number", "minimum":min, "maximum":max})
		} else if key == "anchor" {
			json!({"type":"string", "enum":ANCHORS})
		} else {
			json!({"type":"string", "minLength":1, "maxLength":80})
		};
		if nullable {
			json!({"anyOf":[base, {"type":"null"}]})
		} else {
			base
		}
	}
	schema(
		"",
		&serde_json::to_value(OverlaySettings::default()).expect("overlay serializes"),
		false,
	)
}

/// One-time SQLite migration from the first desktop preview's flattened layout.
pub fn migrate_legacy(value: Value) -> Result<OverlaySettings> {
	if value.get("bumblebee").is_some() {
		let settings: OverlaySettings = serde_json::from_value(value)?;
		settings.validate()?;
		return Ok(settings);
	}
	let mut next = OverlaySettings::default();
	let number = |key: &str| {
		value
			.get(key)
			.and_then(Value::as_f64)
			.filter(|n| n.is_finite())
	};
	let boolean = |key: &str| value.get(key).and_then(Value::as_bool);
	if let Some(v) = number("beeX") {
		next.bumblebee.position.horizontal_percent = v * 100.;
	}
	if let Some(v) = number("beeY") {
		next.bumblebee.position.vertical_percent = v * 100.;
	}
	// The flat preview positioned the actor horizontally around its center.
	next.bumblebee.anchor = "center".into();
	if let Some(v) = number("beeScale") {
		next.bumblebee.scale_percentage = v;
	}
	if let Some(v) = boolean("beeVisible") {
		next.bumblebee.visible = v;
	}
	if let Some(v) = number("puppetScale") {
		next.puppet.scale_percentage = v;
	}
	if let Some(v) = number("puppetHorizontal") {
		next.puppet.position.horizontal_percent = v * 100.;
	}
	if let Some(v) = number("puppetOcclusion") {
		next.puppet.occlusion_percentage = v.clamp(0.1, 0.5);
	}
	if let Some(v) = boolean("puppetsVisible") {
		next.puppet.enabled = v;
	}
	if let Some(v) = boolean("bubblesVisible") {
		next.bumblebee.chat_bubbles_enabled = v;
		next.puppet.chat_bubbles_enabled = v;
	}
	next.validate()?;
	Ok(next)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn legacy_layout_defaults_and_unknown_fields() {
		let settings = OverlaySettings::default();
		settings.validate().unwrap();
		assert_eq!(settings.bumblebee.anchor, "bottom-left");
		assert_eq!(settings.bumblebee.position.vertical_percent, 100.);
		let value = serde_json::to_value(&settings).unwrap();
		assert_eq!(
			serde_json::from_value::<OverlaySettings>(value).unwrap(),
			settings
		);
		assert!(serde_json::from_value::<OverlaySettings>(json!({"promotionQrCode":{}})).is_err());
	}
	#[test]
	fn invalid_nested_geometry_is_rejected_before_persisting() {
		let mut settings = OverlaySettings::default();
		settings.streamer_voice_bubble.tail.vector_x = f64::NAN;
		assert!(settings.validate().is_err());
		settings = OverlaySettings::default();
		settings.streamer_voice_bubble.chat_bubble_max_width_percent = 71.;
		assert!(settings.validate().is_err());
		settings = OverlaySettings::default();
		settings.puppet.anchor = "bogus".into();
		assert!(settings.validate().is_err());
	}
	#[test]
	fn migrating_first_preview_preserves_explicit_hidden_and_placement_choices() {
		let next=migrate_legacy(json!({"beeX":0.72,"beeY":0.6,"beeScale":0.4,"beeVisible":false,"puppetScale":0.23,"puppetHorizontal":0.3,"puppetOcclusion":0.9,"puppetsVisible":false,"bubblesVisible":false})).unwrap();
		assert_eq!(next.bumblebee.position.horizontal_percent, 72.);
		assert_eq!(next.bumblebee.position.vertical_percent, 60.);
		assert_eq!(next.bumblebee.scale_percentage, 0.4);
		assert!(!next.bumblebee.visible);
		assert_eq!(next.puppet.position.horizontal_percent, 30.);
		assert_eq!(next.puppet.occlusion_percentage, 0.5);
		assert!(!next.puppet.enabled);
		assert!(!next.puppet.chat_bubbles_enabled);
		assert!(!next.bumblebee.chat_bubbles_enabled);
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Position {
	pub horizontal_percent: f64,
	pub vertical_percent: f64,
}
impl Default for Position {
	fn default() -> Self {
		Self {
			horizontal_percent: 15.0,
			vertical_percent: 100.0,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PuppetPosition {
	pub horizontal_percent: f64,
}
impl Default for PuppetPosition {
	fn default() -> Self {
		Self {
			horizontal_percent: 85.0,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Tail {
	pub anchor_ratio_x: f64,
	pub anchor_ratio_y: f64,
	pub vector_x: f64,
	pub vector_y: f64,
}
impl Default for Tail {
	fn default() -> Self {
		Self {
			anchor_ratio_x: 0.0,
			anchor_ratio_y: 1.0,
			vector_x: -20.0,
			vector_y: 25.0,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct BumblebeeLayout {
	pub chat_bubbles_enabled: bool,
	pub chat_bubble_style_id: String,
	pub chat_bubble_max_width_percent: f64,
	pub chat_bubble_max_height_percent: f64,
	pub chat_bubble_text_size_percentage: f64,
	pub position: Position,
	pub anchor: String,
	pub scale_percentage: f64,
	pub visible: bool,
}
impl Default for BumblebeeLayout {
	fn default() -> Self {
		Self {
			chat_bubbles_enabled: true,
			chat_bubble_style_id: "classic-comic".into(),
			chat_bubble_max_width_percent: 35.0,
			chat_bubble_max_height_percent: 25.0,
			chat_bubble_text_size_percentage: 0.5,
			position: Position {
				horizontal_percent: 15.0,
				vertical_percent: 100.0,
			},
			anchor: "bottom-left".into(),
			scale_percentage: 0.2,
			visible: true,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PuppetLayout {
	pub chat_bubbles_enabled: bool,
	pub chat_bubble_style_id: String,
	pub chat_bubble_max_width_percent: f64,
	pub chat_bubble_max_height_percent: f64,
	pub chat_bubble_text_size_percentage: f64,
	pub enabled: bool,
	pub position: PuppetPosition,
	pub anchor: String,
	pub scale_percentage: f64,
	pub occlusion_percentage: f64,
	pub show_when_idle: bool,
	pub nameplates_enabled: bool,
}
impl Default for PuppetLayout {
	fn default() -> Self {
		Self {
			chat_bubbles_enabled: true,
			chat_bubble_style_id: "classic-comic".into(),
			chat_bubble_max_width_percent: 35.0,
			chat_bubble_max_height_percent: 25.0,
			chat_bubble_text_size_percentage: 0.5,
			enabled: true,
			position: PuppetPosition {
				horizontal_percent: 85.0,
			},
			anchor: "center".into(),
			scale_percentage: 0.3,
			occlusion_percentage: 0.3,
			show_when_idle: false,
			nameplates_enabled: true,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct StreamerBubbleLayout {
	pub enabled: bool,
	pub chat_bubble_style_id: String,
	pub position: Position,
	pub anchor: String,
	pub text_size_percentage: f64,
	pub chat_bubble_max_width_percent: f64,
	pub chat_bubble_max_height_percent: f64,
	pub subject_radius_px: f64,
	pub tail: Tail,
}
impl Default for StreamerBubbleLayout {
	fn default() -> Self {
		Self {
			enabled: false,
			chat_bubble_style_id: "classic-comic".into(),
			position: Position {
				horizontal_percent: 25.0,
				vertical_percent: 25.0,
			},
			anchor: "center".into(),
			text_size_percentage: 0.5,
			chat_bubble_max_width_percent: 35.0,
			chat_bubble_max_height_percent: 25.0,
			subject_radius_px: 80.0,
			tail: Tail {
				anchor_ratio_x: 0.0,
				anchor_ratio_y: 1.0,
				vector_x: -20.0,
				vector_y: 25.0,
			},
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct EditPreview {
	pub puppet: bool,
	pub streamer_voice_bubble: bool,
}
impl Default for EditPreview {
	fn default() -> Self {
		Self {
			puppet: false,
			streamer_voice_bubble: false,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct OverlaySettings {
	pub bumblebee: BumblebeeLayout,
	pub puppet: PuppetLayout,
	pub streamer_voice_bubble: StreamerBubbleLayout,
	pub edit_preview: EditPreview,
}
impl Default for OverlaySettings {
	fn default() -> Self {
		Self {
			bumblebee: BumblebeeLayout::default(),
			puppet: PuppetLayout::default(),
			streamer_voice_bubble: StreamerBubbleLayout::default(),
			edit_preview: EditPreview::default(),
		}
	}
}
