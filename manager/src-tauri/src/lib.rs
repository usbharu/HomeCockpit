use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use chrono::Utc;
use dcs_bios::{
    import::ImportCommand,
    mem::{MemoryMap, VecMemoryMap},
};
use hcp::{
    decode_set_packet, encode_data_packet, encode_set_packet, AppPacketKind, ByteEncoding,
    ControlEvent, ControlValue, DeviceKind, DisplayData, DisplayPayload, DisplayTarget,
    CONTROL_ID_REQUEST_DEVICE_HELLO,
};
use imcp::{
    frame::{Address, Frame, FramePayload, MAX_ENCODED_FRAME_SIZE},
    parser::FrameParser,
};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tauri::{AppHandle, Emitter, Manager, State};

mod adapter_catalog;
mod mapping;

use adapter_catalog::{
    infer_role_bindings, normalize_aircraft_name, AdapterCatalog, AdapterProfile,
    AdapterProfileConfig,
};
use mapping::{
    control_event_kinds, default_role_definitions, resolve_adapter_actions,
    resolve_logical_input_events, resolve_output_devices, sanitize_adapter_mappings,
    sanitize_device_role_assignments, AdapterActionConfig, AdapterControlMapping,
    AdapterMappingConfig, AdapterOutputMapping, DeviceRoleAssignment, EventKind, LogicalInputEvent,
    LogicalOutputEvent, PhysicalToLogicalBinding, ResolvedAdapterAction, RoleDefinition,
    STATE_SCHEMA_VERSION,
};

const DEFAULT_EXPORT_HOST: &str = "239.255.50.10";
const DEFAULT_EXPORT_PORT: u16 = 5010;
const DEFAULT_COMMAND_HOST: &str = "127.0.0.1";
const DEFAULT_COMMAND_PORT: u16 = 7778;
const MAX_LOG_ENTRIES: usize = 250;
const DEFAULT_DEVICE_ENDPOINT_BAUD_RATE: u32 = 115200;
const DCS_BIOS_AIRCRAFT_NAME_ADDRESS: u16 = 0;
const DCS_BIOS_AIRCRAFT_NAME_LENGTH: usize = 24;
const IMCP_MASTER_ADDRESS: u8 = 0x01;
const IMCP_ROOT_PROBE_TIMEOUT: Duration = Duration::from_millis(900);
const IMCP_CHILD_ENUMERATION_TIMEOUT: Duration = Duration::from_millis(600);
const IMCP_READ_TIMEOUT: Duration = Duration::from_millis(50);
const IMCP_ENDPOINT_RECONNECT_DELAY: Duration = Duration::from_millis(500);
const IMCP_FIRST_DEVICE_ADDRESS: u8 = 0x02;
const IMCP_LAST_DEVICE_ADDRESS: u8 = 0xFE;
const SETTINGS_FILE_NAME: &str = "manager-state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum CommandTransport {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DcsBiosConnectionConfig {
    export_host: String,
    export_port: u16,
    command_host: String,
    command_port: u16,
    command_transport: CommandTransport,
}

impl Default for DcsBiosConnectionConfig {
    fn default() -> Self {
        Self {
            export_host: DEFAULT_EXPORT_HOST.to_string(),
            export_port: DEFAULT_EXPORT_PORT,
            command_host: DEFAULT_COMMAND_HOST.to_string(),
            command_port: DEFAULT_COMMAND_PORT,
            command_transport: CommandTransport::Udp,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DcsBiosStatus {
    connection_state: String,
    last_seen_at: Option<String>,
    last_packet_at: Option<String>,
    packets_per_second: u32,
    total_packets: u64,
    aircraft_name: Option<String>,
    error: Option<String>,
    diagnostics: Vec<String>,
}

impl Default for DcsBiosStatus {
    fn default() -> Self {
        Self {
            connection_state: "stopped".to_string(),
            last_seen_at: None,
            last_packet_at: None,
            packets_per_second: 0,
            total_packets: 0,
            aircraft_name: None,
            error: None,
            diagnostics: vec!["DCS-BIOS listener is stopped.".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagerLogEntry {
    id: u64,
    at: String,
    level: String,
    source: String,
    message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum DeviceEndpointTransport {
    Serial,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum EndpointRoleHint {
    Auto,
    DirectDevice,
    ImcpHub,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DeviceEndpointConfig {
    id: String,
    name: String,
    transport: DeviceEndpointTransport,
    address: String,
    enabled: bool,
    baud_rate: u32,
    role_hint: EndpointRoleHint,
}

impl Default for DeviceEndpointConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            transport: DeviceEndpointTransport::Serial,
            address: String::new(),
            enabled: true,
            baud_rate: DEFAULT_DEVICE_ENDPOINT_BAUD_RATE,
            role_hint: EndpointRoleHint::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManagedDeviceSummary {
    id: String,
    connection_kind: String,
    gateway_id: Option<String>,
    gateway_display_name: Option<String>,
    endpoint_id: String,
    endpoint_name: String,
    endpoint_transport: String,
    endpoint_address: String,
    display_name: String,
    firmware_version: Option<String>,
    state: String,
    protocol: String,
    assigned_address: Option<u8>,
    device_kind: Option<String>,
    device_kind_id: Option<String>,
    protocol_version: Option<u8>,
    device_id: Option<String>,
    displays: Option<u8>,
    controls: Option<u16>,
    features: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DcsBiosFrameEvent {
    received_at: String,
    size: usize,
    preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSnapshot {
    dcsbios_config: DcsBiosConnectionConfig,
    dcsbios_status: DcsBiosStatus,
    adapter_catalog: AdapterCatalog,
    logs: Vec<ManagerLogEntry>,
    devices: Vec<ManagedDeviceSummary>,
    device_endpoints: Vec<DeviceEndpointConfig>,
    device_role_assignments: Vec<DeviceRoleAssignment>,
    adapter_mappings: Vec<AdapterMappingConfig>,
    role_definitions: Vec<RoleDefinition>,
    learn_session: LearnSessionStatus,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DcsBiosCommandRequest {
    raw_command: Option<String>,
    control_id: Option<String>,
    argument: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoleInputTriggerRequest {
    role_id: String,
    logical_control_id: String,
    event_kind: EventKind,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct LearnSessionStatus {
    active: bool,
    role_id: Option<String>,
    logical_control_id: Option<String>,
    target_device_id: Option<String>,
    expected_event_kind: Option<EventKind>,
    mode: Option<LearnMode>,
    armed_at: Option<String>,
    timeout_ms: u64,
    captured_device_id: Option<String>,
    captured_physical_control_id: Option<u16>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum LearnMode {
    Append,
    Replace,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LearnRequest {
    role_id: String,
    logical_control_id: String,
    target_device_id: Option<String>,
    expected_event_kind: Option<EventKind>,
    mode: LearnMode,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct ActiveLearnSession {
    request: LearnRequest,
    armed_at: Instant,
    status: LearnSessionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct PersistedManagerState {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    dcsbios_config: DcsBiosConnectionConfig,
    #[serde(default)]
    device_endpoints: Vec<DeviceEndpointConfig>,
    #[serde(default)]
    device_role_assignments: Vec<DeviceRoleAssignment>,
    #[serde(default)]
    adapter_mappings: Vec<AdapterMappingConfig>,
    #[serde(default)]
    adapter_profiles: Vec<AdapterProfileConfig>,
}

impl Default for PersistedManagerState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            dcsbios_config: DcsBiosConnectionConfig::default(),
            device_endpoints: Vec::new(),
            device_role_assignments: Vec::new(),
            adapter_mappings: Vec::new(),
            adapter_profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdapterProfileImportRequest {
    adapter_id: String,
    profile_id: String,
    label: String,
    aircraft_names: Vec<String>,
    source: String,
}

fn default_schema_version() -> u32 {
    STATE_SCHEMA_VERSION
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyDeviceRoleAssignment {
    device_id: String,
    role: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyDcsBiosMappedAction {
    identifier: String,
    argument: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyRoleControlMapping {
    control_id: u16,
    input_event: String,
    action: LegacyDcsBiosMappedAction,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyRoleMappingConfig {
    role: String,
    #[serde(default)]
    mappings: Vec<LegacyRoleControlMapping>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyPersistedManagerState {
    #[serde(default)]
    device_endpoints: Vec<DeviceEndpointConfig>,
    #[serde(default)]
    device_role_assignments: Vec<LegacyDeviceRoleAssignment>,
    #[serde(default)]
    role_mappings: Vec<LegacyRoleMappingConfig>,
}

struct ListenerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

struct DispatchWorkerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
struct PhysicalControlEvent {
    endpoint_id: String,
    source_address: u8,
    device_id: String,
    control_count: u16,
    control_event: ControlEvent,
}

#[derive(Debug, Clone)]
struct DisplayCommand {
    device_id: String,
    data: DisplayData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DcsBiosMemoryUpdate {
    address: u16,
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DcsBiosStreamEvent {
    FrameBoundary,
    MemoryUpdate(DcsBiosMemoryUpdate),
}

#[derive(Debug, Default)]
struct DcsBiosStreamDecoder {
    buffer: Vec<u8>,
    in_frame: bool,
}

impl DcsBiosStreamDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<DcsBiosStreamEvent> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();

        loop {
            if !self.in_frame {
                let Some(sync_index) = self
                    .buffer
                    .windows(4)
                    .position(|window| window == [0x55; 4])
                else {
                    let keep = self.buffer.len().min(3);
                    if self.buffer.len() > keep {
                        let drain_until = self.buffer.len() - keep;
                        self.buffer.drain(..drain_until);
                    }
                    break;
                };
                self.buffer.drain(..sync_index + 4);
                self.in_frame = true;
            }

            if self.buffer.len() < 4 {
                break;
            }

            if self.buffer[..4] == [0x55; 4] {
                self.buffer.drain(..4);
                events.push(DcsBiosStreamEvent::FrameBoundary);
                continue;
            }

            let address = u16::from_le_bytes([self.buffer[0], self.buffer[1]]);
            let length = u16::from_le_bytes([self.buffer[2], self.buffer[3]]) as usize;
            let total = 4 + length;
            if self.buffer.len() < total {
                if let Some(sync_index) = self
                    .buffer
                    .windows(4)
                    .position(|window| window == [0x55; 4])
                {
                    self.buffer.drain(..sync_index);
                    self.in_frame = false;
                    continue;
                }
                break;
            }

            events.push(DcsBiosStreamEvent::MemoryUpdate(DcsBiosMemoryUpdate {
                address,
                data: self.buffer[4..total].to_vec(),
            }));
            self.buffer.drain(..total);
        }

        events
    }
}

trait InputAdapter: Send {
    fn dispatch_input(
        &self,
        config: &DcsBiosConnectionConfig,
        action: &AdapterActionConfig,
        event: &LogicalInputEvent,
    ) -> Result<(), String>;
}

struct DcsBiosAdapter;

impl InputAdapter for DcsBiosAdapter {
    fn dispatch_input(
        &self,
        config: &DcsBiosConnectionConfig,
        action: &AdapterActionConfig,
        event: &LogicalInputEvent,
    ) -> Result<(), String> {
        let payload = encode_dcsbios_action_payload(action, event)?;
        send_command_to_dcsbios(config, &payload)
    }
}

fn encode_dcsbios_action_payload(
    action: &AdapterActionConfig,
    event: &LogicalInputEvent,
) -> Result<String, String> {
    let identifier = action
        .parameters
        .get("identifier")
        .ok_or_else(|| "DCS-BIOS action is missing identifier parameter.".to_string())?;
    match action.action_id.as_str() {
        "control-command" => {
            let argument = resolve_dcsbios_argument(action, event)?;
            encode_import_command(identifier, &argument)
        }
        "control-pulse" => {
            if event.event_kind != EventKind::ButtonPushed {
                return Err("DCS-BIOS control pulse requires a Button Pushed event.".to_string());
            }
            let press_argument = action
                .parameters
                .get("argument")
                .map(String::as_str)
                .unwrap_or("1");
            let release_argument = action
                .parameters
                .get("releaseArgument")
                .map(String::as_str)
                .unwrap_or("0");
            Ok(format!(
                "{}{}",
                encode_import_command(identifier, press_argument)?,
                encode_import_command(identifier, release_argument)?
            ))
        }
        _ => Err(format!(
            "Unsupported DCS-BIOS action '{}'.",
            action.action_id
        )),
    }
}

struct AdapterRegistry {
    adapters: HashMap<&'static str, Box<dyn InputAdapter>>,
}

impl AdapterRegistry {
    fn new() -> Self {
        let mut adapters: HashMap<&'static str, Box<dyn InputAdapter>> = HashMap::new();
        adapters.insert("dcs-bios", Box::new(DcsBiosAdapter));
        Self { adapters }
    }

    fn get(&self, adapter_id: &str) -> Option<&dyn InputAdapter> {
        self.adapters.get(adapter_id).map(Box::as_ref)
    }
}

#[derive(Debug, Clone)]
struct KnownRuntimeDevice {
    device_id: String,
    control_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingChildDiscovery {
    join_id: u32,
    assigned_address: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingHubDiscovery {
    gateway_id: String,
    gateway_display_name: String,
    children: Vec<PendingChildDiscovery>,
    expires_at: Instant,
}

impl PendingHubDiscovery {
    fn with_expiry(gateway_id: String, gateway_display_name: String, expires_at: Instant) -> Self {
        Self {
            gateway_id,
            gateway_display_name,
            children: Vec::new(),
            expires_at,
        }
    }

    fn is_active(&self, now: Instant) -> bool {
        now < self.expires_at
    }

    fn record_child(&mut self, join_id: u32, assigned_address: u8) {
        if let Some(child) = self
            .children
            .iter_mut()
            .find(|child| child.join_id == join_id)
        {
            child.assigned_address = assigned_address;
            return;
        }

        self.children.push(PendingChildDiscovery {
            join_id,
            assigned_address,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedHubDiscovery {
    hub_address: u8,
    gateway_id: String,
    gateway_display_name: String,
}

impl QueuedHubDiscovery {
    fn new(hub_address: u8, gateway_id: String, gateway_display_name: String) -> Self {
        Self {
            hub_address,
            gateway_id,
            gateway_display_name,
        }
    }
}

fn expire_pending_hub_discoveries(
    pending: &mut HashMap<u8, PendingHubDiscovery>,
    now: Instant,
) -> bool {
    let previous_len = pending.len();
    pending.retain(|_, context| context.is_active(now));
    pending.len() != previous_len
}

fn pending_hub_address_for_join(
    pending: &HashMap<u8, PendingHubDiscovery>,
    source_address: u8,
    now: Instant,
) -> Option<u8> {
    if source_address != 0x00 {
        return None;
    }

    let mut active = pending.iter().filter(|(_, context)| context.is_active(now));
    let (&hub_address, _) = active.next()?;
    if active.next().is_some() {
        return None;
    }

    Some(hub_address)
}

fn record_pending_child_join(
    pending: &mut HashMap<u8, PendingHubDiscovery>,
    source_address: u8,
    join_id: u32,
    assigned_address: u8,
    now: Instant,
) -> Option<u8> {
    let hub_address = pending_hub_address_for_join(pending, source_address, now)?;
    let context = pending.get_mut(&hub_address)?;
    context.record_child(join_id, assigned_address);
    context.expires_at = now + IMCP_CHILD_ENUMERATION_TIMEOUT;
    Some(hub_address)
}

fn pending_gateway_for_child_address(
    pending: &HashMap<u8, PendingHubDiscovery>,
    source_address: u8,
    now: Instant,
) -> Option<(String, String)> {
    pending.values().find_map(|context| {
        if !context.is_active(now) {
            return None;
        }

        context
            .children
            .iter()
            .any(|child| child.assigned_address == source_address)
            .then(|| {
                (
                    context.gateway_id.clone(),
                    context.gateway_display_name.clone(),
                )
            })
    })
}

fn gateway_for_device_hello(
    pending: &HashMap<u8, PendingHubDiscovery>,
    known_child_gateways: &HashMap<String, (String, String)>,
    source_address: u8,
    device_id: &str,
    now: Instant,
) -> Option<(String, String)> {
    pending_gateway_for_child_address(pending, source_address, now)
        .or_else(|| known_child_gateways.get(device_id).cloned())
}

fn is_hub_discovery_traffic(payload: &FramePayload) -> bool {
    match payload {
        FramePayload::Join(_) => true,
        FramePayload::Set(payload) => matches!(
            decode_set_packet(payload.as_slice()),
            Ok(AppPacketKind::DeviceHello(_))
        ),
        _ => false,
    }
}

fn hub_requires_child_discovery(
    observed_hub_addresses: &mut HashMap<String, u8>,
    device_id: &str,
    source_address: u8,
) -> bool {
    observed_hub_addresses.retain(|existing_id, existing_address| {
        *existing_address != source_address || existing_id == device_id
    });
    observed_hub_addresses.insert(device_id.to_string(), source_address) != Some(source_address)
}

fn defer_hub_discovery_during_quiet_period(
    pending: &HashMap<u8, PendingHubDiscovery>,
    quiet_until: &mut Option<Instant>,
    now: Instant,
) {
    if pending.is_empty() && quiet_until.is_some() {
        *quiet_until = Some(now + IMCP_CHILD_ENUMERATION_TIMEOUT);
    }
}

fn hub_discovery_can_advance(
    pending: &mut HashMap<u8, PendingHubDiscovery>,
    quiet_until: &mut Option<Instant>,
    now: Instant,
) -> bool {
    if expire_pending_hub_discoveries(pending, now) {
        *quiet_until = Some(now + IMCP_CHILD_ENUMERATION_TIMEOUT);
        return false;
    }
    if !pending.is_empty() {
        return false;
    }
    if quiet_until.is_some_and(|deadline| now < deadline) {
        return false;
    }

    *quiet_until = None;
    true
}

fn advance_hub_discovery(
    port: &mut dyn serialport::SerialPort,
    pending: &mut HashMap<u8, PendingHubDiscovery>,
    queued: &mut VecDeque<QueuedHubDiscovery>,
    quiet_until: &mut Option<Instant>,
    now: Instant,
) -> Result<(), String> {
    if !hub_discovery_can_advance(pending, quiet_until, now) {
        return Ok(());
    }

    let Some(request) = queued.pop_front() else {
        return Ok(());
    };
    request_child_device_hello(port, request.hub_address)?;
    pending.insert(
        request.hub_address,
        PendingHubDiscovery::with_expiry(
            request.gateway_id,
            request.gateway_display_name,
            now + IMCP_CHILD_ENUMERATION_TIMEOUT,
        ),
    );
    Ok(())
}
struct EndpointListenerChannels {
    dispatch_sender: SyncSender<PhysicalControlEvent>,
    display_sender: SyncSender<DisplayCommand>,
    display_receiver: Receiver<DisplayCommand>,
}

fn known_runtime_devices_for_endpoint(
    devices: &[ManagedDeviceSummary],
    endpoint: &DeviceEndpointConfig,
) -> HashMap<u8, KnownRuntimeDevice> {
    devices
        .iter()
        .filter(|device| {
            device.endpoint_id == endpoint.id && device.endpoint_address == endpoint.address
        })
        .filter_map(|device| {
            Some((
                device.assigned_address?,
                KnownRuntimeDevice {
                    device_id: device.device_id.clone()?,
                    control_count: device.controls?,
                },
            ))
        })
        .collect()
}

#[derive(Debug)]
struct RuntimeAddressAllocator {
    used_addresses: HashSet<u8>,
    join_addresses: HashMap<u32, u8>,
}

impl RuntimeAddressAllocator {
    fn from_known_devices(known_devices: &HashMap<u8, KnownRuntimeDevice>) -> Self {
        Self {
            used_addresses: known_devices.keys().copied().collect(),
            join_addresses: HashMap::new(),
        }
    }

    fn allocate_for_join(&mut self, join_id: u32) -> Result<u8, String> {
        if let Some(&address) = self.join_addresses.get(&join_id) {
            return Ok(address);
        }

        let address = (IMCP_FIRST_DEVICE_ADDRESS..=IMCP_LAST_DEVICE_ADDRESS)
            .find(|address| !self.used_addresses.contains(address))
            .ok_or_else(|| "IMCP device address pool exhausted.".to_string())?;
        self.used_addresses.insert(address);
        self.join_addresses.insert(join_id, address);
        Ok(address)
    }

    fn observe_device_hello(&mut self, address: u8) {
        self.used_addresses.insert(address);
        self.join_addresses
            .retain(|_, assigned_address| *assigned_address != address);
    }

    fn release_address(&mut self, address: u8) {
        if !self
            .join_addresses
            .values()
            .any(|assigned_address| *assigned_address == address)
        {
            self.used_addresses.remove(&address);
        }
    }
}

fn reconcile_known_runtime_device(
    known_devices: &mut HashMap<u8, KnownRuntimeDevice>,
    allocator: &mut RuntimeAddressAllocator,
    source_address: u8,
    probed: &ProbedImcpDevice,
) {
    let stale_addresses = known_devices
        .iter()
        .filter_map(|(&address, device)| {
            (address != source_address && device.device_id == probed.device_id).then_some(address)
        })
        .collect::<Vec<_>>();

    for address in stale_addresses {
        known_devices.remove(&address);
        allocator.release_address(address);
    }

    allocator.observe_device_hello(source_address);
    known_devices.insert(
        source_address,
        KnownRuntimeDevice {
            device_id: probed.device_id.clone(),
            control_count: probed.controls,
        },
    );
}

struct RuntimeState {
    config: Mutex<DcsBiosConnectionConfig>,
    status: Mutex<DcsBiosStatus>,
    adapter_catalog: AdapterCatalog,
    logs: Mutex<VecDeque<ManagerLogEntry>>,
    devices: Mutex<Vec<ManagedDeviceSummary>>,
    device_endpoints: Mutex<Vec<DeviceEndpointConfig>>,
    device_role_assignments: Mutex<Vec<DeviceRoleAssignment>>,
    adapter_mappings: Mutex<Vec<AdapterMappingConfig>>,
    adapter_profiles: Mutex<Vec<AdapterProfileConfig>>,
    learn_session: Mutex<Option<ActiveLearnSession>>,
    log_counter: AtomicU64,
    dispatch_seq: AtomicU64,
    listener: Mutex<Option<ListenerHandle>>,
    endpoint_listeners: Mutex<Vec<ListenerHandle>>,
    dispatch_worker: Mutex<Option<DispatchWorkerHandle>>,
    display_senders: Mutex<HashMap<String, SyncSender<DisplayCommand>>>,
    display_sequences: Mutex<HashMap<String, u16>>,
    dcsbios_memory: Arc<Mutex<VecMemoryMap>>,
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            config: Mutex::new(DcsBiosConnectionConfig::default()),
            status: Mutex::new(DcsBiosStatus::default()),
            adapter_catalog: AdapterCatalog::builtin(),
            logs: Mutex::new(VecDeque::new()),
            devices: Mutex::new(Vec::new()),
            device_endpoints: Mutex::new(Vec::new()),
            device_role_assignments: Mutex::new(Vec::new()),
            adapter_mappings: Mutex::new(Vec::new()),
            adapter_profiles: Mutex::new(Vec::new()),
            learn_session: Mutex::new(None),
            log_counter: AtomicU64::new(0),
            dispatch_seq: AtomicU64::new(0),
            listener: Mutex::new(None),
            endpoint_listeners: Mutex::new(Vec::new()),
            dispatch_worker: Mutex::new(None),
            display_senders: Mutex::new(HashMap::new()),
            display_sequences: Mutex::new(HashMap::new()),
            dcsbios_memory: Arc::new(Mutex::new(VecMemoryMap::default())),
        }
    }

    fn snapshot(&self) -> AppSnapshot {
        let adapter_profiles = self.adapter_profiles.lock().unwrap().clone();
        AppSnapshot {
            dcsbios_config: self.config.lock().unwrap().clone(),
            dcsbios_status: self.status.lock().unwrap().clone(),
            adapter_catalog: self.adapter_catalog.with_custom_profiles(&adapter_profiles),
            logs: self.logs.lock().unwrap().iter().cloned().collect(),
            devices: self.devices.lock().unwrap().clone(),
            device_endpoints: self.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: self.device_role_assignments.lock().unwrap().clone(),
            adapter_mappings: self.adapter_mappings.lock().unwrap().clone(),
            role_definitions: default_role_definitions(),
            learn_session: self
                .learn_session
                .lock()
                .unwrap()
                .as_ref()
                .map(|session| session.status.clone())
                .unwrap_or_default(),
        }
    }

    fn set_status(
        &self,
        app: &AppHandle,
        mut status: DcsBiosStatus,
        config_override: Option<&DcsBiosConnectionConfig>,
    ) {
        let config = config_override
            .cloned()
            .unwrap_or_else(|| self.config.lock().unwrap().clone());
        status.diagnostics = build_diagnostics(&config, &status);
        *self.status.lock().unwrap() = status.clone();
        let _ = app.emit("dcsbios-status-changed", status);
    }

    fn update_status<F>(&self, app: &AppHandle, mutator: F)
    where
        F: FnOnce(&mut DcsBiosStatus),
    {
        let config = self.config.lock().unwrap().clone();
        let mut status = self.status.lock().unwrap().clone();
        mutator(&mut status);
        status.diagnostics = build_diagnostics(&config, &status);
        *self.status.lock().unwrap() = status.clone();
        let _ = app.emit("dcsbios-status-changed", status);
    }

    fn push_log(&self, app: &AppHandle, level: &str, source: &str, message: impl Into<String>) {
        let entry = ManagerLogEntry {
            id: self.log_counter.fetch_add(1, Ordering::Relaxed) + 1,
            at: now_iso8601(),
            level: level.to_string(),
            source: source.to_string(),
            message: message.into(),
        };

        {
            let mut logs = self.logs.lock().unwrap();
            logs.push_front(entry.clone());
            while logs.len() > MAX_LOG_ENTRIES {
                logs.pop_back();
            }
        }

        let _ = app.emit("manager-log", entry);
    }

    fn set_devices(
        &self,
        app: &AppHandle,
        devices: Vec<ManagedDeviceSummary>,
    ) -> Vec<ManagedDeviceSummary> {
        *self.devices.lock().unwrap() = devices.clone();
        let _ = app.emit("devices-changed", devices.clone());
        devices
    }

    fn upsert_device_summary(&self, app: &AppHandle, summary: ManagedDeviceSummary) {
        let devices = {
            let mut devices = self.devices.lock().unwrap();
            let existing_index = devices.iter().position(|existing| {
                let same_device = summary
                    .device_id
                    .as_ref()
                    .is_some_and(|device_id| existing.device_id.as_ref() == Some(device_id));
                let replaces_endpoint_error = existing.device_id.is_none()
                    && existing.endpoint_id == summary.endpoint_id
                    && existing.endpoint_address == summary.endpoint_address;
                same_device || replaces_endpoint_error
            });

            if let Some(index) = existing_index {
                devices[index] = summary;
            } else {
                devices.push(summary);
            }

            devices.clone()
        };
        let _ = app.emit("devices-changed", devices);
    }

    fn set_device_endpoints(
        &self,
        app: &AppHandle,
        device_endpoints: Vec<DeviceEndpointConfig>,
    ) -> Vec<DeviceEndpointConfig> {
        *self.device_endpoints.lock().unwrap() = device_endpoints.clone();
        let _ = app.emit("device-endpoints-changed", device_endpoints.clone());
        device_endpoints
    }

    fn set_device_role_assignments(
        &self,
        app: &AppHandle,
        device_role_assignments: Vec<DeviceRoleAssignment>,
    ) -> Vec<DeviceRoleAssignment> {
        *self.device_role_assignments.lock().unwrap() = device_role_assignments.clone();
        let _ = app.emit(
            "device-role-assignments-changed",
            device_role_assignments.clone(),
        );
        device_role_assignments
    }

    fn set_adapter_mappings(
        &self,
        app: &AppHandle,
        adapter_mappings: Vec<AdapterMappingConfig>,
    ) -> Vec<AdapterMappingConfig> {
        *self.adapter_mappings.lock().unwrap() = adapter_mappings.clone();
        let _ = app.emit("adapter-mappings-changed", adapter_mappings.clone());
        adapter_mappings
    }

    fn set_adapter_profiles(
        &self,
        app: &AppHandle,
        adapter_profiles: Vec<AdapterProfileConfig>,
    ) -> Vec<AdapterProfileConfig> {
        *self.adapter_profiles.lock().unwrap() = adapter_profiles.clone();
        let _ = app.emit("adapter-profiles-changed", adapter_profiles.clone());
        adapter_profiles
    }

    fn effective_adapter_catalog(&self) -> AdapterCatalog {
        let adapter_profiles = self.adapter_profiles.lock().unwrap().clone();
        self.adapter_catalog.with_custom_profiles(&adapter_profiles)
    }

    fn active_adapter_mappings(&self) -> Vec<AdapterMappingConfig> {
        let aircraft_name = self.status.lock().unwrap().aircraft_name.clone();
        let catalog = self.effective_adapter_catalog();
        let persisted = self.adapter_mappings.lock().unwrap().clone();

        if let Some(profile) =
            catalog.find_profile_for_aircraft("dcs-bios", aircraft_name.as_deref())
        {
            let custom = persisted
                .iter()
                .filter(|config| {
                    config.adapter_id == "dcs-bios"
                        && config.profile_id == profile.profile_id
                        && config.aircraft_name.as_deref().is_some_and(|name| {
                            normalize_aircraft_name(name)
                                == normalize_aircraft_name(
                                    aircraft_name.as_deref().unwrap_or_default(),
                                )
                        })
                })
                .cloned()
                .collect::<Vec<_>>();
            if !custom.is_empty() {
                return custom;
            }

            return vec![build_profile_adapter_mapping("dcs-bios", profile)];
        }

        persisted
            .into_iter()
            .filter(|config| {
                config.aircraft_name.as_deref().is_none_or(|name| {
                    aircraft_name.as_deref().is_some_and(|current| {
                        normalize_aircraft_name(name) == normalize_aircraft_name(current)
                    })
                })
            })
            .collect()
    }

    fn register_display_sender(&self, device_id: String, sender: SyncSender<DisplayCommand>) {
        self.display_senders
            .lock()
            .unwrap()
            .insert(device_id, sender);
    }

    fn clear_display_senders(&self) {
        self.display_senders.lock().unwrap().clear();
    }

    fn next_display_sequence(&self, device_id: &str) -> u16 {
        let mut sequences = self.display_sequences.lock().unwrap();
        let sequence = sequences.entry(device_id.to_string()).or_insert(0);
        *sequence = sequence.wrapping_add(1);
        *sequence
    }

    fn dispatch_logical_output_event(
        &self,
        app: &AppHandle,
        assignments: &[DeviceRoleAssignment],
        event: &LogicalOutputEvent,
    ) {
        let dispatch_seq = self.dispatch_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let senders = self.display_senders.lock().unwrap().clone();
        for (device_id, physical_display_id) in resolve_output_devices(assignments, event) {
            let Some(sender) = senders.get(&device_id) else {
                self.push_log(
                    app,
                    "DEBUG",
                    "display",
                    format!(
                        "dispatchSeq={dispatch_seq} no display transport for device {}.",
                        device_id
                    ),
                );
                continue;
            };

            let display_data = DisplayData {
                seq: self.next_display_sequence(&device_id),
                target: DisplayTarget::Indicator(physical_display_id),
                payload: event.payload.clone(),
            };
            match sender.try_send(DisplayCommand {
                device_id: device_id.clone(),
                data: display_data,
            }) {
                Ok(()) => self.push_log(
                    app,
                    "SUCCESS",
                    "display",
                    format!(
                        "dispatchSeq={dispatch_seq} queued logical output {}:{} for device {}.",
                        event.role_id, event.logical_control_id, device_id
                    ),
                ),
                Err(TrySendError::Full(_)) => self.push_log(
                    app,
                    "WARN",
                    "display",
                    format!(
                        "dispatchSeq={dispatch_seq} display queue full for device {}.",
                        device_id
                    ),
                ),
                Err(TrySendError::Disconnected(_)) => self.push_log(
                    app,
                    "WARN",
                    "display",
                    format!(
                        "dispatchSeq={dispatch_seq} display transport disconnected for device {}.",
                        device_id
                    ),
                ),
            }
        }
    }

    fn dispatch_dcsbios_memory_update(&self, app: &AppHandle, update: &DcsBiosMemoryUpdate) {
        let adapter_mappings = self.active_adapter_mappings();
        let catalog = self.effective_adapter_catalog();
        let assignments = self.device_role_assignments.lock().unwrap().clone();
        for config in adapter_mappings
            .iter()
            .filter(|config| config.adapter_id == "dcs-bios")
        {
            for mapping in &config.output_mappings {
                if mapping.source_id != "control-output" && mapping.source_id != "memory-range" {
                    continue;
                }
                let Some((address, length)) =
                    resolve_dcsbios_output_range(&catalog, config, mapping)
                else {
                    self.push_log(
                        app,
                        "WARN",
                        "display",
                        format!(
                            "Ignoring DCS-BIOS output mapping {}:{} because its reference output is unavailable.",
                            mapping.role_id, mapping.logical_control_id
                        ),
                    );
                    continue;
                };
                if update.address != address {
                    continue;
                }
                if let Some(length) = length {
                    if update.data.len() != length {
                        continue;
                    }
                }

                let encoding = match mapping
                    .parameters
                    .get("encoding")
                    .map(|value| value.trim().to_ascii_lowercase())
                    .as_deref()
                {
                    Some("segment-map") => ByteEncoding::SegmentMap,
                    Some("utf8-text") | Some("utf8") => ByteEncoding::Utf8Text,
                    _ => ByteEncoding::MonoBitmap1bpp,
                };
                let mut data = heapless::Vec::new();
                if data.extend_from_slice(&update.data).is_err() {
                    self.push_log(
                        app,
                        "WARN",
                        "display",
                        format!(
                            "DCS-BIOS output mapping {}:{} payload is too large.",
                            mapping.role_id, mapping.logical_control_id
                        ),
                    );
                    continue;
                }
                self.dispatch_logical_output_event(
                    app,
                    &assignments,
                    &LogicalOutputEvent {
                        role_id: mapping.role_id.clone(),
                        logical_control_id: mapping.logical_control_id.clone(),
                        payload: DisplayPayload::Bytes { encoding, data },
                    },
                );
            }
        }
    }

    fn emit_learn_session(&self, app: &AppHandle, status: LearnSessionStatus) {
        let _ = app.emit("learn-session-changed", status);
    }

    fn start_learn(&self, app: &AppHandle, mut request: LearnRequest) -> Result<(), String> {
        request.role_id = request.role_id.trim().to_string();
        request.logical_control_id = request.logical_control_id.trim().to_string();
        request.target_device_id = request
            .target_device_id
            .map(|device_id| device_id.trim().to_string())
            .filter(|device_id| !device_id.is_empty());
        if request.role_id.is_empty() || request.logical_control_id.is_empty() {
            return Err("Learn requires roleId and logicalControlId.".to_string());
        }

        let timeout_ms = request.timeout_ms.unwrap_or(10_000).clamp(500, 60_000);
        request.timeout_ms = Some(timeout_ms);
        let armed_at = Instant::now();
        let status = LearnSessionStatus {
            active: true,
            role_id: Some(request.role_id.clone()),
            logical_control_id: Some(request.logical_control_id.clone()),
            target_device_id: request.target_device_id.clone(),
            expected_event_kind: request.expected_event_kind,
            mode: Some(request.mode),
            armed_at: Some(now_iso8601()),
            timeout_ms,
            captured_device_id: None,
            captured_physical_control_id: None,
        };
        *self.learn_session.lock().unwrap() = Some(ActiveLearnSession {
            request,
            armed_at,
            status: status.clone(),
        });
        self.emit_learn_session(app, status);
        self.push_log(
            app,
            "INFO",
            "mapping",
            "Started physical control learn session.",
        );
        Ok(())
    }

    fn cancel_learn(&self, app: &AppHandle) {
        let was_active = self.learn_session.lock().unwrap().take().is_some();
        if was_active {
            self.emit_learn_session(app, LearnSessionStatus::default());
            self.push_log(
                app,
                "INFO",
                "mapping",
                "Cancelled physical control learn session.",
            );
        }
    }

    fn expire_learn(&self, app: &AppHandle) {
        let expired = {
            let mut session = self.learn_session.lock().unwrap();
            if session.as_ref().is_some_and(|active| {
                active.armed_at.elapsed().as_millis() >= u128::from(active.status.timeout_ms)
            }) {
                session.take()
            } else {
                None
            }
        };

        if expired.is_some() {
            self.emit_learn_session(app, LearnSessionStatus::default());
            self.push_log(
                app,
                "INFO",
                "mapping",
                "Physical control learn session timed out.",
            );
        }
    }

    fn capture_learn(
        &self,
        app: &AppHandle,
        physical_event: &PhysicalControlEvent,
        event_kinds: &[EventKind],
    ) -> Result<bool, String> {
        let Some(mut session) = self.learn_session.lock().unwrap().take() else {
            return Ok(false);
        };

        let target_matches = session
            .request
            .target_device_id
            .as_ref()
            .is_none_or(|target| target == &physical_event.device_id);
        let event_matches = session
            .request
            .expected_event_kind
            .is_none_or(|expected| event_kinds.contains(&expected));
        if !target_matches || !event_matches {
            *self.learn_session.lock().unwrap() = Some(session);
            return Ok(false);
        }

        let request = session.request.clone();
        let assignments = apply_learn_binding(
            self.device_role_assignments.lock().unwrap().clone(),
            &request,
            &physical_event.device_id,
            physical_event.control_event.control_id,
        );
        let persisted = PersistedManagerState {
            schema_version: STATE_SCHEMA_VERSION,
            dcsbios_config: self.config.lock().unwrap().clone(),
            device_endpoints: self.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: assignments.clone(),
            adapter_mappings: self.adapter_mappings.lock().unwrap().clone(),
            adapter_profiles: self.adapter_profiles.lock().unwrap().clone(),
        };
        if let Err(error) = persist_manager_state(app, &persisted) {
            *self.learn_session.lock().unwrap() = Some(session);
            return Err(error);
        }
        self.set_device_role_assignments(app, assignments);

        session.status.active = false;
        session.status.captured_device_id = Some(physical_event.device_id.clone());
        session.status.captured_physical_control_id = Some(physical_event.control_event.control_id);
        let status = session.status;
        self.emit_learn_session(app, status);
        self.push_log(
            app,
            "INFO",
            "mapping",
            format!(
                "Learned physical control {} from device {} for {}:{}.",
                physical_event.control_event.control_id,
                physical_event.device_id,
                request.role_id,
                request.logical_control_id
            ),
        );
        Ok(true)
    }

    fn stop_listener(&self, app: &AppHandle) {
        let handle = self.listener.lock().unwrap().take();
        if let Some(mut handle) = handle {
            handle.stop.store(true, Ordering::Relaxed);
            if let Some(join) = handle.join.take() {
                let _ = join.join();
            }
            self.push_log(app, "INFO", "dcsbios", "Stopped DCS-BIOS listener.");
        }
        self.set_status(
            app,
            DcsBiosStatus {
                connection_state: "stopped".to_string(),
                last_seen_at: None,
                last_packet_at: None,
                packets_per_second: 0,
                total_packets: 0,
                aircraft_name: None,
                error: None,
                diagnostics: Vec::new(),
            },
            None,
        );
    }

    fn stop_endpoint_listeners(&self, app: &AppHandle) {
        let listeners = {
            let mut listeners = self.endpoint_listeners.lock().unwrap();
            std::mem::take(&mut *listeners)
        };

        for mut handle in listeners {
            handle.stop.store(true, Ordering::Relaxed);
            if let Some(join) = handle.join.take() {
                let _ = join.join();
            }
        }

        let dispatch_worker = self.dispatch_worker.lock().unwrap().take();
        if let Some(mut worker) = dispatch_worker {
            worker.stop.store(true, Ordering::Relaxed);
            if let Some(join) = worker.join.take() {
                let _ = join.join();
            }
        }
        self.clear_display_senders();

        self.push_log(app, "INFO", "devices", "Stopped device endpoint listeners.");
    }

    fn start_endpoint_listeners(self: &Arc<Self>, app: AppHandle) -> Result<(), String> {
        self.stop_endpoint_listeners(&app);

        let endpoints = sanitize_device_endpoints(self.device_endpoints.lock().unwrap().clone());
        let (dispatch_sender, dispatch_receiver) = mpsc::sync_channel(256);
        let dispatch_stop = Arc::new(AtomicBool::new(false));
        let dispatch_stop_for_thread = dispatch_stop.clone();
        let dispatch_state = Arc::clone(self);
        let dispatch_app = app.clone();
        let dispatch_join = thread::spawn(move || {
            run_dispatch_worker(
                dispatch_state,
                dispatch_app,
                dispatch_receiver,
                dispatch_stop_for_thread,
            );
        });
        *self.dispatch_worker.lock().unwrap() = Some(DispatchWorkerHandle {
            stop: dispatch_stop,
            join: Some(dispatch_join),
        });

        let mut listeners = Vec::new();
        for endpoint in endpoints.into_iter().filter(|entry| entry.enabled) {
            let initial_known_devices =
                known_runtime_devices_for_endpoint(&self.devices.lock().unwrap(), &endpoint);
            let stop = Arc::new(AtomicBool::new(false));
            let stop_for_thread = stop.clone();
            let app_for_thread = app.clone();
            let state = Arc::clone(self);
            let (display_sender, display_receiver) = mpsc::sync_channel(64);
            let channels = EndpointListenerChannels {
                dispatch_sender: dispatch_sender.clone(),
                display_sender,
                display_receiver,
            };
            for device in initial_known_devices.values() {
                self.register_display_sender(
                    device.device_id.clone(),
                    channels.display_sender.clone(),
                );
            }

            let join = thread::spawn(move || {
                let state_for_run = state.clone();
                if let Err(error) = run_endpoint_listener(
                    state_for_run,
                    app_for_thread.clone(),
                    endpoint,
                    channels,
                    initial_known_devices,
                    stop_for_thread,
                ) {
                    state.push_log(&app_for_thread, "ERROR", "devices", error);
                }
            });

            listeners.push(ListenerHandle {
                stop,
                join: Some(join),
            });
        }

        *self.endpoint_listeners.lock().unwrap() = listeners;
        Ok(())
    }

    fn restart_endpoint_listeners(self: &Arc<Self>, app: &AppHandle) -> Result<(), String> {
        self.start_endpoint_listeners(app.clone())
    }

    fn start_listener(self: &Arc<Self>, app: AppHandle) -> Result<(), String> {
        self.stop_listener(&app);

        let config = self.config.lock().unwrap().clone();
        self.set_status(
            &app,
            DcsBiosStatus {
                connection_state: "connecting".to_string(),
                last_seen_at: None,
                last_packet_at: None,
                packets_per_second: 0,
                total_packets: 0,
                aircraft_name: None,
                error: None,
                diagnostics: Vec::new(),
            },
            Some(&config),
        );
        self.push_log(
            &app,
            "INFO",
            "dcsbios",
            format!(
                "Starting listener on {}:{}.",
                config.export_host, config.export_port
            ),
        );

        let socket = bind_export_socket(&config).inspect_err(|error| {
            self.set_status(
                &app,
                DcsBiosStatus {
                    connection_state: "error".to_string(),
                    last_seen_at: None,
                    last_packet_at: None,
                    packets_per_second: 0,
                    total_packets: 0,
                    aircraft_name: None,
                    error: Some(error.clone()),
                    diagnostics: Vec::new(),
                },
                Some(&config),
            );
            self.push_log(&app, "ERROR", "dcsbios", error.clone());
        })?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let state = Arc::clone(self);
        let app_for_thread = app.clone();
        let dcsbios_memory = self.dcsbios_memory.clone();

        let join = thread::spawn(move || {
            state.push_log(
                &app_for_thread,
                "INFO",
                "dcsbios",
                "Socket bound successfully.",
            );
            state.update_status(&app_for_thread, |status| {
                status.connection_state = "listening".to_string();
                status.error = None;
            });

            let mut buf = [0_u8; 65535];
            let mut stream_decoder = DcsBiosStreamDecoder::default();
            let mut aircraft_name_dirty = false;
            let mut last_rate_tick = Instant::now();
            let mut packets_in_window = 0_u32;

            while !stop_for_thread.load(Ordering::Relaxed) {
                match socket.recv(&mut buf) {
                    Ok(size) => {
                        packets_in_window = packets_in_window.saturating_add(1);
                        let events = stream_decoder.feed(&buf[..size]);
                        let mut aircraft_update = None;
                        for event in events {
                            match event {
                                DcsBiosStreamEvent::FrameBoundary => {
                                    if aircraft_name_dirty {
                                        match extract_aircraft_name_from_memory(&dcsbios_memory) {
                                            Ok(name) => aircraft_update = Some(name),
                                            Err(error) => state.push_log(
                                                &app_for_thread,
                                                "WARN",
                                                "dcsbios",
                                                error,
                                            ),
                                        }
                                        aircraft_name_dirty = false;
                                    }
                                }
                                DcsBiosStreamEvent::MemoryUpdate(update) => {
                                    aircraft_name_dirty |= update_overlaps_aircraft_name(&update);
                                    if let Err(error) = apply_dcsbios_memory_updates(
                                        dcsbios_memory.clone(),
                                        std::slice::from_ref(&update),
                                    ) {
                                        state.push_log(&app_for_thread, "WARN", "dcsbios", error);
                                    }
                                    state.dispatch_dcsbios_memory_update(&app_for_thread, &update);
                                }
                            }
                        }
                        let now = now_iso8601();
                        let preview = extract_ascii_preview(&buf[..size]);
                        state.update_status(&app_for_thread, |status| {
                            status.connection_state = "receiving".to_string();
                            status.last_seen_at = Some(now.clone());
                            status.last_packet_at = Some(now.clone());
                            status.error = None;
                            status.total_packets = status.total_packets.saturating_add(1);
                            if let Some(name) = &aircraft_update {
                                status.aircraft_name = name.clone();
                            }
                        });
                        let _ = app_for_thread.emit(
                            "dcsbios-frame-received",
                            DcsBiosFrameEvent {
                                received_at: now,
                                size,
                                preview,
                            },
                        );

                        if last_rate_tick.elapsed() >= Duration::from_secs(1) {
                            let packets_per_second = packets_in_window;
                            packets_in_window = 0;
                            last_rate_tick = Instant::now();
                            state.update_status(&app_for_thread, |status| {
                                status.packets_per_second = packets_per_second;
                            });
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        let status = state.status.lock().unwrap().clone();
                        let stale = status
                            .last_seen_at
                            .as_ref()
                            .and(status.last_packet_at.as_ref())
                            .is_some();
                        if stale && last_rate_tick.elapsed() >= Duration::from_secs(1) {
                            last_rate_tick = Instant::now();
                            packets_in_window = 0;
                            state.update_status(&app_for_thread, |current| {
                                current.packets_per_second = 0;
                                if current.connection_state == "receiving" {
                                    current.connection_state = "listening".to_string();
                                }
                            });
                        }
                    }
                    Err(error) => {
                        let message = format!("Receive failed: {error}");
                        state.push_log(&app_for_thread, "ERROR", "dcsbios", message.clone());
                        state.update_status(&app_for_thread, |status| {
                            status.connection_state = "error".to_string();
                            status.error = Some(message.clone());
                            status.packets_per_second = 0;
                        });
                        break;
                    }
                }
            }
        });

        *self.listener.lock().unwrap() = Some(ListenerHandle {
            stop,
            join: Some(join),
        });

        Ok(())
    }
}

#[derive(Clone)]
struct AppState {
    inner: Arc<RuntimeState>,
}

impl AppState {
    fn new() -> Self {
        Self {
            inner: Arc::new(RuntimeState::new()),
        }
    }
}

#[tauri::command]
fn get_app_state(state: State<'_, AppState>) -> AppSnapshot {
    state.inner.snapshot()
}

#[tauri::command]
fn update_dcsbios_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: DcsBiosConnectionConfig,
) -> Result<AppSnapshot, String> {
    persist_manager_state(
        &app,
        &PersistedManagerState {
            schema_version: STATE_SCHEMA_VERSION,
            dcsbios_config: config.clone(),
            device_endpoints: state.inner.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: state.inner.device_role_assignments.lock().unwrap().clone(),
            adapter_mappings: state.inner.adapter_mappings.lock().unwrap().clone(),
            adapter_profiles: state.inner.adapter_profiles.lock().unwrap().clone(),
        },
    )?;
    *state.inner.config.lock().unwrap() = config.clone();
    state.inner.restart_endpoint_listeners(&app)?;
    state.inner.update_status(&app, |_| {});
    state.inner.push_log(
        &app,
        "INFO",
        "settings",
        format!(
            "Updated DCS-BIOS config: export {}:{}, command {}:{} ({:?}).",
            config.export_host,
            config.export_port,
            config.command_host,
            config.command_port,
            config.command_transport
        ),
    );
    Ok(state.inner.snapshot())
}

#[tauri::command]
fn start_dcsbios(app: AppHandle, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    state.inner.start_listener(app)?;
    Ok(state.inner.snapshot())
}

#[tauri::command]
fn stop_dcsbios(app: AppHandle, state: State<'_, AppState>) -> AppSnapshot {
    state.inner.stop_listener(&app);
    state.inner.snapshot()
}

#[tauri::command]
fn send_dcsbios_command(
    app: AppHandle,
    state: State<'_, AppState>,
    request: DcsBiosCommandRequest,
) -> Result<(), String> {
    let config = state.inner.config.lock().unwrap().clone();
    let payload = normalize_command_request(request)?;
    send_command_to_dcsbios(&config, &payload)?;
    state.inner.push_log(
        &app,
        "SUCCESS",
        "dcsbios",
        format!(
            "Sent DCS-BIOS command to {}:{} via {:?}: {}",
            config.command_host,
            config.command_port,
            config.command_transport,
            payload.trim_end()
        ),
    );
    Ok(())
}

#[tauri::command]
fn trigger_role_input(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RoleInputTriggerRequest,
) -> Result<usize, String> {
    let event = build_role_input_event(request)?;
    let mappings = state.inner.active_adapter_mappings();
    let actions = resolve_adapter_actions(&mappings, &event);
    if actions.is_empty() {
        return Err(format!(
            "No Adapter mapping for Role action '{}:{} {:?}'.",
            event.role_id, event.logical_control_id, event.event_kind
        ));
    }

    let config = state.inner.config.lock().unwrap().clone();
    let registry = AdapterRegistry::new();
    for resolved in &actions {
        let adapter = registry
            .get(&resolved.adapter_id)
            .ok_or_else(|| format!("Adapter '{}' is not registered.", resolved.adapter_id))?;
        adapter.dispatch_input(&config, &resolved.action, &resolved.event)?;
    }
    state.inner.push_log(
        &app,
        "SUCCESS",
        "mapping",
        format!(
            "Triggered Role action {}:{} {:?} -> {} Adapter action(s).",
            event.role_id,
            event.logical_control_id,
            event.event_kind,
            actions.len()
        ),
    );
    Ok(actions.len())
}

fn build_role_input_event(request: RoleInputTriggerRequest) -> Result<LogicalInputEvent, String> {
    let role = default_role_definitions()
        .into_iter()
        .find(|role| role.role_id == request.role_id)
        .ok_or_else(|| format!("Unknown Role '{}'.", request.role_id))?;
    let control = role
        .controls
        .into_iter()
        .find(|control| control.logical_control_id == request.logical_control_id)
        .ok_or_else(|| {
            format!(
                "Unknown logical control '{}:{}'.",
                request.role_id, request.logical_control_id
            )
        })?;
    if !control.supported_events.contains(&request.event_kind) {
        return Err(format!(
            "Role action {:?} is not supported by '{}:{}'.",
            request.event_kind, request.role_id, request.logical_control_id
        ));
    }

    let value = match request.event_kind {
        EventKind::ButtonDown => ControlValue::Button { pressed: true },
        EventKind::ButtonUp | EventKind::ButtonPushed => ControlValue::Button { pressed: false },
        EventKind::EncoderDelta => ControlValue::EncoderDelta { steps: 1 },
        EventKind::AbsoluteChanged => ControlValue::Absolute { value: 0 },
        EventKind::ToggleOn => ControlValue::Toggle { state: true },
        EventKind::ToggleOff => ControlValue::Toggle { state: false },
    };
    Ok(LogicalInputEvent {
        device_id: "manager-role-input".to_string(),
        physical_control_id: u16::MAX,
        role_id: request.role_id,
        logical_control_id: request.logical_control_id,
        event_kind: request.event_kind,
        value,
    })
}

#[tauri::command]
fn save_device_endpoints(
    app: AppHandle,
    state: State<'_, AppState>,
    device_endpoints: Vec<DeviceEndpointConfig>,
) -> Result<AppSnapshot, String> {
    let device_endpoints = sanitize_device_endpoints(device_endpoints);
    persist_manager_state(
        &app,
        &PersistedManagerState {
            schema_version: STATE_SCHEMA_VERSION,
            dcsbios_config: state.inner.config.lock().unwrap().clone(),
            device_endpoints: device_endpoints.clone(),
            device_role_assignments: state.inner.device_role_assignments.lock().unwrap().clone(),
            adapter_mappings: state.inner.adapter_mappings.lock().unwrap().clone(),
            adapter_profiles: state.inner.adapter_profiles.lock().unwrap().clone(),
        },
    )?;
    state.inner.stop_endpoint_listeners(&app);
    state.inner.set_device_endpoints(&app, device_endpoints);
    state.inner.restart_endpoint_listeners(&app)?;
    state.inner.push_log(
        &app,
        "INFO",
        "devices",
        "Saved device endpoints configuration.",
    );
    Ok(state.inner.snapshot())
}

#[tauri::command]
fn list_serial_ports() -> Result<Vec<String>, String> {
    let mut ports = serialport::available_ports()
        .map_err(|error| format!("Failed to list serial ports: {error}"))?
        .into_iter()
        .map(|port| port.port_name)
        .collect::<Vec<_>>();
    ports.sort();
    Ok(ports)
}

async fn refresh_devices(
    app: AppHandle,
    runtime: Arc<RuntimeState>,
) -> Result<Vec<ManagedDeviceSummary>, String> {
    runtime.stop_endpoint_listeners(&app);
    let endpoints = runtime.device_endpoints.lock().unwrap().clone();
    let endpoints = sanitize_device_endpoints(endpoints);
    let count_endpoints = endpoints.len();
    let result =
        tauri::async_runtime::spawn_blocking(move || list_devices_for_endpoints(&endpoints))
            .await
            .map_err(|error| format!("Failed to join device scan task: {error}"))?;
    let devices = result?;

    let count_devices = devices.len();
    let devices = runtime.set_devices(&app, devices);
    runtime.restart_endpoint_listeners(&app)?;
    runtime.push_log(
        &app,
        "INFO",
        "devices",
        format!(
            "Refreshed devices from {count_endpoints} configured endpoint(s). {count_devices} device(s) available."
        ),
    );
    Ok(devices)
}

#[tauri::command]
fn save_device_role_assignments(
    app: AppHandle,
    state: State<'_, AppState>,
    device_role_assignments: Vec<DeviceRoleAssignment>,
) -> Result<AppSnapshot, String> {
    let device_role_assignments = sanitize_device_role_assignments(device_role_assignments);
    persist_manager_state(
        &app,
        &PersistedManagerState {
            schema_version: STATE_SCHEMA_VERSION,
            dcsbios_config: state.inner.config.lock().unwrap().clone(),
            device_endpoints: state.inner.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: device_role_assignments.clone(),
            adapter_mappings: state.inner.adapter_mappings.lock().unwrap().clone(),
            adapter_profiles: state.inner.adapter_profiles.lock().unwrap().clone(),
        },
    )?;
    state
        .inner
        .set_device_role_assignments(&app, device_role_assignments);
    state.inner.restart_endpoint_listeners(&app)?;
    state
        .inner
        .push_log(&app, "INFO", "devices", "Saved device role assignments.");
    Ok(state.inner.snapshot())
}

#[tauri::command]
fn save_adapter_mappings(
    app: AppHandle,
    state: State<'_, AppState>,
    adapter_mappings: Vec<AdapterMappingConfig>,
) -> Result<AppSnapshot, String> {
    let adapter_mappings = sanitize_adapter_mappings(adapter_mappings);
    persist_manager_state(
        &app,
        &PersistedManagerState {
            schema_version: STATE_SCHEMA_VERSION,
            dcsbios_config: state.inner.config.lock().unwrap().clone(),
            device_endpoints: state.inner.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: state.inner.device_role_assignments.lock().unwrap().clone(),
            adapter_mappings: adapter_mappings.clone(),
            adapter_profiles: state.inner.adapter_profiles.lock().unwrap().clone(),
        },
    )?;
    state.inner.set_adapter_mappings(&app, adapter_mappings);
    state.inner.restart_endpoint_listeners(&app)?;
    state
        .inner
        .push_log(&app, "INFO", "mapping", "Saved adapter control mappings.");
    Ok(state.inner.snapshot())
}

fn parse_adapter_profile_request(
    request: AdapterProfileImportRequest,
) -> Result<AdapterProfileConfig, String> {
    let adapter_id = request.adapter_id.trim().to_string();
    let profile_id = request.profile_id.trim().to_string();
    let label = request.label.trim().to_string();
    if adapter_id.is_empty() || profile_id.is_empty() || label.is_empty() {
        return Err("Adapter profile requires adapterId, profileId, and label.".to_string());
    }
    if request.source.trim().is_empty() {
        return Err("Adapter profile source is empty.".to_string());
    }

    let mut profile =
        adapter_catalog::parse_external_profile(&profile_id, &label, request.source.trim())?;
    profile.aircraft_names = request
        .aircraft_names
        .into_iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    profile.role_bindings = infer_role_bindings(&adapter_id, &profile);
    if profile.aircraft_names.is_empty() {
        return Err("Adapter profile requires at least one aircraft name.".to_string());
    }
    if profile.role_bindings.is_empty() {
        return Err(
            "Adapter profile does not contain a category supported by the built-in Role bindings."
                .to_string(),
        );
    }

    Ok(AdapterProfileConfig {
        adapter_id,
        profile,
    })
}

#[tauri::command]
fn preview_adapter_profile(request: AdapterProfileImportRequest) -> Result<AdapterProfile, String> {
    parse_adapter_profile_request(request).map(|config| config.profile)
}

#[tauri::command]
fn save_adapter_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    request: AdapterProfileImportRequest,
) -> Result<AppSnapshot, String> {
    let profile = parse_adapter_profile_request(request)?;
    let mut adapter_profiles = state.inner.adapter_profiles.lock().unwrap().clone();
    if let Some(existing) = adapter_profiles.iter_mut().find(|existing| {
        existing.adapter_id == profile.adapter_id
            && existing.profile.profile_id == profile.profile.profile_id
    }) {
        *existing = profile;
    } else {
        adapter_profiles.push(profile);
    }
    let adapter_profiles = sanitize_adapter_profiles(adapter_profiles);
    persist_manager_state(
        &app,
        &PersistedManagerState {
            schema_version: STATE_SCHEMA_VERSION,
            dcsbios_config: state.inner.config.lock().unwrap().clone(),
            device_endpoints: state.inner.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: state.inner.device_role_assignments.lock().unwrap().clone(),
            adapter_mappings: state.inner.adapter_mappings.lock().unwrap().clone(),
            adapter_profiles: adapter_profiles.clone(),
        },
    )?;
    state.inner.set_adapter_profiles(&app, adapter_profiles);
    state
        .inner
        .push_log(&app, "INFO", "adapter", "Saved adapter aircraft profile.");
    Ok(state.inner.snapshot())
}

#[tauri::command]
fn start_learn(
    app: AppHandle,
    state: State<'_, AppState>,
    request: LearnRequest,
) -> Result<AppSnapshot, String> {
    state.inner.start_learn(&app, request)?;
    Ok(state.inner.snapshot())
}

#[tauri::command]
fn cancel_learn(app: AppHandle, state: State<'_, AppState>) -> AppSnapshot {
    state.inner.cancel_learn(&app);
    state.inner.snapshot()
}

#[tauri::command]
async fn list_devices(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ManagedDeviceSummary>, String> {
    refresh_devices(app, state.inner.clone()).await
}

fn sanitize_device_endpoints(
    device_endpoints: Vec<DeviceEndpointConfig>,
) -> Vec<DeviceEndpointConfig> {
    device_endpoints
        .into_iter()
        .map(|mut endpoint| {
            if endpoint.baud_rate == 0 {
                endpoint.baud_rate = DEFAULT_DEVICE_ENDPOINT_BAUD_RATE;
            }
            endpoint
        })
        .collect()
}

trait DeviceEndpointProvider {
    fn supports(&self, endpoint: &DeviceEndpointConfig) -> bool;
    fn list_devices(
        &self,
        endpoint: &DeviceEndpointConfig,
    ) -> Result<Vec<ManagedDeviceSummary>, String>;
}

struct SerialImcpEndpointProvider;

impl DeviceEndpointProvider for SerialImcpEndpointProvider {
    fn supports(&self, endpoint: &DeviceEndpointConfig) -> bool {
        matches!(endpoint.transport, DeviceEndpointTransport::Serial)
    }

    fn list_devices(
        &self,
        endpoint: &DeviceEndpointConfig,
    ) -> Result<Vec<ManagedDeviceSummary>, String> {
        enumerate_serial_endpoint(endpoint)
    }
}

fn list_devices_for_endpoints(
    device_endpoints: &[DeviceEndpointConfig],
) -> Result<Vec<ManagedDeviceSummary>, String> {
    let providers: [&dyn DeviceEndpointProvider; 1] = [&SerialImcpEndpointProvider];
    let mut devices = Vec::new();

    for endpoint in device_endpoints {
        if !endpoint.enabled {
            continue;
        }

        let provider = providers
            .iter()
            .find(|provider| provider.supports(endpoint))
            .ok_or_else(|| format!("No provider found for endpoint '{}'.", endpoint.name))?;

        match provider.list_devices(endpoint) {
            Ok(mut discovered) => devices.append(&mut discovered),
            Err(error) => {
                devices.push(unavailable_device_summary(endpoint, error));
            }
        }
    }

    Ok(devices)
}

fn enumerate_serial_endpoint(
    endpoint: &DeviceEndpointConfig,
) -> Result<Vec<ManagedDeviceSummary>, String> {
    let probe = probe_endpoint_root_device(endpoint)?;
    let root_connection_kind = if probe.root.device_kind == DeviceKind::ImcpHub {
        "hub"
    } else {
        "direct"
    };

    let root_summary = probed_device_to_summary(endpoint, &probe.root, root_connection_kind, None);
    let mut devices = vec![root_summary.clone()];

    let should_enumerate_children = match endpoint.role_hint {
        EndpointRoleHint::DirectDevice => false,
        EndpointRoleHint::ImcpHub => true,
        EndpointRoleHint::Auto => probe.root.device_kind == DeviceKind::ImcpHub,
    };

    if should_enumerate_children {
        let mut probe = probe;
        let children = enumerate_children_via_hub(&mut *probe.port, endpoint, &probe.root)?;
        devices.extend(children.into_iter().map(|child| {
            probed_device_to_summary(
                endpoint,
                &child,
                "hub-child",
                Some((&root_summary.id, root_summary.display_name.as_str())),
            )
        }));
    }

    Ok(devices)
}

fn open_serial_endpoint(
    endpoint: &DeviceEndpointConfig,
) -> serialport::Result<Box<dyn serialport::SerialPort>> {
    serialport::new(&endpoint.address, serial_endpoint_open_baud_rate(endpoint))
        .timeout(IMCP_READ_TIMEOUT)
        .open()
}

fn serial_endpoint_open_baud_rate(endpoint: &DeviceEndpointConfig) -> u32 {
    #[cfg(target_os = "macos")]
    if is_macos_pty_path(&endpoint.address) {
        // serialport uses IOSSIOSPEED for every non-zero baud rate on macOS.
        // That ioctl is unsupported by PTYs and fails with ENOTTY. A zero baud
        // rate is the crate's documented way to leave a PTY's speed unchanged.
        return 0;
    }

    endpoint.baud_rate
}

#[cfg(target_os = "macos")]
fn is_macos_pty_path(path: &str) -> bool {
    path.strip_prefix("/dev/ttys").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

struct EndpointProbe {
    port: Box<dyn serialport::SerialPort>,
    root: ProbedImcpDevice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbedImcpDevice {
    display_name: String,
    firmware_version: String,
    assigned_address: Option<u8>,
    device_kind: DeviceKind,
    protocol_version: u8,
    device_id: String,
    displays: u8,
    controls: u16,
    features: String,
}

fn probe_endpoint_root_device(endpoint: &DeviceEndpointConfig) -> Result<EndpointProbe, String> {
    let mut port = open_serial_endpoint(endpoint)
        .map_err(|error| format!("Failed to open {}: {error}", endpoint.address))?;

    let _ = port.clear(serialport::ClearBuffer::All);
    request_device_hello(&mut *port)?;

    let started_at = Instant::now();
    let mut serial_buffer = [0u8; 64];
    let mut rx_buffer = [0u8; 256];
    let mut frame_buffer = [0u8; 256];
    let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
    let mut assigned_address: Option<u8> = None;

    while started_at.elapsed() < IMCP_ROOT_PROBE_TIMEOUT {
        match port.read(&mut serial_buffer) {
            Ok(bytes_read) if bytes_read > 0 => {
                parser
                    .write_data(&serial_buffer[..bytes_read])
                    .map_err(|error| {
                        format!(
                            "Failed to parse IMCP frame on {}: {error:?}",
                            endpoint.address
                        )
                    })?;

                while let Some(frame) = parser.next_frame() {
                    let frame = match frame {
                        Ok(frame) => frame,
                        Err(_) => continue,
                    };

                    match frame.payload() {
                        FramePayload::Join(id) => {
                            let next_address = assigned_address.unwrap_or(0x02);
                            assigned_address = Some(next_address);
                            write_frame(
                                &mut *port,
                                &Frame::new(
                                    Address::Unicast(0x00),
                                    IMCP_MASTER_ADDRESS,
                                    FramePayload::SetAddress {
                                        address: next_address,
                                        id: *id,
                                    },
                                ),
                            )?;
                        }
                        FramePayload::Set(payload) => {
                            if let Some(probed) = decode_device_hello(
                                payload.as_slice(),
                                assigned_address,
                                frame.from_address(),
                            )? {
                                write_frame(
                                    &mut *port,
                                    &Frame::new(
                                        Address::Unicast(frame.from_address()),
                                        IMCP_MASTER_ADDRESS,
                                        FramePayload::Ack(frame.to_address().as_byte()),
                                    ),
                                )?;
                                return Ok(EndpointProbe { port, root: probed });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => return Err(format!("Failed to read {}: {error}", endpoint.address)),
        }
    }

    Err(format!(
        "No IMCP/HCP device responded on configured endpoint {}.",
        endpoint.address
    ))
}

fn enumerate_children_via_hub(
    port: &mut dyn serialport::SerialPort,
    endpoint: &DeviceEndpointConfig,
    hub: &ProbedImcpDevice,
) -> Result<Vec<ProbedImcpDevice>, String> {
    let request = encode_set_packet(&AppPacketKind::ControlEvent(ControlEvent {
        seq: 0,
        control_id: CONTROL_ID_REQUEST_DEVICE_HELLO,
        event: ControlValue::RequestDeviceHello,
    }))
    .map_err(|error| format!("Failed to encode RequestDeviceHello: {error:?}"))?;

    write_frame(
        port,
        &Frame::new(
            Address::Unicast(
                hub.assigned_address
                    .ok_or_else(|| "Hub IMCP address is missing.".to_string())?,
            ),
            IMCP_MASTER_ADDRESS,
            FramePayload::Set(request),
        ),
    )?;

    let started_at = Instant::now();
    let mut serial_buffer = [0u8; 64];
    let mut rx_buffer = [0u8; 256];
    let mut frame_buffer = [0u8; 256];
    let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
    let mut children = Vec::new();

    while started_at.elapsed() < IMCP_CHILD_ENUMERATION_TIMEOUT {
        match port.read(&mut serial_buffer) {
            Ok(bytes_read) if bytes_read > 0 => {
                parser
                    .write_data(&serial_buffer[..bytes_read])
                    .map_err(|error| {
                        format!(
                            "Failed to parse IMCP frame on {}: {error:?}",
                            endpoint.address
                        )
                    })?;

                while let Some(frame) = parser.next_frame() {
                    let frame = match frame {
                        Ok(frame) => frame,
                        Err(_) => continue,
                    };

                    if let FramePayload::Set(payload) = frame.payload() {
                        if let Some(probed) = decode_device_hello(
                            payload.as_slice(),
                            Some(frame.from_address()),
                            frame.from_address(),
                        )? {
                            write_frame(
                                port,
                                &Frame::new(
                                    Address::Unicast(frame.from_address()),
                                    IMCP_MASTER_ADDRESS,
                                    FramePayload::Ack(frame.to_address().as_byte()),
                                ),
                            )?;

                            if probed.device_id != hub.device_id {
                                children.push(probed);
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => return Err(format!("Failed to enumerate hub children: {error}")),
        }
    }

    Ok(children)
}

fn unavailable_device_summary(
    endpoint: &DeviceEndpointConfig,
    error: String,
) -> ManagedDeviceSummary {
    ManagedDeviceSummary {
        id: format!("endpoint-error:{}", endpoint.id),
        connection_kind: "direct".to_string(),
        gateway_id: None,
        gateway_display_name: None,
        endpoint_id: endpoint.id.clone(),
        endpoint_name: endpoint.name.clone(),
        endpoint_transport: format_endpoint_transport(endpoint.transport).to_string(),
        endpoint_address: endpoint.address.clone(),
        display_name: endpoint.name.clone(),
        firmware_version: None,
        state: "error".to_string(),
        protocol: "imcp+hcp".to_string(),
        assigned_address: None,
        device_kind: None,
        device_kind_id: None,
        protocol_version: None,
        device_id: None,
        displays: None,
        controls: None,
        features: Some(error),
    }
}

fn probed_device_to_summary(
    endpoint: &DeviceEndpointConfig,
    device: &ProbedImcpDevice,
    connection_kind: &str,
    gateway: Option<(&str, &str)>,
) -> ManagedDeviceSummary {
    let stable_id = match connection_kind {
        "hub" => format!("hub:{}:{}", endpoint.id, device.device_id),
        "hub-child" => {
            let gateway_id = gateway.map(|(id, _)| id).unwrap_or("unknown");
            format!("hub-child:{gateway_id}:{}", device.device_id)
        }
        _ => format!("direct:{}:{}", endpoint.id, device.device_id),
    };

    ManagedDeviceSummary {
        id: stable_id,
        connection_kind: connection_kind.to_string(),
        gateway_id: gateway.map(|(id, _)| id.to_string()),
        gateway_display_name: gateway.map(|(_, display_name)| display_name.to_string()),
        endpoint_id: endpoint.id.clone(),
        endpoint_name: endpoint.name.clone(),
        endpoint_transport: format_endpoint_transport(endpoint.transport).to_string(),
        endpoint_address: endpoint.address.clone(),
        display_name: device.display_name.clone(),
        firmware_version: Some(device.firmware_version.clone()),
        state: "connected".to_string(),
        protocol: "imcp+hcp".to_string(),
        assigned_address: device.assigned_address,
        device_kind: Some(format_device_kind(device.device_kind).to_string()),
        device_kind_id: Some(format_device_kind_id(device.device_kind).to_string()),
        protocol_version: Some(device.protocol_version),
        device_id: Some(device.device_id.clone()),
        displays: Some(device.displays),
        controls: Some(device.controls),
        features: Some(device.features.clone()),
    }
}

fn persist_manager_state(
    app: &AppHandle,
    manager_state: &PersistedManagerState,
) -> Result<(), String> {
    let file_path = manager_state_file_path(app)?;
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create config directory: {error}"))?;
    }

    let body = serde_json::to_string_pretty(manager_state)
        .map_err(|error| format!("Failed to serialize manager state: {error}"))?;

    fs::write(file_path, body).map_err(|error| format!("Failed to write manager state: {error}"))
}

fn load_manager_state(app: &AppHandle) -> Result<PersistedManagerState, String> {
    let file_path = manager_state_file_path(app)?;
    match fs::read_to_string(file_path) {
        Ok(contents) => {
            let (persisted, migrated) = normalize_manager_state_json(&contents)?;
            if migrated {
                persist_manager_state(app, &persisted)?;
            }
            Ok(persisted)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PersistedManagerState::default())
        }
        Err(error) => Err(format!("Failed to read manager state: {error}")),
    }
}

fn normalize_manager_state_json(contents: &str) -> Result<(PersistedManagerState, bool), String> {
    let value: serde_json::Value = serde_json::from_str(contents)
        .map_err(|error| format!("Failed to parse manager state: {error}"))?;
    let schema_version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);

    if schema_version == STATE_SCHEMA_VERSION as u64 {
        let persisted: PersistedManagerState = serde_json::from_value(value)
            .map_err(|error| format!("Failed to parse manager state v2: {error}"))?;
        return Ok((normalize_persisted_state(persisted), false));
    }

    let legacy: LegacyPersistedManagerState = serde_json::from_value(value)
        .map_err(|error| format!("Failed to parse legacy manager state: {error}"))?;
    Ok((migrate_legacy_state(legacy), true))
}

fn normalize_persisted_state(mut persisted: PersistedManagerState) -> PersistedManagerState {
    persisted.schema_version = STATE_SCHEMA_VERSION;
    persisted.device_endpoints = sanitize_device_endpoints(persisted.device_endpoints);
    persisted.device_role_assignments =
        sanitize_device_role_assignments(persisted.device_role_assignments);
    persisted.adapter_mappings = sanitize_adapter_mappings(persisted.adapter_mappings);
    persisted.adapter_profiles = sanitize_adapter_profiles(persisted.adapter_profiles);
    persisted
}

fn sanitize_adapter_profiles(
    adapter_profiles: Vec<AdapterProfileConfig>,
) -> Vec<AdapterProfileConfig> {
    let mut sanitized = Vec::new();
    for mut config in adapter_profiles {
        config.adapter_id = config.adapter_id.trim().to_string();
        config.profile.profile_id = config.profile.profile_id.trim().to_string();
        config.profile.label = config.profile.label.trim().to_string();
        config.profile.aircraft_names = config
            .profile
            .aircraft_names
            .into_iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        config.profile.role_bindings = config
            .profile
            .role_bindings
            .into_iter()
            .map(|mut binding| {
                binding.role_id = binding.role_id.trim().to_string();
                binding.category = binding.category.trim().to_string();
                binding
            })
            .filter(|binding| !binding.role_id.is_empty() && !binding.category.is_empty())
            .collect();
        if config.adapter_id.is_empty()
            || config.profile.profile_id.is_empty()
            || config.profile.label.is_empty()
            || config.profile.aircraft_names.is_empty()
        {
            continue;
        }
        if let Some(existing) = sanitized
            .iter_mut()
            .find(|existing: &&mut AdapterProfileConfig| {
                existing.adapter_id == config.adapter_id
                    && existing.profile.profile_id == config.profile.profile_id
            })
        {
            *existing = config;
        } else {
            sanitized.push(config);
        }
    }
    sanitized
}

fn migrate_legacy_state(legacy: LegacyPersistedManagerState) -> PersistedManagerState {
    let mut assignments = legacy
        .device_role_assignments
        .into_iter()
        .filter_map(|assignment| {
            let device_id = assignment.device_id.trim().to_string();
            let role_id = normalize_legacy_role_id(&assignment.role);
            if device_id.is_empty() || role_id.is_empty() {
                return None;
            }

            Some(DeviceRoleAssignment {
                device_id,
                role_id,
                bindings: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    let mut dcsbios_mappings = Vec::new();

    for role_config in legacy.role_mappings {
        let role_id = normalize_legacy_role_id(&role_config.role);
        if role_id.is_empty() {
            continue;
        }

        for mapping in role_config.mappings {
            let Some(event_kind) = parse_legacy_event_kind(&mapping.input_event) else {
                continue;
            };
            let logical_control_id = format!("button-{}", mapping.control_id);
            let identifier = mapping.action.identifier.trim().to_string();
            let argument = mapping.action.argument.trim().to_string();
            if identifier.is_empty() || argument.is_empty() {
                continue;
            }

            for assignment in assignments
                .iter_mut()
                .filter(|assignment| assignment.role_id == role_id)
            {
                assignment.bindings.push(PhysicalToLogicalBinding {
                    physical_control_id: mapping.control_id,
                    logical_control_id: logical_control_id.clone(),
                });
            }

            let mut parameters = HashMap::new();
            parameters.insert("identifier".to_string(), identifier);
            parameters.insert("argument".to_string(), argument);
            dcsbios_mappings.push(AdapterControlMapping {
                role_id: role_id.clone(),
                logical_control_id,
                event_kind,
                action: AdapterActionConfig {
                    action_id: "control-command".to_string(),
                    parameters,
                },
            });
        }
    }

    normalize_persisted_state(PersistedManagerState {
        schema_version: STATE_SCHEMA_VERSION,
        dcsbios_config: DcsBiosConnectionConfig::default(),
        device_endpoints: legacy.device_endpoints,
        device_role_assignments: assignments,
        adapter_mappings: if dcsbios_mappings.is_empty() {
            Vec::new()
        } else {
            vec![AdapterMappingConfig {
                adapter_id: "dcs-bios".to_string(),
                profile_id: "default".to_string(),
                aircraft_name: None,
                profile_label: None,
                output_mappings: Vec::new(),
                mappings: dcsbios_mappings,
            }]
        },
        adapter_profiles: Vec::new(),
    })
}

fn normalize_legacy_role_id(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "leftddi" | "left-d-d-i" => "left-ddi".to_string(),
        "rightddi" | "right-d-d-i" => "right-ddi".to_string(),
        _ => normalized,
    }
}

fn apply_learn_binding(
    mut assignments: Vec<DeviceRoleAssignment>,
    request: &LearnRequest,
    device_id: &str,
    physical_control_id: u16,
) -> Vec<DeviceRoleAssignment> {
    if request.mode == LearnMode::Replace {
        for assignment in assignments
            .iter_mut()
            .filter(|assignment| assignment.role_id == request.role_id)
        {
            assignment
                .bindings
                .retain(|binding| binding.logical_control_id != request.logical_control_id);
        }
    }

    let assignment = if let Some(assignment) = assignments.iter_mut().find(|assignment| {
        assignment.device_id == device_id && assignment.role_id == request.role_id
    }) {
        assignment
    } else {
        assignments.push(DeviceRoleAssignment {
            device_id: device_id.to_string(),
            role_id: request.role_id.clone(),
            bindings: Vec::new(),
        });
        let Some(assignment) = assignments.last_mut() else {
            return sanitize_device_role_assignments(assignments);
        };
        assignment
    };

    if !assignment.bindings.iter().any(|binding| {
        binding.physical_control_id == physical_control_id
            && binding.logical_control_id == request.logical_control_id
    }) {
        assignment.bindings.push(PhysicalToLogicalBinding {
            physical_control_id,
            logical_control_id: request.logical_control_id.clone(),
        });
    }

    sanitize_device_role_assignments(assignments)
}

fn parse_legacy_event_kind(value: &str) -> Option<EventKind> {
    match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "button-down" => Some(EventKind::ButtonDown),
        "button-up" => Some(EventKind::ButtonUp),
        "button-pushed" => Some(EventKind::ButtonPushed),
        "encoder-delta" => Some(EventKind::EncoderDelta),
        "absolute-changed" => Some(EventKind::AbsoluteChanged),
        "toggle-on" => Some(EventKind::ToggleOn),
        "toggle-off" => Some(EventKind::ToggleOff),
        _ => None,
    }
}

fn manager_state_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Failed to resolve app config directory: {error}"))?;
    dir.push(SETTINGS_FILE_NAME);
    Ok(dir)
}

fn write_frame(port: &mut dyn serialport::SerialPort, frame: &Frame) -> Result<(), String> {
    let mut encoded = [0u8; MAX_ENCODED_FRAME_SIZE];
    let encoded_len = frame
        .encode(&mut encoded)
        .map_err(|error| format!("Failed to encode IMCP frame: {error:?}"))?;
    port.write_all(&encoded[..encoded_len])
        .map_err(|error| format!("Failed to write IMCP frame: {error}"))?;
    port.flush()
        .map_err(|error| format!("Failed to flush IMCP frame: {error}"))?;
    Ok(())
}

fn device_hello_request_frame() -> Result<Frame, String> {
    let request = encode_set_packet(&AppPacketKind::ControlEvent(ControlEvent {
        seq: 0,
        control_id: CONTROL_ID_REQUEST_DEVICE_HELLO,
        event: ControlValue::RequestDeviceHello,
    }))
    .map_err(|error| format!("Failed to encode RequestDeviceHello: {error:?}"))?;

    Ok(Frame::new(
        Address::Broadcast,
        IMCP_MASTER_ADDRESS,
        FramePayload::Set(request),
    ))
}

fn request_device_hello(port: &mut dyn serialport::SerialPort) -> Result<(), String> {
    let frame = device_hello_request_frame()?;
    write_frame(port, &frame)
}

fn decode_device_hello(
    payload: &[u8],
    assigned_address: Option<u8>,
    source_address: u8,
) -> Result<Option<ProbedImcpDevice>, String> {
    let kind = match decode_set_packet(payload) {
        Ok(kind) => kind,
        Err(_) => return Ok(None),
    };

    let AppPacketKind::DeviceHello(hello) = kind else {
        return Ok(None);
    };

    let assigned_address = assigned_address
        .or_else(|| {
            (0x02..=0xFE)
                .contains(&source_address)
                .then_some(source_address)
        })
        .ok_or_else(|| "Received DeviceHello before IMCP address assignment.".to_string())?;

    Ok(Some(ProbedImcpDevice {
        display_name: format_device_kind(hello.device_kind).to_string(),
        firmware_version: format!(
            "{}.{}.{}",
            hello.firmware_version.major,
            hello.firmware_version.minor,
            hello.firmware_version.patch
        ),
        assigned_address: Some(assigned_address),
        device_kind: hello.device_kind,
        protocol_version: hello.protocol_version,
        device_id: format!("{:016X}", hello.device_id),
        displays: hello.capabilities.displays,
        controls: hello.capabilities.controls,
        features: format_capability_flags(hello.capabilities.features),
    }))
}

fn format_device_kind(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::UpperPanelDdi => "Upper Panel DDI",
        DeviceKind::ButtonPanel => "Button Panel",
        DeviceKind::ImcpHub => "IMCP Hub",
        DeviceKind::Unknown(_) => "Unknown Device",
    }
}

fn format_device_kind_id(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::UpperPanelDdi => "upper-panel-ddi",
        DeviceKind::ButtonPanel => "button-panel",
        DeviceKind::ImcpHub => "imcp-hub",
        DeviceKind::Unknown(_) => "unknown",
    }
}

fn format_endpoint_transport(transport: DeviceEndpointTransport) -> &'static str {
    match transport {
        DeviceEndpointTransport::Serial => "serial",
    }
}

fn format_capability_flags(features: u32) -> String {
    if features == 0 {
        return "none".to_string();
    }

    let mut flags = Vec::new();
    if features & (1 << 0) != 0 {
        flags.push("control-events");
    }

    if flags.is_empty() {
        format!("0x{features:08X}")
    } else {
        flags.join(", ")
    }
}

fn bind_export_socket(config: &DcsBiosConnectionConfig) -> Result<UdpSocket, String> {
    let bind_addr = SocketAddr::from(([0, 0, 0, 0], config.export_port));
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|error| format!("Failed to create UDP socket: {error}"))?;
    socket
        .set_reuse_address(true)
        .map_err(|error| format!("Failed to set SO_REUSEADDR: {error}"))?;
    socket
        .bind(&bind_addr.into())
        .map_err(|error| format!("Failed to bind UDP socket on {}: {error}", bind_addr))?;

    if let Ok(multicast_addr) = config.export_host.parse::<Ipv4Addr>() {
        if multicast_addr.is_multicast() {
            socket
                .join_multicast_v4(&multicast_addr, &Ipv4Addr::UNSPECIFIED)
                .map_err(|error| {
                    format!(
                        "Failed to join multicast group {}:{}: {error}",
                        config.export_host, config.export_port
                    )
                })?;
        }
    }

    let udp = UdpSocket::from(socket);
    udp.set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|error| format!("Failed to set read timeout: {error}"))?;
    Ok(udp)
}

fn send_command_to_dcsbios(config: &DcsBiosConnectionConfig, payload: &str) -> Result<(), String> {
    let target = format!("{}:{}", config.command_host, config.command_port);
    match config.command_transport {
        CommandTransport::Udp => {
            let socket = UdpSocket::bind("0.0.0.0:0")
                .map_err(|error| format!("UDP bind failed: {error}"))?;
            socket
                .send_to(payload.as_bytes(), &target)
                .map_err(|error| format!("UDP send failed: {error}"))?;
            Ok(())
        }
        CommandTransport::Tcp => {
            let mut stream = TcpStream::connect(&target)
                .map_err(|error| format!("TCP connect failed: {error}"))?;
            stream
                .write_all(payload.as_bytes())
                .map_err(|error| format!("TCP send failed: {error}"))?;
            stream
                .flush()
                .map_err(|error| format!("TCP flush failed: {error}"))?;
            Ok(())
        }
    }
}

fn encode_import_command(identifier: &str, argument: &str) -> Result<String, String> {
    ImportCommand::new(identifier.trim(), argument.trim())
        .map(|command| command.encode())
        .map_err(|error| format!("Invalid DCS-BIOS command: {error:?}"))
}

fn build_profile_adapter_mapping(
    adapter_id: &str,
    profile: &AdapterProfile,
) -> AdapterMappingConfig {
    let mut mappings = Vec::new();
    for binding in &profile.role_bindings {
        for (index, control) in profile
            .controls
            .iter()
            .filter(|control| control.category == binding.category)
            .enumerate()
        {
            let momentary_input = control.inputs.iter().find(|input| {
                input.interface == "set_state"
                    && input.max_value == Some(1)
                    && input
                        .argument_options
                        .iter()
                        .any(|option| option.value == "0")
                    && input
                        .argument_options
                        .iter()
                        .any(|option| option.value == "1")
            });
            let input = momentary_input.or_else(|| {
                control
                    .inputs
                    .iter()
                    .find(|input| input.interface == "action" && !input.argument_options.is_empty())
                    .or_else(|| {
                        control
                            .inputs
                            .iter()
                            .find(|input| !input.argument_options.is_empty())
                    })
            });
            let Some(input) = input else {
                continue;
            };
            let logical_control_id = format!("button-{index}");
            let event_arguments: Vec<(EventKind, &str, String)> = if momentary_input.is_some() {
                vec![(EventKind::ButtonPushed, "control-pulse", "1".to_string())]
            } else if let Some(argument) = input.argument_options.first() {
                vec![(
                    EventKind::ButtonPushed,
                    "control-command",
                    argument.value.clone(),
                )]
            } else {
                continue;
            };

            for (event_kind, action_id, argument) in event_arguments {
                let mut parameters = HashMap::new();
                parameters.insert("identifier".to_string(), control.control_id.clone());
                parameters.insert("argument".to_string(), argument);
                parameters.insert("argumentMode".to_string(), "fixed".to_string());
                parameters.insert("referenceAdapter".to_string(), adapter_id.to_string());
                parameters.insert("referenceProfile".to_string(), profile.profile_id.clone());
                parameters.insert("referenceCategory".to_string(), control.category.clone());
                parameters.insert("referenceControl".to_string(), control.control_id.clone());
                parameters.insert("referenceInput".to_string(), input.input_id.clone());
                parameters.insert("referenceInterface".to_string(), input.interface.clone());
                if let Some(max_value) = input.max_value {
                    parameters.insert("maxValue".to_string(), max_value.to_string());
                }
                if let Some(suggested_step) = input.suggested_step {
                    parameters.insert("suggestedStep".to_string(), suggested_step.to_string());
                }
                if action_id == "control-pulse" {
                    parameters.insert("releaseArgument".to_string(), "0".to_string());
                }

                mappings.push(AdapterControlMapping {
                    role_id: binding.role_id.clone(),
                    logical_control_id: logical_control_id.clone(),
                    event_kind,
                    action: AdapterActionConfig {
                        action_id: action_id.to_string(),
                        parameters,
                    },
                });
            }
        }
    }

    AdapterMappingConfig {
        adapter_id: adapter_id.to_string(),
        profile_id: profile.profile_id.clone(),
        aircraft_name: profile.aircraft_names.first().cloned(),
        profile_label: Some(profile.label.clone()),
        mappings,
        output_mappings: Vec::new(),
    }
}

fn resolve_dcsbios_argument(
    action: &AdapterActionConfig,
    event: &LogicalInputEvent,
) -> Result<String, String> {
    let argument = action
        .parameters
        .get("argument")
        .ok_or_else(|| "DCS-BIOS action is missing argument parameter.".to_string())?;
    let argument_mode = action
        .parameters
        .get("argumentMode")
        .map(String::as_str)
        .unwrap_or("fixed");

    if argument_mode != "event-value" && argument != "$event-value" {
        return Ok(argument.clone());
    }

    let max_value = action
        .parameters
        .get("maxValue")
        .and_then(|value| value.parse::<i32>().ok());
    let suggested_step = action
        .parameters
        .get("suggestedStep")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(3200)
        .abs();

    match (&event.value, event.event_kind) {
        (ControlValue::Absolute { value }, EventKind::AbsoluteChanged) => {
            let value = i32::from(*value).max(0);
            Ok(max_value
                .map(|max_value| value.min(max_value))
                .unwrap_or(value)
                .to_string())
        }
        (ControlValue::EncoderDelta { steps }, EventKind::EncoderDelta) => {
            let value = i32::from(*steps) * suggested_step;
            if value == 0 {
                return Err("DCS-BIOS variable-step input received a zero delta.".to_string());
            }
            Ok(format!("{value:+}"))
        }
        (ControlValue::Toggle { state }, EventKind::ToggleOn | EventKind::ToggleOff) => {
            Ok(if *state { "1" } else { "0" }.to_string())
        }
        (ControlValue::Button { pressed }, EventKind::ButtonDown | EventKind::ButtonUp) => {
            Ok(if *pressed { "1" } else { "0" }.to_string())
        }
        _ => Err(format!(
            "DCS-BIOS event-value input is incompatible with {:?}.",
            event.event_kind
        )),
    }
}

fn apply_dcsbios_memory_updates(
    memory_map: Arc<Mutex<VecMemoryMap>>,
    updates: &[DcsBiosMemoryUpdate],
) -> Result<(), String> {
    let mut memory_map = memory_map
        .lock()
        .map_err(|_| "DCS-BIOS memory map lock is poisoned.".to_string())?;
    for update in updates {
        memory_map
            .write(update.address, &update.data)
            .map_err(|error| format!("Failed to update DCS-BIOS memory map: {error:?}"))?;
    }
    Ok(())
}

fn parse_u16_value(value: &str) -> Option<u16> {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u16>().ok()
    }
}

fn resolve_dcsbios_output_range(
    catalog: &AdapterCatalog,
    config: &AdapterMappingConfig,
    mapping: &AdapterOutputMapping,
) -> Option<(u16, Option<usize>)> {
    if mapping.source_id == "control-output" {
        let profile_id = mapping
            .parameters
            .get("referenceProfile")
            .or_else(|| mapping.parameters.get("referenceModule"));
        let identifier = mapping
            .parameters
            .get("referenceControl")
            .or_else(|| mapping.parameters.get("identifier"));
        let output_id = mapping.parameters.get("referenceOutput");
        return profile_id.zip(identifier).zip(output_id).and_then(
            |((profile_id, identifier), output_id)| {
                catalog
                    .find_output(&config.adapter_id, profile_id, identifier, output_id)
                    .map(|output| (output.address, output.length.map(usize::from)))
            },
        );
    }

    mapping
        .parameters
        .get("address")
        .and_then(|value| parse_u16_value(value))
        .map(|address| {
            (
                address,
                mapping
                    .parameters
                    .get("length")
                    .and_then(|value| value.parse::<usize>().ok()),
            )
        })
}

fn request_child_device_hello(
    port: &mut dyn serialport::SerialPort,
    hub_address: u8,
) -> Result<(), String> {
    let request = encode_set_packet(&AppPacketKind::ControlEvent(ControlEvent {
        seq: 0,
        control_id: CONTROL_ID_REQUEST_DEVICE_HELLO,
        event: ControlValue::RequestDeviceHello,
    }))
    .map_err(|error| format!("Failed to encode RequestDeviceHello: {error:?}"))?;

    write_frame(
        port,
        &Frame::new(
            Address::Unicast(hub_address),
            IMCP_MASTER_ADDRESS,
            FramePayload::Set(request),
        ),
    )
}

struct ControlEventContext<'a> {
    state: &'a RuntimeState,
    app: &'a AppHandle,
    config: &'a DcsBiosConnectionConfig,
    device_role_assignments: &'a [DeviceRoleAssignment],
    adapter_mappings: &'a [AdapterMappingConfig],
    adapter_registry: &'a AdapterRegistry,
}

fn process_control_event(
    context: &ControlEventContext<'_>,
    pressed_buttons: &mut HashSet<(String, u16)>,
    physical_event: &PhysicalControlEvent,
) {
    if physical_event.control_event.control_id >= physical_event.control_count {
        context.state.push_log(
            context.app,
            "WARN",
            "devices",
            format!(
                "Ignoring control {} from device {} because the device advertises only {} controls.",
                physical_event.control_event.control_id,
                physical_event.device_id,
                physical_event.control_count
            ),
        );
        return;
    }

    let was_pressed = match &physical_event.control_event.event {
        ControlValue::Button { pressed: true } => {
            let key = (
                physical_event.device_id.clone(),
                physical_event.control_event.control_id,
            );
            let was_pressed = pressed_buttons.contains(&key);
            pressed_buttons.insert(key);
            was_pressed
        }
        ControlValue::Button { pressed: false } => pressed_buttons.remove(&(
            physical_event.device_id.clone(),
            physical_event.control_event.control_id,
        )),
        _ => false,
    };

    let event_kinds = control_event_kinds(&physical_event.control_event.event, was_pressed);
    match context
        .state
        .capture_learn(context.app, physical_event, &event_kinds)
    {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            context.state.push_log(
                context.app,
                "ERROR",
                "mapping",
                format!("Failed to persist learned physical control: {error}"),
            );
            return;
        }
    }

    let mut matched_logical_event = false;
    for input_event in event_kinds {
        let logical_events = resolve_logical_input_events(
            context.device_role_assignments,
            &physical_event.device_id,
            physical_event.control_event.control_id,
            input_event,
            &physical_event.control_event.event,
        );

        for logical_event in logical_events {
            matched_logical_event = true;
            for action in resolve_adapter_actions(context.adapter_mappings, &logical_event) {
                dispatch_adapter_action(context, &action);
            }
        }
    }

    if !matched_logical_event {
        context.state.push_log(
            context.app,
            "DEBUG",
            "mapping",
            format!(
                "No logical binding for device={} control={} endpoint={} address={}",
                physical_event.device_id,
                physical_event.control_event.control_id,
                physical_event.endpoint_id,
                physical_event.source_address
            ),
        );
    }
}

fn dispatch_adapter_action(context: &ControlEventContext<'_>, resolved: &ResolvedAdapterAction) {
    let dispatch_seq = context.state.dispatch_seq.fetch_add(1, Ordering::Relaxed) + 1;
    match context
        .adapter_registry
        .get(&resolved.adapter_id)
        .ok_or_else(|| format!("Adapter '{}' is not registered.", resolved.adapter_id))
        .and_then(|adapter| {
            adapter.dispatch_input(context.config, &resolved.action, &resolved.event)
        }) {
        Ok(()) => context.state.push_log(
            context.app,
            "SUCCESS",
            "mapping",
            format!(
                "dispatchSeq={dispatch_seq} mapped {}:{} {:?} -> {}:{}.",
                resolved.event.role_id,
                resolved.event.logical_control_id,
                resolved.event.event_kind,
                resolved.adapter_id,
                resolved.profile_id
            ),
        ),
        Err(error) => context.state.push_log(
            context.app,
            "ERROR",
            "mapping",
            format!(
                "dispatchSeq={dispatch_seq} failed adapter dispatch for {}:{}: {error}.",
                resolved.event.role_id, resolved.event.logical_control_id
            ),
        ),
    }
}

fn run_dispatch_worker(
    state: Arc<RuntimeState>,
    app: AppHandle,
    receiver: Receiver<PhysicalControlEvent>,
    stop: Arc<AtomicBool>,
) {
    let registry = AdapterRegistry::new();
    let mut pressed_buttons: HashSet<(String, u16)> = HashSet::new();

    state.push_log(
        &app,
        "INFO",
        "mapping",
        "Started common input dispatch worker.",
    );

    while !stop.load(Ordering::Relaxed) {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(physical_event) => {
                let config = state.config.lock().unwrap().clone();
                let assignments = state.device_role_assignments.lock().unwrap().clone();
                let adapter_mappings = state.active_adapter_mappings();
                let context = ControlEventContext {
                    state: state.as_ref(),
                    app: &app,
                    config: &config,
                    device_role_assignments: &assignments,
                    adapter_mappings: &adapter_mappings,
                    adapter_registry: &registry,
                };
                process_control_event(&context, &mut pressed_buttons, &physical_event);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => state.expire_learn(&app),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    state.push_log(
        &app,
        "INFO",
        "mapping",
        "Stopped common input dispatch worker.",
    );
}

fn drain_display_commands(
    port: &mut dyn serialport::SerialPort,
    receiver: &Receiver<DisplayCommand>,
    known_devices: &HashMap<u8, KnownRuntimeDevice>,
    state: &RuntimeState,
    app: &AppHandle,
) -> Result<(), String> {
    loop {
        let command = match receiver.try_recv() {
            Ok(command) => command,
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("Display dispatch queue disconnected.".to_string())
            }
        };
        let Some((&address, _)) = known_devices
            .iter()
            .find(|(_, device)| device.device_id == command.device_id)
        else {
            state.push_log(
                app,
                "DEBUG",
                "display",
                format!(
                    "Skipping display update for unknown device {}.",
                    command.device_id
                ),
            );
            continue;
        };
        let payload = encode_data_packet(&command.data)
            .map_err(|error| format!("Failed to encode HCP DisplayData: {error:?}"))?;
        write_frame(
            port,
            &Frame::new(
                Address::Unicast(address),
                IMCP_MASTER_ADDRESS,
                FramePayload::Data(payload),
            ),
        )?;
    }
}

fn run_endpoint_listener(
    state: Arc<RuntimeState>,
    app: AppHandle,
    endpoint: DeviceEndpointConfig,
    channels: EndpointListenerChannels,
    mut known_devices: HashMap<u8, KnownRuntimeDevice>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut failure_logged = false;
    while !stop.load(Ordering::Relaxed) {
        match run_endpoint_listener_session(
            &state,
            &app,
            &endpoint,
            &channels,
            &mut known_devices,
            &stop,
        ) {
            Ok(()) => return Ok(()),
            Err(error) => {
                if !failure_logged {
                    state.push_log(
                        &app,
                        "WARN",
                        "devices",
                        format!(
                            "Endpoint {} is unavailable: {error}. Retrying automatically.",
                            endpoint.address
                        ),
                    );
                    failure_logged = true;
                }
                wait_for_endpoint_reconnect(&stop);
            }
        }
    }

    Ok(())
}

fn wait_for_endpoint_reconnect(stop: &AtomicBool) {
    let started_at = Instant::now();
    while started_at.elapsed() < IMCP_ENDPOINT_RECONNECT_DELAY {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_endpoint_listener_session(
    state: &RuntimeState,
    app: &AppHandle,
    endpoint: &DeviceEndpointConfig,
    channels: &EndpointListenerChannels,
    known_devices: &mut HashMap<u8, KnownRuntimeDevice>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut port = open_serial_endpoint(endpoint).map_err(|error| {
        format!(
            "Failed to open endpoint listener {}: {error}",
            endpoint.address
        )
    })?;
    let _ = port.clear(serialport::ClearBuffer::All);
    request_device_hello(&mut *port)?;

    let mut serial_buffer = [0u8; 64];
    let mut rx_buffer = [0u8; 256];
    let mut frame_buffer = [0u8; 256];
    let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
    let mut address_allocator = RuntimeAddressAllocator::from_known_devices(known_devices);
    let mut observed_hub_addresses: HashMap<String, u8> = HashMap::new();
    let mut pending_hub_discoveries: HashMap<u8, PendingHubDiscovery> = HashMap::new();
    let mut queued_hub_discoveries: VecDeque<QueuedHubDiscovery> = VecDeque::new();
    let mut hub_discovery_quiet_until = None;
    let mut known_child_gateways: HashMap<String, (String, String)> = HashMap::new();

    state.push_log(
        app,
        "INFO",
        "devices",
        format!("Listening for HCP events on {}.", endpoint.address),
    );

    while !stop.load(Ordering::Relaxed) {
        drain_display_commands(
            &mut *port,
            &channels.display_receiver,
            known_devices,
            state,
            app,
        )?;

        match port.read(&mut serial_buffer) {
            Ok(bytes_read) if bytes_read > 0 => {
                parser
                    .write_data(&serial_buffer[..bytes_read])
                    .map_err(|error| {
                        format!(
                            "Failed to parse IMCP frame on {}: {error:?}",
                            endpoint.address
                        )
                    })?;

                while let Some(frame) = parser.next_frame() {
                    let frame = match frame {
                        Ok(frame) => frame,
                        Err(_) => continue,
                    };

                    let frame_received_at = Instant::now();
                    if expire_pending_hub_discoveries(
                        &mut pending_hub_discoveries,
                        frame_received_at,
                    ) {
                        hub_discovery_quiet_until =
                            Some(frame_received_at + IMCP_CHILD_ENUMERATION_TIMEOUT);
                    }
                    if is_hub_discovery_traffic(frame.payload()) {
                        defer_hub_discovery_during_quiet_period(
                            &pending_hub_discoveries,
                            &mut hub_discovery_quiet_until,
                            frame_received_at,
                        );
                    }

                    match frame.payload() {
                        FramePayload::Join(join_id) => {
                            if frame.from_address() != 0x00 {
                                continue;
                            }
                            let address = address_allocator.allocate_for_join(*join_id)?;
                            write_frame(
                                &mut *port,
                                &Frame::new(
                                    Address::Unicast(0x00),
                                    IMCP_MASTER_ADDRESS,
                                    FramePayload::SetAddress {
                                        address,
                                        id: *join_id,
                                    },
                                ),
                            )?;
                            record_pending_child_join(
                                &mut pending_hub_discoveries,
                                frame.from_address(),
                                *join_id,
                                address,
                                frame_received_at,
                            );
                        }
                        FramePayload::Set(payload) => {
                            if let Some(probed) = decode_device_hello(
                                payload.as_slice(),
                                Some(frame.from_address()),
                                frame.from_address(),
                            )? {
                                write_frame(
                                    &mut *port,
                                    &Frame::new(
                                        Address::Unicast(frame.from_address()),
                                        IMCP_MASTER_ADDRESS,
                                        FramePayload::Ack(frame.to_address().as_byte()),
                                    ),
                                )?;

                                let source_address = frame.from_address();
                                let pending_gateway = if probed.device_kind == DeviceKind::ImcpHub {
                                    None
                                } else {
                                    gateway_for_device_hello(
                                        &pending_hub_discoveries,
                                        &known_child_gateways,
                                        source_address,
                                        &probed.device_id,
                                        frame_received_at,
                                    )
                                };
                                if let Some(gateway) = pending_gateway.as_ref() {
                                    known_child_gateways
                                        .insert(probed.device_id.clone(), gateway.clone());
                                }
                                let connection_kind = if probed.device_kind == DeviceKind::ImcpHub {
                                    "hub"
                                } else if pending_gateway.is_some() {
                                    "hub-child"
                                } else {
                                    "direct"
                                };
                                let summary = probed_device_to_summary(
                                    endpoint,
                                    &probed,
                                    connection_kind,
                                    pending_gateway.as_ref().map(|(id, display_name)| {
                                        (id.as_str(), display_name.as_str())
                                    }),
                                );
                                state.upsert_device_summary(app, summary.clone());
                                reconcile_known_runtime_device(
                                    known_devices,
                                    &mut address_allocator,
                                    source_address,
                                    &probed,
                                );
                                state.register_display_sender(
                                    probed.device_id.clone(),
                                    channels.display_sender.clone(),
                                );

                                if probed.device_kind == DeviceKind::ImcpHub
                                    && hub_requires_child_discovery(
                                        &mut observed_hub_addresses,
                                        &probed.device_id,
                                        source_address,
                                    )
                                {
                                    queued_hub_discoveries.push_back(QueuedHubDiscovery::new(
                                        source_address,
                                        summary.id,
                                        summary.display_name,
                                    ));
                                    advance_hub_discovery(
                                        &mut *port,
                                        &mut pending_hub_discoveries,
                                        &mut queued_hub_discoveries,
                                        &mut hub_discovery_quiet_until,
                                        Instant::now(),
                                    )?;
                                }

                                continue;
                            }

                            let kind = match decode_set_packet(payload.as_slice()) {
                                Ok(kind) => kind,
                                Err(_) => continue,
                            };

                            if let AppPacketKind::ControlEvent(control_event) = kind {
                                write_frame(
                                    &mut *port,
                                    &Frame::new(
                                        Address::Unicast(frame.from_address()),
                                        IMCP_MASTER_ADDRESS,
                                        FramePayload::Ack(frame.to_address().as_byte()),
                                    ),
                                )?;
                                let source_address = frame.from_address();
                                let Some(device) = known_devices.get(&source_address) else {
                                    state.push_log(
                                        app,
                                        "WARN",
                                        "devices",
                                        format!(
                                            "Ignoring control event from unknown IMCP address {source_address}."
                                        ),
                                    );
                                    continue;
                                };
                                let physical_event = PhysicalControlEvent {
                                    endpoint_id: endpoint.id.clone(),
                                    source_address,
                                    device_id: device.device_id.clone(),
                                    control_count: device.control_count,
                                    control_event,
                                };
                                match channels.dispatch_sender.try_send(physical_event) {
                                    Ok(()) => {}
                                    Err(TrySendError::Full(_)) => state.push_log(
                                        app,
                                        "WARN",
                                        "mapping",
                                        format!(
                                            "Input dispatch queue is full; dropping control event from {}.",
                                            device.device_id
                                        ),
                                    ),
                                    Err(TrySendError::Disconnected(_)) => {
                                        return Err("Input dispatch worker disconnected.".to_string())
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                advance_hub_discovery(
                    &mut *port,
                    &mut pending_hub_discoveries,
                    &mut queued_hub_discoveries,
                    &mut hub_discovery_quiet_until,
                    Instant::now(),
                )?;
            }
            Err(error) => {
                return Err(format!(
                    "Failed to read endpoint {} for control events: {error}",
                    endpoint.address
                ));
            }
        }
    }

    Ok(())
}

fn normalize_command_request(request: DcsBiosCommandRequest) -> Result<String, String> {
    let raw = request.raw_command.unwrap_or_default().trim().to_string();
    if !raw.is_empty() {
        return Ok(format!("{}\n", raw.trim_end_matches('\n')));
    }

    let control_id = request.control_id.unwrap_or_default().trim().to_string();
    let argument = request.argument.unwrap_or_default().trim().to_string();

    if control_id.is_empty() || argument.is_empty() {
        return Err("Provide either rawCommand or both controlId and argument.".to_string());
    }

    encode_import_command(&control_id, &argument)
}

fn build_diagnostics(config: &DcsBiosConnectionConfig, status: &DcsBiosStatus) -> Vec<String> {
    match status.connection_state.as_str() {
        "stopped" => vec!["DCS-BIOS listener is stopped.".to_string()],
        "connecting" => vec![format!(
            "Binding local UDP listener for {}:{}.",
            config.export_host, config.export_port
        )],
        "listening" => vec![
            format!(
                "Listening on UDP {}:{} but no fresh export packets are arriving.",
                config.export_host, config.export_port
            ),
            "Check Export.lua / DCS-BIOS installation, firewall rules, and ProtocolIO.lua export target."
                .to_string(),
        ],
        "receiving" => {
            let mut diagnostics = vec![format!(
                "Receiving export packets on {}:{}.",
                config.export_host, config.export_port
            )];
            diagnostics.push(format!(
                "Commands will be sent to {}:{} via {:?}.",
                config.command_host, config.command_port, config.command_transport
            ));
            diagnostics
        }
        "error" => vec![
            status
                .error
                .clone()
                .unwrap_or_else(|| "Unknown DCS-BIOS error.".to_string()),
            "If DCS runs on another PC, confirm multicast routing or switch DCS-BIOS to a reachable unicast address."
                .to_string(),
        ],
        _ => Vec::new(),
    }
}

fn extract_ascii_preview(buf: &[u8]) -> Option<String> {
    let mut current = String::new();
    let mut segments = Vec::new();

    for byte in buf {
        let ch = *byte as char;
        if ch.is_ascii_graphic() || ch == ' ' || ch == '_' || ch == '-' {
            current.push(ch);
        } else if current.len() >= 4 {
            segments.push(current.clone());
            current.clear();
        } else {
            current.clear();
        }
    }

    if current.len() >= 4 {
        segments.push(current);
    }

    segments.into_iter().max_by_key(|segment| segment.len())
}

fn update_overlaps_aircraft_name(update: &DcsBiosMemoryUpdate) -> bool {
    let update_start = usize::from(update.address);
    let update_end = update_start.saturating_add(update.data.len());
    let aircraft_start = usize::from(DCS_BIOS_AIRCRAFT_NAME_ADDRESS);
    let aircraft_end = aircraft_start + DCS_BIOS_AIRCRAFT_NAME_LENGTH;
    update_start < aircraft_end && update_end > aircraft_start
}

fn extract_aircraft_name_from_memory(
    memory_map: &Arc<Mutex<VecMemoryMap>>,
) -> Result<Option<String>, String> {
    let memory_map = memory_map
        .lock()
        .map_err(|_| "DCS-BIOS memory map lock is poisoned.".to_string())?;
    let end_address = DCS_BIOS_AIRCRAFT_NAME_ADDRESS
        .saturating_add(DCS_BIOS_AIRCRAFT_NAME_LENGTH as u16)
        .saturating_sub(1);
    let bytes = memory_map
        .read(DCS_BIOS_AIRCRAFT_NAME_ADDRESS..=end_address)
        .ok_or_else(|| "DCS-BIOS aircraft name is incomplete.".to_string())?
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .collect::<Vec<_>>();
    let name = String::from_utf8_lossy(&bytes).trim().to_string();
    if name.is_empty() || name.eq_ignore_ascii_case("NONE") {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

fn now_iso8601() -> String {
    Utc::now().to_rfc3339()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let state = app.state::<AppState>().inner.clone();

            match load_manager_state(&app_handle) {
                Ok(manager_state) => {
                    *state.config.lock().unwrap() = manager_state.dcsbios_config.clone();
                    state.set_device_endpoints(&app_handle, manager_state.device_endpoints.clone());
                    state.set_device_role_assignments(
                        &app_handle,
                        manager_state.device_role_assignments.clone(),
                    );
                    state.set_adapter_mappings(&app_handle, manager_state.adapter_mappings.clone());
                    state.set_adapter_profiles(&app_handle, manager_state.adapter_profiles.clone());
                    if !manager_state.device_endpoints.is_empty() {
                        tauri::async_runtime::spawn({
                            let app_handle = app_handle.clone();
                            let state = state.clone();
                            async move {
                                if let Err(error) =
                                    refresh_devices(app_handle.clone(), state.clone()).await
                                {
                                    state.push_log(&app_handle, "WARN", "devices", error);
                                }
                            }
                        });
                    } else if let Err(error) = state.restart_endpoint_listeners(&app_handle) {
                        state.push_log(&app_handle, "WARN", "devices", error);
                    }
                }
                Err(error) => state.push_log(&app_handle, "WARN", "devices", error),
            }

            if let Err(error) = state.start_listener(app_handle.clone()) {
                state.push_log(&app_handle, "ERROR", "dcsbios", error);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            update_dcsbios_config,
            start_dcsbios,
            stop_dcsbios,
            send_dcsbios_command,
            trigger_role_input,
            save_device_endpoints,
            save_device_role_assignments,
            save_adapter_mappings,
            preview_adapter_profile,
            save_adapter_profile,
            start_learn,
            cancel_learn,
            list_serial_ports,
            list_devices
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_uses_raw_command_when_present() {
        let payload = normalize_command_request(DcsBiosCommandRequest {
            raw_command: Some("GEAR_TOGGLE TOGGLE".to_string()),
            control_id: Some("IGNORED".to_string()),
            argument: Some("1".to_string()),
        })
        .expect("payload");

        assert_eq!(payload, "GEAR_TOGGLE TOGGLE\n");
    }

    #[test]
    fn normalize_rejects_missing_arguments() {
        let error = normalize_command_request(DcsBiosCommandRequest {
            raw_command: None,
            control_id: Some("MASTER_ARM".to_string()),
            argument: None,
        })
        .expect_err("must fail");

        assert!(error.contains("Provide either rawCommand"));
    }

    #[test]
    fn diagnostics_reflect_listening_state() {
        let config = DcsBiosConnectionConfig::default();
        let status = DcsBiosStatus {
            connection_state: "listening".to_string(),
            last_seen_at: None,
            last_packet_at: None,
            packets_per_second: 0,
            total_packets: 0,
            aircraft_name: None,
            error: None,
            diagnostics: Vec::new(),
        };

        let diagnostics = build_diagnostics(&config, &status);
        assert!(diagnostics[0].contains("Listening on UDP"));
    }

    #[test]
    fn sanitize_endpoints_applies_default_baud_rate() {
        let endpoints = sanitize_device_endpoints(vec![DeviceEndpointConfig {
            id: "serial-1".to_string(),
            name: "Serial".to_string(),
            transport: DeviceEndpointTransport::Serial,
            address: "COM3".to_string(),
            enabled: true,
            baud_rate: 0,
            role_hint: EndpointRoleHint::Auto,
        }]);

        assert_eq!(endpoints[0].baud_rate, DEFAULT_DEVICE_ENDPOINT_BAUD_RATE);
    }

    #[test]
    fn listener_seeds_known_devices_from_discovery_results() {
        let endpoint = DeviceEndpointConfig {
            id: "serial-ddi".to_string(),
            name: "DDI".to_string(),
            transport: DeviceEndpointTransport::Serial,
            address: "COM6".to_string(),
            enabled: true,
            baud_rate: DEFAULT_DEVICE_ENDPOINT_BAUD_RATE,
            role_hint: EndpointRoleHint::DirectDevice,
        };
        let devices = vec![ManagedDeviceSummary {
            id: "direct:serial-ddi:0123456789ABCDEF".to_string(),
            connection_kind: "direct".to_string(),
            gateway_id: None,
            gateway_display_name: None,
            endpoint_id: "serial-ddi".to_string(),
            endpoint_name: "DDI".to_string(),
            endpoint_transport: "serial".to_string(),
            endpoint_address: "COM6".to_string(),
            display_name: "Upper Panel DDI".to_string(),
            firmware_version: Some("0.1.0".to_string()),
            state: "available".to_string(),
            protocol: "imcp+hcp".to_string(),
            assigned_address: Some(2),
            device_kind: Some("Upper Panel DDI".to_string()),
            device_kind_id: Some("upper-panel-ddi".to_string()),
            protocol_version: Some(1),
            device_id: Some("0123456789ABCDEF".to_string()),
            displays: Some(0),
            controls: Some(40),
            features: Some("control-events".to_string()),
        }];

        let known = known_runtime_devices_for_endpoint(&devices, &endpoint);

        assert_eq!(known.len(), 1);
        assert_eq!(known[&2].device_id, "0123456789ABCDEF");
        assert_eq!(known[&2].control_count, 40);
    }

    #[test]
    fn runtime_address_allocator_reuses_pending_join_address() {
        let known_devices = HashMap::from([
            (
                IMCP_FIRST_DEVICE_ADDRESS,
                KnownRuntimeDevice {
                    device_id: "device-a".to_string(),
                    control_count: 20,
                },
            ),
            (
                IMCP_FIRST_DEVICE_ADDRESS + 2,
                KnownRuntimeDevice {
                    device_id: "device-b".to_string(),
                    control_count: 10,
                },
            ),
        ]);
        let mut allocator = RuntimeAddressAllocator::from_known_devices(&known_devices);

        assert_eq!(allocator.allocate_for_join(0xCAFE_BABE), Ok(3));
        assert_eq!(allocator.allocate_for_join(0xCAFE_BABE), Ok(3));
        assert_eq!(allocator.allocate_for_join(0x1234_ABCD), Ok(5));
    }

    #[test]
    fn reconcile_runtime_device_replaces_stale_addresses_and_releases_them() {
        let mut known_devices = HashMap::from([
            (
                2,
                KnownRuntimeDevice {
                    device_id: "device-a".to_string(),
                    control_count: 20,
                },
            ),
            (
                4,
                KnownRuntimeDevice {
                    device_id: "device-b".to_string(),
                    control_count: 10,
                },
            ),
            (
                6,
                KnownRuntimeDevice {
                    device_id: "device-a".to_string(),
                    control_count: 20,
                },
            ),
        ]);
        let mut allocator = RuntimeAddressAllocator::from_known_devices(&known_devices);
        let probed = ProbedImcpDevice {
            display_name: "Upper Panel DDI".to_string(),
            firmware_version: "0.1.0".to_string(),
            assigned_address: Some(3),
            device_kind: DeviceKind::UpperPanelDdi,
            protocol_version: 1,
            device_id: "device-a".to_string(),
            displays: 0,
            controls: 40,
            features: "control-events".to_string(),
        };

        assert_eq!(allocator.allocate_for_join(0xCAFE_BABE), Ok(3));
        reconcile_known_runtime_device(&mut known_devices, &mut allocator, 3, &probed);

        assert_eq!(known_devices.len(), 2);
        assert!(!known_devices.contains_key(&2));
        assert!(!known_devices.contains_key(&6));
        assert_eq!(known_devices[&3].device_id, "device-a");
        assert_eq!(known_devices[&4].device_id, "device-b");
        assert!(allocator.join_addresses.is_empty());
        assert_eq!(allocator.allocate_for_join(0x1234), Ok(2));
    }

    #[test]
    fn repeated_runtime_reconnects_do_not_accumulate_addresses() {
        let mut known_devices = HashMap::from([(
            2,
            KnownRuntimeDevice {
                device_id: "device-a".to_string(),
                control_count: 20,
            },
        )]);
        let mut allocator = RuntimeAddressAllocator::from_known_devices(&known_devices);

        for join_id in 0..16 {
            let source_address = allocator
                .allocate_for_join(join_id)
                .expect("a free address should be available");
            let probed = ProbedImcpDevice {
                display_name: "Upper Panel DDI".to_string(),
                firmware_version: "0.1.0".to_string(),
                assigned_address: Some(source_address),
                device_kind: DeviceKind::UpperPanelDdi,
                protocol_version: 1,
                device_id: "device-a".to_string(),
                displays: 0,
                controls: 20,
                features: "control-events".to_string(),
            };

            reconcile_known_runtime_device(
                &mut known_devices,
                &mut allocator,
                source_address,
                &probed,
            );

            assert_eq!(known_devices.len(), 1);
            assert_eq!(known_devices[&source_address].device_id, "device-a");
            assert_eq!(
                allocator.used_addresses,
                known_devices.keys().copied().collect()
            );
            assert!(allocator.join_addresses.is_empty());
        }
    }

    #[test]
    fn runtime_address_allocator_rejects_reserved_addresses_when_exhausted() {
        let known_devices: HashMap<u8, KnownRuntimeDevice> = (IMCP_FIRST_DEVICE_ADDRESS
            ..=IMCP_LAST_DEVICE_ADDRESS)
            .map(|address| {
                (
                    address,
                    KnownRuntimeDevice {
                        device_id: format!("device-{address}"),
                        control_count: 1,
                    },
                )
            })
            .collect();
        let mut allocator = RuntimeAddressAllocator::from_known_devices(&known_devices);

        assert_eq!(
            allocator.allocate_for_join(1),
            Err("IMCP device address pool exhausted.".to_string())
        );
        assert!(allocator.join_addresses.is_empty());
    }

    #[test]
    fn device_hello_request_is_broadcast() {
        let frame = device_hello_request_frame().expect("request frame");

        assert_eq!(frame.to_address(), Address::Broadcast);
        assert_eq!(frame.from_address(), IMCP_MASTER_ADDRESS);
        let FramePayload::Set(payload) = frame.payload() else {
            panic!("request must use IMCP Set");
        };
        assert!(matches!(
            decode_set_packet(payload),
            Ok(AppPacketKind::ControlEvent(ControlEvent {
                control_id: CONTROL_ID_REQUEST_DEVICE_HELLO,
                event: ControlValue::RequestDeviceHello,
                ..
            }))
        ));
    }

    fn test_serial_endpoint() -> DeviceEndpointConfig {
        DeviceEndpointConfig {
            id: "serial-hub".to_string(),
            name: "Upper Hub".to_string(),
            transport: DeviceEndpointTransport::Serial,
            address: "COM4".to_string(),
            enabled: true,
            baud_rate: DEFAULT_DEVICE_ENDPOINT_BAUD_RATE,
            role_hint: EndpointRoleHint::Auto,
        }
    }

    fn test_probed_child(device_id: &str, assigned_address: u8) -> ProbedImcpDevice {
        ProbedImcpDevice {
            display_name: "Button Panel".to_string(),
            firmware_version: "1.0.0".to_string(),
            assigned_address: Some(assigned_address),
            device_kind: DeviceKind::ButtonPanel,
            protocol_version: 1,
            device_id: device_id.to_string(),
            displays: 0,
            controls: 20,
            features: "control-events".to_string(),
        }
    }

    #[test]
    fn pending_hub_join_correlates_device_hello_with_gateway() {
        let endpoint = test_serial_endpoint();
        let gateway_id = "hub:serial-hub:0000000000000001".to_string();
        let mut pending = HashMap::new();
        pending.insert(
            0x02,
            PendingHubDiscovery::with_expiry(
                gateway_id.clone(),
                "IMCP Hub".to_string(),
                Instant::now() + IMCP_CHILD_ENUMERATION_TIMEOUT,
            ),
        );
        let now = Instant::now();

        assert_eq!(
            record_pending_child_join(&mut pending, 0x00, 0xCAFE_BABE, 0x04, now),
            Some(0x02)
        );
        assert_eq!(
            pending[&0x02].children,
            vec![PendingChildDiscovery {
                join_id: 0xCAFE_BABE,
                assigned_address: 0x04,
            }]
        );

        let gateway = pending_gateway_for_child_address(&pending, 0x04, now).expect("gateway");
        let child = test_probed_child("0000000000001234", 0x04);
        let summary = probed_device_to_summary(
            &endpoint,
            &child,
            "hub-child",
            Some((gateway.0.as_str(), gateway.1.as_str())),
        );

        assert_eq!(summary.connection_kind, "hub-child");
        assert_eq!(summary.gateway_id.as_deref(), Some(gateway_id.as_str()));
        assert_eq!(
            summary.id,
            "hub-child:hub:serial-hub:0000000000000001:0000000000001234"
        );
    }

    #[test]
    fn multiple_pending_hub_joins_share_one_gateway() {
        let gateway_id = "hub:serial-hub:0000000000000001".to_string();
        let mut pending = HashMap::new();
        pending.insert(
            0x02,
            PendingHubDiscovery::with_expiry(
                gateway_id.clone(),
                "IMCP Hub".to_string(),
                Instant::now() + IMCP_CHILD_ENUMERATION_TIMEOUT,
            ),
        );
        let now = Instant::now();

        assert_eq!(
            record_pending_child_join(&mut pending, 0x00, 0x1111_2222, 0x04, now),
            Some(0x02)
        );
        assert_eq!(
            record_pending_child_join(&mut pending, 0x00, 0x3333_4444, 0x05, now),
            Some(0x02)
        );
        assert_eq!(pending[&0x02].children.len(), 2);

        for assigned_address in [0x04, 0x05] {
            let gateway = pending_gateway_for_child_address(&pending, assigned_address, now)
                .expect("gateway");
            assert_eq!(gateway.0, gateway_id);
            assert_eq!(gateway.1, "IMCP Hub");
        }
    }

    #[test]
    fn child_join_refreshes_pending_hub_deadline() {
        let now = Instant::now();
        let mut pending = HashMap::from([(
            0x02,
            PendingHubDiscovery::with_expiry(
                "hub:serial-hub:0000000000000001".to_string(),
                "IMCP Hub".to_string(),
                now + Duration::from_millis(1),
            ),
        )]);

        assert_eq!(
            record_pending_child_join(&mut pending, 0x00, 0xCAFE_BABE, 0x04, now),
            Some(0x02)
        );
        assert_eq!(
            pending[&0x02].expires_at,
            now + IMCP_CHILD_ENUMERATION_TIMEOUT
        );
    }

    #[test]
    fn expired_pending_hub_context_does_not_correlate_later_join() {
        let now = Instant::now();
        let mut pending = HashMap::new();
        pending.insert(
            0x02,
            PendingHubDiscovery::with_expiry(
                "hub:serial-hub:0000000000000001".to_string(),
                "IMCP Hub".to_string(),
                now - Duration::from_secs(1),
            ),
        );

        assert_eq!(pending_hub_address_for_join(&pending, 0x00, now), None);
        assert_eq!(
            record_pending_child_join(&mut pending, 0x00, 0xCAFE_BABE, 0x04, now),
            None
        );
        assert!(pending[&0x02].children.is_empty());

        assert!(expire_pending_hub_discoveries(&mut pending, now));
        assert!(pending.is_empty());
    }

    #[test]
    fn expired_hub_context_requires_a_quiet_period_before_the_next_hub() {
        let now = Instant::now();
        let mut pending = HashMap::from([(
            0x02,
            PendingHubDiscovery::with_expiry(
                "hub:serial-hub:0000000000000001".to_string(),
                "IMCP Hub 1".to_string(),
                now - Duration::from_millis(1),
            ),
        )]);
        let mut quiet_until = None;

        assert!(!hub_discovery_can_advance(
            &mut pending,
            &mut quiet_until,
            now
        ));
        let first_deadline = quiet_until.expect("quiet deadline");

        let delayed_frame_at = now + Duration::from_millis(100);
        defer_hub_discovery_during_quiet_period(&pending, &mut quiet_until, delayed_frame_at);
        assert!(quiet_until.expect("extended deadline") > first_deadline);
        assert!(!hub_discovery_can_advance(
            &mut pending,
            &mut quiet_until,
            first_deadline
        ));
        assert!(hub_discovery_can_advance(
            &mut pending,
            &mut quiet_until,
            delayed_frame_at + IMCP_CHILD_ENUMERATION_TIMEOUT
        ));
    }

    #[test]
    fn only_join_and_device_hello_extend_hub_discovery_quiet_period() {
        let hello = encode_set_packet(&AppPacketKind::DeviceHello(hcp::DeviceHello {
            device_id: 1,
            device_kind: DeviceKind::ImcpHub,
            protocol_version: 1,
            firmware_version: hcp::Version {
                major: 0,
                minor: 1,
                patch: 0,
            },
            capabilities: hcp::Capabilities {
                displays: 0,
                controls: 0,
                features: 0,
            },
        }))
        .expect("encode hello");
        let control = encode_set_packet(&AppPacketKind::ControlEvent(ControlEvent {
            seq: 1,
            control_id: 2,
            event: ControlValue::Button { pressed: true },
        }))
        .expect("encode control");

        assert!(is_hub_discovery_traffic(&FramePayload::Join(1)));
        assert!(is_hub_discovery_traffic(&FramePayload::Set(hello)));
        assert!(!is_hub_discovery_traffic(&FramePayload::Set(control)));
        assert!(!is_hub_discovery_traffic(&FramePayload::Ack(0)));
        assert!(!is_hub_discovery_traffic(&FramePayload::Ping));
    }

    #[test]
    fn hub_child_discovery_restarts_after_address_reuse() {
        let mut observed = HashMap::new();
        let hub_a = "0000000000000001";
        let hub_b = "0000000000000002";

        assert!(hub_requires_child_discovery(&mut observed, hub_a, 0x02));
        assert!(!hub_requires_child_discovery(&mut observed, hub_a, 0x02));
        assert!(hub_requires_child_discovery(&mut observed, hub_a, 0x03));
        assert!(hub_requires_child_discovery(&mut observed, hub_b, 0x02));
        assert!(hub_requires_child_discovery(&mut observed, hub_a, 0x02));
    }

    #[test]
    fn ambiguous_pending_hub_context_does_not_correlate_join() {
        let now = Instant::now();
        let mut pending = HashMap::new();
        pending.insert(
            0x02,
            PendingHubDiscovery::with_expiry(
                "hub:serial-hub:0000000000000001".to_string(),
                "IMCP Hub 1".to_string(),
                now + IMCP_CHILD_ENUMERATION_TIMEOUT,
            ),
        );
        pending.insert(
            0x03,
            PendingHubDiscovery::with_expiry(
                "hub:serial-hub:0000000000000002".to_string(),
                "IMCP Hub 2".to_string(),
                now + IMCP_CHILD_ENUMERATION_TIMEOUT,
            ),
        );

        assert_eq!(pending_hub_address_for_join(&pending, 0x00, now), None);
        assert_eq!(
            record_pending_child_join(&mut pending, 0x00, 0xCAFE_BABE, 0x04, now),
            None
        );
        assert!(pending.values().all(|context| context.children.is_empty()));
    }

    #[test]
    fn hello_without_pending_hub_context_stays_direct() {
        let endpoint = test_serial_endpoint();
        let pending = HashMap::new();
        let child = test_probed_child("0000000000001234", 0x04);
        let now = Instant::now();

        assert!(pending_gateway_for_child_address(&pending, 0x04, now).is_none());
        let summary = probed_device_to_summary(&endpoint, &child, "direct", None);

        assert_eq!(summary.connection_kind, "direct");
        assert_eq!(summary.gateway_id, None);
        assert_eq!(summary.id, "direct:serial-hub:0000000000001234");
    }

    #[test]
    fn child_gateway_memory_is_limited_to_the_listener_session() {
        let pending = HashMap::new();
        let now = Instant::now();
        let device_id = "0000000000001234";
        let known_in_session = HashMap::from([(
            device_id.to_string(),
            (
                "hub:serial-hub:0000000000000001".to_string(),
                "IMCP Hub".to_string(),
            ),
        )]);

        assert!(
            gateway_for_device_hello(&pending, &known_in_session, 0x04, device_id, now).is_some()
        );

        let next_session = HashMap::new();
        assert!(gateway_for_device_hello(&pending, &next_session, 0x04, device_id, now).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_pty_endpoint_opens_without_applying_serial_baud_ioctl() {
        let (_master, slave) = serialport::TTYPort::pair().expect("PTY pair");
        let path = serialport::SerialPort::name(&slave).expect("PTY slave path");
        drop(slave);
        let endpoint = DeviceEndpointConfig {
            id: "mock-ddi".to_string(),
            name: "Mock DDI".to_string(),
            transport: DeviceEndpointTransport::Serial,
            address: path.clone(),
            enabled: true,
            baud_rate: DEFAULT_DEVICE_ENDPOINT_BAUD_RATE,
            role_hint: EndpointRoleHint::DirectDevice,
        };

        assert!(is_macos_pty_path(&path));
        assert_eq!(serial_endpoint_open_baud_rate(&endpoint), 0);
        let reopened = open_serial_endpoint(&endpoint).expect("open PTY endpoint");
        assert_eq!(reopened.name().as_deref(), Some(path.as_str()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_physical_serial_endpoint_keeps_configured_baud_rate() {
        let endpoint = DeviceEndpointConfig {
            id: "physical".to_string(),
            name: "Physical device".to_string(),
            transport: DeviceEndpointTransport::Serial,
            address: "/dev/cu.usbmodem1234".to_string(),
            enabled: true,
            baud_rate: DEFAULT_DEVICE_ENDPOINT_BAUD_RATE,
            role_hint: EndpointRoleHint::Auto,
        };

        assert!(!is_macos_pty_path(&endpoint.address));
        assert_eq!(
            serial_endpoint_open_baud_rate(&endpoint),
            DEFAULT_DEVICE_ENDPOINT_BAUD_RATE
        );
    }

    #[test]
    fn hub_child_summary_includes_gateway_metadata() {
        let endpoint = DeviceEndpointConfig {
            id: "serial-hub".to_string(),
            name: "Upper Hub".to_string(),
            transport: DeviceEndpointTransport::Serial,
            address: "COM4".to_string(),
            enabled: true,
            baud_rate: DEFAULT_DEVICE_ENDPOINT_BAUD_RATE,
            role_hint: EndpointRoleHint::Auto,
        };
        let child = ProbedImcpDevice {
            display_name: "Button Panel".to_string(),
            firmware_version: "1.0.0".to_string(),
            assigned_address: Some(4),
            device_kind: DeviceKind::ButtonPanel,
            protocol_version: 1,
            device_id: "0000000000001234".to_string(),
            displays: 0,
            controls: 20,
            features: "control-events".to_string(),
        };

        let summary = probed_device_to_summary(
            &endpoint,
            &child,
            "hub-child",
            Some(("hub:serial-hub:0000000000000001", "IMCP Hub")),
        );

        assert_eq!(summary.connection_kind, "hub-child");
        assert_eq!(
            summary.id,
            "hub-child:hub:serial-hub:0000000000000001:0000000000001234"
        );
        assert_eq!(
            summary.gateway_id.as_deref(),
            Some("hub:serial-hub:0000000000000001")
        );
        assert_eq!(summary.gateway_display_name.as_deref(), Some("IMCP Hub"));
        assert_eq!(summary.device_kind_id.as_deref(), Some("button-panel"));
    }

    #[test]
    fn unavailable_summary_marks_endpoint_error() {
        let endpoint = DeviceEndpointConfig {
            id: "serial-error".to_string(),
            name: "Broken".to_string(),
            transport: DeviceEndpointTransport::Serial,
            address: "COM9".to_string(),
            enabled: true,
            baud_rate: DEFAULT_DEVICE_ENDPOINT_BAUD_RATE,
            role_hint: EndpointRoleHint::Auto,
        };

        let summary = unavailable_device_summary(&endpoint, "open failed".to_string());

        assert_eq!(summary.state, "error");
        assert_eq!(summary.endpoint_id, "serial-error");
        assert_eq!(summary.features.as_deref(), Some("open failed"));
    }

    #[test]
    fn sanitize_role_assignments_keeps_many_devices_roles_and_bindings() {
        let assignments = sanitize_device_role_assignments(vec![
            DeviceRoleAssignment {
                device_id: "A".to_string(),
                role_id: "left-ddi".to_string(),
                bindings: vec![PhysicalToLogicalBinding {
                    physical_control_id: 3,
                    logical_control_id: "button-1".to_string(),
                }],
            },
            DeviceRoleAssignment {
                device_id: " A ".to_string(),
                role_id: "left-ddi".to_string(),
                bindings: vec![
                    PhysicalToLogicalBinding {
                        physical_control_id: 3,
                        logical_control_id: "button-1".to_string(),
                    },
                    PhysicalToLogicalBinding {
                        physical_control_id: 4,
                        logical_control_id: "button-2".to_string(),
                    },
                ],
            },
            DeviceRoleAssignment {
                device_id: "B".to_string(),
                role_id: "left-ddi".to_string(),
                bindings: vec![PhysicalToLogicalBinding {
                    physical_control_id: 3,
                    logical_control_id: "button-1".to_string(),
                }],
            },
        ]);

        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments[0].bindings.len(), 2);
        assert_eq!(assignments[1].device_id, "B");
    }

    #[test]
    fn learn_append_creates_a_new_device_role_binding() {
        let request = LearnRequest {
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-4".to_string(),
            target_device_id: None,
            expected_event_kind: None,
            mode: LearnMode::Append,
            timeout_ms: Some(10_000),
        };

        let assignments = apply_learn_binding(Vec::new(), &request, "device-new", 12);

        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].device_id, "device-new");
        assert_eq!(assignments[0].role_id, "left-ddi");
        assert_eq!(
            assignments[0].bindings,
            vec![PhysicalToLogicalBinding {
                physical_control_id: 12,
                logical_control_id: "button-4".to_string(),
            }]
        );
    }

    #[test]
    fn learn_replace_removes_only_the_selected_logical_route() {
        let request = LearnRequest {
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-1".to_string(),
            target_device_id: None,
            expected_event_kind: None,
            mode: LearnMode::Replace,
            timeout_ms: Some(10_000),
        };
        let assignments = vec![
            DeviceRoleAssignment {
                device_id: "device-a".to_string(),
                role_id: "left-ddi".to_string(),
                bindings: vec![
                    PhysicalToLogicalBinding {
                        physical_control_id: 1,
                        logical_control_id: "button-1".to_string(),
                    },
                    PhysicalToLogicalBinding {
                        physical_control_id: 2,
                        logical_control_id: "button-2".to_string(),
                    },
                ],
            },
            DeviceRoleAssignment {
                device_id: "device-a".to_string(),
                role_id: "right-ddi".to_string(),
                bindings: vec![PhysicalToLogicalBinding {
                    physical_control_id: 1,
                    logical_control_id: "button-1".to_string(),
                }],
            },
        ];

        let assignments = apply_learn_binding(assignments, &request, "device-b", 9);

        assert_eq!(
            assignments[0].bindings,
            vec![PhysicalToLogicalBinding {
                physical_control_id: 2,
                logical_control_id: "button-2".to_string(),
            }]
        );
        assert_eq!(assignments[1].bindings.len(), 1);
        assert_eq!(assignments[2].device_id, "device-b");
        assert_eq!(assignments[2].bindings[0].physical_control_id, 9);
    }

    #[test]
    fn sanitize_adapter_mappings_keeps_multiple_actions_for_one_logical_input() {
        let mappings = sanitize_adapter_mappings(vec![AdapterMappingConfig {
            adapter_id: "dcs-bios".to_string(),
            profile_id: "default".to_string(),
            aircraft_name: None,
            profile_label: None,
            output_mappings: Vec::new(),
            mappings: vec![
                AdapterControlMapping {
                    role_id: "left-ddi".to_string(),
                    logical_control_id: "button-3".to_string(),
                    event_kind: EventKind::ButtonPushed,
                    action: AdapterActionConfig {
                        action_id: "control-command".to_string(),
                        parameters: HashMap::from([
                            ("identifier".to_string(), "AAA".to_string()),
                            ("argument".to_string(), "1".to_string()),
                        ]),
                    },
                },
                AdapterControlMapping {
                    role_id: "left-ddi".to_string(),
                    logical_control_id: "button-3".to_string(),
                    event_kind: EventKind::ButtonPushed,
                    action: AdapterActionConfig {
                        action_id: "control-command".to_string(),
                        parameters: HashMap::from([
                            ("identifier".to_string(), "BBB".to_string()),
                            ("argument".to_string(), "2".to_string()),
                        ]),
                    },
                },
            ],
        }]);

        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].mappings.len(), 2);
    }

    #[test]
    fn legacy_state_migrates_to_schema_v2_and_identity_bindings() {
        let (state, migrated) = normalize_manager_state_json(
            r#"{
                "deviceEndpoints": [],
                "deviceRoleAssignments": [{"deviceId":"device-a","role":"LEFT_DDI"}],
                "roleMappings": [{"role":"LEFT_DDI","mappings":[
                    {"id":"old-1","controlId":3,"inputEvent":"BUTTON_PUSHED",
                     "action":{"identifier":"MASTER_ARM_SW","argument":"1"}}
                ]}]
            }"#,
        )
        .expect("legacy state");

        assert!(migrated);
        assert_eq!(state.schema_version, STATE_SCHEMA_VERSION);
        assert_eq!(
            state.device_role_assignments[0].bindings[0].physical_control_id,
            3
        );
        assert_eq!(
            state.device_role_assignments[0].bindings[0].logical_control_id,
            "button-3"
        );
        assert_eq!(state.adapter_mappings[0].adapter_id, "dcs-bios");
        assert_eq!(
            state.adapter_mappings[0].mappings[0].action.parameters["identifier"],
            "MASTER_ARM_SW"
        );
    }

    #[test]
    fn current_state_preserves_remote_dcsbios_command_destination() {
        let (state, migrated) = normalize_manager_state_json(
            r#"{
                "schemaVersion": 2,
                "dcsbiosConfig": {
                    "exportHost": "239.255.50.10",
                    "exportPort": 5010,
                    "commandHost": "192.168.1.97",
                    "commandPort": 7778,
                    "commandTransport": "udp"
                },
                "deviceEndpoints": [],
                "deviceRoleAssignments": [],
                "adapterMappings": [],
                "adapterProfiles": []
            }"#,
        )
        .expect("current state");

        assert!(!migrated);
        assert_eq!(state.dcsbios_config.command_host, "192.168.1.97");
        assert_eq!(state.dcsbios_config.command_port, 7778);
        assert!(matches!(
            state.dcsbios_config.command_transport,
            CommandTransport::Udp
        ));
    }

    struct FakeAdapter {
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl InputAdapter for FakeAdapter {
        fn dispatch_input(
            &self,
            _config: &DcsBiosConnectionConfig,
            action: &AdapterActionConfig,
            _event: &LogicalInputEvent,
        ) -> Result<(), String> {
            self.calls.lock().unwrap().push(action.action_id.clone());
            Ok(())
        }
    }

    #[test]
    fn fake_adapters_receive_all_resolved_actions_without_game_io() {
        let event = mapping::LogicalInputEvent {
            device_id: "device-a".to_string(),
            physical_control_id: 3,
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-1".to_string(),
            event_kind: EventKind::ButtonPushed,
            value: ControlValue::Button { pressed: false },
        };
        let mappings = vec![
            AdapterMappingConfig {
                adapter_id: "fake-a".to_string(),
                profile_id: "default".to_string(),
                aircraft_name: None,
                profile_label: None,
                output_mappings: Vec::new(),
                mappings: vec![AdapterControlMapping {
                    role_id: "left-ddi".to_string(),
                    logical_control_id: "button-1".to_string(),
                    event_kind: EventKind::ButtonPushed,
                    action: AdapterActionConfig {
                        action_id: "fake-a-action".to_string(),
                        parameters: HashMap::new(),
                    },
                }],
            },
            AdapterMappingConfig {
                adapter_id: "fake-b".to_string(),
                profile_id: "default".to_string(),
                aircraft_name: None,
                profile_label: None,
                output_mappings: Vec::new(),
                mappings: vec![AdapterControlMapping {
                    role_id: "left-ddi".to_string(),
                    logical_control_id: "button-1".to_string(),
                    event_kind: EventKind::ButtonPushed,
                    action: AdapterActionConfig {
                        action_id: "fake-b-action".to_string(),
                        parameters: HashMap::new(),
                    },
                }],
            },
        ];
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut registry = AdapterRegistry {
            adapters: HashMap::new(),
        };
        registry.adapters.insert(
            "fake-a",
            Box::new(FakeAdapter {
                calls: calls.clone(),
            }),
        );
        registry.adapters.insert(
            "fake-b",
            Box::new(FakeAdapter {
                calls: calls.clone(),
            }),
        );

        for resolved in resolve_adapter_actions(&mappings, &event) {
            registry
                .get(&resolved.adapter_id)
                .expect("fake adapter is registered")
                .dispatch_input(
                    &DcsBiosConnectionConfig::default(),
                    &resolved.action,
                    &resolved.event,
                )
                .expect("fake adapter dispatch must succeed");
        }

        assert_eq!(
            *calls.lock().unwrap(),
            vec!["fake-a-action".to_string(), "fake-b-action".to_string()]
        );
    }

    #[test]
    fn import_command_rejects_invalid_identifier() {
        let error = encode_import_command("BAD IDENT", "1").expect_err("must fail");
        assert!(error.contains("Invalid DCS-BIOS command"));
    }

    #[test]
    fn dcsbios_event_value_argument_uses_reference_step_for_encoder_delta() {
        let action = AdapterActionConfig {
            action_id: "control-command".to_string(),
            parameters: HashMap::from([
                ("argument".to_string(), "$event-value".to_string()),
                ("argumentMode".to_string(), "event-value".to_string()),
                ("suggestedStep".to_string(), "3200".to_string()),
            ]),
        };
        let event = LogicalInputEvent {
            device_id: "device-a".to_string(),
            physical_control_id: 1,
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-1".to_string(),
            event_kind: EventKind::EncoderDelta,
            value: ControlValue::EncoderDelta { steps: -2 },
        };

        assert_eq!(
            resolve_dcsbios_argument(&action, &event).expect("argument"),
            "-6400"
        );
    }

    #[test]
    fn dcsbios_event_value_argument_clamps_absolute_value_to_reference_maximum() {
        let action = AdapterActionConfig {
            action_id: "control-command".to_string(),
            parameters: HashMap::from([
                ("argument".to_string(), "$event-value".to_string()),
                ("argumentMode".to_string(), "event-value".to_string()),
                ("maxValue".to_string(), "5".to_string()),
            ]),
        };
        let event = LogicalInputEvent {
            device_id: "device-a".to_string(),
            physical_control_id: 1,
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-1".to_string(),
            event_kind: EventKind::AbsoluteChanged,
            value: ControlValue::Absolute { value: 9 },
        };

        assert_eq!(
            resolve_dcsbios_argument(&action, &event).expect("argument"),
            "5"
        );
    }

    #[test]
    fn dcsbios_memory_updates_write_memory_map() {
        let memory = Arc::new(Mutex::new(VecMemoryMap::default()));
        let packet = vec![0x55, 0x55, 0x55, 0x55, 0x00, 0x10, 0x02, 0x00, 0x34, 0x12];
        let mut decoder = DcsBiosStreamDecoder::default();
        let updates = decoder
            .feed(&packet)
            .into_iter()
            .filter_map(|event| match event {
                DcsBiosStreamEvent::MemoryUpdate(update) => Some(update),
                DcsBiosStreamEvent::FrameBoundary => None,
            })
            .collect::<Vec<_>>();

        apply_dcsbios_memory_updates(memory.clone(), &updates).expect("packet must decode");

        let binding = memory.lock().unwrap();
        let bytes = binding.read(0x1000..=0x1001).expect("bytes must exist");
        assert_eq!(bytes, &[0x34, 0x12]);
    }

    #[test]
    fn aircraft_name_is_reassembled_from_split_memory_updates() {
        let memory = Arc::new(Mutex::new(VecMemoryMap::default()));
        let fa18_updates = [
            DcsBiosMemoryUpdate {
                address: DCS_BIOS_AIRCRAFT_NAME_ADDRESS,
                data: b"FA-18C_h".to_vec(),
            },
            DcsBiosMemoryUpdate {
                address: DCS_BIOS_AIRCRAFT_NAME_ADDRESS + 8,
                data: b"ornet\0\0\0\0\0\0\0\0\0\0\0".to_vec(),
            },
        ];
        assert!(fa18_updates.iter().all(update_overlaps_aircraft_name));
        apply_dcsbios_memory_updates(memory.clone(), &fa18_updates).expect("FA-18 update");
        assert_eq!(
            extract_aircraft_name_from_memory(&memory).expect("aircraft name"),
            Some("FA-18C_hornet".to_string())
        );

        let f16_updates = [
            DcsBiosMemoryUpdate {
                address: DCS_BIOS_AIRCRAFT_NAME_ADDRESS,
                data: b"F-16".to_vec(),
            },
            DcsBiosMemoryUpdate {
                address: DCS_BIOS_AIRCRAFT_NAME_ADDRESS + 4,
                data: b"C_50\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec(),
            },
        ];
        apply_dcsbios_memory_updates(memory.clone(), &f16_updates).expect("F-16 update");
        assert_eq!(
            extract_aircraft_name_from_memory(&memory).expect("aircraft name"),
            Some("F-16C_50".to_string())
        );

        assert!(!update_overlaps_aircraft_name(&DcsBiosMemoryUpdate {
            address: DCS_BIOS_AIRCRAFT_NAME_ADDRESS + DCS_BIOS_AIRCRAFT_NAME_LENGTH as u16,
            data: vec![0],
        }));
    }

    #[test]
    fn built_in_profile_generates_logical_button_actions_in_catalog_order() {
        let profile = AdapterCatalog::builtin().adapters[0].profiles[0].clone();
        let mapping = build_profile_adapter_mapping("dcs-bios", &profile);

        assert_eq!(mapping.mappings.len(), 40);
        assert_eq!(
            mapping.mappings[0]
                .action
                .parameters
                .get("referenceControl")
                .map(String::as_str),
            Some("MFD_L_1")
        );
        assert_eq!(
            mapping.mappings[1]
                .action
                .parameters
                .get("referenceControl")
                .map(String::as_str),
            Some("MFD_L_2")
        );
        assert_eq!(
            mapping.mappings[20]
                .action
                .parameters
                .get("referenceControl")
                .map(String::as_str),
            Some("MFD_R_1")
        );
        assert_eq!(mapping.mappings[0].event_kind, EventKind::ButtonPushed);
        assert_eq!(mapping.mappings[0].action.action_id, "control-pulse");
        assert_eq!(
            mapping.mappings[0]
                .action
                .parameters
                .get("argument")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            mapping.mappings[0]
                .action
                .parameters
                .get("releaseArgument")
                .map(String::as_str),
            Some("0")
        );
    }

    #[test]
    fn button_pushed_pulse_encodes_press_and_release_commands() {
        let action = AdapterActionConfig {
            action_id: "control-pulse".to_string(),
            parameters: HashMap::from([
                ("identifier".to_string(), "LEFT_DDI_PB_01".to_string()),
                ("argument".to_string(), "1".to_string()),
                ("releaseArgument".to_string(), "0".to_string()),
            ]),
        };
        let event = LogicalInputEvent {
            device_id: "device-a".to_string(),
            physical_control_id: 0,
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-0".to_string(),
            event_kind: EventKind::ButtonPushed,
            value: ControlValue::Button { pressed: false },
        };

        assert_eq!(
            encode_dcsbios_action_payload(&action, &event).expect("pulse payload"),
            "LEFT_DDI_PB_01 1\nLEFT_DDI_PB_01 0\n"
        );
    }

    #[test]
    fn manual_role_button_pushed_uses_the_role_event_path() {
        let event = build_role_input_event(RoleInputTriggerRequest {
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-0".to_string(),
            event_kind: EventKind::ButtonPushed,
        })
        .expect("built-in Role action");

        assert_eq!(event.device_id, "manager-role-input");
        assert_eq!(event.event_kind, EventKind::ButtonPushed);
        assert_eq!(event.value, ControlValue::Button { pressed: false });
    }

    #[test]
    fn manual_role_input_rejects_an_unsupported_action() {
        let error = build_role_input_event(RoleInputTriggerRequest {
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-0".to_string(),
            event_kind: EventKind::EncoderDelta,
        })
        .expect_err("DDI pushbutton does not support encoder actions");

        assert!(error.contains("is not supported"));
    }

    #[test]
    fn dcsbios_output_mapping_resolves_address_from_adapter_identity() {
        let catalog = AdapterCatalog::builtin();
        let config = AdapterMappingConfig {
            adapter_id: "dcs-bios".to_string(),
            profile_id: "F-16C_50".to_string(),
            aircraft_name: None,
            profile_label: None,
            mappings: Vec::new(),
            output_mappings: Vec::new(),
        };
        let mapping = AdapterOutputMapping {
            role_id: "left-ddi".to_string(),
            logical_control_id: "button-0".to_string(),
            source_id: "control-output".to_string(),
            parameters: HashMap::from([
                ("referenceProfile".to_string(), "F-16C_50".to_string()),
                ("referenceControl".to_string(), "MFD_L_1".to_string()),
                (
                    "referenceOutput".to_string(),
                    "F_16C_50_MFD_L_1".to_string(),
                ),
            ]),
        };

        assert_eq!(
            resolve_dcsbios_output_range(&catalog, &config, &mapping),
            Some((17_502, Some(2)))
        );
    }

    #[test]
    fn dcsbios_stream_decoder_skips_noise_and_recovers_from_bad_length() {
        let packet = [
            0x01, 0x02, 0x03, 0x55, 0x55, 0x55, 0x55, 0x00, 0x10, 0xFF, 0xFF, 0x55, 0x55, 0x55,
            0x55, 0x00, 0x20, 0x01, 0x00, 0x7F,
        ];
        let mut decoder = DcsBiosStreamDecoder::default();

        assert_eq!(
            decoder.feed(&packet),
            vec![DcsBiosStreamEvent::MemoryUpdate(DcsBiosMemoryUpdate {
                address: 0x2000,
                data: vec![0x7F],
            })]
        );
    }

    #[test]
    fn dcsbios_stream_decoder_reassembles_datagram_boundaries() {
        let packet = [0x55, 0x55, 0x55, 0x55, 0x00, 0x10, 0x02, 0x00, 0x34, 0x12];
        let mut decoder = DcsBiosStreamDecoder::default();

        assert!(decoder.feed(&packet[..7]).is_empty());
        assert_eq!(
            decoder.feed(&packet[7..]),
            vec![DcsBiosStreamEvent::MemoryUpdate(DcsBiosMemoryUpdate {
                address: 0x1000,
                data: vec![0x34, 0x12],
            })]
        );
    }

    #[test]
    fn dcsbios_stream_decoder_handles_multiple_writes_and_frames() {
        let bytes = [
            0x55, 0x55, 0x55, 0x55, 0x00, 0x10, 0x01, 0x00, 0xAA, 0x10, 0x10, 0x01, 0x00, 0xBB,
            0x55, 0x55, 0x55, 0x55, 0x20, 0x10, 0x01, 0x00, 0xCC,
        ];
        let mut decoder = DcsBiosStreamDecoder::default();

        assert_eq!(
            decoder.feed(&bytes),
            vec![
                DcsBiosStreamEvent::MemoryUpdate(DcsBiosMemoryUpdate {
                    address: 0x1000,
                    data: vec![0xAA],
                }),
                DcsBiosStreamEvent::MemoryUpdate(DcsBiosMemoryUpdate {
                    address: 0x1010,
                    data: vec![0xBB],
                }),
                DcsBiosStreamEvent::FrameBoundary,
                DcsBiosStreamEvent::MemoryUpdate(DcsBiosMemoryUpdate {
                    address: 0x1020,
                    data: vec![0xCC],
                }),
            ]
        );
    }

    #[test]
    fn button_release_after_press_generates_pushed_event() {
        let mut pressed_buttons = HashSet::new();
        let device_id = "DEVICE-1".to_string();

        pressed_buttons.insert((device_id.clone(), 5));
        let released = pressed_buttons.remove(&(device_id, 5));

        assert!(released);
        let events = control_event_kinds(&ControlValue::Button { pressed: false }, released);
        assert_eq!(events, [EventKind::ButtonUp, EventKind::ButtonPushed]);
    }

    #[test]
    fn display_sequence_is_monotonic_per_device() {
        let state = RuntimeState::new();

        assert_eq!(state.next_display_sequence("device-a"), 1);
        assert_eq!(state.next_display_sequence("device-b"), 1);
        assert_eq!(state.next_display_sequence("device-a"), 2);
    }
}
