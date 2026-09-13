"use client";

import { useEffect, useMemo, useState } from "react";
import { ArrowRightLeft, ChevronRight, Plus, Radio, Trash2 } from "lucide-react";

import {
  deviceRoleLabels,
  getImplementedRoleControls,
  getPhysicalControlCatalog,
  getRoleDefinition,
} from "@/lib/control-catalog";
import type {
  AdapterControlMapping,
  AdapterMappingConfig,
  AdapterOutputMapping,
  AdapterCatalog,
  DeviceRoleAssignment,
  EventKind,
  LearnRequest,
  LearnSessionStatus,
  ManagedDeviceSummary,
  RoleDefinition,
} from "@/lib/manager-types";

type MappingSettingsProps = {
  devices: ManagedDeviceSummary[];
  roleDefinitions: RoleDefinition[];
  deviceRoleAssignments: DeviceRoleAssignment[];
  adapterMappings: AdapterMappingConfig[];
  adapterCatalog: AdapterCatalog;
  learnSession: LearnSessionStatus;
  busyAction: string | null;
  onSaveDeviceRoleAssignments: (assignments: DeviceRoleAssignment[]) => Promise<void>;
  onSaveAdapterMappings: (adapterMappings: AdapterMappingConfig[]) => Promise<void>;
  onStartLearn: (request: LearnRequest) => Promise<void>;
  onCancelLearn: () => Promise<void>;
};

const eventLabels: Record<EventKind, string> = {
  "button-down": "Button Down",
  "button-up": "Button Up",
  "button-pushed": "Button Pushed",
  "encoder-delta": "Encoder Delta",
  "absolute-changed": "Absolute Changed",
  "toggle-on": "Toggle On",
  "toggle-off": "Toggle Off",
};

const emptyAdapterDraft = () => ({
  adapterId: "",
  profileId: "",
  category: "",
  identifier: "",
  inputId: "",
  inputInterface: "",
  argument: "",
  argumentMode: "fixed" as "fixed" | "event-value",
  logicalControlId: "",
  eventKind: "button-pushed" as EventKind,
});

export function MappingSettings({
  devices,
  roleDefinitions,
  deviceRoleAssignments,
  adapterMappings,
  adapterCatalog,
  learnSession,
  busyAction,
  onSaveDeviceRoleAssignments,
  onSaveAdapterMappings,
  onStartLearn,
  onCancelLearn,
}: MappingSettingsProps) {
  const [selectedRoleId, setSelectedRoleId] = useState(roleDefinitions[0]?.roleId ?? "");
  const [selectedDeviceId, setSelectedDeviceId] = useState("");
  const [selectedLogicalControlId, setSelectedLogicalControlId] = useState("");
  const [selectedPhysicalControlId, setSelectedPhysicalControlId] = useState("");
  const [learnMode, setLearnMode] = useState<"append" | "replace">("append");
  const [learnDeviceId, setLearnDeviceId] = useState("");
  const [learnExpectedEventKind, setLearnExpectedEventKind] = useState<EventKind | "">("");
  const [adapterDraft, setAdapterDraft] = useState(emptyAdapterDraft);
  const [outputDraft, setOutputDraft] = useState({
    adapterId: "",
    profileId: "",
    category: "",
    identifier: "",
    outputId: "",
    logicalControlId: "",
    encoding: "segment-map",
  });

  const roleDefinition = useMemo(
    () => getRoleDefinition(selectedRoleId, roleDefinitions),
    [roleDefinitions, selectedRoleId],
  );
  const roleAssignments = useMemo(
    () => deviceRoleAssignments.filter((entry) => entry.roleId === selectedRoleId),
    [deviceRoleAssignments, selectedRoleId],
  );
  const roleControlCapacity = roleAssignments.reduce((maximum, assignment) => {
    const device = devices.find((entry) => entry.deviceId === assignment.deviceId);
    return Math.max(maximum, device?.controls ?? 0);
  }, 0);
  const roleControls = getImplementedRoleControls(roleDefinition, roleControlCapacity);
  const learnDevices = useMemo(
    () =>
      devices.filter(
        (device): device is ManagedDeviceSummary & { deviceId: string } =>
          device.deviceId !== null,
      ),
    [devices],
  );
  const selectedControl = roleControls.find(
    (control) => control.logicalControlId === selectedLogicalControlId,
  );
  const adapterSelectedControl = roleControls.find(
    (control) => control.logicalControlId === adapterDraft.logicalControlId,
  );
  const selectedDevice = devices.find((device) => device.deviceId === selectedDeviceId) ?? null;
  const selectedPhysicalControls = getPhysicalControlCatalog(
    selectedDevice?.deviceKindId ?? null,
    selectedDevice?.controls ?? null,
  );
  const selectedEventKind = adapterSelectedControl?.supportedEvents.includes(adapterDraft.eventKind)
    ? adapterDraft.eventKind
    : adapterSelectedControl?.supportedEvents[0] ?? "button-pushed";
  const adapterDefinitions = adapterCatalog.adapters;
  const selectedAdapter = adapterDefinitions.find(
    (adapter) => adapter.adapterId === adapterDraft.adapterId,
  );
  const adapterProfiles = selectedAdapter?.profiles ?? [];
  const selectedAdapterProfile = adapterProfiles.find(
    (profile) => profile.profileId === adapterDraft.profileId,
  );
  const dcsBiosModules = adapterProfiles.map((profile) => ({
    moduleId: profile.profileId,
    label: profile.label,
    controlCount: profile.controlCount,
  }));
  const dcsBiosModuleControls = useMemo(
    () => selectedAdapterProfile?.controls ?? [],
    [selectedAdapterProfile],
  );
  const dcsBiosCategories = useMemo(
    () => Array.from(new Set(dcsBiosModuleControls.map((control) => control.category))).sort(),
    [dcsBiosModuleControls],
  );
  const dcsBiosInputControls = useMemo(
    () =>
      dcsBiosModuleControls.filter(
        (control) => control.category === adapterDraft.category && control.inputs.length > 0,
      ),
    [adapterDraft.category, dcsBiosModuleControls],
  );
  const selectedDcsBiosInputControl =
    dcsBiosInputControls.find((control) => control.controlId === adapterDraft.identifier) ?? null;
  const dcsBiosInputs = selectedDcsBiosInputControl?.inputs ?? [];
  const selectedDcsBiosInput =
    dcsBiosInputs.find((input) => input.inputId === adapterDraft.inputId) ?? null;
  const dcsBiosArgumentOptions = selectedDcsBiosInput?.argumentOptions ?? [];
  const supportsSelectedEventValue =
    selectedDcsBiosInput?.supportsEventValue === true &&
    ((selectedDcsBiosInput.interface === "set_state" && selectedEventKind === "absolute-changed") ||
      (selectedDcsBiosInput.interface === "variable_step" && selectedEventKind === "encoder-delta"));
  const dcsBiosOutputModuleControls = useMemo(
    () =>
      adapterDefinitions
        .find((adapter) => adapter.adapterId === outputDraft.adapterId)
        ?.profiles.find((profile) => profile.profileId === outputDraft.profileId)?.controls ?? [],
    [adapterDefinitions, outputDraft.adapterId, outputDraft.profileId],
  );
  const dcsBiosOutputCategories = useMemo(
    () => Array.from(new Set(dcsBiosOutputModuleControls.map((control) => control.category))).sort(),
    [dcsBiosOutputModuleControls],
  );
  const dcsBiosOutputControls = useMemo(
    () =>
      dcsBiosOutputModuleControls.filter(
        (control) => control.category === outputDraft.category && control.outputs.length > 0,
      ),
    [dcsBiosOutputModuleControls, outputDraft.category],
  );
  const selectedDcsBiosOutputControl =
    dcsBiosOutputControls.find((control) => control.controlId === outputDraft.identifier) ?? null;
  const dcsBiosOutputs = selectedDcsBiosOutputControl?.outputs ?? [];
  const selectedDcsBiosOutput =
    dcsBiosOutputs.find((output) => output.outputId === outputDraft.outputId) ?? null;

  useEffect(() => {
    if (!roleDefinitions.some((definition) => definition.roleId === selectedRoleId)) {
      setSelectedRoleId(roleDefinitions[0]?.roleId ?? "");
    }
  }, [roleDefinitions, selectedRoleId]);

  useEffect(() => {
    const firstControl = roleControls[0]?.logicalControlId ?? "";
    if (!roleControls.some((control) => control.logicalControlId === selectedLogicalControlId)) {
      setSelectedLogicalControlId(firstControl);
    }
    if (roleAssignments.every((assignment) => assignment.deviceId !== selectedDeviceId)) {
      setSelectedDeviceId(roleAssignments[0]?.deviceId ?? "");
    }
  }, [roleAssignments, roleControls, selectedDeviceId, selectedLogicalControlId]);

  useEffect(() => {
    if (!selectedPhysicalControls.some((control) => String(control.physicalControlId) === selectedPhysicalControlId)) {
      setSelectedPhysicalControlId(
        selectedPhysicalControls[0] ? String(selectedPhysicalControls[0].physicalControlId) : "",
      );
    }
  }, [selectedPhysicalControls, selectedPhysicalControlId]);

  useEffect(() => {
    if (!selectedControl) {
      return;
    }
    setAdapterDraft((current) => ({
      ...current,
      logicalControlId: selectedControl.logicalControlId,
      eventKind: selectedControl.supportedEvents.includes(current.eventKind)
        ? current.eventKind
        : selectedControl.supportedEvents[0] ?? "button-pushed",
    }));
  }, [selectedControl]);

  useEffect(() => {
    if (!adapterSelectedControl) {
      return;
    }
    if (!adapterSelectedControl.supportedEvents.includes(adapterDraft.eventKind)) {
      setAdapterDraft((current) => ({
        ...current,
        eventKind: adapterSelectedControl.supportedEvents[0] ?? "button-pushed",
      }));
    }
  }, [adapterDraft.eventKind, adapterSelectedControl]);

  useEffect(() => {
    const firstAdapter = adapterDefinitions[0];
    if (!firstAdapter) {
      return;
    }
    const selectedAdapterProfiles =
      adapterDefinitions.find((adapter) => adapter.adapterId === adapterDraft.adapterId)?.profiles ?? [];
    if (!adapterDefinitions.some((adapter) => adapter.adapterId === adapterDraft.adapterId)) {
      setAdapterDraft((current) => ({
        ...current,
        adapterId: firstAdapter.adapterId,
        profileId: "",
        category: "",
        identifier: "",
        inputId: "",
        inputInterface: "",
        argument: "",
      }));
    } else if (!selectedAdapterProfiles.some((profile) => profile.profileId === adapterDraft.profileId)) {
      setAdapterDraft((current) => ({
        ...current,
        profileId: dcsBiosModules[0].moduleId,
        category: "",
        identifier: "",
        inputId: "",
        inputInterface: "",
        argument: "",
      }));
    }
    const outputAdapterProfiles =
      adapterDefinitions.find((adapter) => adapter.adapterId === outputDraft.adapterId)?.profiles ?? [];
    if (!adapterDefinitions.some((adapter) => adapter.adapterId === outputDraft.adapterId)) {
      setOutputDraft((current) => ({
        ...current,
        adapterId: firstAdapter.adapterId,
        profileId: "",
        category: "",
        identifier: "",
        outputId: "",
      }));
    } else if (!outputAdapterProfiles.some((profile) => profile.profileId === outputDraft.profileId)) {
      setOutputDraft((current) => ({
        ...current,
        profileId: dcsBiosModules[0].moduleId,
        category: "",
        identifier: "",
        outputId: "",
      }));
    }
  }, [adapterDefinitions, adapterDraft.adapterId, adapterDraft.profileId, dcsBiosModules, outputDraft.adapterId, outputDraft.profileId]);

  useEffect(() => {
    if (dcsBiosCategories.length > 0 && !dcsBiosCategories.includes(adapterDraft.category)) {
      setAdapterDraft((current) => ({
        ...current,
        category: dcsBiosCategories[0],
        identifier: "",
        inputId: "",
        inputInterface: "",
        argument: "",
      }));
    }
    if (
      dcsBiosOutputCategories.length > 0 &&
      !dcsBiosOutputCategories.includes(outputDraft.category)
    ) {
      setOutputDraft((current) => ({
        ...current,
        category: dcsBiosOutputCategories[0],
        identifier: "",
        outputId: "",
      }));
    }
  }, [
    adapterDraft.category,
    dcsBiosCategories,
    dcsBiosOutputCategories,
    outputDraft.category,
  ]);

  useEffect(() => {
    const firstInputControl = dcsBiosInputControls[0];
    if (!firstInputControl) {
      return;
    }
    if (!dcsBiosInputControls.some((control) => control.controlId === adapterDraft.identifier)) {
      setAdapterDraft((current) => ({
        ...current,
        identifier: firstInputControl.controlId,
        inputId: "",
        inputInterface: "",
        argument: "",
      }));
    }
  }, [adapterDraft.identifier, dcsBiosInputControls]);

  useEffect(() => {
    const firstInput = dcsBiosInputs[0];
    if (!firstInput) {
      return;
    }
    if (!dcsBiosInputs.some((input) => input.inputId === adapterDraft.inputId)) {
      setAdapterDraft((current) => ({
        ...current,
        inputId: firstInput.inputId,
        inputInterface: firstInput.interface,
        argument: firstInput.argumentOptions[0]?.value ?? (firstInput.supportsEventValue ? "$event-value" : ""),
        argumentMode: firstInput.argumentOptions.length > 0 ? "fixed" : "event-value",
      }));
    }
  }, [adapterDraft.inputId, dcsBiosInputs]);

  useEffect(() => {
    if (!selectedDcsBiosInput) {
      return;
    }
    const isFixedArgument = dcsBiosArgumentOptions.some((option) => option.value === adapterDraft.argument);
    if (!isFixedArgument && !(selectedDcsBiosInput.supportsEventValue && adapterDraft.argument === "$event-value")) {
      setAdapterDraft((current) => ({
        ...current,
        argument: dcsBiosArgumentOptions[0]?.value ?? (selectedDcsBiosInput.supportsEventValue ? "$event-value" : ""),
        argumentMode: dcsBiosArgumentOptions.length > 0 ? "fixed" : "event-value",
      }));
    }
  }, [adapterDraft.argument, dcsBiosArgumentOptions, selectedDcsBiosInput]);

  useEffect(() => {
    const outputControl = dcsBiosOutputControls[0];
    if (!outputControl) {
      return;
    }
    if (!dcsBiosOutputControls.some((control) => control.controlId === outputDraft.identifier)) {
      setOutputDraft((current) => ({
        ...current,
        identifier: outputControl.controlId,
        outputId: "",
      }));
    }
  }, [dcsBiosOutputControls, outputDraft.identifier]);

  useEffect(() => {
    const output = dcsBiosOutputs[0];
    if (!output) {
      return;
    }
    if (!dcsBiosOutputs.some((candidate) => candidate.outputId === outputDraft.outputId)) {
      setOutputDraft((current) => ({ ...current, outputId: output.outputId }));
    }
  }, [dcsBiosOutputs, outputDraft.outputId]);

  useEffect(() => {
    if (!roleControls.some((control) => control.logicalControlId === outputDraft.logicalControlId)) {
      setOutputDraft((current) => ({
        ...current,
        logicalControlId: roleControls[0]?.logicalControlId ?? "",
      }));
    }
  }, [outputDraft.logicalControlId, roleControls]);

  const updateAssignments = async (updater: (assignments: DeviceRoleAssignment[]) => DeviceRoleAssignment[]) => {
    await onSaveDeviceRoleAssignments(updater(deviceRoleAssignments));
  };

  const addManualBinding = async () => {
    const physicalControlId = Number(selectedPhysicalControlId);
    if (!selectedDeviceId || !selectedLogicalControlId || !Number.isInteger(physicalControlId)) {
      return;
    }

    await updateAssignments((assignments) =>
      assignments.map((assignment) => {
        if (assignment.deviceId !== selectedDeviceId || assignment.roleId !== selectedRoleId) {
          return assignment;
        }
        if (
          assignment.bindings.some(
            (binding) =>
              binding.physicalControlId === physicalControlId &&
              binding.logicalControlId === selectedLogicalControlId,
          )
        ) {
          return assignment;
        }
        return {
          ...assignment,
          bindings: [
            ...assignment.bindings,
            { physicalControlId, logicalControlId: selectedLogicalControlId },
          ],
        };
      }),
    );
  };

  const removeBinding = async (
    deviceId: string,
    logicalControlId: string,
    physicalControlId: number,
  ) => {
    await updateAssignments((assignments) =>
      assignments.map((assignment) =>
        assignment.deviceId === deviceId && assignment.roleId === selectedRoleId
          ? {
              ...assignment,
              bindings: assignment.bindings.filter(
                (binding) =>
                  !(
                    binding.logicalControlId === logicalControlId &&
                    binding.physicalControlId === physicalControlId
                  ),
              ),
            }
          : assignment,
      ),
    );
  };

  const startLearn = async () => {
    if (!selectedLogicalControlId || learnSession.active) {
      return;
    }
    await onStartLearn({
      roleId: selectedRoleId,
      logicalControlId: selectedLogicalControlId,
      targetDeviceId: learnDeviceId || null,
      expectedEventKind: learnExpectedEventKind || null,
      mode: learnMode,
      timeoutMs: 10_000,
    });
  };

  const addAdapterMapping = async () => {
    if (
      !adapterDraft.adapterId.trim() ||
      !adapterDraft.profileId.trim() ||
      !adapterDraft.logicalControlId ||
      !adapterDraft.identifier.trim() ||
      !adapterDraft.inputInterface.trim() ||
      !adapterDraft.argument.trim() ||
      !selectedDcsBiosInputControl ||
      !selectedDcsBiosInput
    ) {
      return;
    }

    const mapping: AdapterControlMapping = {
      roleId: selectedRoleId,
      logicalControlId: adapterDraft.logicalControlId,
      eventKind: selectedEventKind,
      action: {
        actionId: "control-command",
        parameters: {
          identifier: selectedDcsBiosInputControl.controlId,
          argument: adapterDraft.argument.trim(),
          argumentMode: adapterDraft.argumentMode,
          referenceAdapter: adapterDraft.adapterId,
          referenceProfile: adapterDraft.profileId,
          referenceCategory: selectedDcsBiosInputControl.category,
          referenceControl: selectedDcsBiosInputControl.controlId,
          referenceInput: selectedDcsBiosInput.inputId,
          referenceInterface: selectedDcsBiosInput.interface,
          ...(selectedDcsBiosInput.maxValue === null
            ? {}
            : { maxValue: String(selectedDcsBiosInput.maxValue) }),
          ...(selectedDcsBiosInput.suggestedStep === null
            ? {}
            : { suggestedStep: String(selectedDcsBiosInput.suggestedStep) }),
        },
      },
    };
    const next = adapterMappings.map((config) => ({
      ...config,
      mappings: [...config.mappings],
    }));
    const configIndex = next.findIndex(
      (config) =>
        config.adapterId === adapterDraft.adapterId.trim() &&
        config.profileId === adapterDraft.profileId.trim(),
    );
    if (configIndex >= 0) {
      next[configIndex].mappings.push(mapping);
    } else {
      next.push({
        adapterId: adapterDraft.adapterId.trim(),
        profileId: adapterDraft.profileId.trim(),
        mappings: [mapping],
      });
    }
    await onSaveAdapterMappings(next);
  };

  const deleteAdapterMapping = async (configIndex: number, mappingIndex: number) => {
    const next = adapterMappings
      .map((config, currentConfigIndex) =>
        currentConfigIndex === configIndex
          ? { ...config, mappings: config.mappings.filter((_, index) => index !== mappingIndex) }
          : config,
      )
      .filter((config) => config.mappings.length > 0 || (config.outputMappings ?? []).length > 0);
    await onSaveAdapterMappings(next);
  };

  const addOutputMapping = async () => {
    if (
      !outputDraft.adapterId.trim() ||
      !outputDraft.profileId.trim() ||
      !outputDraft.logicalControlId ||
      !outputDraft.identifier.trim() ||
      !outputDraft.outputId.trim() ||
      !selectedDcsBiosOutputControl ||
      !selectedDcsBiosOutput
    ) {
      return;
    }

    const outputMapping: AdapterOutputMapping = {
      roleId: selectedRoleId,
      logicalControlId: outputDraft.logicalControlId,
      sourceId: "control-output",
      parameters: {
        referenceAdapter: outputDraft.adapterId,
        referenceProfile: outputDraft.profileId,
        referenceCategory: selectedDcsBiosOutputControl.category,
        referenceControl: selectedDcsBiosOutputControl.controlId,
        referenceOutput: selectedDcsBiosOutput.outputId,
        outputType: selectedDcsBiosOutput.outputType,
        encoding: outputDraft.encoding,
      },
    };
    const next = adapterMappings.map((config) => ({
      ...config,
      mappings: [...config.mappings],
      outputMappings: [...(config.outputMappings ?? [])],
    }));
    const configIndex = next.findIndex(
      (config) =>
        config.adapterId === outputDraft.adapterId.trim() &&
        config.profileId === outputDraft.profileId.trim(),
    );
    if (configIndex >= 0) {
      next[configIndex].outputMappings?.push(outputMapping);
    } else {
      next.push({
        adapterId: outputDraft.adapterId.trim(),
        profileId: outputDraft.profileId.trim(),
        mappings: [],
        outputMappings: [outputMapping],
      });
    }
    await onSaveAdapterMappings(next);
  };

  const deleteOutputMapping = async (configIndex: number, outputIndex: number) => {
    const next = adapterMappings
      .map((config, currentConfigIndex) =>
        currentConfigIndex === configIndex
          ? {
              ...config,
              outputMappings: (config.outputMappings ?? []).filter((_, index) => index !== outputIndex),
            }
          : config,
      )
      .filter((config) => config.mappings.length > 0 || (config.outputMappings ?? []).length > 0);
    await onSaveAdapterMappings(next);
  };

  const labelForPhysicalControl = (device: ManagedDeviceSummary, physicalControlId: number) => {
    const control = getPhysicalControlCatalog(device.deviceKindId, device.controls).find(
      (entry) => entry.physicalControlId === physicalControlId,
    );
    return control ? `${control.label} (${control.description})` : `Physical ${physicalControlId}`;
  };

  const implementedControlCount = (roleId: string) =>
    deviceRoleAssignments
      .filter((assignment) => assignment.roleId === roleId)
      .reduce((maximum, assignment) => {
        const device = devices.find((entry) => entry.deviceId === assignment.deviceId);
        return Math.max(maximum, device?.controls ?? 0);
      }, 0);

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
        <div className="grid gap-6 xl:grid-cols-[320px_minmax(0,1fr)]">
          <aside className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
            <p className="text-sm font-medium text-gray-500">論理 Role</p>
            <h3 className="mt-1 text-xl font-semibold text-gray-900">Role 一覧</h3>
            <div className="mt-5 space-y-3">
              {roleDefinitions.map((definition) => {
                const isSelected = definition.roleId === selectedRoleId;
                const assignmentCount = deviceRoleAssignments.filter(
                  (assignment) => assignment.roleId === definition.roleId,
                ).length;
                return (
                  <button
                    key={definition.roleId}
                    type="button"
                    onClick={() => setSelectedRoleId(definition.roleId)}
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
                          <p className="font-semibold text-gray-900">
                            {deviceRoleLabels[definition.roleId] ?? definition.roleId}
                          </p>
                          <ChevronRight size={18} className={isSelected ? "text-blue-600" : "text-gray-400"} />
                        </div>
                        <p className="mt-1 text-sm text-gray-500">{definition.roleId}</p>
                        <div className="mt-3 flex items-center justify-between">
                          <span className="rounded-full border border-gray-200 bg-gray-50 px-2.5 py-1 text-xs text-gray-600">
                            {assignmentCount} device(s)
                          </span>
                          <span className="text-xs text-gray-400">
                            {implementedControlCount(definition.roleId)} implemented controls
                          </span>
                        </div>
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          </aside>

          <section className="space-y-6">
            {!roleDefinition ? (
              <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-8 text-sm text-gray-500">
                Role 定義がありません。
              </div>
            ) : (
              <>
                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                    <div>
                      <p className="text-sm font-medium text-gray-500">Role 論理インターフェース</p>
                      <h3 className="mt-1 text-3xl font-semibold tracking-tight text-gray-900">
                        {deviceRoleLabels[selectedRoleId] ?? selectedRoleId}
                      </h3>
                      <p className="mt-2 text-sm text-gray-500">
                        論理コントロールはゲームや物理デバイスを知りません。下の全デバイス結線へ fan-out します。
                      </p>
                    </div>
                    <div className="rounded-full border border-gray-200 bg-gray-50 px-4 py-2 text-sm text-gray-700">
                      {roleAssignments.length} device(s) / {roleControls.length} logical control(s)
                    </div>
                  </div>
                </section>

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div className="flex items-center justify-between gap-4">
                    <div>
                      <h3 className="text-xl font-semibold text-gray-900">物理 → 論理結線</h3>
                      <p className="mt-1 text-sm text-gray-500">
                        1 つの物理入力を複数 Role／論理コントロールへ登録できます。重複は警告だけで拒否しません。
                      </p>
                    </div>
                    <div className="rounded-full border border-gray-200 bg-gray-100 px-4 py-2 text-sm text-gray-700">
                      {roleAssignments.reduce((count, assignment) => count + assignment.bindings.length, 0)} binding(s)
                    </div>
                  </div>

                  <div className="mt-5 grid gap-4 rounded-lg border border-blue-100 bg-blue-50/50 p-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_120px]">
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Device</span>
                      <select
                        value={selectedDeviceId}
                        onChange={(event) => setSelectedDeviceId(event.target.value)}
                        disabled={roleAssignments.length === 0}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">デバイスを選択</option>
                        {roleAssignments.map((assignment) => (
                          <option key={assignment.deviceId} value={assignment.deviceId}>
                            {devices.find((device) => device.deviceId === assignment.deviceId)?.displayName ?? assignment.deviceId}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Logical Control</span>
                      <select
                        value={selectedLogicalControlId}
                        onChange={(event) => setSelectedLogicalControlId(event.target.value)}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                      >
                        {roleControls.map((control) => (
                          <option key={control.logicalControlId} value={control.logicalControlId}>
                            {control.label} ({control.logicalControlId})
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Physical Control</span>
                      <select
                        value={selectedPhysicalControlId}
                        onChange={(event) => setSelectedPhysicalControlId(event.target.value)}
                        disabled={selectedPhysicalControls.length === 0}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">物理コントロールを選択</option>
                        {selectedPhysicalControls.map((control) => (
                          <option key={control.physicalControlId} value={control.physicalControlId}>
                            {control.label} ({control.description})
                          </option>
                        ))}
                      </select>
                    </label>
                    <button
                      type="button"
                      onClick={() => void addManualBinding()}
                      disabled={busyAction !== null || !selectedDeviceId || selectedPhysicalControls.length === 0}
                      className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-blue-600 px-4 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      <Plus size={16} />
                      追加
                    </button>
                  </div>

                  {roleAssignments.length === 0 ? (
                    <div className="mt-5 rounded-lg border border-dashed border-gray-300 bg-gray-50 p-6 text-sm text-gray-500">
                      デバイス設定タブで、この Role に 1 台以上のデバイスを追加してください。
                    </div>
                  ) : (
                    <div className="mt-5 space-y-3">
                      {roleControls.map((control) => {
                        const bindings = roleAssignments.flatMap((assignment) => {
                          const device = devices.find((entry) => entry.deviceId === assignment.deviceId);
                          return assignment.bindings
                            .filter((binding) => binding.logicalControlId === control.logicalControlId)
                            .map((binding) => ({ assignment, device, binding }));
                        });
                        return (
                          <div key={control.logicalControlId} className="rounded-lg border border-gray-200 bg-gray-50 p-4">
                            <div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
                              <div>
                                <p className="font-medium text-gray-900">{control.label}</p>
                                <p className="text-xs text-gray-500">
                                  {control.logicalControlId} · {control.supportedEvents.map((event) => eventLabels[event]).join(", ")}
                                </p>
                              </div>
                              {bindings.length > 1 && (
                                <span className="inline-flex items-center gap-2 rounded-full border border-amber-200 bg-amber-50 px-3 py-1 text-xs text-amber-800">
                                  <Radio size={13} /> 複数の経路に接続されています
                                </span>
                              )}
                            </div>
                            <div className="mt-3 flex flex-wrap gap-2">
                              {bindings.length === 0 ? (
                                <span className="text-xs text-gray-400">未結線</span>
                              ) : (
                                bindings.map(({ assignment, device, binding }) => (
                                  <span
                                    key={`${assignment.deviceId}:${binding.physicalControlId}:${binding.logicalControlId}`}
                                    className="inline-flex items-center gap-2 rounded-full border border-gray-200 bg-white px-3 py-1.5 text-xs text-gray-700"
                                  >
                                    {device?.displayName ?? assignment.deviceId} · {device ? labelForPhysicalControl(device, binding.physicalControlId) : `Physical ${binding.physicalControlId}`}
                                    <button
                                      type="button"
                                      onClick={() => void removeBinding(assignment.deviceId, binding.logicalControlId, binding.physicalControlId)}
                                      disabled={busyAction !== null}
                                      className="text-gray-400 hover:text-red-600 disabled:opacity-50"
                                      aria-label="物理結線を削除"
                                    >
                                      <Trash2 size={13} />
                                    </button>
                                  </span>
                                ))
                              )}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </section>

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                    <div>
                      <h3 className="text-xl font-semibold text-gray-900">コントロールを学習</h3>
                      <p className="mt-1 text-sm text-gray-500">
                        指定した論理コントロールに対して、次に受信した HCP ControlEvent を登録します。学習中のイベントは Adapter へ送信しません。
                      </p>
                    </div>
                    {learnSession.active && (
                      <button
                        type="button"
                        onClick={() => void onCancelLearn()}
                        disabled={busyAction !== null}
                        className="rounded-md border border-red-200 bg-red-50 px-4 py-2 text-sm font-medium text-red-700 hover:bg-red-100 disabled:opacity-50"
                      >
                        学習をキャンセル
                      </button>
                    )}
                  </div>
                  <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_180px_180px_140px]">
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Logical Control</span>
                      <select
                        value={selectedLogicalControlId}
                        onChange={(event) => setSelectedLogicalControlId(event.target.value)}
                        disabled={learnSession.active}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        {roleControls.map((control) => (
                          <option key={control.logicalControlId} value={control.logicalControlId}>
                            {control.label}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>対象 Device（任意）</span>
                      <select
                        value={learnDeviceId}
                        onChange={(event) => setLearnDeviceId(event.target.value)}
                        disabled={learnSession.active}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">最初に受信した Device</option>
                        {learnDevices.map((device) => (
                          <option key={device.deviceId} value={device.deviceId}>
                            {device.displayName} · {device.deviceId}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>イベント種別（任意）</span>
                      <select
                        value={learnExpectedEventKind}
                        onChange={(event) => setLearnExpectedEventKind(event.target.value as EventKind | "")}
                        disabled={learnSession.active}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">任意</option>
                        {(selectedControl?.supportedEvents ?? []).map((event) => (
                          <option key={event} value={event}>{eventLabels[event]}</option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>既存結線</span>
                      <select
                        value={learnMode}
                        onChange={(event) => setLearnMode(event.target.value as "append" | "replace")}
                        disabled={learnSession.active}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="append">追加</option>
                        <option value="replace">上書き</option>
                      </select>
                    </label>
                    <button
                      type="button"
                      onClick={() => void startLearn()}
                      disabled={busyAction !== null || learnSession.active || !selectedLogicalControlId}
                      className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-indigo-600 px-4 text-sm font-medium text-white hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      <Radio size={16} />
                      {learnSession.active ? "入力待ち…" : "学習開始"}
                    </button>
                  </div>
                  {learnSession.active && (
                    <p className="mt-4 rounded-md border border-indigo-100 bg-indigo-50 px-4 py-3 text-sm text-indigo-800">
                      {learnSession.targetDeviceId ? `Device ${learnSession.targetDeviceId}` : "最初に受信した Device"} の入力を待っています。{learnSession.timeoutMs / 1000} 秒でタイムアウトします。
                    </p>
                  )}
                </section>

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                    <div>
                      <h3 className="text-xl font-semibold text-gray-900">Adapter Mapping</h3>
                      <p className="mt-1 text-sm text-gray-500">
                        Adapterが同梱する実装済みカタログからコントロールと入力インターフェースを選びます。ゲーム固有の識別子や引数を手入力する必要はありません。
                      </p>
                    </div>
                    <div className="flex items-center gap-3">
                      <span className="rounded-full border border-gray-200 bg-gray-50 px-4 py-2 text-sm text-gray-700">
                        {adapterMappings.reduce(
                          (count, config) =>
                            count + config.mappings.filter((mapping) => mapping.roleId === selectedRoleId).length,
                          0,
                        )} mapping(s)
                      </span>
                    </div>
                  </div>

                  <div className="mt-5 rounded-lg border border-gray-200 bg-gray-50 p-4 text-sm">
                    {adapterCatalog.state === "loaded" ? (
                      <>
                        <div className="flex flex-wrap items-center gap-2 text-gray-700">
                          <span className="rounded-full border border-green-200 bg-green-50 px-3 py-1 text-green-800">
                            Adapterカタログを内蔵
                          </span>
                          <span>{adapterDefinitions.length} adapter(s)</span>
                          <span>·</span>
                          <span>{adapterProfiles.length} profile(s)</span>
                        </div>
                        {adapterCatalog.error && (
                          <p className="mt-2 text-amber-700">{adapterCatalog.error}</p>
                        )}
                      </>
                    ) : (
                      <p className="text-amber-800">
                        Adapterカタログを読み込めません。{adapterCatalog.error ? " " + adapterCatalog.error : ""}
                      </p>
                    )}
                  </div>

                  {adapterCatalog.state === "loaded" && (
                    <div className="mt-5 grid gap-4 lg:grid-cols-2">
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Adapter</span>
                        <select
                          value={adapterDraft.adapterId}
                          onChange={(event) =>
                            setAdapterDraft((current) => ({
                              ...current,
                              adapterId: event.target.value,
                              profileId: "",
                              category: "",
                              identifier: "",
                              inputId: "",
                              inputInterface: "",
                              argument: "",
                            }))
                          }
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {adapterDefinitions.map((adapter) => (
                            <option key={adapter.adapterId} value={adapter.adapterId}>
                              {adapter.label}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Adapter Profile</span>
                        <select
                          value={adapterDraft.profileId}
                          onChange={(event) =>
                            setAdapterDraft((current) => ({
                              ...current,
                              profileId: event.target.value,
                              category: "",
                              identifier: "",
                              inputId: "",
                              inputInterface: "",
                              argument: "",
                            }))
                          }
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {dcsBiosModules.map((module) => (
                            <option key={module.moduleId} value={module.moduleId}>
                              {module.label} ({module.controlCount})
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Category / Panel</span>
                        <select
                          value={adapterDraft.category}
                          onChange={(event) =>
                            setAdapterDraft((current) => ({
                              ...current,
                              category: event.target.value,
                              identifier: "",
                              inputId: "",
                              inputInterface: "",
                              argument: "",
                            }))
                          }
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {dcsBiosCategories.map((category) => (
                            <option key={category} value={category}>{category}</option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Control</span>
                        <select
                          value={adapterDraft.identifier}
                          onChange={(event) =>
                            setAdapterDraft((current) => ({
                              ...current,
                              identifier: event.target.value,
                              inputId: "",
                              inputInterface: "",
                              argument: "",
                            }))
                          }
                          disabled={dcsBiosInputControls.length === 0}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                        >
                          {dcsBiosInputControls.map((control) => (
                            <option key={control.controlId} value={control.controlId}>
                              {control.controlId} — {control.description || control.controlType}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Input Interface</span>
                        <select
                          value={adapterDraft.inputId}
                          onChange={(event) =>
                            setAdapterDraft((current) => {
                              const input = dcsBiosInputs.find(
                                (candidate) => candidate.inputId === event.target.value,
                              );
                              return {
                                ...current,
                                inputId: event.target.value,
                                inputInterface: input?.interface ?? "",
                                argument: input?.argumentOptions[0]?.value ??
                                  (input?.supportsEventValue ? "$event-value" : ""),
                                argumentMode: input?.argumentOptions.length ? "fixed" : "event-value",
                              };
                            })
                          }
                          disabled={dcsBiosInputs.length === 0}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                        >
                          {dcsBiosInputs.map((input) => (
                            <option key={input.inputId} value={input.inputId}>
                              {input.interface} — {input.description}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Argument</span>
                        <select
                          value={adapterDraft.argument}
                          onChange={(event) =>
                            setAdapterDraft((current) => ({
                              ...current,
                              argument: event.target.value,
                              argumentMode: event.target.value === "$event-value" ? "event-value" : "fixed",
                            }))
                          }
                          disabled={!selectedDcsBiosInput || (dcsBiosArgumentOptions.length === 0 && !supportsSelectedEventValue)}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                        >
                          {dcsBiosArgumentOptions.map((option) => (
                            <option key={option.value} value={option.value}>{option.label}</option>
                          ))}
                          {supportsSelectedEventValue && (
                            <option value="$event-value">物理イベントの値を送る</option>
                          )}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Logical Control</span>
                        <select
                          value={adapterDraft.logicalControlId}
                          onChange={(event) => setAdapterDraft((current) => ({ ...current, logicalControlId: event.target.value }))}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {roleControls.map((control) => (
                            <option key={control.logicalControlId} value={control.logicalControlId}>{control.label}</option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Manager Event</span>
                        <select
                          value={selectedEventKind}
                          onChange={(event) => setAdapterDraft((current) => ({ ...current, eventKind: event.target.value as EventKind }))}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {(adapterSelectedControl?.supportedEvents ?? []).map((event) => (
                            <option key={event} value={event}>{eventLabels[event]}</option>
                          ))}
                        </select>
                      </label>
                      <button
                        type="button"
                        onClick={() => void addAdapterMapping()}
                        disabled={
                          busyAction !== null ||
                          !adapterDraft.logicalControlId ||
                          !selectedDcsBiosInputControl ||
                          !selectedDcsBiosInput ||
                          !adapterDraft.argument
                        }
                        className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-blue-600 px-4 text-sm font-medium text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
                      >
                        <Plus size={16} />
                        参照から追加
                      </button>
                    </div>
                  )}

                  <div className="mt-5 space-y-3">
                    {adapterMappings.flatMap((config, configIndex) =>
                      config.mappings
                        .map((mapping, mappingIndex) => ({ config, configIndex, mapping, mappingIndex }))
                        .filter(({ mapping }) => mapping.roleId === selectedRoleId),
                    ).length === 0 ? (
                      <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-6 text-sm text-gray-500">
                        この Role の Adapter Mapping はまだありません。
                      </div>
                    ) : (
                      adapterMappings.flatMap((config, configIndex) =>
                        config.mappings
                          .map((mapping, mappingIndex) => ({ config, configIndex, mapping, mappingIndex }))
                          .filter(({ mapping }) => mapping.roleId === selectedRoleId)
                          .map(({ config: currentConfig, configIndex, mapping, mappingIndex }) => (
                            <div key={`${configIndex}:${mappingIndex}`} className="grid gap-3 rounded-lg border border-gray-200 bg-gray-50 p-4 lg:grid-cols-[140px_140px_minmax(0,1fr)_170px_minmax(0,1fr)_48px]">
                              <span className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">{currentConfig.adapterId}</span>
                              <span className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">{currentConfig.profileId}</span>
                              <span className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">{mapping.logicalControlId}</span>
                              <span className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">{eventLabels[mapping.eventKind]}</span>
                              <span className="truncate rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">
                                {mapping.action.parameters.referenceControl ??
                                  mapping.action.parameters.identifier ??
                                  mapping.action.actionId}
                                {" · "}
                                {mapping.action.parameters.referenceInterface ?? ""}
                                {" · "}
                                {mapping.action.parameters.argument === "$event-value"
                                  ? "物理イベントの値"
                                  : mapping.action.parameters.argument ?? ""}
                              </span>
                              <button
                                type="button"
                                onClick={() => void deleteAdapterMapping(configIndex, mappingIndex)}
                                disabled={busyAction !== null}
                                className="inline-flex h-10 items-center justify-center rounded-md border border-red-200 bg-red-50 text-red-700 hover:bg-red-100 disabled:opacity-50"
                                aria-label="Adapter Mapping を削除"
                              >
                                <Trash2 size={16} />
                              </button>
                            </div>
                          )),
                      )
                    )}
                  </div>
                </section>

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div>
                    <h3 className="text-xl font-semibold text-gray-900">Adapter Output Mapping</h3>
                    <p className="mt-1 text-sm text-gray-500">
                      Adapterの出力定義を選ぶと、ゲーム固有のアドレス・マスク・文字列長はカタログから自動的に設定されます。
                    </p>
                  </div>
                  {adapterCatalog.state === "loaded" ? (
                    <div className="mt-5 grid gap-4 lg:grid-cols-2">
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Adapter</span>
                        <select
                          value={outputDraft.adapterId}
                          onChange={(event) =>
                            setOutputDraft((current) => ({
                              ...current,
                              adapterId: event.target.value,
                              profileId: "",
                              category: "",
                              identifier: "",
                              outputId: "",
                            }))
                          }
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {adapterDefinitions.map((adapter) => (
                            <option key={adapter.adapterId} value={adapter.adapterId}>
                              {adapter.label}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Adapter Profile</span>
                        <select
                          value={outputDraft.profileId}
                          onChange={(event) =>
                            setOutputDraft((current) => ({
                              ...current,
                              profileId: event.target.value,
                              category: "",
                              identifier: "",
                              outputId: "",
                            }))
                          }
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {dcsBiosModules.map((module) => (
                            <option key={module.moduleId} value={module.moduleId}>
                              {module.label} ({module.controlCount})
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Category / Panel</span>
                        <select
                          value={outputDraft.category}
                          onChange={(event) =>
                            setOutputDraft((current) => ({
                              ...current,
                              category: event.target.value,
                              identifier: "",
                              outputId: "",
                            }))
                          }
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {dcsBiosOutputCategories.map((category) => (
                            <option key={category} value={category}>{category}</option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Output Control</span>
                        <select
                          value={outputDraft.identifier}
                          onChange={(event) =>
                            setOutputDraft((current) => ({
                              ...current,
                              identifier: event.target.value,
                              outputId: "",
                            }))
                          }
                          disabled={dcsBiosOutputControls.length === 0}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                        >
                          {dcsBiosOutputControls.map((control) => (
                            <option key={control.controlId} value={control.controlId}>
                              {control.controlId} — {control.description || control.controlType}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>DCS-BIOS Output</span>
                        <select
                          value={outputDraft.outputId}
                          onChange={(event) => setOutputDraft((current) => ({ ...current, outputId: event.target.value }))}
                          disabled={dcsBiosOutputs.length === 0}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                        >
                          {dcsBiosOutputs.map((output) => (
                            <option key={output.outputId} value={output.outputId}>
                              {output.description || output.outputId} ({output.outputType})
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>Logical Output</span>
                        <select
                          value={outputDraft.logicalControlId}
                          onChange={(event) => setOutputDraft((current) => ({ ...current, logicalControlId: event.target.value }))}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          {roleControls.map((control) => (
                            <option key={control.logicalControlId} value={control.logicalControlId}>{control.label}</option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-2 text-sm text-gray-700">
                        <span>HCP Encoding</span>
                        <select
                          value={outputDraft.encoding}
                          onChange={(event) => setOutputDraft((current) => ({ ...current, encoding: event.target.value }))}
                          className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                        >
                          <option value="segment-map">Segment Map</option>
                          <option value="mono-bitmap">Mono Bitmap</option>
                          <option value="utf8-text">UTF-8 Text</option>
                        </select>
                      </label>
                      <button
                        type="button"
                        onClick={() => void addOutputMapping()}
                        disabled={
                          busyAction !== null ||
                          !outputDraft.logicalControlId ||
                          !selectedDcsBiosOutputControl ||
                          !selectedDcsBiosOutput
                        }
                        className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-indigo-600 px-4 text-sm font-medium text-white hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-60"
                      >
                        <Plus size={16} />
                        参照から追加
                      </button>
                    </div>
                  ) : (
                    <div className="mt-5 rounded-lg border border-dashed border-gray-300 bg-gray-50 p-6 text-sm text-gray-500">
                      Adapterカタログが利用できると、出力コントロールを選択できます。
                    </div>
                  )}

                  <div className="mt-5 space-y-3">
                    {adapterMappings.flatMap((config, configIndex) =>
                      (config.outputMappings ?? [])
                        .map((mapping, outputIndex) => ({ config, configIndex, mapping, outputIndex }))
                        .filter(({ mapping }) => mapping.roleId === selectedRoleId),
                    ).length === 0 ? (
                      <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-6 text-sm text-gray-500">
                        この Role の Adapter Output Mapping はまだありません。
                      </div>
                    ) : (
                      adapterMappings.flatMap((config, configIndex) =>
                        (config.outputMappings ?? [])
                          .map((mapping, outputIndex) => ({ config, configIndex, mapping, outputIndex }))
                          .filter(({ mapping }) => mapping.roleId === selectedRoleId)
                          .map(({ config: currentConfig, configIndex, mapping, outputIndex }) => (
                            <div key={`${configIndex}:output:${outputIndex}`} className="grid gap-3 rounded-lg border border-gray-200 bg-gray-50 p-4 lg:grid-cols-[140px_140px_minmax(0,1fr)_150px_minmax(0,1fr)_48px]">
                              <span className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">{currentConfig.adapterId}</span>
                              <span className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">{currentConfig.profileId}</span>
                              <span className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">{mapping.logicalControlId}</span>
                              <span className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">{mapping.sourceId}</span>
                              <span className="truncate rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700">
                                {mapping.parameters.referenceControl ?? mapping.parameters.address ?? mapping.sourceId}
                                {" · "}
                                {mapping.parameters.referenceOutput ?? mapping.parameters.encoding ?? ""}
                              </span>
                              <button
                                type="button"
                                onClick={() => void deleteOutputMapping(configIndex, outputIndex)}
                                disabled={busyAction !== null}
                                className="inline-flex h-10 items-center justify-center rounded-md border border-red-200 bg-red-50 text-red-700 hover:bg-red-100 disabled:opacity-50"
                                aria-label="Adapter Output Mapping を削除"
                              >
                                <Trash2 size={16} />
                              </button>
                            </div>
                          )),
                      )
                    )}
                  </div>
                </section>
              </>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}

export default MappingSettings;
