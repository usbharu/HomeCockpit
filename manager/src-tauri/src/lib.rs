use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use chrono::Utc;
use hcp::{decode_set_packet, AppPacketKind, DeviceKind};
use imcp::{
    frame::{Address, Frame, FramePayload, MAX_ENCODED_FRAME_SIZE},
    parser::FrameParser,
};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tauri::{AppHandle, Emitter, Manager, State};

const DEFAULT_EXPORT_HOST: &str = "239.255.50.10";
const DEFAULT_EXPORT_PORT: u16 = 5010;
const DEFAULT_COMMAND_HOST: &str = "127.0.0.1";
const DEFAULT_COMMAND_PORT: u16 = 7778;
const MAX_LOG_ENTRIES: usize = 250;
const IMCP_PROBE_BAUD_RATE: u32 = 115200;
const IMCP_MASTER_ADDRESS: u8 = 0x01;
const IMCP_PROBE_TIMEOUT: Duration = Duration::from_millis(900);
const IMCP_READ_TIMEOUT: Duration = Duration::from_millis(50);

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImcpDeviceSummary {
    id: String,
    transport: String,
    port_name: String,
    display_name: String,
    firmware_version: Option<String>,
    state: String,
    protocol: String,
    assigned_address: Option<u8>,
    device_kind: Option<String>,
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
    devices: Vec<ImcpDeviceSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DcsBiosCommandRequest {
    raw_command: Option<String>,
    control_id: Option<String>,
    argument: Option<String>,
}

struct ListenerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

struct RuntimeState {
    config: Mutex<DcsBiosConnectionConfig>,
    status: Mutex<DcsBiosStatus>,
    logs: Mutex<VecDeque<ManagerLogEntry>>,
    devices: Mutex<Vec<ImcpDeviceSummary>>,
    log_counter: AtomicU64,
    listener: Mutex<Option<ListenerHandle>>,
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            config: Mutex::new(DcsBiosConnectionConfig::default()),
            status: Mutex::new(DcsBiosStatus::default()),
            logs: Mutex::new(VecDeque::new()),
            devices: Mutex::new(Vec::new()),
            log_counter: AtomicU64::new(0),
            listener: Mutex::new(None),
        }
    }

    fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            dcsbios_config: self.config.lock().unwrap().clone(),
            dcsbios_status: self.status.lock().unwrap().clone(),
            logs: self.logs.lock().unwrap().iter().cloned().collect(),
            devices: self.devices.lock().unwrap().clone(),
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
        devices: Vec<ImcpDeviceSummary>,
    ) -> Vec<ImcpDeviceSummary> {
        *self.devices.lock().unwrap() = devices.clone();
        let _ = app.emit("imcp-devices-changed", devices.clone());
        devices
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
fn list_imcp_devices(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ImcpDeviceSummary>, String> {
    let ports = serialport::available_ports().map_err(|error| error.to_string())?;
    let scanned_ports = ports.len();
    let devices = ports
        .into_iter()
        .filter_map(|port| {
            let probed = match probe_imcp_device(&port.port_name) {
                Ok(Some(probed)) => probed,
                Ok(None) | Err(_) => return None,
            };

            Some(ImcpDeviceSummary {
                id: port.port_name.clone(),
                transport: "serial".to_string(),
                port_name: port.port_name,
                display_name: probed.display_name,
                firmware_version: Some(probed.firmware_version),
                state: "connected".to_string(),
                protocol: "imcp+hcp".to_string(),
                assigned_address: Some(probed.assigned_address),
                device_kind: Some(probed.device_kind),
                protocol_version: Some(probed.protocol_version),
                device_id: Some(probed.device_id),
                displays: Some(probed.displays),
                controls: Some(probed.controls),
                features: Some(probed.features),
            })
        })
        .collect::<Vec<_>>();

    let count = devices.len();
    let devices = state.inner.set_devices(&app, devices);
    state.inner.push_log(
        &app,
        "INFO",
        "imcp",
        format!(
            "Refreshed IMCP/HCP devices. {count} device(s) responded across {scanned_ports} scanned port(s)."
        ),
    );
    Ok(devices)
}

#[derive(Debug, Clone)]
struct ProbedImcpDevice {
    display_name: String,
    firmware_version: String,
    assigned_address: u8,
    device_kind: String,
    protocol_version: u8,
    device_id: String,
    displays: u8,
    controls: u16,
    features: String,
}

fn probe_imcp_device(port_name: &str) -> Result<Option<ProbedImcpDevice>, String> {
    let mut port = serialport::new(port_name, IMCP_PROBE_BAUD_RATE)
        .timeout(IMCP_READ_TIMEOUT)
        .open()
        .map_err(|error| format!("Failed to open {port_name}: {error}"))?;

    let _ = port.clear(serialport::ClearBuffer::All);

    let started_at = Instant::now();
    let mut serial_buffer = [0u8; 64];
    let mut rx_buffer = [0u8; 256];
    let mut frame_buffer = [0u8; 256];
    let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
    let mut assigned_address: Option<u8> = None;

    while started_at.elapsed() < IMCP_PROBE_TIMEOUT {
        match port.read(&mut serial_buffer) {
            Ok(bytes_read) if bytes_read > 0 => {
                parser
                    .write_data(&serial_buffer[..bytes_read])
                    .map_err(|error| {
                        format!("Failed to parse IMCP frame on {port_name}: {error:?}")
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
                                return Ok(Some(probed));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => return Err(format!("Failed to read {port_name}: {error}")),
        }
    }

    Ok(None)
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
        assigned_address,
        device_kind: format_device_kind(hello.device_kind).to_string(),
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
        DeviceKind::Unknown(_) => "Unknown Device",
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

fn normalize_command_request(request: DcsBiosCommandRequest) -> Result<String, String> {
    let raw = request.raw_command.unwrap_or_default().trim().to_string();
    let payload = if !raw.is_empty() {
        raw
    } else {
        let control_id = request.control_id.unwrap_or_default().trim().to_string();
        let argument = request.argument.unwrap_or_default().trim().to_string();

        if control_id.is_empty() || argument.is_empty() {
            return Err("Provide either rawCommand or both controlId and argument.".to_string());
        }

        format!("{control_id} {argument}")
    };

    Ok(format!("{}\n", payload.trim_end_matches('\n')))
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

            match list_imcp_devices(app_handle.clone(), app.state::<AppState>()) {
                Ok(_) => {}
                Err(error) => state.push_log(&app_handle, "WARN", "imcp", error),
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
            list_imcp_devices
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
}
