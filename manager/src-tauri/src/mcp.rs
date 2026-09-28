use std::{io, net::TcpListener, sync::Arc};

use axum::Router;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::{RequestContext, RoleServer},
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ErrorData, ServerHandler,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use tokio_util::sync::CancellationToken;

use super::*;

pub struct McpServerHandle {
    pub(super) cancel: CancellationToken,
    done: std::sync::mpsc::Receiver<()>,
}

impl McpServerHandle {
    pub(super) fn stop(&self) {
        self.cancel.cancel();
        let _ = self.done.recv_timeout(std::time::Duration::from_secs(2));
    }
}

impl Drop for McpServerHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Clone)]
struct McpHandler {
    app: AppHandle,
    state: Arc<RuntimeState>,
}

const MCP_SERVER_INSTRUCTIONS: &str = "\
HomeCockpit Manager MCP exposes state, configuration, DCS-BIOS, and serial protocol tools. \
Operations may change persisted settings, restart serial endpoint listeners, or send commands to DCS and hardware. \
Serial writes (imcp_send, hcp_send) and DCS import (dcsbios_send_command) only confirm transport delivery, not IMCP ACK, device application, or simulator state. \
Use manager_snapshot for full AppSnapshot; manager_logs is logs only. \
manager_devices returns cached discovery results; call manager_refresh_devices to rescan endpoints (restarts listeners). \
manager_save_* tools replace entire arrays from the snapshot—partial patches are not supported. \
manager_preview_adapter_profile validates without saving; manager_save_adapter_profile persists. \
manager_save_dcsbios_config persists export/command settings and restarts serial endpoint listeners; it does not rebind the export UDP listener—after export host/port changes, call dcsbios_stop then dcsbios_start. \
Trace tools (dcsbios_recent_packets, imcp_recent_frames, hcp_recent_packets) poll in-memory rings (128 entries max) with sinceId; limit defaults to 50. \
Use dcsbios_packet_read for full UDP payload bytes by trace id.";

impl ServerHandler for McpHandler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("homecockpit-manager", env!("CARGO_PKG_VERSION")))
            .with_instructions(MCP_SERVER_INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tool_catalog(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = self.dispatch(request.name.as_ref(), args).await;
        Ok(match result {
            Ok(value) => CallToolResult::structured(value).into(),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error)]).into(),
        })
    }
}

impl McpHandler {
    async fn dispatch(&self, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "manager_snapshot" => to_value(self.state.snapshot()),
            "manager_logs" => to_value(
                self.state
                    .logs
                    .lock()
                    .unwrap()
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            "manager_devices" => to_value(self.state.devices.lock().unwrap().clone()),
            "manager_scan_serial_ports" => {
                to_value(scan_serial_ports_inner(self.state.clone()).await?)
            }
            "manager_preview_adapter_profile" => {
                let request: AdapterProfileImportRequest = field(&args, "request")?;
                to_value(parse_adapter_profile_request(request)?.profile)
            }
            "manager_start_learn" => {
                let request: LearnRequest = field(&args, "request")?;
                self.state.start_learn(&self.app, request)?;
                to_value(self.state.snapshot())
            }
            "manager_cancel_learn" => {
                self.state.cancel_learn(&self.app);
                to_value(self.state.snapshot())
            }
            "dcsbios_recent_packets" => {
                let (since_id, limit) = trace_query(&args)?;
                let entries = self.state.dcs_packet_trace.lock().unwrap();
                to_value(recent_trace(&entries, since_id, limit, |_| true))
            }
            "dcsbios_packet_read" => {
                let id: u64 = field(&args, "id")?;
                let entries = self.state.dcs_packet_trace.lock().unwrap();
                let packet = entries
                    .iter()
                    .find(|packet| packet.id == id)
                    .ok_or("packet ID is no longer in the trace")?;
                Ok(
                    json!({"id":packet.id,"receivedAt":packet.received_at,"source":packet.source,"size":packet.size,"hex":hex_encode(&packet.bytes)}),
                )
            }
            "dcsbios_status" => to_value(
                json!({"config": self.state.config.lock().unwrap().clone(), "status": self.state.status.lock().unwrap().clone()}),
            ),
            "dcsbios_memory_read" => {
                let address: u16 = field(&args, "address")?;
                let length: u16 = field(&args, "length")?;
                if length == 0 || length > 512 {
                    return Err("length must be 1..512 bytes".into());
                }
                let end = address
                    .checked_add(length - 1)
                    .ok_or("memory range exceeds 0xFFFF")?;
                let memory = self
                    .state
                    .dcsbios_memory
                    .lock()
                    .map_err(|_| "DCS-BIOS memory lock failed")?;
                let bytes = memory
                    .read(address..=end)
                    .ok_or("memory range has not been received")?;
                Ok(json!({"address":address,"length":length,"hex":hex_encode(bytes)}))
            }
            "dcsbios_start" => {
                self.state.start_listener(self.app.clone())?;
                to_value(self.state.status.lock().unwrap().clone())
            }
            "dcsbios_stop" => {
                self.state.stop_listener(&self.app);
                to_value(self.state.status.lock().unwrap().clone())
            }
            "dcsbios_send_command" => {
                let request: DcsBiosCommandRequest = parse(args)?;
                let transport = send_dcsbios_command_inner(&self.app, &self.state, request)?;
                Ok(json!({"sent":true,"appliedInDcs":null,"transport":transport}))
            }
            "manager_refresh_devices" => {
                to_value(refresh_devices(self.app.clone(), self.state.clone()).await?)
            }
            "manager_trigger_role_input" => {
                let request: RoleInputTriggerRequest = parse(args)?;
                to_value(trigger_role_input_inner(&self.app, &self.state, request)?)
            }
            "manager_save_dcsbios_config" => {
                let config: DcsBiosConnectionConfig = field(&args, "config")?;
                update_dcsbios_config_inner(&self.app, &self.state, config)?;
                to_value(self.state.snapshot())
            }
            "manager_save_endpoints" => {
                save_device_endpoints_inner(
                    &self.app,
                    &self.state,
                    field(&args, "deviceEndpoints")?,
                )?;
                to_value(self.state.snapshot())
            }
            "manager_save_role_assignments" => {
                save_device_role_assignments_inner(
                    &self.app,
                    &self.state,
                    field(&args, "deviceRoleAssignments")?,
                )?;
                to_value(self.state.snapshot())
            }
            "manager_save_adapter_mappings" => {
                save_adapter_mappings_inner(
                    &self.app,
                    &self.state,
                    field(&args, "adapterMappings")?,
                )?;
                to_value(self.state.snapshot())
            }
            "manager_save_adapter_profile" => {
                let request: AdapterProfileImportRequest = field(&args, "request")?;
                save_adapter_profile_inner(&self.app, &self.state, request)?;
                to_value(self.state.snapshot())
            }
            "hcp_encode" => {
                let kind: String = field(&args, "kind")?;
                let packet = args.get("packet").ok_or("packet is required")?.clone();
                Ok(json!({"hex":hex_encode(&encode_hcp(&kind, packet)?)}))
            }
            "hcp_decode" => {
                let bytes = hex_decode(&field::<String>(&args, "hex")?)?;
                to_value(
                    hcp::decode_app_packet(&bytes)
                        .map_err(|error| format!("HCP decode failed: {error:?}"))?,
                )
            }
            "hcp_send" => {
                let device_id: String = field(&args, "deviceId")?;
                let kind: String = field(&args, "kind")?;
                let packet = args.get("packet").ok_or("packet is required")?.clone();
                let payload = encode_hcp(&kind, packet)?;
                let device = self
                    .state
                    .devices
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|item| {
                        item.id == device_id || item.device_id.as_deref() == Some(&device_id)
                    })
                    .cloned()
                    .ok_or("device not found")?;
                let address = device
                    .assigned_address
                    .ok_or("device has no IMCP address")?;
                let payload = heapless::Vec::from_slice(&payload)
                    .map_err(|_| "HCP payload exceeds IMCP limit")?;
                let frame_payload = if kind == "data" {
                    FramePayload::Data(payload)
                } else {
                    FramePayload::Set(payload)
                };
                let frame = Frame::new(
                    Address::Unicast(address),
                    IMCP_MASTER_ADDRESS,
                    frame_payload,
                );
                self.write_frame(&device.endpoint_id, frame).await
            }
            "imcp_encode" => {
                let frame = parse_frame(&args)?;
                Ok(json!({"hex":encode_frame_hex(&frame)?}))
            }
            "imcp_decode" => {
                let bytes = hex_decode(&field::<String>(&args, "hex")?)?;
                let mut rx = [0u8; 512];
                let mut decoded = [0u8; 256];
                let mut parser = FrameParser::new(&mut rx, &mut decoded);
                parser
                    .write_data(&bytes)
                    .map_err(|error| format!("IMCP parse failed: {error:?}"))?;
                let frame = parser
                    .next_frame()
                    .ok_or("no complete IMCP frame")?
                    .map_err(|error| format!("IMCP decode failed: {error:?}"))?;
                if parser.next_frame().is_some() {
                    return Err("expected exactly one IMCP frame".into());
                }
                Ok(frame_json(&frame))
            }
            "imcp_send" => {
                let endpoint_id: String = field(&args, "endpointId")?;
                let frame = parse_frame(args.get("frame").ok_or("frame is required")?)?;
                self.write_frame(&endpoint_id, frame).await
            }
            "imcp_recent_frames" => {
                let (since_id, limit) = trace_query(&args)?;
                let endpoint_id = args
                    .get("endpointId")
                    .map(|_| field::<String>(&args, "endpointId"))
                    .transpose()?;
                let entries = self.state.imcp_frame_trace.lock().unwrap();
                to_value(recent_trace(&entries, since_id, limit, |entry| {
                    endpoint_id
                        .as_ref()
                        .is_none_or(|id| entry.endpoint_id == *id)
                }))
            }
            "hcp_recent_packets" => {
                let (since_id, limit) = trace_query(&args)?;
                let endpoint_id = args
                    .get("endpointId")
                    .map(|_| field::<String>(&args, "endpointId"))
                    .transpose()?;
                let entries = self.state.imcp_frame_trace.lock().unwrap();
                to_value(recent_trace(&entries, since_id, limit, |entry| {
                    entry.hcp.is_some()
                        && endpoint_id
                            .as_ref()
                            .is_none_or(|id| entry.endpoint_id == *id)
                }))
            }
            _ => Err(format!("Unknown MCP tool: {name}")),
        }
    }

    async fn write_frame(&self, endpoint_id: &str, frame: Frame) -> Result<Value, String> {
        let endpoint_id = endpoint_id.to_string();
        let state = self.state.clone();
        let result =
            tokio::task::spawn_blocking(move || state.send_endpoint_frame(&endpoint_id, frame))
                .await
                .map_err(|error| format!("IMCP write task failed: {error}"))?;
        result?;
        self.state
            .push_log(&self.app, "SUCCESS", "mcp", "Wrote IMCP frame to endpoint.");
        Ok(json!({"written":true,"acknowledged":null,"appliedByDevice":null}))
    }
}

fn to_value(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("Invalid tool arguments: {error}"))
}

fn field<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    parse(
        args.get(name)
            .ok_or_else(|| format!("{name} is required"))?
            .clone(),
    )
}

fn trace_query(args: &Value) -> Result<(u64, usize), String> {
    let since_id = args
        .get("sinceId")
        .map(|_| field(args, "sinceId"))
        .transpose()?
        .unwrap_or(0);
    let limit: usize = args
        .get("limit")
        .map(|_| field(args, "limit"))
        .transpose()?
        .unwrap_or(50);
    if !(1..=MAX_PROTOCOL_TRACE_ENTRIES).contains(&limit) {
        return Err(format!("limit must be 1..{MAX_PROTOCOL_TRACE_ENTRIES}"));
    }
    Ok((since_id, limit))
}

trait TraceEntry {
    fn id(&self) -> u64;
}

impl TraceEntry for DcsPacketTrace {
    fn id(&self) -> u64 {
        self.id
    }
}

impl TraceEntry for ImcpFrameTrace {
    fn id(&self) -> u64 {
        self.id
    }
}

fn recent_trace<T: Clone + TraceEntry>(
    entries: &std::collections::VecDeque<T>,
    since_id: u64,
    limit: usize,
    filter: impl Fn(&T) -> bool,
) -> Vec<T> {
    entries
        .iter()
        .filter(|entry| entry.id() > since_id && filter(entry))
        .take(limit)
        .cloned()
        .collect()
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    let value = value.trim();
    if !value.is_ascii() {
        return Err("hex must contain ASCII digits".into());
    }
    if !value.len().is_multiple_of(2) {
        return Err("hex must contain an even number of digits".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| format!("invalid hex at position {index}"))
        })
        .collect()
}

fn encode_hcp(kind: &str, packet: Value) -> Result<Vec<u8>, String> {
    match kind {
        "data" => {
            let display: DisplayData = parse(packet)?;
            encode_data_packet(&display)
                .map(|bytes| bytes.to_vec())
                .map_err(|error| format!("HCP encode failed: {error:?}"))
        }
        "set" => {
            let value: AppPacketKind = parse(packet)?;
            encode_set_packet(&value)
                .map(|bytes| bytes.to_vec())
                .map_err(|error| format!("HCP encode failed: {error:?}"))
        }
        _ => Err("kind must be data or set".into()),
    }
}

fn parse_frame(args: &Value) -> Result<Frame, String> {
    let to: u8 = field(args, "to")?;
    let from: u8 = field(args, "from")?;
    let kind: String = field(args, "kind")?;
    let payload = match kind.as_str() {
        "ping" => FramePayload::Ping,
        "pong" => FramePayload::Pong,
        "ack" => FramePayload::Ack(field(args, "address")?),
        "join" => FramePayload::Join(field(args, "id")?),
        "setAddress" => FramePayload::SetAddress {
            address: field(args, "address")?,
            id: field(args, "id")?,
        },
        "data" | "set" => {
            let data = hex_decode(&field::<String>(args, "payloadHex")?)?;
            let data =
                heapless::Vec::from_slice(&data).map_err(|_| "IMCP payload exceeds 128 bytes")?;
            if kind == "data" {
                FramePayload::Data(data)
            } else {
                FramePayload::Set(data)
            }
        }
        _ => return Err("kind must be ping, pong, ack, join, setAddress, data, or set".into()),
    };
    Ok(Frame::new(Address::from_byte(to), from, payload))
}

fn encode_frame_hex(frame: &Frame) -> Result<String, String> {
    let mut buffer = [0u8; MAX_ENCODED_FRAME_SIZE];
    let length = frame
        .encode(&mut buffer)
        .map_err(|error| format!("IMCP encode failed: {error:?}"))?;
    Ok(hex_encode(&buffer[..length]))
}

fn schema(properties: Value, required: &[&str]) -> Map<String, Value> {
    let Value::Object(schema) = json!({"type":"object", "properties":properties, "required":required, "additionalProperties":false})
    else {
        unreachable!()
    };
    schema
}

fn tool_catalog() -> Vec<Tool> {
    let mut tools = Vec::new();
    macro_rules! add {
        ($name:expr, $description:expr, $properties:expr, $required:expr) => {
            tools.push(Tool::new(
                $name,
                $description,
                schema($properties, $required),
            ));
        };
    }
    let text = json!({"type":"string"});
    let object = json!({"type":"object"});
    let byte = json!({"type":"integer","minimum":0,"maximum":255});
    let hex = json!({"type":"string","pattern":"^(?:[0-9A-Fa-f]{2})*$"});
    let event_kind = json!({"type":"string","enum":["button-down","button-up","button-pushed","encoder-delta","absolute-changed","toggle-on","toggle-off"]});
    let dcs_config = json!({"type":"object","properties":{"exportHost":text,"exportPort":{"type":"integer","minimum":1,"maximum":65535},"commandHost":text,"commandPort":{"type":"integer","minimum":1,"maximum":65535},"commandTransport":{"type":"string","enum":["udp","tcp"]}},"required":["exportHost","exportPort","commandHost","commandPort","commandTransport"]});
    let endpoint = json!({"type":"object","properties":{"id":text,"name":text,"transport":{"type":"string","const":"serial"},"address":text,"enabled":{"type":"boolean"},"baudRate":{"type":"integer","minimum":1},"roleHint":{"type":"string","enum":["auto","direct-device","imcp-hub"]}},"required":["id","name","transport","address","enabled","baudRate","roleHint"]});
    let assignment = json!({"type":"object","properties":{"deviceId":text,"roleId":text,"bindings":{"type":"array","items":{"type":"object","properties":{"physicalControlId":{"type":"integer","minimum":0,"maximum":65535},"logicalControlId":text},"required":["physicalControlId","logicalControlId"]}}},"required":["deviceId","roleId","bindings"]});
    add!(
        "manager_snapshot",
        "Get full AppSnapshot: persisted settings, runtime status, devices, mappings, roles, logs, and learn session",
        json!({}),
        &[]
    );
    add!(
        "manager_logs",
        "Get retained log entries only; use manager_snapshot for configuration and device state",
        json!({}),
        &[]
    );
    add!(
        "manager_devices",
        "Get cached device summaries from the last discovery; does not rescan serial endpoints",
        json!({}),
        &[]
    );
    add!(
        "manager_scan_serial_ports",
        "List serial ports not in saved endpoints; opens each and returns only ports with a successful IMCP root probe",
        json!({}),
        &[]
    );
    add!(
        "manager_preview_adapter_profile",
        "Parse DCS-BIOS adapter profile JSON; fails if no category is supported by built-in Role bindings; validates only, does not persist",
        json!({"request":{"type":"object"}}),
        &["request"]
    );
    add!(
        "manager_start_learn",
        "Arm in-memory learn for a Role logical input; returns AppSnapshot; bindings persist after a matching physical event",
        json!({"request":{"type":"object","properties":{"roleId":text,"logicalControlId":text,"targetDeviceId":text,"expectedEventKind":event_kind,"mode":{"type":"string","enum":["append","replace"]},"timeoutMs":{"type":"integer","minimum":1}},"required":["roleId","logicalControlId","mode"]}}),
        &["request"]
    );
    add!(
        "manager_cancel_learn",
        "Cancel the active learn session; returns updated AppSnapshot",
        json!({}),
        &[]
    );
    add!(
        "manager_refresh_devices",
        "Rescan saved endpoints, restarting listeners; update cache and return device summaries",
        json!({}),
        &[]
    );
    add!(
        "manager_trigger_role_input",
        "Dispatch Role logical input via adapter mappings; returns integer action count on success; errors if no mapping (DCS-BIOS commands today)",
        json!({"roleId":text,"logicalControlId":text,"eventKind":event_kind}),
        &["roleId", "logicalControlId", "eventKind"]
    );
    add!(
        "manager_save_dcsbios_config",
        "Persist DCS-BIOS export and command settings; restarts serial endpoint listeners; use dcsbios_stop/start to rebind export UDP; returns AppSnapshot",
        json!({"config":dcs_config}),
        &["config"]
    );
    add!(
        "manager_save_endpoints",
        "Replace all saved device endpoints, persist, and restart endpoint listeners; returns AppSnapshot",
        json!({"deviceEndpoints":{"type":"array","items":endpoint}}),
        &["deviceEndpoints"]
    );
    add!(
        "manager_save_role_assignments",
        "Replace all device Role assignments, persist, and restart endpoint listeners; returns AppSnapshot",
        json!({"deviceRoleAssignments":{"type":"array","items":assignment}}),
        &["deviceRoleAssignments"]
    );
    add!(
        "manager_save_adapter_mappings",
        "Replace all adapter control mappings, persist, and restart endpoint listeners; returns AppSnapshot",
        json!({"adapterMappings":{"type":"array","items":object}}),
        &["adapterMappings"]
    );
    add!(
        "manager_save_adapter_profile",
        "Parse adapter profile JSON, upsert to disk, return AppSnapshot; fails if no category is supported by built-in Role bindings",
        json!({"request":{"type":"object","properties":{"adapterId":text,"profileId":text,"label":text,"aircraftNames":{"type":"array","items":text},"source":text},"required":["adapterId","profileId","label","aircraftNames","source"]}}),
        &["request"]
    );
    add!(
        "dcsbios_status",
        "Get DCS-BIOS connection config and export listener status only; use manager_snapshot for full AppSnapshot",
        json!({}),
        &[]
    );
    add!(
        "dcsbios_memory_read",
        "Read 1-512 bytes of received export memory as hex; fails if that range was never received",
        json!({"address":{"type":"integer","minimum":0,"maximum":65535},"length":{"type":"integer","minimum":1,"maximum":512}}),
        &["address", "length"]
    );
    let trace_fields = json!({
        "sinceId":{
            "type":"integer",
            "minimum":0,
            "description":"Return entries with id greater than this; use 0 on the first poll"
        },
        "limit":{
            "type":"integer",
            "minimum":1,
            "maximum":128,
            "description":"Maximum entries to return (default 50 when omitted)"
        }
    });
    add!(
        "dcsbios_recent_packets",
        "Poll retained export UDP previews (hexPreview up to 256 bytes); sinceId cursor, limit default 50; full bytes via dcsbios_packet_read",
        trace_fields.clone(),
        &[]
    );
    add!(
        "dcsbios_packet_read",
        "Full hex of one UDP datagram by trace id from dcsbios_recent_packets; fails if id was evicted from the 128-entry ring or never existed",
        json!({"id":{"type":"integer","minimum":1}}),
        &["id"]
    );
    add!(
        "dcsbios_start",
        "Start the DCS-BIOS export UDP listener using saved config; returns DcsBiosStatus (connectionState, diagnostics, counters)",
        json!({}),
        &[]
    );
    add!(
        "dcsbios_stop",
        "Stop the DCS-BIOS export UDP listener; returns DcsBiosStatus (connectionState reset, diagnostics, counters)",
        json!({}),
        &[]
    );
    add!(
        "dcsbios_send_command",
        "Send rawCommand, or both controlId and argument, on the command transport; returns {sent, transport, appliedInDcs:null}; sent does not confirm simulator state",
        json!({"rawCommand":text,"controlId":text,"argument":text}),
        &[]
    );
    add!(
        "hcp_encode",
        "Encode DisplayData (data) or DeviceHello/ControlEvent (set) as HCP payload hex without sending",
        json!({"kind":{"type":"string","enum":["data","set"]},"packet":object}),
        &["kind", "packet"]
    );
    add!(
        "hcp_decode",
        "Decode HCP payload hex to a packet object without reading a device",
        json!({"hex":hex}),
        &["hex"]
    );
    add!(
        "hcp_send",
        "Send HCP packet to a discovered device; ok means transport write only; fails if device id is not in the current discovery cache",
        json!({"deviceId":text,"kind":{"type":"string","enum":["data","set"]},"packet":object}),
        &["deviceId", "kind", "packet"]
    );
    let frame_fields = json!({"to":byte,"from":byte,"kind":{"type":"string","enum":["ping","pong","ack","join","setAddress","data","set"]},"address":byte,"id":{"type":"integer","minimum":0,"maximum":4294967295u64},"payloadHex":hex});
    add!(
        "imcp_encode",
        "Validate and encode one IMCP frame as stuffed wire hex without sending",
        frame_fields.clone(),
        &["to", "from", "kind"]
    );
    add!(
        "imcp_decode",
        "Decode exactly one complete IMCP wire frame from hex",
        json!({"hex":hex}),
        &["hex"]
    );
    add!(
        "imcp_send",
        "Send IMCP frame on a saved serial endpoint; ok means bytes written only—not IMCP ACK; fails if endpoint listener is not active",
        json!({"endpointId":text,"frame":{"type":"object","properties":frame_fields,"required":["to","from","kind"]}}),
        &["endpointId", "frame"]
    );
    let mut endpoint_trace_fields = trace_fields.as_object().cloned().unwrap_or_default();
    endpoint_trace_fields.insert(
        "endpointId".to_string(),
        json!({
            "type":"string",
            "description":"Optional saved endpoint id; omit to include all endpoints"
        }),
    );
    add!(
        "imcp_recent_frames",
        "Poll retained IMCP RX, parse errors, and MCP-queued TX by sinceId; includes ACK; excludes automatic Manager TX",
        Value::Object(endpoint_trace_fields.clone()),
        &[]
    );
    add!(
        "hcp_recent_packets",
        "Poll successfully decoded HCP packets from retained IMCP traffic by sinceId; optional endpointId filter",
        Value::Object(endpoint_trace_fields),
        &[]
    );
    tools
}

pub fn start(
    app: AppHandle,
    state: Arc<RuntimeState>,
    port: u16,
) -> Result<McpServerHandle, String> {
    if port == 0 {
        return Err("MCP port must be 1..65535.".to_string());
    }
    let listener = bind_listener(port)?;
    let cancel = CancellationToken::new();
    let cancel_for_server = cancel.clone();
    let (done_sender, done) = std::sync::mpsc::channel();
    let config = server_config(port, cancel.child_token());
    let handler = McpHandler { app, state };
    let service = StreamableHttpService::new(
        move || Ok::<_, io::Error>(handler.clone()),
        LocalSessionManager::default().into(),
        config,
    );
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("MCP listener setup failed: {error}");
                let _ = done_sender.send(());
                return;
            }
        };
        let router = Router::new().nest_service("/mcp", service);
        if let Err(error) = axum::serve(listener, router)
            .with_graceful_shutdown(cancel_for_server.cancelled_owned())
            .await
        {
            eprintln!("MCP server failed: {error}");
        }
        let _ = done_sender.send(());
    });
    Ok(McpServerHandle { cancel, done })
}

fn bind_listener(port: u16) -> Result<TcpListener, String> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .map_err(|error| format!("Failed to bind MCP port {port}: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    Ok(listener)
}

fn server_config(port: u16, cancel: CancellationToken) -> StreamableHttpServerConfig {
    let host = format!("127.0.0.1:{port}");
    let localhost = format!("localhost:{port}");
    StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_cancellation_token(cancel)
        .with_allowed_hosts([host.clone(), localhost.clone()])
        .with_allowed_origins([format!("http://{host}"), format!("http://{localhost}")])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[derive(Clone)]
    struct SmokeHandler {
        state: Arc<RuntimeState>,
    }

    impl ServerHandler for SmokeHandler {
        fn get_info(&self) -> ServerConfig {
            ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
                .with_server_info(Implementation::new("homecockpit-manager", "smoke-test"))
                .with_instructions(MCP_SERVER_INSTRUCTIONS)
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            Ok(ListToolsResult {
                tools: tool_catalog(),
                ..Default::default()
            })
        }

        async fn call_tool(
            &self,
            request: CallToolRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, ErrorData> {
            let args = Value::Object(request.arguments.unwrap_or_default());
            let result = match request.name.as_ref() {
                "manager_snapshot" => to_value(self.state.snapshot()),
                "dcsbios_status" => to_value(json!({
                    "config": self.state.config.lock().unwrap().clone(),
                    "status": self.state.status.lock().unwrap().clone(),
                })),
                "imcp_encode" => {
                    let frame = parse_frame(&args);
                    match frame {
                        Ok(frame) => match encode_frame_hex(&frame) {
                            Ok(hex) => Ok(json!({"hex": hex})),
                            Err(error) => Err(error),
                        },
                        Err(error) => Err(error),
                    }
                }
                other => Err(format!("smoke test does not implement {other}")),
            };
            Ok(match result {
                Ok(value) => CallToolResult::structured(value).into(),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(error)]).into(),
            })
        }
    }

    async fn with_smoke_server<F, Fut>(run: F)
    where
        F: FnOnce(u16) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let listener = bind_listener(0).unwrap();
        let port = listener.local_addr().unwrap().port();
        let cancel = CancellationToken::new();
        let state = Arc::new(RuntimeState::new());
        let service = StreamableHttpService::new(
            move || Ok::<_, io::Error>(SmokeHandler { state: state.clone() }),
            LocalSessionManager::default().into(),
            server_config(port, cancel.child_token()),
        );
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let server_cancel = cancel.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().nest_service("/mcp", service))
                .with_graceful_shutdown(server_cancel.cancelled_owned())
                .await
                .unwrap();
        });
        run(port).await;
        cancel.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[derive(Clone)]
    struct CatalogHandler;

    impl ServerHandler for CatalogHandler {
        fn get_info(&self) -> ServerConfig {
            ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
                .with_server_info(Implementation::new("homecockpit-test", "1"))
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            Ok(ListToolsResult {
                tools: tool_catalog(),
                ..Default::default()
            })
        }

        async fn call_tool(
            &self,
            _request: CallToolRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, ErrorData> {
            Ok(CallToolResult::structured(json!({"ok":true})).into())
        }
    }

    fn http_post(port: u16, host: &str, origin: &str, body: &str) -> String {
        let mut socket =
            std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        write!(socket, "POST /mcp HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: 2025-11-25\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        response
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_smoke_server_lists_tuned_tools_and_calls_read_handlers() {
        with_smoke_server(|port| async move {
            let host = format!("127.0.0.1:{port}");
            let origin = format!("http://{host}");
            let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}"#;
            let response = tokio::task::spawn_blocking({
                let host = host.clone();
                let origin = origin.clone();
                move || http_post(port, &host, &origin, init)
            })
            .await
            .unwrap();
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            assert!(
                response.contains("manager_devices returns cached discovery results"),
                "{response}"
            );
            let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
            let response = tokio::task::spawn_blocking({
                let host = host.clone();
                let origin = origin.clone();
                move || http_post(port, &host, &origin, list)
            })
            .await
            .unwrap();
            assert!(
                response.contains("cached device summaries")
                    && response.contains("dcsbios_stop"),
                "{response}"
            );
            let snapshot_call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"manager_snapshot","arguments":{}}}"#;
            let response = tokio::task::spawn_blocking({
                let host = host.clone();
                let origin = origin.clone();
                move || http_post(port, &host, &origin, snapshot_call)
            })
            .await
            .unwrap();
            assert!(
                response.contains("deviceEndpoints") && response.contains("dcsbiosConfig"),
                "{response}"
            );
            let encode_call = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"imcp_encode","arguments":{"to":255,"from":1,"kind":"ping"}}}"#;
            let response = tokio::task::spawn_blocking({
                let host = host.clone();
                let origin = origin.clone();
                move || http_post(port, &host, &origin, encode_call)
            })
            .await
            .unwrap();
            assert!(
                response.contains("\"hex\"") || response.contains("\\\"hex\\\""),
                "{response}"
            );
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_initializes_lists_calls_and_rejects_remote_origins() {
        let listener = bind_listener(0).unwrap();
        let port = listener.local_addr().unwrap().port();
        let cancel = CancellationToken::new();
        let service = StreamableHttpService::new(
            || Ok::<_, io::Error>(CatalogHandler),
            LocalSessionManager::default().into(),
            server_config(port, cancel.child_token()),
        );
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let server_cancel = cancel.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().nest_service("/mcp", service))
                .with_graceful_shutdown(server_cancel.cancelled_owned())
                .await
                .unwrap();
        });
        let host = format!("127.0.0.1:{port}");
        let origin = format!("http://{host}");
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
        let response = tokio::task::spawn_blocking({
            let host = host.clone();
            let origin = origin.clone();
            move || http_post(port, &host, &origin, init)
        })
        .await
        .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("homecockpit-test"), "{response}");
        let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        let response = tokio::task::spawn_blocking({
            let host = host.clone();
            let origin = origin.clone();
            move || http_post(port, &host, &origin, list)
        })
        .await
        .unwrap();
        assert!(response.contains("imcp_send"), "{response}");
        let call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"manager_snapshot","arguments":{}}}"#;
        let response = tokio::task::spawn_blocking({
            let host = host.clone();
            let origin = origin.clone();
            move || http_post(port, &host, &origin, call)
        })
        .await
        .unwrap();
        assert!(
            response.contains("\\\"ok\\\":true") || response.contains("\"ok\":true"),
            "{response}"
        );
        let response = tokio::task::spawn_blocking({
            let host = host.clone();
            move || http_post(port, &host, "http://evil.example", init)
        })
        .await
        .unwrap();
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
        let response = tokio::task::spawn_blocking({
            let origin = origin.clone();
            move || http_post(port, "evil.example", &origin, init)
        })
        .await
        .unwrap();
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
        cancel.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
        assert!(std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_err());
    }

    #[test]
    fn tool_catalog_has_unique_names() {
        let tools = tool_catalog();
        let names: std::collections::HashSet<_> =
            tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert_eq!(tools.len(), 29);
        assert_eq!(names.len(), tools.len());
        assert!(names.contains("imcp_send"));
        assert!(names.contains("dcsbios_memory_read"));
        assert!(names.contains("dcsbios_recent_packets"));
        assert!(names.contains("imcp_recent_frames"));
    }

    #[test]
    fn tool_catalog_descriptions_guide_llm_tool_choice() {
        let tools = tool_catalog();
        let descriptions: std::collections::HashMap<&str, &str> = tools
            .iter()
            .map(|tool| {
                let description = tool
                    .description
                    .as_deref()
                    .expect("every tool must have a description");
                assert!(
                    description.len() >= 40 && description.len() <= 220,
                    "description length for {}: {}",
                    tool.name.as_ref(),
                    description.len()
                );
                (tool.name.as_ref(), description)
            })
            .collect();

        let must_contain = [
            (
                "manager_devices",
                ["cached", "does not rescan"],
            ),
            (
                "manager_refresh_devices",
                ["Rescan", "restarting listeners"],
            ),
            (
                "manager_save_dcsbios_config",
                ["serial endpoint", "dcsbios_stop"],
            ),
            (
                "manager_trigger_role_input",
                ["action count", "DCS-BIOS"],
            ),
            (
                "dcsbios_send_command",
                ["rawCommand", "does not confirm"],
            ),
            (
                "imcp_send",
                ["bytes written", "ACK"],
            ),
            (
                "hcp_send",
                ["transport write", "discovery cache"],
            ),
            (
                "dcsbios_recent_packets",
                ["sinceId", "dcsbios_packet_read"],
            ),
        ];
        for (name, needles) in must_contain {
            let description = descriptions[name];
            for needle in needles {
                assert!(
                    description.contains(needle),
                    "tool {} description must mention '{}': {}",
                    name,
                    needle,
                    description
                );
            }
        }
    }

    #[test]
    fn imcp_wire_round_trip_and_payload_limit() {
        let frame =
            parse_frame(&json!({"to":255,"from":1,"kind":"data","payloadHex":"FDFEFF"})).unwrap();
        let encoded = hex_decode(&encode_frame_hex(&frame).unwrap()).unwrap();
        let mut rx = [0u8; 512];
        let mut decoded = [0u8; 256];
        let mut parser = FrameParser::new(&mut rx, &mut decoded);
        parser.write_data(&encoded).unwrap();
        let decoded = parser.next_frame().unwrap().unwrap();
        assert_eq!(
            frame_json(&decoded),
            json!({"to":255,"from":1,"kind":"data","payloadHex":"FDFEFF"})
        );
        assert!(
            parse_frame(&json!({"to":2,"from":1,"kind":"set","payloadHex":"AA".repeat(129)}))
                .is_err()
        );
        assert!(hex_decode("FFあ").is_err());
    }

    #[test]
    fn hcp_request_device_hello_round_trip() {
        let packet = AppPacketKind::ControlEvent(ControlEvent {
            seq: 0,
            control_id: CONTROL_ID_REQUEST_DEVICE_HELLO,
            event: ControlValue::RequestDeviceHello,
        });
        let bytes = encode_hcp("set", serde_json::to_value(&packet).unwrap()).unwrap();
        assert_eq!(hcp::decode_set_packet(&bytes).unwrap(), packet);
    }

    #[test]
    fn protocol_traces_are_bounded_filterable_and_include_ack_and_hcp() {
        let state = RuntimeState::new();
        let source = "127.0.0.1:5010".parse().unwrap();
        state.record_dcs_packet(source, &[0x55; 4]);
        for _ in 0..MAX_PROTOCOL_TRACE_ENTRIES {
            state.record_dcs_packet(source, &[0x01; 300]);
        }
        let packets = state.dcs_packet_trace.lock().unwrap();
        assert_eq!(packets.len(), MAX_PROTOCOL_TRACE_ENTRIES);
        assert!(packets.front().unwrap().id > 1);
        assert!(packets.back().unwrap().truncated);
        assert!(!packets.back().unwrap().starts_with_sync);
        assert_eq!(packets.back().unwrap().bytes.len(), 300);
        assert!(serde_json::to_value(packets.back().unwrap())
            .unwrap()
            .get("bytes")
            .is_none());
        let newest = packets.back().unwrap().id;
        assert!(recent_trace(&packets, newest, 10, |_| true).is_empty());
        drop(packets);

        let ack = Frame::new(Address::Unicast(1), 2, FramePayload::Ack(2));
        state.record_imcp_frame("endpoint-a", "rx", &ack);
        let packet = AppPacketKind::ControlEvent(ControlEvent {
            seq: 0,
            control_id: CONTROL_ID_REQUEST_DEVICE_HELLO,
            event: ControlValue::RequestDeviceHello,
        });
        let bytes = encode_set_packet(&packet).unwrap();
        let payload = heapless::Vec::from_slice(bytes.as_slice()).unwrap();
        let frame = Frame::new(Address::Unicast(1), 2, FramePayload::Set(payload));
        state.record_imcp_frame("endpoint-a", "rx", &frame);
        state.record_imcp_decode_error("endpoint-b", "bad checksum".into());
        let entries = state.imcp_frame_trace.lock().unwrap();
        let selected = recent_trace(&entries, newest, 2, |entry| {
            entry.endpoint_id == "endpoint-a"
        });
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].frame.as_ref().unwrap()["kind"], "ack");
        assert!(selected[1].hcp.is_some());
        assert!(entries.back().unwrap().decode_error.is_some());
        assert!(trace_query(&json!({"limit":129})).is_err());
    }

    #[test]
    fn endpoint_frame_send_uses_endpoint_command_queue() {
        let state = RuntimeState::new();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        state
            .endpoint_senders
            .lock()
            .unwrap()
            .insert("test".into(), sender);
        let worker = std::thread::spawn(move || {
            let command = receiver.recv().unwrap();
            assert_eq!(frame_json(&command.frame)["kind"], "ping");
            command.reply.send(Ok(())).unwrap();
        });
        let frame = Frame::new(Address::Unicast(2), 1, FramePayload::Ping);
        assert!(state.send_endpoint_frame("test", frame.clone()).is_ok());
        assert!(state.send_endpoint_frame("missing", frame).is_err());
        worker.join().unwrap();
    }

    #[test]
    fn listener_binds_only_loopback_and_reports_port_conflict() {
        let listener = bind_listener(0).unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
        let port = listener.local_addr().unwrap().port();
        assert!(bind_listener(port).is_err());
        drop(listener);
        assert!(bind_listener(port).is_ok());
    }
}
