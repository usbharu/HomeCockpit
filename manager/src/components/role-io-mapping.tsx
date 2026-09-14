"use client";

import { useEffect, useMemo, useState } from "react";
import { Play, Save, Trash2 } from "lucide-react";

import { findAdapterProfileForAircraft, normalizeAircraftName } from "@/lib/control-catalog";
import type {
  AdapterCatalog,
  AdapterControlDefinition,
  AdapterControlMapping,
  AdapterInputDefinition,
  AdapterMappingConfig,
  AdapterOutputDefinition,
  AdapterOutputMapping,
  EventKind,
  RoleControlDefinition,
  RoleInputTriggerRequest,
} from "@/lib/manager-types";

type RoleIoMappingProps = {
  roleId: string;
  roleControls: RoleControlDefinition[];
  aircraftName: string | null;
  adapterCatalog: AdapterCatalog;
  adapterMappings: AdapterMappingConfig[];
  busyAction: string | null;
  onSaveAdapterMappings: (mappings: AdapterMappingConfig[]) => Promise<void>;
  onTriggerRoleInput: (request: RoleInputTriggerRequest) => Promise<number>;
};

type RoleIoRowProps = {
  roleId: string;
  logicalControl: RoleControlDefinition;
  adapterId: string;
  profileId: string;
  controls: AdapterControlDefinition[];
  suggestedControl: AdapterControlDefinition | null;
  inputMappings: AdapterControlMapping[];
  outputMapping: AdapterOutputMapping | null;
  inputPersisted: boolean;
  busy: boolean;
  onSaveInput: (mappings: AdapterControlMapping[]) => Promise<void>;
  onRemoveInput: () => Promise<void>;
  onSaveOutput: (mapping: AdapterOutputMapping) => Promise<void>;
  onRemoveOutput: () => Promise<void>;
  onTriggerRoleInput: (request: RoleInputTriggerRequest) => Promise<number>;
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

function preferredInput(control: AdapterControlDefinition): AdapterInputDefinition | null {
  return (
    control.inputs.find(isMomentaryInput) ??
    control.inputs.find(
      (input) => input.interface === "action" && input.argumentOptions.length > 0,
    ) ??
    control.inputs.find((input) => input.argumentOptions.length > 0) ??
    control.inputs[0] ??
    null
  );
}

function isMomentaryInput(input: AdapterInputDefinition): boolean {
  return (
    input.interface === "set_state" &&
    input.maxValue === 1 &&
    input.argumentOptions.some((option) => option.value === "0") &&
    input.argumentOptions.some((option) => option.value === "1")
  );
}

function buildControlMappings(
  roleId: string,
  logicalControlId: string,
  adapterId: string,
  profileId: string,
  control: AdapterControlDefinition,
  input: AdapterInputDefinition,
  momentaryMode: "pulse" | "hold" = "pulse",
): AdapterControlMapping[] {
  if (isMomentaryInput(input)) {
    if (momentaryMode === "pulse") {
      const parameters = buildInputParameters(adapterId, profileId, control, input, "1");
      parameters.releaseArgument = "0";
      return [{
        roleId,
        logicalControlId,
        eventKind: "button-pushed",
        action: {
          actionId: "control-pulse",
          parameters,
        },
      }];
    }
    return [
      { eventKind: "button-down" as EventKind, argument: "1" },
      { eventKind: "button-up" as EventKind, argument: "0" },
    ].map(({ eventKind, argument }) => ({
      roleId,
      logicalControlId,
      eventKind,
      action: {
        actionId: "control-command",
        parameters: buildInputParameters(adapterId, profileId, control, input, argument),
      },
    }));
  }

  const argument = input.argumentOptions[0]?.value;
  if (!argument) {
    return [];
  }
  return [{
    roleId,
    logicalControlId,
    eventKind: "button-pushed",
    action: {
      actionId: "control-command",
      parameters: buildInputParameters(adapterId, profileId, control, input, argument),
    },
  }];
}

function buildInputParameters(
  adapterId: string,
  profileId: string,
  control: AdapterControlDefinition,
  input: AdapterInputDefinition,
  argument: string,
) {
  const parameters: Record<string, string> = {
    identifier: control.controlId,
    argument,
    argumentMode: argument === "$event-value" ? "event-value" : "fixed",
    referenceAdapter: adapterId,
    referenceProfile: profileId,
    referenceCategory: control.category,
    referenceControl: control.controlId,
    referenceInput: input.inputId,
    referenceInterface: input.interface,
  };
  if (input.maxValue !== null) {
    parameters.maxValue = String(input.maxValue);
  }
  if (input.suggestedStep !== null) {
    parameters.suggestedStep = String(input.suggestedStep);
  }
  return parameters;
}

function buildDefaultConfig(
  adapterId: string,
  profile: NonNullable<ReturnType<typeof findAdapterProfileForAircraft>>["profile"],
  aircraftName: string,
): AdapterMappingConfig {
  const mappings = profile.roleBindings.flatMap((binding) =>
    profile.controls
      .filter((control) => control.category === binding.category)
      .map((control, index) => ({ control, input: preferredInput(control), index }))
      .filter(
        (entry): entry is {
          control: AdapterControlDefinition;
          input: AdapterInputDefinition;
          index: number;
        } => entry.input !== null && entry.input.argumentOptions.length > 0,
      )
      .flatMap(({ control, input, index }) =>
        buildControlMappings(
          binding.roleId,
          `button-${index}`,
          adapterId,
          profile.profileId,
          control,
          input,
        ),
      ),
  );

  return {
    adapterId,
    profileId: profile.profileId,
    aircraftName,
    profileLabel: profile.label,
    mappings,
    outputMappings: [],
  };
}

function RoleIoRow({
  roleId,
  logicalControl,
  adapterId,
  profileId,
  controls,
  suggestedControl,
  inputMappings,
  outputMapping,
  inputPersisted,
  busy,
  onSaveInput,
  onRemoveInput,
  onSaveOutput,
  onRemoveOutput,
  onTriggerRoleInput,
}: RoleIoRowProps) {
  const inputControls = useMemo(
    () => controls.filter((control) => control.inputs.length > 0),
    [controls],
  );
  const outputControls = useMemo(
    () => controls.filter((control) => control.outputs.length > 0),
    [controls],
  );
  const inputMapping = inputMappings.find((mapping) => mapping.eventKind === "button-down") ?? inputMappings[0] ?? null;
  const initialInputControlId =
    inputMapping?.action.parameters.referenceControl ?? suggestedControl?.controlId ?? inputControls[0]?.controlId ?? "";
  const initialOutputControlId =
    outputMapping?.parameters.referenceControl ?? suggestedControl?.controlId ?? outputControls[0]?.controlId ?? "";
  const [inputControlId, setInputControlId] = useState(initialInputControlId);
  const [inputId, setInputId] = useState(inputMapping?.action.parameters.referenceInput ?? "");
  const [argument, setArgument] = useState(inputMapping?.action.parameters.argument ?? "");
  const [eventKind, setEventKind] = useState<EventKind>(inputMapping?.eventKind ?? "button-pushed");
  const [outputControlId, setOutputControlId] = useState(initialOutputControlId);
  const [outputId, setOutputId] = useState(outputMapping?.parameters.referenceOutput ?? "");
  const [notice, setNotice] = useState<string | null>(null);
  const [sendingEvent, setSendingEvent] = useState<EventKind | null>(null);

  const inputControl = inputControls.find((control) => control.controlId === inputControlId) ?? null;
  const selectedInput =
    inputControl?.inputs.find((input) => input.inputId === inputId) ??
    (inputControl ? preferredInput(inputControl) : null);
  const outputControl = outputControls.find((control) => control.controlId === outputControlId) ?? null;
  const selectedOutput =
    outputControl?.outputs.find((output) => output.outputId === outputId) ?? outputControl?.outputs[0] ?? null;

  useEffect(() => {
    setInputControlId(initialInputControlId);
    setInputId(inputMapping?.action.parameters.referenceInput ?? "");
    setArgument(inputMapping?.action.parameters.argument ?? "");
    setEventKind(inputMapping?.eventKind ?? "button-pushed");
    setOutputControlId(initialOutputControlId);
    setOutputId(outputMapping?.parameters.referenceOutput ?? "");
    setNotice(null);
  }, [initialInputControlId, initialOutputControlId, inputMapping, outputMapping]);

  useEffect(() => {
    if (!selectedInput) {
      return;
    }
    setInputId(selectedInput.inputId);
    setArgument((current) =>
      selectedInput.argumentOptions.some((option) => option.value === current)
        ? current
        : selectedInput.argumentOptions[0]?.value ?? (selectedInput.supportsEventValue ? "$event-value" : ""),
    );
  }, [selectedInput]);

  useEffect(() => {
    if (selectedOutput) {
      setOutputId(selectedOutput.outputId);
    }
  }, [selectedOutput]);

  const saveInput = async () => {
    if (!inputControl || !selectedInput || !argument) {
      return;
    }
    const mappings = isMomentaryInput(selectedInput)
      ? buildControlMappings(
          roleId,
          logicalControl.logicalControlId,
          adapterId,
          profileId,
          inputControl,
          selectedInput,
          eventKind === "button-pushed" ? "pulse" : "hold",
        )
      : [{
          roleId,
          logicalControlId: logicalControl.logicalControlId,
          eventKind,
          action: {
            actionId: "control-command",
            parameters: buildInputParameters(
              adapterId,
              profileId,
              inputControl,
              selectedInput,
              argument,
            ),
          },
        }];
    await onSaveInput(mappings);
    setNotice("Input mappingを保存しました。");
  };

  const saveOutput = async () => {
    if (!outputControl || !selectedOutput) {
      return;
    }
    await onSaveOutput({
      roleId,
      logicalControlId: logicalControl.logicalControlId,
      sourceId: "control-output",
      parameters: {
        referenceAdapter: adapterId,
        referenceProfile: profileId,
        referenceCategory: outputControl.category,
        referenceControl: outputControl.controlId,
        referenceOutput: selectedOutput.outputId,
      },
    });
    setNotice("Output mappingを保存しました。");
  };

  const triggerRoleAction = async (triggerEventKind: EventKind) => {
    setSendingEvent(triggerEventKind);
    setNotice(null);
    try {
      const actionCount = await onTriggerRoleInput({
        roleId,
        logicalControlId: logicalControl.logicalControlId,
        eventKind: triggerEventKind,
      });
      setNotice(`${eventLabels[triggerEventKind]} をRoleへ入力し、${actionCount}件のAdapter actionを実行しました。`);
    } catch (error) {
      setNotice(`Role操作に失敗しました: ${String(error)}`);
    } finally {
      setSendingEvent(null);
    }
  };

  const inputOptions = selectedInput?.argumentOptions ?? [];
  const momentary = selectedInput ? isMomentaryInput(selectedInput) : false;

  return (
    <article className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-2 border-b border-gray-100 pb-3">
        <div>
          <p className="font-semibold text-gray-900">{logicalControl.label}</p>
          <p className="font-mono text-xs text-gray-500">{roleId}:{logicalControl.logicalControlId}</p>
        </div>
        <span className="rounded-full border border-gray-200 bg-gray-50 px-2.5 py-1 text-xs text-gray-600">
          Input / Output
        </span>
      </div>

      <div className="grid gap-4 xl:grid-cols-2">
        <section className="rounded-md border border-blue-100 bg-blue-50/40 p-3">
          <div className="mb-3 flex items-center justify-between gap-2">
            <h4 className="text-sm font-semibold text-blue-950">Role Input → DCS-BIOS</h4>
            <span className="text-xs text-blue-700">{inputPersisted ? "保存済み" : "自動"}</span>
          </div>
          <div className="grid gap-2 sm:grid-cols-2">
            <select
              value={inputControl?.controlId ?? ""}
              onChange={(event) => {
                setInputControlId(event.target.value);
                setInputId("");
                setArgument("");
              }}
              className="rounded-md border border-gray-300 bg-white px-2 py-2 text-xs"
            >
              {inputControls.map((control) => (
                <option key={control.controlId} value={control.controlId}>
                  {control.controlId} — {control.description}
                </option>
              ))}
            </select>
            <select
              value={selectedInput?.inputId ?? ""}
              onChange={(event) => {
                setInputId(event.target.value);
                setArgument("");
              }}
              className="rounded-md border border-gray-300 bg-white px-2 py-2 text-xs"
            >
              {(inputControl?.inputs ?? []).map((input) => (
                <option key={input.inputId} value={input.inputId}>
                  {input.interface} — {input.description}
                </option>
              ))}
            </select>
            {momentary ? (
              <select
                value={eventKind === "button-pushed" ? "button-pushed" : "button-down"}
                onChange={(event) => setEventKind(event.target.value as EventKind)}
                className="rounded-md border border-gray-300 bg-white px-2 py-2 text-xs sm:col-span-2"
              >
                <option value="button-pushed">Button Pushed → 1（押す）→ 0（離す）</option>
                <option value="button-down">Button Down / Up → 1 / 0（押下状態を追従）</option>
              </select>
            ) : <>
              <select
                value={eventKind}
                onChange={(event) => setEventKind(event.target.value as EventKind)}
                className="rounded-md border border-gray-300 bg-white px-2 py-2 text-xs"
              >
                {logicalControl.supportedEvents.map((event) => (
                  <option key={event} value={event}>{eventLabels[event]}</option>
                ))}
              </select>
              {inputOptions.length > 0 ? (
              <select
                value={argument}
                onChange={(event) => setArgument(event.target.value)}
                className="rounded-md border border-gray-300 bg-white px-2 py-2 text-xs"
              >
                {inputOptions.map((option) => (
                  <option key={`${selectedInput?.inputId}:${option.value}`} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            ) : (
              <input
                value={argument}
                onChange={(event) => setArgument(event.target.value)}
                placeholder={selectedInput?.supportsEventValue ? "$event-value" : "argument"}
                className="rounded-md border border-gray-300 bg-white px-2 py-2 text-xs"
              />
              )}
            </>}
          </div>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => void saveInput()}
              disabled={busy || !inputControl || !selectedInput || !argument}
              className="inline-flex items-center gap-1.5 rounded-md bg-blue-600 px-3 py-2 text-xs font-medium text-white disabled:opacity-50"
            >
              <Save size={13} /> Input mappingを保存
            </button>
            {inputPersisted && (
              <button
                type="button"
                onClick={() => void onRemoveInput()}
                disabled={busy}
                className="inline-flex items-center gap-1.5 rounded-md border border-red-200 bg-white px-3 py-2 text-xs text-red-700 disabled:opacity-50"
              >
                <Trash2 size={13} /> 解除
              </button>
            )}
          </div>
          <div className="mt-3 border-t border-blue-100 pt-3">
            <p className="mb-1 text-xs font-medium text-blue-950">Role操作</p>
            <p className="mb-2 text-xs text-blue-700">実機入力と同じRoleイベントをAdapterマッピングへ流します。</p>
            <div className="flex flex-wrap gap-2">
              {logicalControl.supportedEvents.map((supportedEvent) => (
                <button
                  key={`trigger:${supportedEvent}`}
                  type="button"
                  onClick={() => void triggerRoleAction(supportedEvent)}
                  disabled={busy || sendingEvent !== null}
                  className="inline-flex items-center gap-1.5 rounded-md border border-blue-200 bg-white px-3 py-1.5 text-xs text-blue-700 transition active:scale-95 active:bg-blue-100 disabled:opacity-50"
                >
                  <Play size={12} /> {sendingEvent === supportedEvent ? "実行中…" : eventLabels[supportedEvent]}
                </button>
              ))}
            </div>
          </div>
        </section>

        <section className="rounded-md border border-emerald-100 bg-emerald-50/40 p-3">
          <div className="mb-3 flex items-center justify-between gap-2">
            <h4 className="text-sm font-semibold text-emerald-950">DCS-BIOS → Role Output</h4>
            <span className="text-xs text-emerald-700">{outputMapping ? "保存済み" : "未設定"}</span>
          </div>
          <div className="grid gap-2 sm:grid-cols-2">
            <select
              value={outputControl?.controlId ?? ""}
              onChange={(event) => {
                setOutputControlId(event.target.value);
                setOutputId("");
              }}
              className="rounded-md border border-gray-300 bg-white px-2 py-2 text-xs"
            >
              {outputControls.map((control) => (
                <option key={control.controlId} value={control.controlId}>
                  {control.controlId} — {control.description}
                </option>
              ))}
            </select>
            <select
              value={selectedOutput?.outputId ?? ""}
              onChange={(event) => setOutputId(event.target.value)}
              className="rounded-md border border-gray-300 bg-white px-2 py-2 text-xs"
            >
              {(outputControl?.outputs ?? []).map((output: AdapterOutputDefinition) => (
                <option key={output.outputId} value={output.outputId}>
                  {output.outputId} · 0x{output.address.toString(16).toUpperCase()}
                </option>
              ))}
            </select>
          </div>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => void saveOutput()}
              disabled={busy || !outputControl || !selectedOutput}
              className="inline-flex items-center gap-1.5 rounded-md bg-emerald-600 px-3 py-2 text-xs font-medium text-white disabled:opacity-50"
            >
              <Save size={13} /> Output mappingを保存
            </button>
            {outputMapping && (
              <button
                type="button"
                onClick={() => void onRemoveOutput()}
                disabled={busy}
                className="inline-flex items-center gap-1.5 rounded-md border border-red-200 bg-white px-3 py-2 text-xs text-red-700 disabled:opacity-50"
              >
                <Trash2 size={13} /> 解除
              </button>
            )}
          </div>
          {selectedOutput && (
            <p className="mt-3 text-xs text-emerald-800">
              {selectedOutput.description} · {selectedOutput.outputType} · length {selectedOutput.length ?? "auto"}
            </p>
          )}
        </section>
      </div>
      {notice && <p className="mt-3 text-xs text-gray-600">{notice}</p>}
    </article>
  );
}

export function RoleIoMapping({
  roleId,
  roleControls,
  aircraftName,
  adapterCatalog,
  adapterMappings,
  busyAction,
  onSaveAdapterMappings,
  onTriggerRoleInput,
}: RoleIoMappingProps) {
  const match = useMemo(
    () => findAdapterProfileForAircraft(adapterCatalog, aircraftName),
    [adapterCatalog, aircraftName],
  );
  const roleBinding = match?.profile.roleBindings.find((binding) => binding.roleId === roleId) ?? null;
  const controls = useMemo(
    () =>
      match && roleBinding
        ? match.profile.controls.filter((control) => control.category === roleBinding.category)
        : [],
    [match, roleBinding],
  );
  const persistedConfig = useMemo(() => {
    if (!match || !aircraftName) {
      return null;
    }
    const normalizedAircraft = normalizeAircraftName(aircraftName);
    return (
      adapterMappings.find(
        (config) =>
          config.adapterId === match.adapter.adapterId &&
          config.profileId === match.profile.profileId &&
          normalizeAircraftName(config.aircraftName ?? "") === normalizedAircraft,
      ) ?? null
    );
  }, [adapterMappings, aircraftName, match]);
  const defaultConfig = useMemo(
    () =>
      match && aircraftName
        ? buildDefaultConfig(match.adapter.adapterId, match.profile, aircraftName)
        : null,
    [aircraftName, match],
  );
  const effectiveConfig = persistedConfig ?? defaultConfig;

  const saveConfig = async (next: AdapterMappingConfig) => {
    const index = persistedConfig ? adapterMappings.indexOf(persistedConfig) : -1;
    const nextMappings = [...adapterMappings];
    if (index >= 0) {
      nextMappings[index] = next;
    } else {
      nextMappings.push(next);
    }
    await onSaveAdapterMappings(nextMappings);
  };

  if (!match || !roleBinding || !effectiveConfig) {
    return (
      <section className="rounded-lg border border-amber-200 bg-amber-50 p-5">
        <h3 className="font-semibold text-amber-950">Role Input / Output</h3>
        <p className="mt-2 text-sm text-amber-800">
          {roleControls.length}個のRole I/Oを利用できます。航空機プロファイルが確定すると、ここでAdapter Input/Outputを直接設定できます。
        </p>
      </section>
    );
  }

  return (
    <section className="space-y-4">
      <div className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
        <h3 className="text-xl font-semibold text-gray-900">Role Input / Output 一覧</h3>
        <p className="mt-1 text-sm text-gray-500">
          {match.profile.label} · {roleBinding.category}。各Role I/Oに対してDCS-BIOSのInput/Outputを直接割り当て、Inputを手動操作できます。
        </p>
      </div>
      <div className="space-y-3">
        {roleControls.map((logicalControl, index) => {
          const inputMappings = effectiveConfig.mappings.filter(
            (mapping) =>
              mapping.roleId === roleId &&
              mapping.logicalControlId === logicalControl.logicalControlId,
          );
          const outputMapping =
            (effectiveConfig.outputMappings ?? []).find(
              (mapping) =>
                mapping.roleId === roleId &&
                mapping.logicalControlId === logicalControl.logicalControlId,
            ) ?? null;
          const inputPersisted =
            persistedConfig?.mappings.some(
              (mapping) =>
                mapping.roleId === roleId &&
                mapping.logicalControlId === logicalControl.logicalControlId,
            ) ?? false;

          return (
            <RoleIoRow
              key={logicalControl.logicalControlId}
              roleId={roleId}
              logicalControl={logicalControl}
              adapterId={match.adapter.adapterId}
              profileId={match.profile.profileId}
              controls={controls}
              suggestedControl={controls[index] ?? null}
              inputMappings={inputMappings}
              outputMapping={outputMapping}
              inputPersisted={inputPersisted}
              busy={busyAction !== null}
              onSaveInput={async (mappings) => {
                await saveConfig({
                  ...effectiveConfig,
                  aircraftName,
                  mappings: [
                    ...effectiveConfig.mappings.filter(
                      (candidate) =>
                        candidate.roleId !== roleId ||
                        candidate.logicalControlId !== logicalControl.logicalControlId,
                    ),
                    ...mappings,
                  ],
                });
              }}
              onRemoveInput={async () => {
                await saveConfig({
                  ...effectiveConfig,
                  aircraftName,
                  mappings: effectiveConfig.mappings.filter(
                    (candidate) =>
                      candidate.roleId !== roleId ||
                      candidate.logicalControlId !== logicalControl.logicalControlId,
                  ),
                });
              }}
              onSaveOutput={async (mapping) => {
                await saveConfig({
                  ...effectiveConfig,
                  aircraftName,
                  outputMappings: [
                    ...(effectiveConfig.outputMappings ?? []).filter(
                      (candidate) =>
                        candidate.roleId !== roleId ||
                        candidate.logicalControlId !== logicalControl.logicalControlId,
                    ),
                    mapping,
                  ],
                });
              }}
              onRemoveOutput={async () => {
                await saveConfig({
                  ...effectiveConfig,
                  aircraftName,
                  outputMappings: (effectiveConfig.outputMappings ?? []).filter(
                    (candidate) =>
                      candidate.roleId !== roleId ||
                      candidate.logicalControlId !== logicalControl.logicalControlId,
                  ),
                });
              }}
              onTriggerRoleInput={onTriggerRoleInput}
            />
          );
        })}
      </div>
    </section>
  );
}
