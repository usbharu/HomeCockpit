"use client";

import React from 'react';
import * as Tabs from '@radix-ui/react-tabs';
import {
  Activity,
  Cable,
  ArrowRightLeft,
  Radar,
} from 'lucide-react';

import { useManagerState } from "@/lib/use-manager-state";
import { SoftwareSettings } from "@/components/tabs/software-settings";
import DeviceSettings from "@/components/tabs/device-settings";
import MappingSettings from "@/components/tabs/mapping-settings";
import StatusPage from "@/components/tabs/status-page";


export default function ManagerTabs() {
    const {
        snapshot,
        runtimeError,
        busyAction,
        serialPorts,
        saveConfig,
        startDcsBios,
        stopDcsBios,
        refreshDevices,
        saveDeviceEndpoints,
        saveDeviceRoleAssignments,
        saveRoleMappings,
    } = useManagerState();

    const tabs = [
        {
            id: 'software',
            label: 'ソフトウェア接続',
            icon: Cable,
            content: (
                <SoftwareSettings
                    config={snapshot.dcsbiosConfig}
                    status={snapshot.dcsbiosStatus}
                    busyAction={busyAction}
                    runtimeError={runtimeError}
                    onSave={saveConfig}
                    onStart={startDcsBios}
                    onStop={stopDcsBios}
                />
            ),
        },
        {
            id: 'device',
            label: 'デバイス設定',
            icon: Radar,
            content: (
                <DeviceSettings
                    devices={snapshot.devices}
                    deviceEndpoints={snapshot.deviceEndpoints}
                    deviceRoleAssignments={snapshot.deviceRoleAssignments}
                    serialPorts={serialPorts}
                    busyAction={busyAction}
                    onRefresh={refreshDevices}
                    onSaveEndpoints={saveDeviceEndpoints}
                    onSaveDeviceRoleAssignments={saveDeviceRoleAssignments}
                />
            ),
        },
        {
            id: 'mapping',
            label: 'マッピング',
            icon: ArrowRightLeft,
            content: (
                <MappingSettings
                    devices={snapshot.devices}
                    deviceRoleAssignments={snapshot.deviceRoleAssignments}
                    roleMappings={snapshot.roleMappings}
                    busyAction={busyAction}
                    onSaveRoleMappings={saveRoleMappings}
                />
            ),
        },
        {
            id: 'status',
            label: 'ログ',
            icon: Activity,
            content: <StatusPage logs={snapshot.logs} status={snapshot.dcsbiosStatus} />,
        },
    ];

    return (
        <Tabs.Root defaultValue="software" className="flex h-screen flex-col bg-gray-50 text-gray-900">
            <header className="z-10 border-b border-gray-200 bg-white shadow-sm">
                <div className="flex items-center justify-between px-6 pt-5">
                    <div>
                        <p className="text-xs uppercase tracking-[0.35em] text-gray-400">HomeCockpit Manager</p>
                        <h1 className="text-2xl font-semibold text-gray-900">接続管理コンソール</h1>
                    </div>
                    <div className="rounded-full border border-gray-200 bg-gray-100 px-4 py-2 text-sm text-gray-700">
                        {snapshot.dcsbiosStatus.connectionState}
                    </div>
                </div>
                <Tabs.List className="flex px-6 pt-2 -mb-px">
                    {tabs.map(tab => {
                        const Icon = tab.icon;
                        return (
                            <Tabs.Trigger
                                key={tab.id}
                                value={tab.id}
                                className="flex items-center space-x-2 border-b-2 px-4 py-3 text-sm font-medium transition-colors
                data-[state=active]:border-blue-600 data-[state=active]:text-blue-600
                data-[state=inactive]:border-transparent data-[state=inactive]:text-gray-500 hover:text-gray-800 hover:border-gray-300
                focus:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-blue-500"
                            >
                                <Icon size={18} />
                                <span>{tab.label}</span>
                            </Tabs.Trigger>
                        );
                    })}
                </Tabs.List>
            </header>
            <main className="flex-1 overflow-hidden">
                {tabs.map(tab => (
                    <Tabs.Content
                        key={tab.id}
                        value={tab.id}
                        className="h-full focus:outline-none"
                    >
                        {tab.content}
                    </Tabs.Content>
                ))}
            </main>
        </Tabs.Root>
    );
}
