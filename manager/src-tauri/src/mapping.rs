use std::collections::HashMap;

use hcp::{ControlValue, DisplayPayload};
use serde::{Deserialize, Serialize};

pub const STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    ButtonDown,
    ButtonUp,
    ButtonPushed,
    EncoderDelta,
    AbsoluteChanged,
    ToggleOn,
    ToggleOff,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoleControlDefinition {
    pub logical_control_id: String,
    pub label: String,
    pub supported_events: Vec<EventKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoleDefinition {
    pub role_id: String,
    pub version: u32,
    pub controls: Vec<RoleControlDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalToLogicalBinding {
    pub physical_control_id: u16,
    pub logical_control_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRoleAssignment {
    pub device_id: String,
    pub role_id: String,
    #[serde(default)]
    pub bindings: Vec<PhysicalToLogicalBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterActionConfig {
    pub action_id: String,
    #[serde(default)]
    pub parameters: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterControlMapping {
    pub role_id: String,
    pub logical_control_id: String,
    pub event_kind: EventKind,
    pub action: AdapterActionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterMappingConfig {
    pub adapter_id: String,
    pub profile_id: String,
    #[serde(default)]
    pub mappings: Vec<AdapterControlMapping>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_mappings: Vec<AdapterOutputMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterOutputMapping {
    pub role_id: String,
    pub logical_control_id: String,
    pub source_id: String,
    #[serde(default)]
    pub parameters: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalInputEvent {
    pub device_id: String,
    pub physical_control_id: u16,
    pub role_id: String,
    pub logical_control_id: String,
    pub event_kind: EventKind,
    pub value: ControlValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAdapterAction {
    pub adapter_id: String,
    pub profile_id: String,
    pub event: LogicalInputEvent,
    pub action: AdapterActionConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalOutputEvent {
    pub role_id: String,
    pub logical_control_id: String,
    pub payload: DisplayPayload,
}

pub fn default_role_definitions() -> Vec<RoleDefinition> {
    [("left-ddi", "Left DDI"), ("right-ddi", "Right DDI")]
        .into_iter()
        .map(|(role_id, _label)| RoleDefinition {
            role_id: role_id.to_string(),
            version: 1,
            controls: (0..40)
                .map(|index| RoleControlDefinition {
                    logical_control_id: format!("button-{index}"),
                    label: format!("Button {}", index + 1),
                    supported_events: vec![
                        EventKind::ButtonDown,
                        EventKind::ButtonUp,
                        EventKind::ButtonPushed,
                    ],
                })
                .collect(),
        })
        .collect()
}

pub fn control_event_kinds(event: &ControlValue, was_pressed: bool) -> Vec<EventKind> {
    match event {
        ControlValue::Button { pressed: true } => vec![EventKind::ButtonDown],
        ControlValue::Button { pressed: false } => {
            let mut events = vec![EventKind::ButtonUp];
            if was_pressed {
                events.push(EventKind::ButtonPushed);
            }
            events
        }
        ControlValue::EncoderDelta { .. } => vec![EventKind::EncoderDelta],
        ControlValue::Absolute { .. } => vec![EventKind::AbsoluteChanged],
        ControlValue::Toggle { state: true } => vec![EventKind::ToggleOn],
        ControlValue::Toggle { state: false } => vec![EventKind::ToggleOff],
        ControlValue::RequestDeviceHello => Vec::new(),
    }
}

pub fn resolve_logical_input_events(
    assignments: &[DeviceRoleAssignment],
    device_id: &str,
    physical_control_id: u16,
    event_kind: EventKind,
    value: &ControlValue,
) -> Vec<LogicalInputEvent> {
    assignments
        .iter()
        .filter(|assignment| assignment.device_id == device_id)
        .flat_map(|assignment| {
            assignment
                .bindings
                .iter()
                .filter(move |binding| binding.physical_control_id == physical_control_id)
                .map(move |binding| LogicalInputEvent {
                    device_id: device_id.to_string(),
                    physical_control_id,
                    role_id: assignment.role_id.clone(),
                    logical_control_id: binding.logical_control_id.clone(),
                    event_kind,
                    value: value.clone(),
                })
        })
        .collect()
}

pub fn resolve_adapter_actions(
    adapter_mappings: &[AdapterMappingConfig],
    event: &LogicalInputEvent,
) -> Vec<ResolvedAdapterAction> {
    adapter_mappings
        .iter()
        .flat_map(|config| {
            config
                .mappings
                .iter()
                .filter(|mapping| {
                    mapping.role_id == event.role_id
                        && mapping.logical_control_id == event.logical_control_id
                        && mapping.event_kind == event.event_kind
                })
                .map(move |mapping| ResolvedAdapterAction {
                    adapter_id: config.adapter_id.clone(),
                    profile_id: config.profile_id.clone(),
                    event: event.clone(),
                    action: mapping.action.clone(),
                })
        })
        .collect()
}

pub fn resolve_output_devices(
    assignments: &[DeviceRoleAssignment],
    event: &LogicalOutputEvent,
) -> Vec<(String, u16)> {
    assignments
        .iter()
        .filter(|assignment| assignment.role_id == event.role_id)
        .flat_map(|assignment| {
            assignment
                .bindings
                .iter()
                .filter(|binding| binding.logical_control_id == event.logical_control_id)
                .map(|binding| (assignment.device_id.clone(), binding.physical_control_id))
        })
        .collect()
}

pub fn sanitize_device_role_assignments(
    assignments: Vec<DeviceRoleAssignment>,
) -> Vec<DeviceRoleAssignment> {
    let mut pair_indices: HashMap<(String, String), usize> = HashMap::new();
    let mut sanitized = Vec::new();

    for assignment in assignments {
        let device_id = assignment.device_id.trim().to_string();
        let role_id = assignment.role_id.trim().to_string();
        if device_id.is_empty() || role_id.is_empty() {
            continue;
        }

        let pair = (device_id.clone(), role_id.clone());
        let assignment_index = if let Some(index) = pair_indices.get(&pair) {
            *index
        } else {
            let index = sanitized.len();
            pair_indices.insert(pair, index);
            sanitized.push(DeviceRoleAssignment {
                device_id,
                role_id,
                bindings: Vec::new(),
            });
            index
        };

        for binding in assignment.bindings {
            let logical_control_id = binding.logical_control_id.trim().to_string();
            if logical_control_id.is_empty() {
                continue;
            }

            let target = &mut sanitized[assignment_index].bindings;
            if !target.iter().any(|existing| {
                existing.physical_control_id == binding.physical_control_id
                    && existing.logical_control_id == logical_control_id
            }) {
                target.push(PhysicalToLogicalBinding {
                    physical_control_id: binding.physical_control_id,
                    logical_control_id,
                });
            }
        }
    }

    sanitized
}

pub fn sanitize_adapter_mappings(
    adapter_mappings: Vec<AdapterMappingConfig>,
) -> Vec<AdapterMappingConfig> {
    adapter_mappings
        .into_iter()
        .filter_map(|mut config| {
            config.adapter_id = config.adapter_id.trim().to_string();
            config.profile_id = config.profile_id.trim().to_string();
            if config.adapter_id.is_empty() || config.profile_id.is_empty() {
                return None;
            }

            config.mappings = config
                .mappings
                .into_iter()
                .filter_map(|mut mapping| {
                    mapping.role_id = mapping.role_id.trim().to_string();
                    mapping.logical_control_id = mapping.logical_control_id.trim().to_string();
                    mapping.action.action_id = mapping.action.action_id.trim().to_string();
                    if mapping.role_id.is_empty()
                        || mapping.logical_control_id.is_empty()
                        || mapping.action.action_id.is_empty()
                    {
                        return None;
                    }

                    mapping.action.parameters = mapping
                        .action
                        .parameters
                        .into_iter()
                        .filter_map(|(key, value)| {
                            let key = key.trim().to_string();
                            if key.is_empty() {
                                None
                            } else {
                                Some((key, value))
                            }
                        })
                        .collect();
                    Some(mapping)
                })
                .collect();

            config.output_mappings = config
                .output_mappings
                .into_iter()
                .filter_map(|mut mapping| {
                    mapping.role_id = mapping.role_id.trim().to_string();
                    mapping.logical_control_id = mapping.logical_control_id.trim().to_string();
                    mapping.source_id = mapping.source_id.trim().to_string();
                    if mapping.role_id.is_empty()
                        || mapping.logical_control_id.is_empty()
                        || mapping.source_id.is_empty()
                    {
                        return None;
                    }
                    mapping.parameters = mapping
                        .parameters
                        .into_iter()
                        .filter_map(|(key, value)| {
                            let key = key.trim().to_string();
                            if key.is_empty() {
                                None
                            } else {
                                Some((key, value))
                            }
                        })
                        .collect();
                    Some(mapping)
                })
                .collect();

            Some(config)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(action_id: &str, identifier: &str) -> AdapterActionConfig {
        AdapterActionConfig {
            action_id: action_id.to_string(),
            parameters: HashMap::from([(String::from("identifier"), identifier.to_string())]),
        }
    }

    fn assignment(
        device_id: &str,
        role_id: &str,
        bindings: &[(u16, &str)],
    ) -> DeviceRoleAssignment {
        DeviceRoleAssignment {
            device_id: device_id.to_string(),
            role_id: role_id.to_string(),
            bindings: bindings
                .iter()
                .map(
                    |(physical_control_id, logical_control_id)| PhysicalToLogicalBinding {
                        physical_control_id: *physical_control_id,
                        logical_control_id: (*logical_control_id).to_string(),
                    },
                )
                .collect(),
        }
    }

    #[test]
    fn one_device_can_fan_out_to_multiple_roles_and_logical_controls() {
        let assignments = vec![
            assignment("device-a", "left-ddi", &[(3, "button-1"), (3, "button-2")]),
            assignment("device-a", "right-ddi", &[(3, "button-7")]),
        ];

        let events = resolve_logical_input_events(
            &assignments,
            "device-a",
            3,
            EventKind::ButtonPushed,
            &ControlValue::Button { pressed: false },
        );

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].role_id, "left-ddi");
        assert_eq!(events[1].logical_control_id, "button-2");
        assert_eq!(events[2].role_id, "right-ddi");
    }

    #[test]
    fn multiple_devices_can_target_one_role() {
        let assignments = vec![
            assignment("device-a", "left-ddi", &[(3, "button-1")]),
            assignment("device-b", "left-ddi", &[(8, "button-1")]),
        ];

        let first = resolve_logical_input_events(
            &assignments,
            "device-b",
            8,
            EventKind::ButtonDown,
            &ControlValue::Button { pressed: true },
        );

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].role_id, "left-ddi");
        assert_eq!(first[0].logical_control_id, "button-1");
    }

    #[test]
    fn one_logical_input_can_fan_out_to_multiple_adapters() {
        let event = LogicalInputEvent {
            device_id: "device-a".to_string(),
            physical_control_id: 3,
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-1".to_string(),
            event_kind: EventKind::ButtonPushed,
            value: ControlValue::Button { pressed: false },
        };
        let mappings = vec![
            AdapterMappingConfig {
                adapter_id: "dcs-bios".to_string(),
                profile_id: "default".to_string(),
                output_mappings: Vec::new(),
                mappings: vec![AdapterControlMapping {
                    role_id: "left-ddi".to_string(),
                    logical_control_id: "button-1".to_string(),
                    event_kind: EventKind::ButtonPushed,
                    action: action("control-command", "DCS_ACTION"),
                }],
            },
            AdapterMappingConfig {
                adapter_id: "falcon-bms".to_string(),
                profile_id: "default".to_string(),
                output_mappings: Vec::new(),
                mappings: vec![AdapterControlMapping {
                    role_id: "left-ddi".to_string(),
                    logical_control_id: "button-1".to_string(),
                    event_kind: EventKind::ButtonPushed,
                    action: action("command", "BMS_ACTION"),
                }],
            },
        ];

        let actions = resolve_adapter_actions(&mappings, &event);

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].adapter_id, "dcs-bios");
        assert_eq!(actions[1].adapter_id, "falcon-bms");
    }

    #[test]
    fn assignment_sanitizer_only_removes_exact_binding_duplicates() {
        let sanitized = sanitize_device_role_assignments(vec![
            assignment(
                " device-a ",
                "left-ddi",
                &[(3, "button-1"), (3, "button-1")],
            ),
            assignment("device-a", "left-ddi", &[(3, "button-2")]),
            assignment("device-a", "right-ddi", &[(3, "button-1")]),
            assignment("device-b", "left-ddi", &[(3, "button-1")]),
        ]);

        assert_eq!(sanitized.len(), 3);
        assert_eq!(sanitized[0].bindings.len(), 2);
        assert_eq!(sanitized[1].role_id, "right-ddi");
        assert_eq!(sanitized[2].device_id, "device-b");
    }

    #[test]
    fn output_resolution_fans_out_to_every_matching_device() {
        let assignments = vec![
            assignment("device-a", "left-ddi", &[(0, "master-caution")]),
            assignment("device-b", "left-ddi", &[(1, "master-caution")]),
        ];
        let event = LogicalOutputEvent {
            role_id: "left-ddi".to_string(),
            logical_control_id: "master-caution".to_string(),
            payload: DisplayPayload::Bytes {
                encoding: hcp::ByteEncoding::SegmentMap,
                data: Default::default(),
            },
        };

        assert_eq!(
            resolve_output_devices(&assignments, &event),
            vec![("device-a".to_string(), 0), ("device-b".to_string(), 1)]
        );
    }
}
