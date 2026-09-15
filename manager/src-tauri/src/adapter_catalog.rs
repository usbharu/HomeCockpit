use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DCS_BIOS_DDI_CATALOG: &str = include_str!("../resources/adapters/dcs-bios/f-16c-50-ddi.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterCatalogState {
    Loaded,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterCatalog {
    pub state: AdapterCatalogState,
    pub adapters: Vec<AdapterDefinition>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterDefinition {
    pub adapter_id: String,
    pub label: String,
    pub profiles: Vec<AdapterProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterProfile {
    pub profile_id: String,
    pub label: String,
    pub control_count: usize,
    #[serde(default)]
    pub aircraft_names: Vec<String>,
    #[serde(default)]
    pub role_bindings: Vec<AdapterRoleBinding>,
    pub controls: Vec<AdapterControlDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterControlDefinition {
    pub category: String,
    pub control_id: String,
    pub control_type: String,
    pub description: String,
    pub positions: Vec<String>,
    pub inputs: Vec<AdapterInputDefinition>,
    pub outputs: Vec<AdapterOutputDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterRoleBinding {
    pub role_id: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterProfileConfig {
    pub adapter_id: String,
    #[serde(flatten)]
    pub profile: AdapterProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInputDefinition {
    pub input_id: String,
    pub interface: String,
    pub description: String,
    pub max_value: Option<u32>,
    pub suggested_step: Option<i32>,
    pub argument_options: Vec<AdapterArgumentOption>,
    pub supports_event_value: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterArgumentOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterOutputDefinition {
    pub output_id: String,
    pub output_type: String,
    pub description: String,
    pub address: u16,
    pub length: Option<u16>,
    pub mask: Option<u32>,
    pub shift_by: Option<u8>,
    pub max_value: Option<u32>,
    pub suffix: String,
}

impl AdapterCatalog {
    pub fn builtin() -> Self {
        match parse_external_profile("F-16C_50", "F-16C 50 DDI", DCS_BIOS_DDI_CATALOG) {
            Ok(mut profile) => {
                profile.aircraft_names = vec![
                    "F-16C_50".to_string(),
                    "F-16C".to_string(),
                    "F-16C bl.50".to_string(),
                ];
                profile.role_bindings = infer_role_bindings("dcs-bios", &profile);
                Self {
                    state: AdapterCatalogState::Loaded,
                    adapters: vec![AdapterDefinition {
                        adapter_id: "dcs-bios".to_string(),
                        label: "DCS-BIOS".to_string(),
                        profiles: vec![profile],
                    }],
                    error: None,
                }
            }
            Err(error) => Self {
                state: AdapterCatalogState::Error,
                adapters: Vec::new(),
                error: Some(format!("Failed to load built-in adapter catalog: {error}")),
            },
        }
    }

    pub fn with_custom_profiles(&self, custom_profiles: &[AdapterProfileConfig]) -> Self {
        let mut catalog = self.clone();
        for custom_profile in custom_profiles {
            let Some(adapter) = catalog
                .adapters
                .iter_mut()
                .find(|adapter| adapter.adapter_id == custom_profile.adapter_id)
            else {
                continue;
            };

            if let Some(profile) = adapter
                .profiles
                .iter_mut()
                .find(|profile| profile.profile_id == custom_profile.profile.profile_id)
            {
                *profile = custom_profile.profile.clone();
            } else {
                adapter.profiles.push(custom_profile.profile.clone());
            }
        }
        catalog
    }

    pub fn find_profile_for_aircraft(
        &self,
        adapter_id: &str,
        aircraft_name: Option<&str>,
    ) -> Option<&AdapterProfile> {
        let aircraft_name = aircraft_name?;
        let normalized_aircraft = normalize_aircraft_name(aircraft_name);
        self.adapters
            .iter()
            .find(|adapter| adapter.adapter_id == adapter_id)
            .and_then(|adapter| {
                adapter.profiles.iter().find(|profile| {
                    profile
                        .aircraft_names
                        .iter()
                        .map(|name| normalize_aircraft_name(name))
                        .any(|name| name == normalized_aircraft)
                })
            })
    }

    pub fn find_output(
        &self,
        adapter_id: &str,
        profile_id: &str,
        control_id: &str,
        output_id: &str,
    ) -> Option<&AdapterOutputDefinition> {
        self.adapters
            .iter()
            .find(|adapter| adapter.adapter_id == adapter_id)
            .and_then(|adapter| {
                adapter
                    .profiles
                    .iter()
                    .find(|profile| profile.profile_id == profile_id)
            })
            .and_then(|profile| {
                profile
                    .controls
                    .iter()
                    .find(|control| control.control_id == control_id)
            })
            .and_then(|control| {
                control
                    .outputs
                    .iter()
                    .find(|output| output.output_id == output_id)
            })
    }
}

fn parse_profile(
    profile_id: &str,
    label: &str,
    document: &Value,
) -> Result<AdapterProfile, String> {
    let controls = parse_profile_controls(profile_id, document)?;
    Ok(AdapterProfile {
        profile_id: profile_id.to_string(),
        label: label.to_string(),
        control_count: controls.len(),
        aircraft_names: Vec::new(),
        role_bindings: Vec::new(),
        controls,
    })
}

pub fn parse_external_profile(
    profile_id: &str,
    label: &str,
    contents: &str,
) -> Result<AdapterProfile, String> {
    parse_json_or_jsonp(contents).and_then(|document| parse_profile(profile_id, label, &document))
}

pub fn normalize_aircraft_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-', ' '], "")
}

pub fn infer_role_bindings(adapter_id: &str, profile: &AdapterProfile) -> Vec<AdapterRoleBinding> {
    if adapter_id != "dcs-bios" {
        return Vec::new();
    }

    [("left-ddi", "mfdleft"), ("right-ddi", "mfdright")]
        .into_iter()
        .filter_map(|(role_id, normalized_category)| {
            profile
                .controls
                .iter()
                .map(|control| control.category.as_str())
                .find(|category| normalize_aircraft_name(category) == normalized_category)
                .map(|category| AdapterRoleBinding {
                    role_id: role_id.to_string(),
                    category: category.to_string(),
                })
        })
        .collect()
}

fn parse_profile_controls(
    profile_id: &str,
    document: &Value,
) -> Result<Vec<AdapterControlDefinition>, String> {
    let root = unwrap_profile_document(profile_id, document);
    let Some(root_object) = root.as_object() else {
        return Err("adapter profile root is not an object".to_string());
    };

    let mut controls = Vec::new();
    for (category_key, category_value) in root_object {
        let Some(category_object) = category_value.as_object() else {
            continue;
        };

        if category_object.contains_key("identifier") {
            if let Some(control) = parse_control(category_key, category_key, category_value) {
                controls.push(control);
            }
            continue;
        }

        for (control_id, control_value) in category_object {
            if let Some(control) = parse_control(category_key, control_id, control_value) {
                controls.push(control);
            }
        }
    }

    if controls.is_empty() {
        return Err("adapter profile contains no controls".to_string());
    }

    controls.sort_by(|left, right| {
        let category_order = left.category.cmp(&right.category);
        if category_order != Ordering::Equal {
            return category_order;
        }
        natural_control_id_cmp(&left.control_id, &right.control_id)
    });
    Ok(controls)
}

fn natural_control_id_cmp(left: &str, right: &str) -> Ordering {
    let (left_prefix, left_number) = split_numeric_suffix(left);
    let (right_prefix, right_number) = split_numeric_suffix(right);
    left_prefix
        .cmp(right_prefix)
        .then_with(|| left_number.cmp(&right_number))
        .then_with(|| left.cmp(right))
}

fn split_numeric_suffix(value: &str) -> (&str, Option<u32>) {
    let Some(index) = value.rfind('_') else {
        return (value, None);
    };
    let suffix = &value[index + 1..];
    let Ok(number) = suffix.parse::<u32>() else {
        return (value, None);
    };
    (&value[..index], Some(number))
}

fn unwrap_profile_document<'a>(profile_id: &str, document: &'a Value) -> &'a Value {
    let Some(object) = document.as_object() else {
        return document;
    };
    if object.len() == 1 {
        if let Some(value) = object.get(profile_id) {
            return value;
        }
    }
    document
}

fn parse_control(
    category_key: &str,
    control_id_key: &str,
    value: &Value,
) -> Option<AdapterControlDefinition> {
    let object = value.as_object()?;
    let control_id = string_field(object, "identifier")
        .or_else(|| (!control_id_key.is_empty()).then(|| control_id_key.to_string()))?;
    let category = string_field(object, "category")
        .or_else(|| (!category_key.is_empty()).then(|| category_key.to_string()))
        .unwrap_or_default();

    let inputs = object
        .get("inputs")
        .and_then(Value::as_array)
        .map(|inputs| {
            inputs
                .iter()
                .enumerate()
                .filter_map(|(index, input)| parse_input(index, input))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let outputs = object
        .get("outputs")
        .and_then(Value::as_array)
        .map(|outputs| {
            outputs
                .iter()
                .enumerate()
                .filter_map(|(index, output)| parse_output(control_id.as_str(), index, output))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(AdapterControlDefinition {
        category,
        control_id,
        control_type: string_field(object, "control_type").unwrap_or_default(),
        description: string_field(object, "description").unwrap_or_default(),
        positions: object
            .get("positions")
            .and_then(Value::as_array)
            .map(|positions| {
                positions
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        inputs,
        outputs,
    })
}

fn parse_input(index: usize, value: &Value) -> Option<AdapterInputDefinition> {
    let object = value.as_object()?;
    let interface = string_field(object, "interface")?;
    let max_value = unsigned_field(object, "max_value");
    let suggested_step = signed_field(object, "suggested_step");
    let description = string_field(object, "description").unwrap_or_default();
    let mut argument_options = Vec::new();

    if let Some(argument) = string_field(object, "argument") {
        argument_options.push(AdapterArgumentOption {
            label: if description.is_empty() {
                argument.clone()
            } else {
                format!("{argument} — {description}")
            },
            value: argument,
        });
    } else {
        match interface.as_str() {
            "fixed_step" => {
                argument_options.push(option("DEC", "DEC — 1段階戻る"));
                argument_options.push(option("INC", "INC — 1段階進む"));
            }
            "variable_step" => {
                let step = suggested_step.unwrap_or(3200).abs();
                argument_options.push(option(&format!("-{step}"), &format!("-{step} — 左／減少")));
                argument_options.push(option(&format!("+{step}"), &format!("+{step} — 右／増加")));
            }
            "set_state" if max_value.is_some_and(|value| value <= 32) => {
                if let Some(max_value) = max_value {
                    for value in 0..=max_value {
                        argument_options.push(option(&value.to_string(), &value.to_string()));
                    }
                }
            }
            _ => {}
        }
    }

    Some(AdapterInputDefinition {
        input_id: format!("{interface}-{index}"),
        supports_event_value: matches!(interface.as_str(), "set_state" | "variable_step"),
        interface,
        description,
        max_value,
        suggested_step,
        argument_options,
    })
}

fn parse_output(control_id: &str, index: usize, value: &Value) -> Option<AdapterOutputDefinition> {
    let object = value.as_object()?;
    let address = unsigned_field(object, "address")?.try_into().ok()?;
    let output_type = string_field(object, "type").unwrap_or_else(|| "integer".to_string());
    let output_id = string_field(object, "address_identifier")
        .or_else(|| string_field(object, "address_mask_shift_identifier"))
        .unwrap_or_else(|| format!("{control_id}:output-{index}"));

    Some(AdapterOutputDefinition {
        output_id,
        output_type: output_type.clone(),
        description: string_field(object, "description").unwrap_or_default(),
        address,
        length: unsigned_field(object, "length")
            .or_else(|| unsigned_field(object, "max_length"))
            .or_else(|| (output_type == "integer").then_some(2))
            .and_then(|value| value.try_into().ok()),
        mask: unsigned_field(object, "mask"),
        shift_by: unsigned_field(object, "shift_by").and_then(|value| value.try_into().ok()),
        max_value: unsigned_field(object, "max_value"),
        suffix: string_field(object, "suffix").unwrap_or_default(),
    })
}

fn parse_json_or_jsonp(contents: &str) -> Result<Value, String> {
    let trimmed = contents.trim().trim_start_matches('\u{feff}').trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }

    let start = trimmed
        .find('{')
        .ok_or_else(|| "adapter catalog does not contain a JSON object".to_string())?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| "adapter catalog has an unterminated JSON object".to_string())?;
    serde_json::from_str(&trimmed[start..=end])
        .map_err(|error| format!("failed to parse JSON/JSONP adapter catalog: {error}"))
}

fn option(value: &str, label: &str) -> AdapterArgumentOption {
    AdapterArgumentOption {
        value: value.to_string(),
        label: label.to_string(),
    }
}

fn string_field(object: &serde_json::Map<String, Value>, field: &str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

fn unsigned_field(object: &serde_json::Map<String, Value>, field: &str) -> Option<u32> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| value.try_into().ok())
}

fn signed_field(object: &serde_json::Map<String, Value>, field: &str) -> Option<i32> {
    object
        .get(field)
        .and_then(Value::as_i64)
        .and_then(|value| value.try_into().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jsonp_module_categories_and_io_definitions() {
        let document = r#"
            docdata["TEST_MODULE"] = {
              "Panel": {
                "MODE_SW": {
                  "category": "Panel",
                  "control_type": "selector",
                  "description": "Mode switch",
                  "identifier": "MODE_SW",
                  "inputs": [
                    {"interface": "fixed_step", "description": "step"},
                    {"interface": "set_state", "description": "set", "max_value": 2},
                    {"interface": "action", "argument": "TOGGLE", "description": "toggle"}
                  ],
                  "outputs": [{
                    "address": 4096,
                    "mask": 3,
                    "shift_by": 0,
                    "max_value": 2,
                    "type": "integer"
                  }],
                  "positions": ["OFF", "ON", "TEST"]
                }
              }
            };
        "#;
        let value = parse_json_or_jsonp(document).expect("jsonp should parse");
        let profile = parse_profile("TEST_MODULE", "Test", &value).expect("profile");

        assert_eq!(profile.controls.len(), 1);
        assert_eq!(profile.controls[0].control_id, "MODE_SW");
        assert_eq!(
            profile.controls[0].inputs[0].argument_options[0].value,
            "DEC"
        );
        assert_eq!(profile.controls[0].inputs[1].argument_options.len(), 3);
        assert_eq!(
            profile.controls[0].inputs[2].argument_options[0].value,
            "TOGGLE"
        );
        assert_eq!(profile.controls[0].outputs[0].address, 4096);
        assert_eq!(profile.controls[0].outputs[0].length, Some(2));
    }

    #[test]
    fn built_in_catalog_is_available_without_dcs_installation() {
        let catalog = AdapterCatalog::builtin();

        assert_eq!(catalog.state, AdapterCatalogState::Loaded);
        assert_eq!(catalog.adapters.len(), 1);
        assert_eq!(catalog.adapters[0].adapter_id, "dcs-bios");
        assert_eq!(catalog.adapters[0].profiles[0].profile_id, "F-16C_50");
        assert_eq!(catalog.adapters[0].profiles[0].control_count, 40);
        assert_eq!(
            catalog.adapters[0].profiles[0].aircraft_names,
            ["F-16C_50", "F-16C", "F-16C bl.50"]
        );
        assert_eq!(
            catalog.adapters[0].profiles[0].role_bindings,
            [
                AdapterRoleBinding {
                    role_id: "left-ddi".to_string(),
                    category: "MFD Left".to_string(),
                },
                AdapterRoleBinding {
                    role_id: "right-ddi".to_string(),
                    category: "MFD Right".to_string(),
                },
            ]
        );
        assert!(catalog.adapters[0].profiles[0]
            .controls
            .iter()
            .all(|control| matches!(control.category.as_str(), "MFD Left" | "MFD Right")));
        assert!(catalog.adapters[0].profiles[0]
            .controls
            .iter()
            .all(|control| control
                .control_id
                .chars()
                .last()
                .is_some_and(|last| last.is_ascii_digit())));
        let left_controls = catalog.adapters[0].profiles[0]
            .controls
            .iter()
            .filter(|control| control.category == "MFD Left")
            .collect::<Vec<_>>();
        assert_eq!(left_controls[0].control_id, "MFD_L_1");
        assert_eq!(left_controls[1].control_id, "MFD_L_2");
        assert_eq!(left_controls[9].control_id, "MFD_L_10");
        assert!(left_controls.iter().all(|control| control
            .inputs
            .iter()
            .any(|input| !input.argument_options.is_empty())));
    }

    #[test]
    fn finds_profile_by_normalized_aircraft_name() {
        let catalog = AdapterCatalog::builtin();

        assert_eq!(
            catalog
                .find_profile_for_aircraft("dcs-bios", Some(" f-16c-bl.50 "))
                .map(|profile| profile.profile_id.as_str()),
            Some("F-16C_50")
        );
        assert!(catalog
            .find_profile_for_aircraft("dcs-bios", Some("F/A-18C"))
            .is_none());
    }

    #[test]
    fn output_lookup_uses_adapter_and_profile_identity() {
        let catalog = AdapterCatalog::builtin();
        let output = catalog
            .find_output("dcs-bios", "F-16C_50", "MFD_L_1", "F_16C_50_MFD_L_1")
            .expect("MFD output");

        assert_eq!(output.address, 17_502);
    }
}
