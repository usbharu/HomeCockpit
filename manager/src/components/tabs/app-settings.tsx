"use client";

import React, { useState } from 'react';

const AppSettings = () => {
    const [autostart, setAutostart] = useState(true);
    const [theme, setTheme] = useState('system');

    return (
        <div className="p-8 space-y-8">
            <h2 className="text-2xl font-semibold text-gray-800">アプリケーション設定</h2>

            <div className="space-y-4">
                <h3 className="text-lg font-semibold text-gray-700">一般</h3>
                <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200">
                    <div className="flex items-center justify-between">
                        <div>
                            <p className="font-medium text-gray-900">PC起動時にアプリを自動で起動する</p>
                            <p className="text-sm text-gray-500">バックグラウンドで起動し、すぐにデバイスを利用できます。</p>
                        </div>
                        <button onClick={() => setAutostart(!autostart)} className={`relative inline-flex items-center h-6 rounded-full w-11 transition-colors ${autostart ? 'bg-blue-600' : 'bg-gray-300'}`}>
                            <span className={`inline-block w-4 h-4 transform bg-white rounded-full transition-transform ${autostart ? 'translate-x-6' : 'translate-x-1'}`}/>
                        </button>
                    </div>
                </div>
            </div>

            <div className="space-y-4">
                <h3 className="text-lg font-semibold text-gray-700">外観</h3>
                <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200">
                    <p className="font-medium text-gray-900 mb-2">テーマ</p>
                    <div className="flex space-x-2">
                        {['ライト', 'ダーク', 'システム'].map(t => (
                            <button
                                key={t}
                                onClick={() => setTheme(t)}
                                className={`px-4 py-2 rounded-md text-sm font-medium transition-colors ${
                                    theme === t ? 'bg-blue-600 text-white' : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
                                }`}
                            >
                                {t}
                            </button>
                        ))}
                    </div>
                </div>
            </div>

            <div className="space-y-4">
                <h3 className="text-lg font-semibold text-gray-700">バージョン情報</h3>
                <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200 flex items-center justify-between">
                    <div>
                        <p className="font-medium text-gray-900">現在のバージョン: 1.0.0</p>
                        <p className="text-sm text-gray-500">お使いのバージョンは最新です。</p>
                    </div>
                    <button className="bg-white text-gray-800 px-4 py-2 rounded-md border border-gray-300 hover:bg-gray-50 transition-colors font-medium text-sm">
                        アップデートを確認
                    </button>
                </div>
            </div>
        </div>
    );
};

export default AppSettings;
