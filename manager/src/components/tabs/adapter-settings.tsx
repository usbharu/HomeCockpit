"use client";

import { useEffect, useMemo, useState } from "react";
import {
  CheckCircle2,
  FileJson,
  Plane,
  RefreshCw,
  Save,
  Settings2,
  ShieldCheck,
} from "lucide-react";

import { findAdapterProfileForAircraft } from "@/lib/control-catalog";
import type {
  AdapterCatalog,
  AdapterProfile,
  AdapterProfileImportRequest,
  DcsBiosStatus,
} from "@/lib/manager-types";

type AdapterSettingsProps = {
  status: DcsBiosStatus;
  adapterCatalog: AdapterCatalog;
  busyAction: string | null;
  onPreviewAdapterProfile: (request: AdapterProfileImportRequest) => Promise<AdapterProfile>;
  onSaveAdapterProfile: (request: AdapterProfileImportRequest) => Promise<void>;
};

type ProfileForm = {
  adapterId: string;
  profileId: string;
  label: string;
  aircraftNames: string;
  source: string;
};

const emptyProfileForm: ProfileForm = {
  adapterId: "",
  profileId: "",
  label: "",
  aircraftNames: "",
  source: "",
};

function splitAircraftNames(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((name) => name.trim())
    .filter((name, index, names) => name.length > 0 && names.indexOf(name) === index);
}

function getCategories(profile: AdapterProfile | null): string[] {
  if (!profile) {
    return [];
  }

  return Array.from(new Set(profile.controls.map((control) => control.category))).sort();
}

function ProfileSummary({
  adapterLabel,
  profile,
}: {
  adapterLabel: string;
  profile: AdapterProfile;
}) {
  const categories = getCategories(profile);

  return (
    <div className="rounded-lg border border-emerald-200 bg-emerald-50 p-5">
      <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
        <div>
          <div className="flex items-center gap-2 text-sm font-medium text-emerald-800">
            <ShieldCheck size={17} />
            自動プロファイルを適用中
          </div>
          <h3 className="mt-1 text-xl font-semibold text-gray-900">{profile.label}</h3>
          <p className="mt-1 text-sm text-gray-600">
            {adapterLabel} · {profile.profileId} · {profile.controlCount} controls
          </p>
        </div>
        <span className="rounded-full border border-emerald-200 bg-white px-3 py-1 text-xs text-emerald-800">
          Role → Category 内蔵
        </span>
      </div>

      <div className="mt-4 grid gap-3 md:grid-cols-2">
        <div className="rounded-md border border-emerald-100 bg-white p-3">
          <p className="text-xs font-medium uppercase tracking-wide text-gray-400">Aircraft names</p>
          <p className="mt-1 text-sm text-gray-700">
            {profile.aircraftNames.length > 0 ? profile.aircraftNames.join(", ") : "未設定"}
          </p>
        </div>
        <div className="rounded-md border border-emerald-100 bg-white p-3">
          <p className="text-xs font-medium uppercase tracking-wide text-gray-400">Categories</p>
          <p className="mt-1 text-sm text-gray-700">{categories.join(", ") || "未設定"}</p>
        </div>
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        {profile.roleBindings.length === 0 ? (
          <span className="text-sm text-amber-700">Role結線がないため入力マッピングは生成されません。</span>
        ) : (
          profile.roleBindings.map((binding) => (
            <span
              key={`${binding.roleId}:${binding.category}`}
              className="rounded-full border border-emerald-200 bg-white px-3 py-1.5 text-xs text-gray-700"
            >
              {binding.roleId} → {binding.category}
            </span>
          ))
        )}
      </div>
    </div>
  );
}

export function AdapterSettings({
  status,
  adapterCatalog,
  busyAction,
  onPreviewAdapterProfile,
  onSaveAdapterProfile,
}: AdapterSettingsProps) {
  const [selectedAdapterId, setSelectedAdapterId] = useState(
    adapterCatalog.adapters[0]?.adapterId ?? "",
  );
  const [form, setForm] = useState<ProfileForm>(emptyProfileForm);
  const [previewProfile, setPreviewProfile] = useState<AdapterProfile | null>(null);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const currentProfileMatch = useMemo(
    () => findAdapterProfileForAircraft(adapterCatalog, status.aircraftName),
    [adapterCatalog, status.aircraftName],
  );

  useEffect(() => {
    if (adapterCatalog.adapters.some((adapter) => adapter.adapterId === selectedAdapterId)) {
      return;
    }
    setSelectedAdapterId(adapterCatalog.adapters[0]?.adapterId ?? "");
  }, [adapterCatalog.adapters, selectedAdapterId]);

  const beginProfileEditor = () => {
    setEditing(true);
    setError(null);
    setNotice(null);
    setPreviewProfile(null);
    setForm((current) => ({
      ...emptyProfileForm,
      adapterId: selectedAdapterId || current.adapterId,
      aircraftNames: status.aircraftName ?? "",
    }));
  };

  const buildRequest = (): AdapterProfileImportRequest | null => {
    const aircraftNames = splitAircraftNames(form.aircraftNames);
    if (!form.adapterId.trim() || !form.profileId.trim() || !form.label.trim()) {
      setError("Adapter、Profile ID、表示名を入力してください。");
      return null;
    }
    if (aircraftNames.length === 0) {
      setError("少なくとも1つの航空機名を入力してください。");
      return null;
    }
    if (!form.source.trim()) {
      setError("Adapter定義ソースを入力してください。");
      return null;
    }
    return {
      adapterId: form.adapterId.trim(),
      profileId: form.profileId.trim(),
      label: form.label.trim(),
      aircraftNames,
      source: form.source,
    };
  };

  const preview = async () => {
    setError(null);
    setNotice(null);
    const request = buildRequest();
    if (!request) {
      return;
    }
    try {
      const profile = await onPreviewAdapterProfile(request);
      setPreviewProfile(profile);
      setNotice(`${profile.controlCount}個のコントロールを読み込みました。Adapter内蔵のRole対応を確認して保存してください。`);
    } catch (previewError) {
      setError(String(previewError));
    }
  };

  const save = async () => {
    setError(null);
    setNotice(null);
    const request = buildRequest();
    if (!request || !previewProfile) {
      if (!previewProfile && !error) {
        setError("先にAdapter定義を読み込んでください。");
      }
      return;
    }
    try {
      await onSaveAdapterProfile(request);
      setNotice("Adapterプロファイルを保存しました。現在の航空機に自動適用されます。");
      setEditing(false);
    } catch (saveError) {
      setError(String(saveError));
    }
  };

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
        <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
          <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
            <div>
              <div className="flex items-center gap-2 text-sm font-medium text-gray-500">
                <Settings2 size={17} />
                Adapter設定
              </div>
              <h2 className="mt-1 text-3xl font-semibold tracking-tight text-gray-900">
                航空機プロファイル
              </h2>
              <p className="mt-2 max-w-3xl text-sm text-gray-500">
                Adapterは接続先ゲームから現在の航空機を判定し、論理コントロールへ自動マッピングします。RoleとAdapterの対応やメモリアドレスは、Mapping画面では設定しません。
              </p>
            </div>
            <div className="rounded-lg border border-gray-200 bg-gray-50 px-4 py-3 text-sm text-gray-700">
              <p className="text-xs text-gray-400">接続状態</p>
              <p className="mt-1 font-medium">{status.connectionState}</p>
            </div>
          </div>

          <div className="mt-6 grid gap-4 md:grid-cols-2">
            <div className="flex items-start gap-3 rounded-lg border border-blue-100 bg-blue-50/60 p-4">
              <Plane className="mt-0.5 text-blue-600" size={19} />
              <div>
                <p className="text-xs font-medium uppercase tracking-wide text-blue-700">Current aircraft</p>
                <p className="mt-1 text-lg font-semibold text-gray-900">
                  {status.aircraftName ?? "未検出"}
                </p>
                <p className="mt-1 text-xs text-gray-500">
                  {status.aircraftName
                    ? "Adapterのプロファイル照合に使用しています。"
                    : "ゲーム接続後にAdapterが機体名を取得します。"}
                </p>
              </div>
            </div>
            <div className="rounded-lg border border-gray-200 bg-gray-50 p-4">
              <p className="text-xs font-medium uppercase tracking-wide text-gray-400">Catalog</p>
              <p className="mt-1 text-lg font-semibold text-gray-900">
                {adapterCatalog.adapters.length} adapter(s) / {adapterCatalog.adapters.reduce((count, adapter) => count + adapter.profiles.length, 0)} profile(s)
              </p>
              <p className="mt-1 text-xs text-gray-500">
                {adapterCatalog.state === "loaded"
                  ? "内蔵カタログと保存済みプロファイルを使用中"
                  : adapterCatalog.error ?? "Adapterカタログを利用できません。"}
              </p>
            </div>
          </div>
        </section>

        {currentProfileMatch ? (
          <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
            <div className="mb-4 flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
              <div>
                <h2 className="text-xl font-semibold text-gray-900">現在の自動マッピング</h2>
                <p className="mt-1 text-sm text-gray-500">
                  機体名に一致したプロファイルから、論理コントロールとAdapterアクションを生成しています。
                </p>
              </div>
              <button
                type="button"
                onClick={beginProfileEditor}
                className="inline-flex items-center justify-center gap-2 rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
              >
                <FileJson size={16} />
                別の機体プロファイルを作成
              </button>
            </div>
            <ProfileSummary
              adapterLabel={currentProfileMatch.adapter.label}
              profile={currentProfileMatch.profile}
            />
          </section>
        ) : (
          <section className="rounded-lg border border-amber-200 bg-amber-50 p-6 shadow-sm">
            <div className="flex items-start gap-3">
              <RefreshCw className="mt-0.5 text-amber-700" size={19} />
              <div>
                <h2 className="text-xl font-semibold text-amber-950">未知の航空機</h2>
                <p className="mt-1 text-sm text-amber-900">
                  一致するプロファイルがないため、Adapterアクションはまだ生成されていません。下のAdapter定義を読み込んでプロファイルを作成してください。
                </p>
              </div>
            </div>
          </section>
        )}

        {editing && (
          <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
            <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
              <div>
                <h2 className="text-xl font-semibold text-gray-900">Adapterプロファイルを作成</h2>
                <p className="mt-1 text-sm text-gray-500">
                  ゲーム側Adapterが提供するJSON / JSONP定義を読み込みます。Manager PCにゲーム本体がインストールされている必要はありません。
                </p>
              </div>
              {previewProfile && (
                <span className="inline-flex items-center gap-2 rounded-full border border-emerald-200 bg-emerald-50 px-3 py-1.5 text-xs text-emerald-800">
                  <CheckCircle2 size={14} /> 読み込み済み
                </span>
              )}
            </div>

            <div className="mt-5 grid gap-4 lg:grid-cols-2">
              <label className="space-y-2 text-sm text-gray-700">
                <span>Adapter</span>
                <select
                  value={form.adapterId || selectedAdapterId}
                  onChange={(event) => {
                    setSelectedAdapterId(event.target.value);
                    setForm((current) => ({ ...current, adapterId: event.target.value }));
                  }}
                  disabled={busyAction !== null || adapterCatalog.adapters.length === 0}
                  className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                >
                  {adapterCatalog.adapters.map((adapter) => (
                    <option key={adapter.adapterId} value={adapter.adapterId}>
                      {adapter.label} ({adapter.adapterId})
                    </option>
                  ))}
                </select>
              </label>
              <label className="space-y-2 text-sm text-gray-700">
                <span>Profile ID</span>
                <input
                  value={form.profileId}
                  onChange={(event) => setForm((current) => ({ ...current, profileId: event.target.value }))}
                  placeholder="f-16c-unknown-block"
                  className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                />
              </label>
              <label className="space-y-2 text-sm text-gray-700">
                <span>表示名</span>
                <input
                  value={form.label}
                  onChange={(event) => setForm((current) => ({ ...current, label: event.target.value }))}
                  placeholder="F-16C custom profile"
                  className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                />
              </label>
              <label className="space-y-2 text-sm text-gray-700">
                <span>航空機名（改行またはカンマ区切り）</span>
                <input
                  value={form.aircraftNames}
                  onChange={(event) => setForm((current) => ({ ...current, aircraftNames: event.target.value }))}
                  placeholder="F-16C_52"
                  className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                />
              </label>
            </div>

            <label className="mt-4 block space-y-2 text-sm text-gray-700">
              <span>Adapter定義ソース（JSON / JSONP）</span>
              <textarea
                value={form.source}
                onChange={(event) => {
                  setForm((current) => ({ ...current, source: event.target.value }));
                  setPreviewProfile(null);
                }}
                rows={12}
                placeholder={'{"MFD Left":{"MFD_L_1":{"identifier":"MFD_L_1","inputs":[]}}}'}
                className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 font-mono text-xs outline-none transition focus:border-blue-500"
              />
            </label>

            {previewProfile && (
              <div className="mt-5">
                <ProfileSummary
                  adapterLabel={
                    adapterCatalog.adapters.find(
                      (adapter) => adapter.adapterId === form.adapterId,
                    )?.label ?? form.adapterId
                  }
                  profile={previewProfile}
                />
              </div>
            )}

            {error && <p className="mt-4 rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">{error}</p>}
            {notice && <p className="mt-4 rounded-md border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-800">{notice}</p>}

            <div className="mt-5 flex flex-wrap justify-end gap-3">
              <button
                type="button"
                onClick={() => {
                  setEditing(false);
                  setError(null);
                  setNotice(null);
                }}
                className="rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
              >
                閉じる
              </button>
              <button
                type="button"
                onClick={() => void preview()}
                disabled={busyAction !== null}
                className="inline-flex items-center gap-2 rounded-md border border-blue-200 bg-blue-50 px-4 py-2 text-sm font-medium text-blue-700 hover:bg-blue-100 disabled:cursor-not-allowed disabled:opacity-60"
              >
                <RefreshCw size={16} />
                定義を読み込む
              </button>
              <button
                type="button"
                onClick={() => void save()}
                disabled={busyAction !== null || !previewProfile}
                className="inline-flex items-center gap-2 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
              >
                <Save size={16} />
                プロファイルを保存
              </button>
            </div>
          </section>
        )}

        {!editing && !currentProfileMatch && (
          <button
            type="button"
            onClick={beginProfileEditor}
            className="inline-flex items-center justify-center gap-2 rounded-md bg-blue-600 px-4 py-3 text-sm font-medium text-white hover:bg-blue-700"
          >
            <FileJson size={16} />
            未知の航空機のプロファイルを作成
          </button>
        )}
      </div>
    </div>
  );
}

export default AdapterSettings;
