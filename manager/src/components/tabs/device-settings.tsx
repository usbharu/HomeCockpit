"use client";

import { useEffect, useMemo, useState } from "react";
import { Cable, Cpu, Plus, RefreshCw, Trash2, Waypoints } from "lucide-react";

import { deviceRoleLabels } from "@/lib/control-catalog";
import type {
  DeviceRole,
  DeviceRoleAssignment,
  DeviceEndpointConfig,
  EndpointRoleHint,
  ManagedDeviceSummary,
} from "@/lib/manager-types";

type DeviceSettingsProps = {
  devices: ManagedDeviceSummary[];
  deviceEndpoints: DeviceEndpointConfig[];
  deviceRoleAssignments: DeviceRoleAssignment[];
  serialPorts: string[];
  busyAction: string | null;
  onRefresh: () => Promise<void>;
  onSaveEndpoints: (deviceEndpoints: DeviceEndpointConfig[]) => Promise<void>;
  onSaveDeviceRoleAssignments: (deviceRoleAssignments: DeviceRoleAssignment[]) => Promise<void>;
};

type EndpointDraft = DeviceEndpointConfig;

const defaultDraft = (): EndpointDraft => ({
  id: "",
  name: "",
  transport: "serial",
  address: "",
  enabled: true,
  baudRate: 115200,
  roleHint: "auto",
});

const roleHintLabels: Record<EndpointRoleHint, string> = {
  auto: "自動",
  "direct-device": "直結デバイス",
  "imcp-hub": "IMCP Hub",
};

const baudRateOptions = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

const DeviceSettings = ({
  devices,
  deviceEndpoints,
  deviceRoleAssignments,
  serialPorts,
  busyAction,
  onRefresh,
  onSaveEndpoints,
  onSaveDeviceRoleAssignments,
}: DeviceSettingsProps) => {
  const [draftEndpoints, setDraftEndpoints] = useState<DeviceEndpointConfig[]>(deviceEndpoints);
  const [newEndpoint, setNewEndpoint] = useState<EndpointDraft>(defaultDraft);

  useEffect(() => {
    setDraftEndpoints(deviceEndpoints);
  }, [deviceEndpoints]);

  const hasUnsavedChanges = useMemo(
    () => JSON.stringify(draftEndpoints) !== JSON.stringify(deviceEndpoints),
    [deviceEndpoints, draftEndpoints],
  );

  useEffect(() => {
    if (!hasUnsavedChanges) {
      return;
    }

    const timeoutId = window.setTimeout(() => {
      void onSaveEndpoints(draftEndpoints);
    }, 400);

    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [draftEndpoints, hasUnsavedChanges, onSaveEndpoints]);

  const updateEndpoint = (
    endpointId: string,
    updater: (endpoint: DeviceEndpointConfig) => DeviceEndpointConfig,
  ) => {
    setDraftEndpoints((current) =>
      current.map((endpoint) => (endpoint.id === endpointId ? updater(endpoint) : endpoint)),
    );
  };

  const removeEndpoint = (endpointId: string) => {
    setDraftEndpoints((current) => current.filter((endpoint) => endpoint.id !== endpointId));
  };

  const assignRole = async (deviceId: string | null, role: DeviceRole | "") => {
    if (!deviceId) {
      return;
    }

    const nextAssignments = deviceRoleAssignments.filter(
      (entry) => entry.deviceId !== deviceId && entry.role !== role,
    );

    if (role) {
      nextAssignments.push({ deviceId, role });
    }

    await onSaveDeviceRoleAssignments(nextAssignments);
  };

  const addEndpoint = () => {
    if (!newEndpoint.name.trim() || !newEndpoint.address.trim()) {
      return;
    }

    setDraftEndpoints((current) => [
      ...current,
      {
        ...newEndpoint,
        id: crypto.randomUUID(),
        name: newEndpoint.name.trim(),
        address: newEndpoint.address.trim(),
      },
    ]);
    setNewEndpoint(defaultDraft());
  };

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
        <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
          <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
            <div>
              <h2 className="text-2xl font-semibold text-gray-800">デバイス接続先</h2>
              <p className="mt-1 text-sm text-gray-500">
                COM ポートを自動走査せず、ここで登録した endpoint だけを IMCP/HCP デバイス探索対象にします。
              </p>
              <p className="mt-1 text-xs text-gray-400">変更は自動保存されます。</p>
            </div>

            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                onClick={() => void onRefresh()}
                disabled={busyAction !== null}
                className="inline-flex items-center gap-2 rounded-md bg-blue-600 px-5 py-2.5 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
              >
                <RefreshCw size={16} />
                再読込
              </button>
            </div>
          </div>

          <div className="mt-6 grid gap-4 border-t border-gray-200 pt-6 lg:grid-cols-[minmax(0,1fr)_280px_160px_160px_140px_120px]">
            <label className="flex flex-col gap-2 text-sm text-gray-700">
              <span>表示名</span>
              <input
                value={newEndpoint.name}
                onChange={(event) =>
                  setNewEndpoint((current) => ({ ...current, name: event.target.value }))
                }
                placeholder="Upper hub"
                className="rounded-md border border-gray-300 px-3 py-2 outline-none transition focus:border-blue-500"
              />
            </label>
            <label className="flex flex-col gap-2 text-sm text-gray-700">
              <span>COM ポート</span>
              <select
                value={newEndpoint.address}
                onChange={(event) =>
                  setNewEndpoint((current) => ({ ...current, address: event.target.value }))
                }
                className="rounded-md border border-gray-300 px-3 py-2 outline-none transition focus:border-blue-500"
              >
                <option value="">選択してください</option>
                {serialPorts.map((port) => (
                  <option key={port} value={port}>
                    {port}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-2 text-sm text-gray-700">
              <span>Baud Rate</span>
              <select
                value={newEndpoint.baudRate}
                onChange={(event) =>
                  setNewEndpoint((current) => ({
                    ...current,
                    baudRate: Number(event.target.value) || 115200,
                  }))
                }
                className="rounded-md border border-gray-300 px-3 py-2 outline-none transition focus:border-blue-500"
              >
                {baudRateOptions.map((baudRate) => (
                  <option key={baudRate} value={baudRate}>
                    {baudRate}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-2 text-sm text-gray-700">
              <span>Role Hint</span>
              <select
                value={newEndpoint.roleHint}
                onChange={(event) =>
                  setNewEndpoint((current) => ({
                    ...current,
                    roleHint: event.target.value as EndpointRoleHint,
                  }))
                }
                className="rounded-md border border-gray-300 px-3 py-2 outline-none transition focus:border-blue-500"
              >
                <option value="auto">自動</option>
                <option value="direct-device">直結デバイス</option>
                <option value="imcp-hub">IMCP Hub</option>
              </select>
            </label>
            <label className="flex items-end gap-2 pb-2 text-sm text-gray-700">
              <input
                type="checkbox"
                checked={newEndpoint.enabled}
                onChange={(event) =>
                  setNewEndpoint((current) => ({ ...current, enabled: event.target.checked }))
                }
              />
              有効
            </label>
            <button
              type="button"
              onClick={addEndpoint}
              className="inline-flex h-11 items-center justify-center gap-2 self-end rounded-md border border-dashed border-blue-300 bg-blue-50 px-4 text-sm font-medium text-blue-700 transition hover:bg-blue-100"
            >
              <Plus size={16} />
              追加
            </button>
          </div>

          <div className="mt-6 space-y-3">
            {draftEndpoints.length === 0 ? (
              <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-6 text-sm text-gray-500">
                まだ endpoint がありません。COM ポートを追加してから保存してください。
              </div>
            ) : (
              draftEndpoints.map((endpoint) => (
                <div
                  key={endpoint.id}
                  className="grid gap-4 rounded-lg border border-gray-200 bg-gray-50 p-4 lg:grid-cols-[minmax(0,1fr)_280px_160px_160px_120px_48px]"
                >
                  <input
                    value={endpoint.name}
                    onChange={(event) =>
                      updateEndpoint(endpoint.id, (current) => ({
                        ...current,
                        name: event.target.value,
                      }))
                    }
                    className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm outline-none transition focus:border-blue-500"
                  />
                  <select
                    value={endpoint.address}
                    onChange={(event) =>
                      updateEndpoint(endpoint.id, (current) => ({
                        ...current,
                        address: event.target.value,
                      }))
                    }
                    className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm outline-none transition focus:border-blue-500"
                  >
                    <option value="">選択してください</option>
                    {serialPorts.map((port) => (
                      <option key={port} value={port}>
                        {port}
                      </option>
                    ))}
                  </select>
                  <select
                    value={endpoint.baudRate}
                    onChange={(event) =>
                      updateEndpoint(endpoint.id, (current) => ({
                        ...current,
                        baudRate: Number(event.target.value) || 115200,
                      }))
                    }
                    className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm outline-none transition focus:border-blue-500"
                  >
                    {baudRateOptions.map((baudRate) => (
                      <option key={baudRate} value={baudRate}>
                        {baudRate}
                      </option>
                    ))}
                  </select>
                  <select
                    value={endpoint.roleHint}
                    onChange={(event) =>
                      updateEndpoint(endpoint.id, (current) => ({
                        ...current,
                        roleHint: event.target.value as EndpointRoleHint,
                      }))
                    }
                    className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm outline-none transition focus:border-blue-500"
                  >
                    <option value="auto">自動</option>
                    <option value="direct-device">直結デバイス</option>
                    <option value="imcp-hub">IMCP Hub</option>
                  </select>
                  <label className="flex items-center gap-2 rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">
                    <input
                      type="checkbox"
                      checked={endpoint.enabled}
                      onChange={(event) =>
                        updateEndpoint(endpoint.id, (current) => ({
                          ...current,
                          enabled: event.target.checked,
                        }))
                      }
                    />
                    有効
                  </label>
                  <button
                    type="button"
                    onClick={() => removeEndpoint(endpoint.id)}
                    className="inline-flex h-10 items-center justify-center rounded-md border border-red-200 bg-red-50 text-red-700 transition hover:bg-red-100"
                    aria-label={`${endpoint.name} を削除`}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              ))
            )}
          </div>
        </section>

        <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
          <div className="flex items-center justify-between gap-4">
            <div>
              <h3 className="text-xl font-semibold text-gray-800">検出されたデバイス</h3>
              <p className="mt-1 text-sm text-gray-500">
                保存済み endpoint に対して応答した直結デバイスと IMCP Hub 配下デバイスを表示します。
              </p>
            </div>
            <div className="rounded-full border border-gray-200 bg-gray-100 px-4 py-2 text-sm text-gray-700">
              {devices.length} device(s)
            </div>
          </div>

          <div className="mt-6 grid gap-4 md:grid-cols-2 xl:grid-cols-3">
            {devices.length === 0 ? (
              <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-8 text-sm text-gray-500">
                登録済み endpoint から応答した IMCP/HCP デバイスはありません。
              </div>
            ) : (
              devices.map((device) => (
                <section
                  key={device.id}
                  className="rounded-lg border border-gray-200 bg-white p-6 text-gray-900 shadow-sm"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="flex items-center gap-3">
                      <div className="rounded-lg bg-gray-100 p-3 text-blue-600">
                        {device.connectionKind === "hub-child" ? (
                          <Waypoints size={20} />
                        ) : (
                          <Cpu size={20} />
                        )}
                      </div>
                      <div>
                        <h4 className="font-semibold text-gray-800">{device.displayName}</h4>
                        <p className="text-sm text-gray-500">
                          {device.endpointName} · {device.endpointAddress}
                        </p>
                        {device.gatewayDisplayName && (
                          <p className="mt-1 text-xs font-medium text-blue-600">
                            Via {device.gatewayDisplayName}
                          </p>
                        )}
                      </div>
                    </div>
                    <span className="rounded-full border border-gray-200 bg-gray-100 px-3 py-1 text-xs text-gray-700">
                      {device.state}
                    </span>
                  </div>

                  <div className="mt-5 grid gap-3 text-sm text-gray-700">
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Connection</span>
                      <span className="capitalize">{device.connectionKind}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Protocol</span>
                      <span className="inline-flex items-center gap-2">
                        <Cable size={14} />
                        {device.protocol}
                      </span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Transport</span>
                      <span>{device.endpointTransport}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Role Hint</span>
                      <span>{roleHintLabels[(draftEndpoints.find((entry) => entry.id === device.endpointId)?.roleHint ?? "auto") as EndpointRoleHint]}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Identifier</span>
                      <span className="truncate pl-4 text-right">{device.id}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Device Kind</span>
                      <span>{device.deviceKind ?? "Unknown"}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Device ID</span>
                      <span className="truncate pl-4 text-right">{device.deviceId ?? "Unknown"}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Device Role</span>
                      <select
                        value={
                          deviceRoleAssignments.find((entry) => entry.deviceId === device.deviceId)
                            ?.role ?? ""
                        }
                        onChange={(event) =>
                          void assignRole(
                            device.deviceId,
                            (event.target.value || "") as DeviceRole | "",
                          )
                        }
                        className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm outline-none transition focus:border-blue-500"
                      >
                        <option value="">未割当</option>
                        {Object.entries(deviceRoleLabels).map(([role, label]) => (
                          <option key={role} value={role}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Firmware</span>
                      <span>{device.firmwareVersion ?? "Unknown"}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>IMCP Address</span>
                      <span>{device.assignedAddress ?? "N/A"}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>HCP Version</span>
                      <span>{device.protocolVersion ?? "N/A"}</span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Capabilities</span>
                      <span className="pl-4 text-right">
                        {device.displays !== null && device.controls !== null
                          ? `${device.displays} displays / ${device.controls} controls`
                          : "Unknown"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                      <span>Features</span>
                      <span className="truncate pl-4 text-right">{device.features ?? "Unknown"}</span>
                    </div>
                  </div>
                </section>
              ))
            )}
          </div>
        </section>
      </div>
    </div>
  );
};

export default DeviceSettings;
