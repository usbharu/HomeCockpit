"use client";

import React from 'react';
import * as Tabs from '@radix-ui/react-tabs';
import { Cable, ToyBrick, AreaChart, Settings as SettingsIcon } from 'lucide-react';
import { SoftwareSettings } from "@/components/tabs/software-settings";
import DeviceSettings from "@/components/tabs/device-settings";
import StatusPage from "@/components/tabs/status-page";
import AppSettings from "@/components/tabs/app-settings";


export default function ManagerTabs() {
    const tabs = [
        { id: 'software', label: 'ソフトウェア接続', icon: Cable, content: <SoftwareSettings /> },
        { id: 'device', label: 'デバイス設定', icon: ToyBrick, content: <DeviceSettings /> },
        { id: 'status', label: 'ステータス', icon: AreaChart, content: <StatusPage /> },
        { id: 'settings', label: 'アプリ設定', icon: SettingsIcon, content: <AppSettings /> },
    ];

    return (
        <Tabs.Root defaultValue="device" className="flex flex-col h-screen">
            <header className="bg-white border-b border-gray-200 shadow-sm z-10">
                <Tabs.List className="flex px-6 pt-2 -mb-px">
                    {tabs.map(tab => {
                        const Icon = tab.icon;
                        return (
                            <Tabs.Trigger
                                key={tab.id}
                                value={tab.id}
                                className="flex items-center space-x-2 px-4 py-3 text-sm font-medium border-b-2 transition-colors
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
            <main className="flex-1">
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


