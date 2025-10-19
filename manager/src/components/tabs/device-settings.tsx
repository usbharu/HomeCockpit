"use client";

import React, { useState } from 'react';
import { Gamepad2, Keyboard, MousePointerClick, HardDrive } from 'lucide-react';

const DeviceSettings = () => {
    const [devices, setDevices] = useState([
        { id: 1, name: 'My Custom Controller Alpha', type: 'Gamepad', firmware: 'v1.2.3', battery: 92, editing: false, icon: Gamepad2 },
        { id: 2, name: 'Foot Pedal Pro', type: 'Pedal', firmware: 'v0.9.1', battery: 78, editing: false, icon: MousePointerClick },
        { id: 3, name: 'Stream Deck Mini', type: 'Keyboard', firmware: 'v2.5.0', battery: null, editing: false, icon: Keyboard },
        { id: 4, name: 'Audio Mixer Lite', type: 'Mixer', firmware: 'v1.0.8', battery: null, editing: false, icon: HardDrive },
        { id: 5, name: 'Super Controller Omega', type: 'Gamepad', firmware: 'v3.0.1', battery: 55, editing: false, icon: Gamepad2 },
    ]);
    const [selectedDeviceId, setSelectedDeviceId] = useState(devices[0]?.id || null);

    const handleNameChange = (id, newName) => {
        setDevices(devices.map(d => d.id === id ? { ...d, name: newName } : d));
    };

    const toggleEditing = (id) => {
        setDevices(devices.map(d => d.id === id ? { ...d, editing: !d.editing } : { ...d, editing: false }));
    };

    const selectedDevice = devices.find(d => d.id === selectedDeviceId);

    // デバイス設定の詳細を表示するコンポーネント
    const DeviceDetail = ({ device }) => {
        if (!device) {
            return <div className="p-8 text-center text-gray-500">デバイスを選択してください。</div>;
        }

        return (
            <div className="p-8 space-y-6">
                <div className="flex items-start justify-between">
                    <div>
                        {device.editing ? (
                            <input
                                type="text"
                                value={device.name}
                                onChange={(e) => handleNameChange(device.id, e.target.value)}
                                onBlur={() => toggleEditing(device.id)}
                                className="text-2xl font-semibold text-gray-800 bg-transparent border-b-2 border-blue-500 focus:outline-none"
                                autoFocus
                            />
                        ) : (
                            <h2 className="text-2xl font-semibold text-gray-800" onClick={() => toggleEditing(device.id)}>{device.name}</h2>
                        )}
                        <p className="text-sm text-gray-500">デバイスタイプ: {device.type}</p>
                    </div>
                    <div className="flex items-center space-x-4 text-sm">
                        {device.battery !== null && <span className="text-gray-600">バッテリー: {device.battery}%</span>}
                        <span className="bg-gray-200 text-gray-700 px-2 py-1 rounded">FW: {device.firmware}</span>
                    </div>
                </div>

                <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200 space-y-4">
                    <h3 className="font-semibold text-gray-700">基本操作</h3>
                    <div className="flex space-x-4">
                        <button className="bg-gray-100 text-gray-800 px-4 py-2 rounded-md hover:bg-gray-200 transition-colors font-medium text-sm">
                            キャリブレーション
                        </button>
                        <button className="bg-gray-100 text-gray-800 px-4 py-2 rounded-md hover:bg-gray-200 transition-colors font-medium text-sm">
                            設定バックアップ
                        </button>
                        <button className="bg-gray-100 text-gray-800 px-4 py-2 rounded-md hover:bg-gray-200 transition-colors font-medium text-sm">
                            設定を復元
                        </button>
                    </div>
                </div>
                <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200 space-y-4">
                    <h3 className="font-semibold text-gray-700">危険な操作</h3>
                    <div className="flex items-center justify-between">
                        <p className="text-sm text-gray-600">このデバイスの登録を解除します。再接続には初期設定が必要です。</p>
                        <button className="bg-red-100 text-red-700 px-4 py-2 rounded-md hover:bg-red-200 transition-colors font-medium text-sm">
                            デバイス登録解除
                        </button>
                    </div>
                </div>
            </div>
        );
    }

    return (
        <div className="flex h-full">
            {/* 左側の縦タブ */}
            <div className="w-24 bg-white border-r border-gray-200 p-2 space-y-2 overflow-y-auto">
                {devices.map(device => {
                    const Icon = device.icon;
                    const isActive = device.id === selectedDeviceId;
                    return (
                        <div key={device.id} className="relative">
                            <button
                                onClick={() => setSelectedDeviceId(device.id)}
                                className={`w-full h-20 p-2 flex flex-col items-center justify-center rounded-lg transition-colors duration-200 ${
                                    isActive
                                        ? 'bg-blue-100 text-blue-700'
                                        : 'bg-gray-50 text-gray-500 hover:bg-gray-200 hover:text-gray-800'
                                }`}
                            >
                                <Icon size={28} />
                                <span className="text-xs mt-2 truncate w-full text-center">{device.type}</span>
                            </button>
                        </div>
                    );
                })}
            </div>

            {/* 右側の設定コンテンツ */}
            <div className="flex-1 overflow-y-auto bg-gray-50 h-full">
                <DeviceDetail device={selectedDevice} />
            </div>
        </div>
    );
};

export default DeviceSettings;

