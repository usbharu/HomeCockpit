use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use chrono::Utc;
use dcs_bios::{
    import::ImportCommand,
    mem::{MemoryMap, VecMemoryMap},
    source::Source,
    DcsBios, DcsBiosImpl,
};
use hcp::{
    decode_set_packet, encode_set_packet, AppPacketKind, ControlEvent, ControlValue, DeviceKind,
    CONTROL_ID_REQUEST_DEVICE_HELLO,
};
use imcp::{
    frame::{Address, Frame, FramePayload, MAX_ENCODED_FRAME_SIZE},
    parser::FrameParser,
};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

const DEFAULT_EXPORT_HOST: &str = "239.255.50.10";
const DEFAULT_EXPORT_PORT: u16 = 5010;
const DEFAULT_COMMAND_HOST: &str = "127.0.0.1";
const DEFAULT_COMMAND_PORT: u16 = 7778;
const MAX_LOG_ENTRIES: usize = 250;
const DEFAULT_DEVICE_ENDPOINT_BAUD_RATE: u32 = 115200;
const IMCP_MASTER_ADDRESS: u8 = 0x01;
const IMCP_ROOT_PROBE_TIMEOUT: Duration = Duration::from_millis(900);
const IMCP_CHILD_ENUMERATION_TIMEOUT: Duration = Duration::from_millis(600);
const IMCP_READ_TIMEOUT: Duration = Duration::from_millis(50);
const SETTINGS_FILE_NAME: &str = "manager-state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CommandTransport {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    logs: Vec<ManagerLogEntry>,
    devices: Vec<ManagedDeviceSummary>,
    device_endpoints: Vec<DeviceEndpointConfig>,
    device_role_assignments: Vec<DeviceRoleAssignment>,
    role_mappings: Vec<RoleMappingConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DcsBiosCommandRequest {
    raw_command: Option<String>,
    control_id: Option<String>,
    argument: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
enum DeviceRole {
    LeftDdi,
    RightDdi,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum NormalizedControlEvent {
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
struct DeviceRoleAssignment {
    device_id: String,
    role: DeviceRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DcsBiosMappedAction {
    identifier: String,
    argument: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RoleControlMapping {
    id: String,
    control_id: u16,
    input_event: NormalizedControlEvent,
    action: DcsBiosMappedAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RoleMappingConfig {
    role: DeviceRole,
    mappings: Vec<RoleControlMapping>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedManagerState {
    device_endpoints: Vec<DeviceEndpointConfig>,
    device_role_assignments: Vec<DeviceRoleAssignment>,
    role_mappings: Vec<RoleMappingConfig>,
}

struct ListenerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
struct KnownRuntimeDevice {
    device_id: String,
    device_kind: DeviceKind,
}

#[derive(Debug, Clone)]
struct SinglePacketSource {
    packet: Option<Vec<u8>>,
}

impl SinglePacketSource {
    fn new(packet: Vec<u8>) -> Self {
        Self {
            packet: Some(packet),
        }
    }
}

impl Source for SinglePacketSource {
    fn setup(&self) -> Result<(), dcs_bios::error::Error> {
        Ok(())
    }

    fn read(&mut self) -> Result<Option<Vec<u8>>, dcs_bios::error::Error> {
        Ok(self.packet.take())
    }
}

#[derive(Clone)]
struct SharedMemoryMap {
    inner: Arc<Mutex<VecMemoryMap>>,
}

impl MemoryMap for SharedMemoryMap {
    fn write(
        &mut self,
        address: u16,
        data: &[u8],
    ) -> Result<std::ops::RangeInclusive<u16>, dcs_bios::error::Error> {
        self.inner
            .lock()
            .unwrap()
            .write(address, data)
            .map_err(|_| dcs_bios::error::Error::MemoryMapError())
    }

    fn read(&self, range: std::ops::RangeInclusive<u16>) -> Option<&[u8]> {
        let _ = range;
        None
    }
}

struct RuntimeState {
    config: Mutex<DcsBiosConnectionConfig>,
    status: Mutex<DcsBiosStatus>,
    logs: Mutex<VecDeque<ManagerLogEntry>>,
    devices: Mutex<Vec<ManagedDeviceSummary>>,
    device_endpoints: Mutex<Vec<DeviceEndpointConfig>>,
    device_role_assignments: Mutex<Vec<DeviceRoleAssignment>>,
    role_mappings: Mutex<Vec<RoleMappingConfig>>,
    log_counter: AtomicU64,
    listener: Mutex<Option<ListenerHandle>>,
    endpoint_listeners: Mutex<Vec<ListenerHandle>>,
    dcsbios_memory: Arc<Mutex<VecMemoryMap>>,
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            config: Mutex::new(DcsBiosConnectionConfig::default()),
            status: Mutex::new(DcsBiosStatus::default()),
            logs: Mutex::new(VecDeque::new()),
            devices: Mutex::new(Vec::new()),
            device_endpoints: Mutex::new(Vec::new()),
            device_role_assignments: Mutex::new(Vec::new()),
            role_mappings: Mutex::new(Vec::new()),
            log_counter: AtomicU64::new(0),
            listener: Mutex::new(None),
            endpoint_listeners: Mutex::new(Vec::new()),
            dcsbios_memory: Arc::new(Mutex::new(VecMemoryMap::default())),
        }
    }

    fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            dcsbios_config: self.config.lock().unwrap().clone(),
            dcsbios_status: self.status.lock().unwrap().clone(),
            logs: self.logs.lock().unwrap().iter().cloned().collect(),
            devices: self.devices.lock().unwrap().clone(),
            device_endpoints: self.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: self.device_role_assignments.lock().unwrap().clone(),
            role_mappings: self.role_mappings.lock().unwrap().clone(),
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

    fn set_role_mappings(
        &self,
        app: &AppHandle,
        role_mappings: Vec<RoleMappingConfig>,
    ) -> Vec<RoleMappingConfig> {
        *self.role_mappings.lock().unwrap() = role_mappings.clone();
        let _ = app.emit("role-mappings-changed", role_mappings.clone());
        role_mappings
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

        self.push_log(app, "INFO", "devices", "Stopped device endpoint listeners.");
    }

    fn start_endpoint_listeners(self: &Arc<Self>, app: AppHandle) -> Result<(), String> {
        self.stop_endpoint_listeners(&app);

        let endpoints = sanitize_device_endpoints(self.device_endpoints.lock().unwrap().clone());
        let assignments = self.device_role_assignments.lock().unwrap().clone();
        let role_mappings = self.role_mappings.lock().unwrap().clone();

        let mut listeners = Vec::new();
        for endpoint in endpoints.into_iter().filter(|entry| entry.enabled) {
            let stop = Arc::new(AtomicBool::new(false));
            let stop_for_thread = stop.clone();
            let app_for_thread = app.clone();
            let state = Arc::clone(self);
            let assignments = assignments.clone();
            let role_mappings = role_mappings.clone();

            let join = thread::spawn(move || {
                let state_for_run = state.clone();
                if let Err(error) = run_endpoint_listener(
                    state_for_run,
                    app_for_thread.clone(),
                    endpoint,
                    assignments,
                    role_mappings,
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

        let socket = bind_export_socket(&config).map_err(|error| {
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
            error
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
            let mut last_rate_tick = Instant::now();
            let mut packets_in_window = 0_u32;

            while !stop_for_thread.load(Ordering::Relaxed) {
                match socket.recv(&mut buf) {
                    Ok(size) => {
                        packets_in_window = packets_in_window.saturating_add(1);
                        if let Err(error) =
                            apply_dcsbios_export_packet(dcsbios_memory.clone(), buf[..size].to_vec())
                        {
                            state.push_log(&app_for_thread, "WARN", "dcsbios", error);
                        }
                        let now = now_iso8601();
                        let preview = extract_ascii_preview(&buf[..size]);
                        let maybe_aircraft_name = preview.clone().and_then(extract_aircraft_name);
                        state.update_status(&app_for_thread, |status| {
                            status.connection_state = "receiving".to_string();
                            status.last_seen_at = Some(now.clone());
                            status.last_packet_at = Some(now.clone());
                            status.error = None;
                            status.total_packets = status.total_packets.saturating_add(1);
                            if let Some(name) = &maybe_aircraft_name {
                                status.aircraft_name = Some(name.clone());
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
                            .and_then(|_| status.last_packet_at.as_ref())
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
fn save_device_endpoints(
    app: AppHandle,
    state: State<'_, AppState>,
    device_endpoints: Vec<DeviceEndpointConfig>,
) -> Result<AppSnapshot, String> {
    let device_endpoints = sanitize_device_endpoints(device_endpoints);
    persist_manager_state(
        &app,
        &PersistedManagerState {
            device_endpoints: device_endpoints.clone(),
            device_role_assignments: state.inner.device_role_assignments.lock().unwrap().clone(),
            role_mappings: state.inner.role_mappings.lock().unwrap().clone(),
        },
    )?;
    state.inner.stop_endpoint_listeners(&app);
    state
        .inner
        .set_device_endpoints(&app, device_endpoints);
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
    runtime.restart_endpoint_listeners(&app)?;
    let devices = result?;

    let count_devices = devices.len();
    let devices = runtime.set_devices(&app, devices);
    runtime.push_log(
        &app,
        "INFO",
        "devices",
        format!(
            "Refreshed devices from {count_endpoints} configured endpoint(s). {count_devices} device(s) available."
        ),
    );
    runtime.restart_endpoint_listeners(&app)?;
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
            device_endpoints: state.inner.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: device_role_assignments.clone(),
            role_mappings: state.inner.role_mappings.lock().unwrap().clone(),
        },
    )?;
    state
        .inner
        .set_device_role_assignments(&app, device_role_assignments);
    state.inner.restart_endpoint_listeners(&app)?;
    state.inner.push_log(
        &app,
        "INFO",
        "devices",
        "Saved device role assignments.",
    );
    Ok(state.inner.snapshot())
}

#[tauri::command]
fn save_role_mappings(
    app: AppHandle,
    state: State<'_, AppState>,
    role_mappings: Vec<RoleMappingConfig>,
) -> Result<AppSnapshot, String> {
    let role_mappings = sanitize_role_mappings(role_mappings);
    persist_manager_state(
        &app,
        &PersistedManagerState {
            device_endpoints: state.inner.device_endpoints.lock().unwrap().clone(),
            device_role_assignments: state.inner.device_role_assignments.lock().unwrap().clone(),
            role_mappings: role_mappings.clone(),
        },
    )?;
    state.inner.set_role_mappings(&app, role_mappings);
    state.inner.restart_endpoint_listeners(&app)?;
    state
        .inner
        .push_log(&app, "INFO", "devices", "Saved role control mappings.");
    Ok(state.inner.snapshot())
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

fn sanitize_device_role_assignments(
    device_role_assignments: Vec<DeviceRoleAssignment>,
) -> Vec<DeviceRoleAssignment> {
    let mut seen_roles = HashSet::new();
    let mut seen_devices = HashSet::new();
    let mut sanitized = Vec::new();

    for assignment in device_role_assignments.into_iter().rev() {
        let device_id = assignment.device_id.trim().to_string();
        if device_id.is_empty() {
            continue;
        }

        if !seen_roles.insert(assignment.role) || !seen_devices.insert(device_id.clone()) {
            continue;
        }

        sanitized.push(DeviceRoleAssignment {
            device_id,
            role: assignment.role,
        });
    }

    sanitized.reverse();
    sanitized
}

fn sanitize_role_mappings(role_mappings: Vec<RoleMappingConfig>) -> Vec<RoleMappingConfig> {
    let mut roles = HashSet::new();
    let mut sanitized = Vec::new();

    for config in role_mappings {
        if !roles.insert(config.role) {
            continue;
        }

        let mut seen_bindings = HashSet::new();
        let mappings = config
            .mappings
            .into_iter()
            .filter_map(|mapping| {
                let identifier = mapping.action.identifier.trim().to_string();
                let argument = mapping.action.argument.trim().to_string();
                if identifier.is_empty() || argument.is_empty() {
                    return None;
                }

                if !seen_bindings.insert((mapping.control_id, mapping.input_event)) {
                    return None;
                }

                Some(RoleControlMapping {
                    id: if mapping.id.trim().is_empty() {
                        Uuid::new_v4().to_string()
                    } else {
                        mapping.id
                    },
                    control_id: mapping.control_id,
                    input_event: mapping.input_event,
                    action: DcsBiosMappedAction {
                        identifier,
                        argument,
                    },
                })
            })
            .collect();

        sanitized.push(RoleMappingConfig {
            role: config.role,
            mappings,
        });
    }

    sanitized
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
    let mut port = serialport::new(&endpoint.address, endpoint.baud_rate)
        .timeout(IMCP_READ_TIMEOUT)
        .open()
        .map_err(|error| format!("Failed to open {}: {error}", endpoint.address))?;

    let _ = port.clear(serialport::ClearBuffer::All);

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
                            if let Some(probed) =
                                decode_device_hello(payload.as_slice(), assigned_address)?
                            {
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
                        if let Some(probed) =
                            decode_device_hello(payload.as_slice(), Some(frame.from_address()))?
                        {
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
            let persisted: PersistedManagerState = serde_json::from_str(&contents)
                .map_err(|error| format!("Failed to parse manager state: {error}"))?;
            Ok(PersistedManagerState {
                device_endpoints: sanitize_device_endpoints(persisted.device_endpoints),
                device_role_assignments: sanitize_device_role_assignments(
                    persisted.device_role_assignments,
                ),
                role_mappings: sanitize_role_mappings(persisted.role_mappings),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PersistedManagerState::default())
        }
        Err(error) => Err(format!("Failed to read manager state: {error}")),
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

fn decode_device_hello(
    payload: &[u8],
    assigned_address: Option<u8>,
) -> Result<Option<ProbedImcpDevice>, String> {
    let kind = match decode_set_packet(payload) {
        Ok(kind) => kind,
        Err(_) => return Ok(None),
    };

    let AppPacketKind::DeviceHello(hello) = kind else {
        return Ok(None);
    };

    let assigned_address = assigned_address
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

fn find_role_for_device(
    device_role_assignments: &[DeviceRoleAssignment],
    device_id: &str,
) -> Option<DeviceRole> {
    device_role_assignments
        .iter()
        .find(|assignment| assignment.device_id == device_id)
        .map(|assignment| assignment.role)
}

fn find_mapping_action(
    role_mappings: &[RoleMappingConfig],
    role: DeviceRole,
    control_id: u16,
    input_event: NormalizedControlEvent,
) -> Option<DcsBiosMappedAction> {
    role_mappings
        .iter()
        .find(|config| config.role == role)
        .and_then(|config| {
            config
                .mappings
                .iter()
                .find(|mapping| {
                    mapping.control_id == control_id && mapping.input_event == input_event
                })
                .map(|mapping| mapping.action.clone())
        })
}

fn control_supported_events(
    device_kind: DeviceKind,
    control_id: u16,
) -> Option<&'static [NormalizedControlEvent]> {
    match device_kind {
        DeviceKind::UpperPanelDdi if control_id < 40 => Some(&[
            NormalizedControlEvent::ButtonDown,
            NormalizedControlEvent::ButtonUp,
            NormalizedControlEvent::ButtonPushed,
        ]),
        _ => None,
    }
}

fn apply_dcsbios_export_packet(
    memory_map: Arc<Mutex<VecMemoryMap>>,
    packet: Vec<u8>,
) -> Result<(), String> {
    let mut reader = DcsBiosImpl::new(
        SinglePacketSource::new(packet),
        SharedMemoryMap { inner: memory_map },
    );
    reader
        .read_packet()
        .map(|_| ())
        .map_err(|error| format!("Failed to decode DCS-BIOS export packet: {error:?}"))
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

fn process_control_event(
    state: &Arc<RuntimeState>,
    app: &AppHandle,
    config: &DcsBiosConnectionConfig,
    known_devices: &HashMap<u8, KnownRuntimeDevice>,
    pressed_buttons: &mut HashSet<(String, u16)>,
    device_role_assignments: &[DeviceRoleAssignment],
    role_mappings: &[RoleMappingConfig],
    source_address: u8,
    control_event: &ControlEvent,
) {
    let Some(device) = known_devices.get(&source_address) else {
        state.push_log(
            app,
            "WARN",
            "devices",
            format!(
                "Ignoring control event from unknown IMCP address {} on control {}.",
                source_address, control_event.control_id
            ),
        );
        return;
    };

    if control_supported_events(device.device_kind, control_event.control_id).is_none() {
        state.push_log(
            app,
            "WARN",
            "devices",
            format!(
                "Ignoring control {} from unsupported catalog device {}.",
                control_event.control_id, device.device_id
            ),
        );
        return;
    }

    let Some(role) = find_role_for_device(device_role_assignments, &device.device_id) else {
        state.push_log(
            app,
            "WARN",
            "devices",
            format!(
                "Ignoring control event for unassigned device {}.",
                device.device_id
            ),
        );
        return;
    };

    let mut events = Vec::new();
    match control_event.event {
        ControlValue::Button { pressed: true } => {
            pressed_buttons.insert((device.device_id.clone(), control_event.control_id));
            events.push(NormalizedControlEvent::ButtonDown);
        }
        ControlValue::Button { pressed: false } => {
            events.push(NormalizedControlEvent::ButtonUp);
            if pressed_buttons.remove(&(device.device_id.clone(), control_event.control_id)) {
                events.push(NormalizedControlEvent::ButtonPushed);
            }
        }
        ControlValue::EncoderDelta { .. } => events.push(NormalizedControlEvent::EncoderDelta),
        ControlValue::Absolute { .. } => events.push(NormalizedControlEvent::AbsoluteChanged),
        ControlValue::Toggle { state: true } => events.push(NormalizedControlEvent::ToggleOn),
        ControlValue::Toggle { state: false } => events.push(NormalizedControlEvent::ToggleOff),
        ControlValue::RequestDeviceHello => {}
    }

    for input_event in events {
        let Some(action) =
            find_mapping_action(role_mappings, role, control_event.control_id, input_event)
        else {
            continue;
        };

        match encode_import_command(&action.identifier, &action.argument)
            .and_then(|payload| send_command_to_dcsbios(config, &payload))
        {
            Ok(()) => state.push_log(
                app,
                "SUCCESS",
                "mapping",
                format!(
                    "Mapped {:?} control {} {:?} -> {} {}",
                    role,
                    control_event.control_id,
                    input_event,
                    action.identifier,
                    action.argument
                ),
            ),
            Err(error) => state.push_log(
                app,
                "ERROR",
                "mapping",
                format!(
                    "Failed to send mapped DCS-BIOS command for device {}: {}",
                    device.device_id, error
                ),
            ),
        }
    }
}

fn run_endpoint_listener(
    state: Arc<RuntimeState>,
    app: AppHandle,
    endpoint: DeviceEndpointConfig,
    device_role_assignments: Vec<DeviceRoleAssignment>,
    role_mappings: Vec<RoleMappingConfig>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let config = state.config.lock().unwrap().clone();
    let mut port = serialport::new(&endpoint.address, endpoint.baud_rate)
        .timeout(IMCP_READ_TIMEOUT)
        .open()
        .map_err(|error| format!("Failed to open endpoint listener {}: {error}", endpoint.address))?;
    let _ = port.clear(serialport::ClearBuffer::All);

    let mut serial_buffer = [0u8; 64];
    let mut rx_buffer = [0u8; 256];
    let mut frame_buffer = [0u8; 256];
    let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
    let mut join_addresses: HashMap<u32, u8> = HashMap::new();
    let mut next_address: u8 = 0x02;
    let mut known_devices: HashMap<u8, KnownRuntimeDevice> = HashMap::new();
    let mut requested_children = HashSet::new();
    let mut pressed_buttons: HashSet<(String, u16)> = HashSet::new();

    state.push_log(
        &app,
        "INFO",
        "devices",
        format!("Listening for HCP events on {}.", endpoint.address),
    );

    while !stop.load(Ordering::Relaxed) {
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
                        FramePayload::Join(join_id) => {
                            let address = *join_addresses.entry(*join_id).or_insert_with(|| {
                                let current = next_address;
                                next_address = next_address.saturating_add(1);
                                current
                            });
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
                        }
                        FramePayload::Set(payload) => {
                            if let Some(probed) =
                                decode_device_hello(payload.as_slice(), Some(frame.from_address()))?
                            {
                                write_frame(
                                    &mut *port,
                                    &Frame::new(
                                        Address::Unicast(frame.from_address()),
                                        IMCP_MASTER_ADDRESS,
                                        FramePayload::Ack(frame.to_address().as_byte()),
                                    ),
                                )?;

                                let source_address = frame.from_address();
                                known_devices.insert(
                                    source_address,
                                    KnownRuntimeDevice {
                                        device_id: probed.device_id.clone(),
                                        device_kind: probed.device_kind,
                                    },
                                );

                                if probed.device_kind == DeviceKind::ImcpHub
                                    && requested_children.insert(source_address)
                                {
                                    request_child_device_hello(&mut *port, source_address)?;
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
                                process_control_event(
                                    &state,
                                    &app,
                                    &config,
                                    &known_devices,
                                    &mut pressed_buttons,
                                    &device_role_assignments,
                                    &role_mappings,
                                    frame.from_address(),
                                    &control_event,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
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

fn extract_aircraft_name(preview: String) -> Option<String> {
    let trimmed = preview.trim();
    if trimmed.len() < 3 || trimmed.len() > 32 {
        return None;
    }

    let allowed = trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '/' | '.'));

    if allowed && trimmed.chars().any(|ch| ch.is_ascii_alphabetic()) {
        Some(trimmed.to_string())
    } else {
        None
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
                    state.set_device_endpoints(&app_handle, manager_state.device_endpoints.clone());
                    state.set_device_role_assignments(
                        &app_handle,
                        manager_state.device_role_assignments.clone(),
                    );
                    state.set_role_mappings(&app_handle, manager_state.role_mappings.clone());
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
                    }
                    if let Err(error) = state.restart_endpoint_listeners(&app_handle) {
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
            save_device_endpoints,
            save_device_role_assignments,
            save_role_mappings,
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
    fn sanitize_role_assignments_keeps_one_device_per_role() {
        let assignments = sanitize_device_role_assignments(vec![
            DeviceRoleAssignment {
                device_id: "A".to_string(),
                role: DeviceRole::LeftDdi,
            },
            DeviceRoleAssignment {
                device_id: "B".to_string(),
                role: DeviceRole::LeftDdi,
            },
        ]);

        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].device_id, "B");
    }

    #[test]
    fn sanitize_role_mappings_deduplicates_control_event_bindings() {
        let mappings = sanitize_role_mappings(vec![RoleMappingConfig {
            role: DeviceRole::LeftDdi,
            mappings: vec![
                RoleControlMapping {
                    id: String::new(),
                    control_id: 3,
                    input_event: NormalizedControlEvent::ButtonPushed,
                    action: DcsBiosMappedAction {
                        identifier: "AAA".to_string(),
                        argument: "1".to_string(),
                    },
                },
                RoleControlMapping {
                    id: String::new(),
                    control_id: 3,
                    input_event: NormalizedControlEvent::ButtonPushed,
                    action: DcsBiosMappedAction {
                        identifier: "BBB".to_string(),
                        argument: "2".to_string(),
                    },
                },
            ],
        }]);

        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].mappings.len(), 1);
        assert_eq!(mappings[0].mappings[0].action.identifier, "AAA");
    }

    #[test]
    fn find_mapping_action_matches_role_control_and_event() {
        let action = find_mapping_action(
            &[RoleMappingConfig {
                role: DeviceRole::RightDdi,
                mappings: vec![RoleControlMapping {
                    id: "1".to_string(),
                    control_id: 7,
                    input_event: NormalizedControlEvent::ButtonDown,
                    action: DcsBiosMappedAction {
                        identifier: "MASTER_ARM".to_string(),
                        argument: "1".to_string(),
                    },
                }],
            }],
            DeviceRole::RightDdi,
            7,
            NormalizedControlEvent::ButtonDown,
        )
        .expect("action");

        assert_eq!(action.identifier, "MASTER_ARM");
    }

    #[test]
    fn import_command_rejects_invalid_identifier() {
        let error = encode_import_command("BAD IDENT", "1").expect_err("must fail");
        assert!(error.contains("Invalid DCS-BIOS command"));
    }

    #[test]
    fn dcsbios_export_packet_updates_memory_map() {
        let memory = Arc::new(Mutex::new(VecMemoryMap::default()));
        let packet = vec![0x55, 0x55, 0x55, 0x55, 0x00, 0x10, 0x02, 0x00, 0x34, 0x12];

        apply_dcsbios_export_packet(memory.clone(), packet).expect("packet must decode");

        let binding = memory.lock().unwrap();
        let bytes = binding
            .read(0x1000..=0x1001)
            .expect("bytes must exist");
        assert_eq!(bytes, &[0x34, 0x12]);
    }

    #[test]
    fn button_release_after_press_generates_pushed_event() {
        let mut pressed_buttons = HashSet::new();
        let device_id = "DEVICE-1".to_string();

        pressed_buttons.insert((device_id.clone(), 5));
        let released = pressed_buttons.remove(&(device_id, 5));

        assert!(released);
        assert_eq!(
            [
                NormalizedControlEvent::ButtonUp,
                NormalizedControlEvent::ButtonPushed
            ],
            [
                NormalizedControlEvent::ButtonUp,
                NormalizedControlEvent::ButtonPushed
            ]
        );
    }
}
