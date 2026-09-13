use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const REFERENCE_FILE_EXTENSIONS: &[&str] = &["json", "jsonp"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DcsBiosReferenceState {
    Unavailable,
    Loaded,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DcsBiosReferenceCatalog {
    pub state: DcsBiosReferenceState,
    pub source_path: Option<String>,
    pub modules: Vec<DcsBiosReferenceModule>,
    pub controls: Vec<DcsBiosReferenceControl>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DcsBiosReferenceModule {
    pub module_id: String,
    pub label: String,
    pub control_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DcsBiosReferenceControl {
    pub module_id: String,
    pub category: String,
    pub identifier: String,
    pub control_type: String,
    pub description: String,
    pub positions: Vec<String>,
    pub inputs: Vec<DcsBiosReferenceInput>,
    pub outputs: Vec<DcsBiosReferenceOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DcsBiosReferenceInput {
    pub input_id: String,
    pub interface: String,
    pub description: String,
    pub max_value: Option<u32>,
    pub suggested_step: Option<i32>,
    pub argument_options: Vec<DcsBiosArgumentOption>,
    pub supports_event_value: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DcsBiosArgumentOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DcsBiosReferenceOutput {
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

impl DcsBiosReferenceCatalog {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            state: DcsBiosReferenceState::Unavailable,
            source_path: None,
            modules: Vec::new(),
            controls: Vec::new(),
            error: Some(message.into()),
        }
    }

    pub fn discover() -> Self {
        let mut last_error = None;
        for candidate in reference_candidates() {
            if !candidate.exists() {
                continue;
            }

            match Self::load_from_path(&candidate) {
                Ok(catalog) if catalog.state == DcsBiosReferenceState::Loaded => return catalog,
                Ok(catalog) => last_error = catalog.error,
                Err(error) => last_error = Some(error),
            }
        }

        Self::unavailable(last_error.unwrap_or_else(|| {
            "DCS-BIOS control reference data was not found in the standard locations.".to_string()
        }))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let path = path
            .canonicalize()
            .map_err(|error| format!("Failed to resolve DCS-BIOS reference path: {error}"))?;
        let mut files = Vec::new();
        collect_reference_files(&path, &mut files)
            .map_err(|error| format!("Failed to enumerate DCS-BIOS reference files: {error}"))?;
        files.sort();

        if files.is_empty() {
            return Err(format!(
                "No .json or .jsonp DCS-BIOS reference files were found under '{}'.",
                path.display()
            ));
        }

        let mut controls = Vec::new();
        let mut seen_controls = HashSet::new();
        let mut errors = Vec::new();

        for file in files {
            match parse_reference_file(&file) {
                Ok(parsed) => {
                    for control in parsed {
                        let key = (
                            control.module_id.clone(),
                            control.category.clone(),
                            control.identifier.clone(),
                        );
                        if seen_controls.insert(key) {
                            controls.push(control);
                        }
                    }
                }
                Err(error) => errors.push(format!("{}: {error}", file.display())),
            }
        }

        if controls.is_empty() {
            let detail = if errors.is_empty() {
                "No DCS-BIOS controls could be parsed.".to_string()
            } else {
                errors.join("; ")
            };
            return Err(detail);
        }

        controls.sort_by(|left, right| {
            (&left.module_id, &left.category, &left.identifier).cmp(&(
                &right.module_id,
                &right.category,
                &right.identifier,
            ))
        });

        let mut module_counts = BTreeMap::new();
        for control in &controls {
            *module_counts.entry(control.module_id.clone()).or_insert(0) += 1;
        }
        let modules = module_counts
            .into_iter()
            .map(|(module_id, control_count)| DcsBiosReferenceModule {
                label: module_id.replace('_', " "),
                module_id,
                control_count,
            })
            .collect();

        Ok(Self {
            state: DcsBiosReferenceState::Loaded,
            source_path: Some(path.display().to_string()),
            modules,
            controls,
            error: (!errors.is_empty())
                .then(|| format!("{} reference file(s) could not be parsed.", errors.len())),
        })
    }
}

fn reference_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut push = |path: PathBuf| {
        if !candidates.iter().any(|candidate| candidate == &path) {
            candidates.push(path);
        }
    };

    for variable in ["DCS_BIOS_REFERENCE_DIR", "DCS_BIOS_REFERENCE_PATH"] {
        if let Ok(path) = env::var(variable) {
            if !path.trim().is_empty() {
                push(PathBuf::from(path));
            }
        }
    }

    if let Ok(program_files) = env::var("ProgramFiles") {
        push(PathBuf::from(program_files).join("DCS-BIOS/control-reference-json"));
    }
    if let Ok(app_data) = env::var("APPDATA") {
        push(PathBuf::from(app_data).join("DCS-BIOS/control-reference-json"));
    }

    if let Ok(home) = env::var("HOME") {
        let home = PathBuf::from(home);
        push(home.join("Library/Application Support/DCS-BIOS/control-reference-json"));
        push(home.join(".config/DCS-BIOS/control-reference-json"));
        push(home.join("AppData/Roaming/DCS-BIOS/control-reference-json"));
        push(home.join("Saved Games/DCS/Scripts/DCS-BIOS/doc/json"));
        push(home.join("Saved Games/DCS.openbeta/Scripts/DCS-BIOS/doc/json"));
        push(home.join("Saved Games/DCS.openbeta/Scripts/DCS-BIOS/doc"));
        push(home.join("Saved Games/DCS/Scripts/DCS-BIOS/doc"));
    }

    candidates
}

fn collect_reference_files(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_file() {
        if is_reference_file(path) {
            files.push(path.to_path_buf());
        }
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_reference_files(&entry_path, files)?;
        } else if is_reference_file(&entry_path) {
            files.push(entry_path);
        }
    }
    Ok(())
}

fn is_reference_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            REFERENCE_FILE_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn parse_reference_file(path: &Path) -> Result<Vec<DcsBiosReferenceControl>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read reference file: {error}"))?;
    let module_id = module_id_from_contents(&contents)
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })
        .ok_or_else(|| "reference file has no module name".to_string())?;
    let document = parse_json_or_jsonp(&contents)?;
    parse_module_controls(&module_id, &document)
}

fn module_id_from_contents(contents: &str) -> Option<String> {
    let marker = "docdata[\"";
    let start = contents.find(marker)? + marker.len();
    let end = contents[start..].find("\"]")?;
    let module_id = &contents[start..start + end];
    (!module_id.trim().is_empty()).then(|| module_id.to_string())
}

fn parse_json_or_jsonp(contents: &str) -> Result<Value, String> {
    let trimmed = contents.trim().trim_start_matches('\u{feff}').trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }

    let start = trimmed
        .find('{')
        .ok_or_else(|| "reference file does not contain a JSON object".to_string())?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| "reference file has an unterminated JSON object".to_string())?;
    serde_json::from_str(&trimmed[start..=end])
        .map_err(|error| format!("failed to parse JSON/JSONP: {error}"))
}

fn parse_module_controls(
    module_id: &str,
    document: &Value,
) -> Result<Vec<DcsBiosReferenceControl>, String> {
    let root = unwrap_module_document(module_id, document);
    let mut controls = Vec::new();

    let Some(root_object) = root.as_object() else {
        return Err("reference document root is not an object".to_string());
    };

    if let Some(control_list) = root_object.get("controls").and_then(Value::as_array) {
        for control in control_list {
            if let Some(parsed) = parse_control(module_id, "", "", control) {
                controls.push(parsed);
            }
        }
    }

    for (category_key, category_value) in root_object {
        if category_key == "controls" {
            continue;
        }
        let Some(category_object) = category_value.as_object() else {
            continue;
        };

        if category_object.contains_key("identifier") {
            if let Some(parsed) =
                parse_control(module_id, category_key, category_key, category_value)
            {
                controls.push(parsed);
            }
            continue;
        }

        for (identifier_key, control_value) in category_object {
            if let Some(parsed) =
                parse_control(module_id, category_key, identifier_key, control_value)
            {
                controls.push(parsed);
            }
        }
    }

    if controls.is_empty() {
        return Err("reference document contains no controls".to_string());
    }
    Ok(controls)
}

fn unwrap_module_document<'a>(module_id: &str, document: &'a Value) -> &'a Value {
    let Some(object) = document.as_object() else {
        return document;
    };
    if object.len() == 1 {
        if let Some(value) = object.get(module_id) {
            return value;
        }
    }
    document
}

fn parse_control(
    module_id: &str,
    category_key: &str,
    identifier_key: &str,
    value: &Value,
) -> Option<DcsBiosReferenceControl> {
    let object = value.as_object()?;
    let identifier = string_field(object, "identifier")
        .or_else(|| (!identifier_key.is_empty()).then(|| identifier_key.to_string()))?;
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
                .filter_map(|(index, output)| parse_output(identifier.as_str(), index, output))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(DcsBiosReferenceControl {
        module_id: module_id.to_string(),
        category,
        identifier,
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

fn parse_input(index: usize, value: &Value) -> Option<DcsBiosReferenceInput> {
    let object = value.as_object()?;
    let interface = string_field(object, "interface")?;
    let max_value = unsigned_field(object, "max_value");
    let suggested_step = signed_field(object, "suggested_step");
    let description = string_field(object, "description").unwrap_or_default();
    let mut argument_options = Vec::new();

    if let Some(argument) = string_field(object, "argument") {
        argument_options.push(DcsBiosArgumentOption {
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
            "set_state" if max_value.is_some_and(|max_value| max_value <= 32) => {
                if let Some(max_value) = max_value {
                    for value in 0..=max_value {
                        argument_options.push(option(&value.to_string(), &value.to_string()));
                    }
                }
            }
            _ => {}
        }
    }

    Some(DcsBiosReferenceInput {
        input_id: format!("{interface}-{index}"),
        supports_event_value: matches!(interface.as_str(), "set_state" | "variable_step"),
        interface,
        description,
        max_value,
        suggested_step,
        argument_options,
    })
}

fn parse_output(identifier: &str, index: usize, value: &Value) -> Option<DcsBiosReferenceOutput> {
    let object = value.as_object()?;
    let address = unsigned_field(object, "address")?.try_into().ok()?;
    let output_type = string_field(object, "type").unwrap_or_else(|| "integer".to_string());
    let output_id = string_field(object, "address_identifier")
        .or_else(|| string_field(object, "address_mask_shift_identifier"))
        .unwrap_or_else(|| format!("{identifier}:output-{index}"));

    Some(DcsBiosReferenceOutput {
        output_id,
        output_type: output_type.clone(),
        description: string_field(object, "description").unwrap_or_default(),
        address,
        length: unsigned_field(object, "length")
            .or_else(|| unsigned_field(object, "max_length"))
            .or_else(|| (output_type == "integer").then_some(2))
            .and_then(|length| length.try_into().ok()),
        mask: unsigned_field(object, "mask"),
        shift_by: unsigned_field(object, "shift_by").and_then(|shift| shift.try_into().ok()),
        max_value: unsigned_field(object, "max_value"),
        suffix: string_field(object, "suffix").unwrap_or_default(),
    })
}

fn option(value: &str, label: &str) -> DcsBiosArgumentOption {
    DcsBiosArgumentOption {
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
        let controls = parse_module_controls("TEST_MODULE", &value).expect("controls");

        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0].identifier, "MODE_SW");
        assert_eq!(controls[0].inputs[0].argument_options[0].value, "DEC");
        assert_eq!(controls[0].inputs[1].argument_options.len(), 3);
        assert_eq!(controls[0].inputs[2].argument_options[0].value, "TOGGLE");
        assert_eq!(controls[0].outputs[0].address, 4096);
        assert_eq!(controls[0].outputs[0].length, Some(2));
    }

    #[test]
    fn parses_string_output_length_from_max_length() {
        let value: Value = serde_json::json!({
            "Display": {
                "TEXT": {
                    "identifier": "TEXT",
                    "inputs": [],
                    "outputs": [{
                        "address": 8192,
                        "max_length": 24,
                        "type": "string"
                    }]
                }
            }
        });
        let controls = parse_module_controls("TEST", &value).expect("controls");
        assert_eq!(controls[0].outputs[0].length, Some(24));
        assert_eq!(controls[0].outputs[0].output_type, "string");
    }
}
