"use client";

import { useMemo, useState } from "react";
import { ArrowRightLeft, ChevronRight, Plus, Server, Trash2 } from "lucide-react";

import { deviceRoleLabels, getControlCatalog } from "@/lib/control-catalog";
import type {
  DeviceRole,
  DeviceRoleAssignment,
  ManagedDeviceSummary,
  NormalizedControlEvent,
  RoleControlMapping,
  RoleMappingConfig,
} from "@/lib/manager-types";

type MappingSettingsProps = {
  devices: ManagedDeviceSummary[];
  deviceRoleAssignments: DeviceRoleAssignment[];
  roleMappings: RoleMappingConfig[];
  busyAction: string | null;
  onSaveRoleMappings: (roleMappings: RoleMappingConfig[]) => Promise<void>;
};

const roleOrder: DeviceRole[] = ["left-ddi", "right-ddi"];

const emptyDraft = () => ({
  controlId: "",
  inputEvent: "BUTTON_PUSHED" as NormalizedControlEvent,
  identifier: "",
  argument: "",
});

export function MappingSettings({
  devices,
  deviceRoleAssignments,
  roleMappings,
  busyAction,
  onSaveRoleMappings,
}: MappingSettingsProps) {
  const [selectedRole, setSelectedRole] = useState<DeviceRole>("left-ddi");
  const [draft, setDraft] = useState(emptyDraft);

  const assignedDevice = useMemo(() => {
    const assignment = deviceRoleAssignments.find((entry) => entry.role === selectedRole);
    if (!assignment) {
      return null;
    }

    return devices.find((device) => device.deviceId === assignment.deviceId) ?? null;
  }, [deviceRoleAssignments, devices, selectedRole]);

  const availableControls = useMemo(
    () => getControlCatalog(assignedDevice?.deviceKindId ?? null),
    [assignedDevice?.deviceKindId],
  );

  const roleMapping = roleMappings.find((entry) => entry.role === selectedRole);
  const mappings = roleMapping?.mappings ?? [];
  const selectedControl = availableControls.find(
    (control) => control.controlId === Number(draft.controlId),
  );

  const saveMappingsForRole = async (nextMappings: RoleControlMapping[]) => {
    const remaining = roleMappings.filter((entry) => entry.role !== selectedRole);
    await onSaveRoleMappings([...remaining, { role: selectedRole, mappings: nextMappings }]);
  };

  const handleAddMapping = async () => {
    const controlId = Number(draft.controlId);
    if (!Number.isInteger(controlId) || !draft.identifier.trim() || !draft.argument.trim()) {
      return;
    }

    const nextMappings = [
      ...mappings.filter(
        (entry) => !(entry.controlId === controlId && entry.inputEvent === draft.inputEvent),
      ),
      {
        id: crypto.randomUUID(),
        controlId,
        inputEvent: draft.inputEvent,
        action: {
          identifier: draft.identifier.trim(),
          argument: draft.argument.trim(),
        },
      },
    ];

    await saveMappingsForRole(nextMappings);
    setDraft(emptyDraft());
  };

  const handleDeleteMapping = async (mappingId: string) => {
    await saveMappingsForRole(mappings.filter((entry) => entry.id !== mappingId));
  };

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
        <div className="grid gap-6 xl:grid-cols-[340px_minmax(0,1fr)]">
          <aside className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
            <div>
              <p className="text-sm font-medium text-gray-500">設定対象の DeviceRole</p>
              <h3 className="mt-1 text-xl font-semibold text-gray-900">ロール一覧</h3>
            </div>

            <div className="mt-5 space-y-3">
              {roleOrder.map((role) => {
                const assignment = deviceRoleAssignments.find((entry) => entry.role === role);
                const device = devices.find((entry) => entry.deviceId === assignment?.deviceId) ?? null;
                const isSelected = role === selectedRole;

                return (
                  <button
                    key={role}
                    type="button"
                    onClick={() => setSelectedRole(role)}
                    className={`w-full rounded-lg border p-4 text-left transition ${
                      isSelected
                        ? "border-blue-200 bg-blue-50"
                        : "border-gray-200 bg-white hover:border-gray-300 hover:bg-gray-50"
                    }`}
                  >
                    <div className="flex items-start gap-3">
                      <div className="mt-0.5 flex h-11 w-11 items-center justify-center rounded-lg bg-gray-100 text-blue-600">
                        <ArrowRightLeft size={18} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center justify-between gap-3">
                          <p className="font-semibold text-gray-900">{deviceRoleLabels[role]}</p>
                          <ChevronRight
                            size={18}
                            className={isSelected ? "text-blue-600" : "text-gray-400"}
                          />
                        </div>
                        <p className="mt-1 text-sm text-gray-500">
                          {device
                            ? `${device.displayName} (${device.deviceId ?? "unknown"})`
                            : "デバイス未割当"}
                        </p>
                        <div className="mt-3 flex items-center justify-between">
                          <span className="rounded-full border border-gray-200 bg-gray-50 px-2.5 py-1 text-xs text-gray-600">
                            {device ? "割当済み" : "未割当"}
                          </span>
                          <span className="text-xs text-gray-400">
                            {isSelected ? "詳細を表示中" : "クリックで選択"}
                          </span>
                        </div>
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          </aside>

          <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
            {!selectedRole ? (
              <div className="flex h-full min-h-[420px] flex-col items-center justify-center rounded-lg border border-dashed border-gray-300 bg-gray-50 px-8 text-center">
                <Server className="text-gray-400" size={28} />
                <h3 className="mt-4 text-xl font-semibold text-gray-900">
                  DeviceRole を選択してください
                </h3>
                <p className="mt-2 max-w-md text-sm text-gray-500">
                  左のロール一覧から対象を選ぶと、このエリアにマッピング設定が表示されます。
                </p>
              </div>
            ) : (
              <div className="space-y-6">
                <div className="flex flex-col gap-4 rounded-lg border border-gray-200 bg-gray-50 p-6 lg:flex-row lg:items-start lg:justify-between">
                  <div className="flex items-start gap-4">
                    <div className="flex h-14 w-14 items-center justify-center rounded-lg bg-gray-100 text-blue-600">
                      <ArrowRightLeft size={24} />
                    </div>
                    <div>
                      <p className="text-sm font-medium text-gray-600">選択中の DeviceRole</p>
                      <h3 className="mt-1 text-3xl font-semibold tracking-tight text-gray-900">
                        {deviceRoleLabels[selectedRole]}
                      </h3>
                      <p className="mt-2 text-sm text-gray-500">
                        {assignedDevice
                          ? `${assignedDevice.displayName} / ${assignedDevice.deviceKind ?? "Unknown"}`
                          : "対象デバイスをロールに割り当てると control 候補が表示されます。"}
                      </p>
                    </div>
                  </div>

                  <div className="flex flex-wrap gap-3">
                    <div className="rounded-full border border-gray-200 bg-white px-4 py-2 text-sm text-gray-700">
                      {mappings.length} mapping(s)
                    </div>
                  </div>
                </div>

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <h3 className="text-xl font-semibold text-gray-900">新しいマッピング</h3>
                  <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_220px_minmax(0,1fr)_220px_120px]">
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Control</span>
                      <select
                        value={draft.controlId}
                        onChange={(event) =>
                          setDraft((current) => ({ ...current, controlId: event.target.value }))
                        }
                        disabled={availableControls.length === 0}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">選択してください</option>
                        {availableControls.map((control) => (
                          <option key={control.controlId} value={control.controlId}>
                            {control.label} ({control.description})
                          </option>
                        ))}
                      </select>
                    </label>

                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Event</span>
                      <select
                        value={draft.inputEvent}
                        onChange={(event) =>
                          setDraft((current) => ({
                            ...current,
                            inputEvent: event.target.value as NormalizedControlEvent,
                          }))
                        }
                        disabled={!selectedControl}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        {(selectedControl?.supportedEvents ?? []).map((inputEvent) => (
                          <option key={inputEvent} value={inputEvent}>
                            {inputEvent}
                          </option>
                        ))}
                      </select>
                    </label>

                    <label className="space-y-2 text-sm text-gray-700">
                      <span>DCS-BIOS Identifier</span>
                      <input
                        value={draft.identifier}
                        onChange={(event) =>
                          setDraft((current) => ({ ...current, identifier: event.target.value }))
                        }
                        placeholder="UFC_COMM1_CHANNEL_SELECT"
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                      />
                    </label>

                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Argument</span>
                      <input
                        value={draft.argument}
                        onChange={(event) =>
                          setDraft((current) => ({ ...current, argument: event.target.value }))
                        }
                        placeholder="1"
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                      />
                    </label>

                    <button
                      type="button"
                      onClick={() => void handleAddMapping()}
                      disabled={busyAction !== null || availableControls.length === 0}
                      className="inline-flex h-11 items-center justify-center gap-2 self-end rounded-md bg-blue-600 px-4 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      <Plus size={16} />
                      追加
                    </button>
                  </div>
                </section>

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <h3 className="text-xl font-semibold text-gray-900">保存済みマッピング</h3>
                  <div className="mt-5 space-y-3">
                    {mappings.length === 0 ? (
                      <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-6 text-sm text-gray-500">
                        このロールのマッピングはまだありません。
                      </div>
                    ) : (
                      mappings.map((mapping) => {
                        const control = availableControls.find(
                          (entry) => entry.controlId === mapping.controlId,
                        );

                        return (
                          <div
                            key={mapping.id}
                            className="grid gap-4 rounded-lg border border-gray-200 bg-gray-50 p-4 lg:grid-cols-[minmax(0,1fr)_180px_minmax(0,1fr)_180px_48px]"
                          >
                            <div>
                              <p className="font-medium text-gray-900">
                                {control?.label ?? `Control ${mapping.controlId}`}
                              </p>
                              <p className="mt-1 text-sm text-gray-500">
                                {control?.description ?? "保存済み control"}
                              </p>
                            </div>
                            <div className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">
                              {mapping.inputEvent}
                            </div>
                            <div className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">
                              {mapping.action.identifier}
                            </div>
                            <div className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">
                              {mapping.action.argument}
                            </div>
                            <button
                              type="button"
                              onClick={() => void handleDeleteMapping(mapping.id)}
                              disabled={busyAction !== null}
                              className="inline-flex h-10 items-center justify-center rounded-md border border-red-200 bg-red-50 text-red-700 transition hover:bg-red-100 disabled:cursor-not-allowed disabled:opacity-60"
                              aria-label="マッピングを削除"
                            >
                              <Trash2 size={16} />
                            </button>
                          </div>
                        );
                      })
                    )}
                  </div>
                </section>
              </div>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}

export default MappingSettings;
