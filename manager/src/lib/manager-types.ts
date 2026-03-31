export type CommandTransport = "udp" | "tcp";

export type DcsBiosConnectionConfig = {
  exportHost: string;
  exportPort: number;
  commandHost: string;
  commandPort: number;
  commandTransport: CommandTransport;
};

export type DcsBiosStatus = {
  connectionState: string;
  lastSeenAt: string | null;
  lastPacketAt: string | null;
  packetsPerSecond: number;
  totalPackets: number;
  aircraftName: string | null;
  error: string | null;
  diagnostics: string[];
};

export type ManagerLogEntry = {
  id: number;
  at: string;
  level: "INFO" | "WARN" | "ERROR" | "SUCCESS" | string;
  source: string;
  message: string;
};

export type ImcpDeviceSummary = {
  id: string;
  transport: string;
  portName: string;
  displayName: string;
  firmwareVersion: string | null;
  state: string;
};

export type AppSnapshot = {
  dcsbiosConfig: DcsBiosConnectionConfig;
  dcsbiosStatus: DcsBiosStatus;
  logs: ManagerLogEntry[];
  devices: ImcpDeviceSummary[];
};

export type DcsBiosCommandRequest = {
  rawCommand?: string | null;
  controlId?: string | null;
  argument?: string | null;
};

export const defaultSnapshot: AppSnapshot = {
  dcsbiosConfig: {
    exportHost: "239.255.50.10",
    exportPort: 5010,
    commandHost: "127.0.0.1",
    commandPort: 7778,
    commandTransport: "udp",
  },
  dcsbiosStatus: {
    connectionState: "stopped",
    lastSeenAt: null,
    lastPacketAt: null,
    packetsPerSecond: 0,
    totalPackets: 0,
    aircraftName: null,
    error: null,
    diagnostics: ["DCS-BIOS listener is stopped."],
  },
  logs: [],
  devices: [],
};
