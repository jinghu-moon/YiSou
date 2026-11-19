#include "localization.h"
#include "windows_services.h"
#include "browser.h"
#include "config.h"
#include "performance.h"
#include "shortcut.h"

#include <windows.h>
#include <shellapi.h>
#include <objbase.h>

#include <algorithm>
#include <array>
#include <cstring>
#include <span>

namespace yisou::platform {
using i18n::Text;
using i18n::Tr;

namespace {

constexpr wchar_t settingsKey[] = LR"(Software\YiSou)";
constexpr wchar_t startupKey[] = LR"(Software\Microsoft\Windows\CurrentVersion\Run)";
constexpr wchar_t startupName[] = L"YiSou";
constexpr DWORD showOnManualLaunchFlag = 1;
constexpr DWORD closeToTrayFlag = 2;
constexpr wchar_t engineKey[] = LR"(Software\YiSou\CustomEngines)";
constexpr wchar_t engineListName[] = L"EngineList";
constexpr wchar_t historyKey[] = LR"(Software\YiSou\SearchHistory)";
constexpr wchar_t displayKey[] = LR"(Software\YiSou\DisplayPositions)";
constexpr DWORD maxEngineListBytes = 4 * 1024 * 1024;
constexpr DWORD previewShortcutFlag = 0x1000;
constexpr size_t iconBytes = iconSize * iconSize * sizeof(std::uint32_t);
constexpr std::uint32_t svgFlag = 0x80000000;
// A record contains two NUL-terminated UTF-16 strings, one RGB color, and optional PBGRA pixels.
constexpr size_t maxRecord = (engineNameLimit + engineUrlLimit + 2) * sizeof(wchar_t) + sizeof(std::uint32_t) + iconBytes;

void AppendField(std::wstring& record, const std::wstring& field)
{
    record.append(field);
    record.push_back(L'\0');
}

bool ReadField(std::span<const wchar_t>& record, std::wstring& field)
{
    const auto end = std::ranges::find(record, L'\0');
    if (end == record.end()) return false;
    field.assign(record.begin(), end);
    record = record.subspan(field.size() + 1);
    return true;
}

bool ReadEngineRecord(Engine& engine, std::span<const wchar_t> record)
{
    if (!ReadField(record, engine.name) || !ReadField(record, engine.url)) return false;
    if (record.size_bytes() < sizeof(engine.color)) return false;
    std::memcpy(&engine.color, record.data(), sizeof(engine.color));
    record = record.subspan(sizeof(engine.color) / sizeof(wchar_t));
    if (engine.color & svgFlag) {
        engine.color &= ~svgFlag;
        if (record.size_bytes() < sizeof(std::uint32_t)) return false;
        std::uint32_t svgBytes = 0;
        std::memcpy(&svgBytes, record.data(), sizeof(svgBytes));
        record = record.subspan(sizeof(svgBytes) / sizeof(wchar_t));
        const auto paddedBytes = svgBytes + svgBytes % sizeof(wchar_t);
        if (!svgBytes || svgBytes > svgIconLimit || record.size_bytes() != paddedBytes) return false;
        engine.svgIcon.assign(reinterpret_cast<const char*>(record.data()), svgBytes);
    } else if (!record.empty()) {
        if (record.size_bytes() != iconBytes) return false;
        engine.icon.resize(iconSize * iconSize);
        std::memcpy(engine.icon.data(), record.data(), iconBytes);
    }
    return true;
}

bool ReadEngineList(std::vector<Engine>& result)
{
    DWORD bytes = 0;
    if (RegGetValueW(HKEY_CURRENT_USER, settingsKey, engineListName, RRF_RT_REG_BINARY, nullptr, nullptr, &bytes)
        != ERROR_SUCCESS) return false;
    if (bytes > maxEngineListBytes || bytes % sizeof(wchar_t)) return true;
    std::wstring data(bytes / sizeof(wchar_t), L'\0');
    if (RegGetValueW(HKEY_CURRENT_USER, settingsKey, engineListName, RRF_RT_REG_BINARY, nullptr, data.data(), &bytes)
        != ERROR_SUCCESS) return true;
    std::span<const wchar_t> record(data.data(), bytes / sizeof(wchar_t));
    while (!record.empty()) {
        Engine engine;
        if (!ReadField(record, engine.id) || !ReadField(record, engine.alias) || record.empty()) break;
        const size_t length = record.front();
        record = record.subspan(1);
        if (length > record.size() || !ReadEngineRecord(engine, record.first(length))) break;
        result.push_back(std::move(engine));
        record = record.subspan(length);
    }
    return true;
}

std::wstring ReadString(const wchar_t* name)
{
    DWORD bytes = 0;
    if (RegGetValueW(HKEY_CURRENT_USER, settingsKey, name, RRF_RT_REG_SZ, nullptr, nullptr, &bytes)
        != ERROR_SUCCESS || bytes > 65536 || bytes < sizeof(wchar_t) || bytes % sizeof(wchar_t)) return {};

    std::wstring value(bytes / sizeof(wchar_t), L'\0');
    if (RegGetValueW(HKEY_CURRENT_USER, settingsKey, name, RRF_RT_REG_SZ, nullptr, value.data(), &bytes)
        != ERROR_SUCCESS) return {};
    value.resize(std::char_traits<wchar_t>::length(value.c_str()));
    return value;
}

bool WriteString(const wchar_t* name, const std::wstring& value)
{
    return RegSetKeyValueW(HKEY_CURRENT_USER, settingsKey, name, REG_SZ, value.c_str(),
        static_cast<DWORD>((value.size() + 1) * sizeof(wchar_t))) == ERROR_SUCCESS;
}

}

Preferences LoadPreferences()
{
    YISOU_PERF_MEMORY_SCOPE("load_preferences");
    if (const auto* config = config::Active()) {
        auto preferences = config->preferences;
        preferences.startWithWindows = StartsWithWindows();
        return preferences;
    }
    Preferences preferences;
    const auto font = ReadString(L"Font");
    if (font.starts_with(L"file:") && font.size() > 5) preferences.font = {FontSource::file, font.substr(5)};
    if (font.starts_with(L"system:") && font.size() > 7) preferences.font = {FontSource::system, font.substr(7)};

    if (auto key = ReadString(L"Engine"); !key.empty()) preferences.engine = std::move(key);
    preferences.browser = ReadString(L"Browser");
    DWORD shortcut = 0;
    DWORD bytes = sizeof(shortcut);
    if (RegGetValueW(HKEY_CURRENT_USER, settingsKey, L"Shortcut", RRF_RT_REG_DWORD, nullptr, &shortcut, &bytes) == ERROR_SUCCESS) {
        const Shortcut saved{shortcut >> 8, shortcut & 0xff};
        if (IsShortcut(saved)) preferences.shortcut = saved;
    }
    const auto binding = ReadString(L"SelectionShortcut");
    const auto delimiter = binding.find(L':');
    if (delimiter != std::wstring::npos && delimiter > 0 && delimiter <= 4 && delimiter + 1 < binding.size()) {
        wchar_t* end = nullptr;
        const auto packed = wcstoul(binding.c_str(), &end, 10);
        const Shortcut saved{(packed & ~previewShortcutFlag) >> 8, packed & 0xff};
        if (end == binding.c_str() + delimiter && saved.key && IsShortcut(saved) && saved != preferences.shortcut) {
            preferences.selectionShortcut = SelectionShortcut{saved, binding.substr(delimiter + 1), (packed & previewShortcutFlag) != 0};
        }
    }
    const auto storedPins = ReadString(L"PinnedEngines");
    std::wstring_view pins = storedPins;
    while (!pins.empty()) {
        const auto next = pins.find(L'\n');
        if (next != 0) preferences.pinnedEngines.emplace_back(pins.substr(0, next));
        if (next == pins.npos) break;
        pins.remove_prefix(next + 1);
    }
    DWORD history = 0;
    bytes = sizeof(history);
    preferences.recordHistory = RegGetValueW(HKEY_CURRENT_USER, settingsKey, L"RecordHistory",
        RRF_RT_REG_DWORD, nullptr, &history, &bytes) == ERROR_SUCCESS && history == 1;
    DWORD behavior = 0;
    bytes = sizeof(behavior);
    if (RegGetValueW(HKEY_CURRENT_USER, settingsKey, L"WindowBehavior", RRF_RT_REG_DWORD,
        nullptr, &behavior, &bytes) == ERROR_SUCCESS) {
        preferences.windowBehavior = {(behavior & showOnManualLaunchFlag) != 0, (behavior & closeToTrayFlag) != 0};
    }
    preferences.startWithWindows = StartsWithWindows();
    return preferences;
}

bool SaveWindowBehavior(WindowBehavior behavior)
{
    if (config::Active()) {
        return config::Update([&](auto& settings) { settings.preferences.windowBehavior = behavior; });
    }
    const DWORD flags = (behavior.showOnManualLaunch ? showOnManualLaunchFlag : 0)
        | (behavior.closeToTray ? closeToTrayFlag : 0);
    return RegSetKeyValueW(HKEY_CURRENT_USER, settingsKey, L"WindowBehavior", REG_DWORD,
        &flags, sizeof(flags)) == ERROR_SUCCESS;
}

bool StartsWithWindows()
{
    DWORD bytes = 0;
    return RegGetValueW(HKEY_CURRENT_USER, startupKey, startupName, RRF_RT_REG_SZ,
        nullptr, nullptr, &bytes) == ERROR_SUCCESS && bytes > sizeof(wchar_t);
}

bool SetStartWithWindows(bool enabled)
{
    if (!enabled) {
        const auto status = RegDeleteKeyValueW(HKEY_CURRENT_USER, startupKey, startupName);
        return status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND;
    }
    // Windows documents a 260-character limit for Run command lines.
    wchar_t path[260];
    const auto length = GetModuleFileNameW(nullptr, path, static_cast<DWORD>(std::size(path)));
    if (!length || length >= std::size(path)) return false;
    auto command = L"\"" + std::wstring(path, length) + L"\" --startup";
    if (const auto configuration = config::StartupOverride(); !configuration.empty()) {
        command += L" --config \"" + configuration.native() + L"\"";
    }
    if (command.size() > 260) return false;
    return RegSetKeyValueW(HKEY_CURRENT_USER, startupKey, startupName, REG_SZ, command.c_str(),
        static_cast<DWORD>((command.size() + 1) * sizeof(wchar_t))) == ERROR_SUCCESS;
}

bool SaveFont(const FontChoice& font)
{
    if (config::Active()) return config::Update([&](auto& settings) { settings.preferences.font = font; });
    switch (font.source) {
    case FontSource::defaults: return WriteString(L"Font", L"");
    case FontSource::system: return WriteString(L"Font", L"system:" + font.value);
    case FontSource::file: return WriteString(L"Font", L"file:" + font.value);
    }
    return false;
}

bool SaveEngine(const EngineId& id)
{
    if (config::Active()) return config::Update([&](auto& settings) { settings.preferences.engine = id; });
    return WriteString(L"Engine", id);
}

std::vector<Engine> LoadCustomEngines()
{
    YISOU_PERF_MEMORY_SCOPE("load_custom_engines");
    if (const auto* settings = config::Active()) return settings->engines;
    std::vector<Engine> result;
    if (ReadEngineList(result)) return result;
    HKEY key;
    if (RegOpenKeyExW(HKEY_CURRENT_USER, engineKey, 0, KEY_QUERY_VALUE, &key) != ERROR_SUCCESS) return result;

    for (DWORD index = 0; ; ++index) {
        wchar_t id[64];
        std::array<wchar_t, maxRecord / sizeof(wchar_t)> record;
        DWORD length = static_cast<DWORD>(std::size(id));
        DWORD bytes = static_cast<DWORD>(maxRecord);
        DWORD type = 0;
        const auto status = RegEnumValueW(key, index, id, &length, nullptr, &type,
            reinterpret_cast<BYTE*>(record.data()), &bytes);
        if (status == ERROR_MORE_DATA) continue;
        if (status != ERROR_SUCCESS) break;
        if (type != REG_BINARY || bytes % sizeof(wchar_t)) continue;

        Engine engine;
        engine.id.assign(id, length);
        if (ReadEngineRecord(engine, {record.data(), bytes / sizeof(wchar_t)})) result.push_back(std::move(engine));
    }
    RegCloseKey(key);
    return result;
}

EngineId SaveCustomEngine(const Engine& engine)
{
    GUID guid;
    wchar_t guidText[39];
    if (FAILED(CoCreateGuid(&guid)) || !StringFromGUID2(guid, guidText, static_cast<int>(std::size(guidText)))) return {};
    const EngineId id = std::wstring(L"custom-") + guidText;
    auto engines = LoadCustomEngines();
    engines.push_back(engine);
    engines.back().id = id;
    return SaveEngines(engines) ? id : EngineId{};
}

bool SaveEngines(const std::vector<Engine>& engines)
{
    if (config::Active()) {
        return config::Update([&](auto& settings) {
            settings.engines.clear();
            for (const auto& engine : engines) {
                if (engine.group == EngineGroup::custom) settings.engines.push_back(engine);
            }
            const auto exists = [&](const EngineId& id) {
                return !id.starts_with(L"custom-") || std::ranges::find(engines, id, &Engine::id) != engines.end();
            };
            auto& prefs = settings.preferences;
            if (!exists(prefs.engine)) prefs.engine = L"bing";
            if (prefs.selectionShortcut && !exists(prefs.selectionShortcut->engine)) prefs.selectionShortcut.reset();
            std::erase_if(prefs.pinnedEngines, [&](const auto& id) { return !exists(id); });
        });
    }
    std::wstring record;
    for (const auto& engine : engines) {
        if (engine.group != EngineGroup::custom) continue;
        AppendField(record, engine.id);
        AppendField(record, engine.alias);
        const bool hasSvg = !engine.svgIcon.empty();
        const auto svgBytes = static_cast<std::uint32_t>(engine.svgIcon.size());
        const auto pixelBytes = engine.icon.size() * sizeof(std::uint32_t);
        if (engine.color > 0xffffff || engine.svgIcon.size() > svgIconLimit
            || (pixelBytes && (hasSvg || pixelBytes != iconBytes))) return false;
        const auto imageBytes = hasSvg ? sizeof(svgBytes) + svgBytes + svgBytes % sizeof(wchar_t) : pixelBytes;
        const auto payloadUnits = (sizeof(engine.color) + imageBytes) / sizeof(wchar_t);
        const auto length = engine.name.size() + engine.url.size() + 2 + payloadUnits;
        if (length > 0xffff) return false;
        record.push_back(static_cast<wchar_t>(length));
        AppendField(record, engine.name);
        AppendField(record, engine.url);
        const auto offset = record.size();
        record.resize(offset + payloadUnits);
        auto* tail = reinterpret_cast<BYTE*>(record.data() + offset);
        const auto color = hasSvg ? engine.color | svgFlag : engine.color;
        std::memcpy(tail, &color, sizeof(color));
        tail += sizeof(color);
        if (hasSvg) {
            std::memcpy(tail, &svgBytes, sizeof(svgBytes));
            std::memcpy(tail + sizeof(svgBytes), engine.svgIcon.data(), svgBytes);
        } else if (pixelBytes) {
            std::memcpy(tail, engine.icon.data(), pixelBytes);
        }
        if (record.size() * sizeof(wchar_t) > maxEngineListBytes) return false;
    }
    return RegSetKeyValueW(HKEY_CURRENT_USER, settingsKey, engineListName, REG_BINARY, record.data(),
        static_cast<DWORD>(record.size() * sizeof(wchar_t))) == ERROR_SUCCESS;
}

bool SaveShortcut(Shortcut shortcut)
{
    if (!IsShortcut(shortcut)) return false;
    if (config::Active()) return config::Update([&](auto& settings) { settings.preferences.shortcut = shortcut; });
    const DWORD value = (shortcut.modifiers << 8) | shortcut.key;
    return RegSetKeyValueW(HKEY_CURRENT_USER, settingsKey, L"Shortcut", REG_DWORD, &value, sizeof(value)) == ERROR_SUCCESS;
}

bool SaveBrowser(const std::wstring& path)
{
    if (!path.empty() && BrowserPath(path).empty()) return false;
    if (config::Active()) return config::Update([&](auto& settings) { settings.preferences.browser = path; });
    return WriteString(L"Browser", path);
}

bool SaveSelectionShortcut(const std::optional<SelectionShortcut>& binding)
{
    if (binding && (!binding->shortcut.key || !IsShortcut(binding->shortcut) || binding->engine.empty())) return false;
    if (config::Active()) return config::Update([&](auto& settings) { settings.preferences.selectionShortcut = binding; });
    if (!binding) return WriteString(L"SelectionShortcut", L"");
    const auto& [shortcut, engine, preview] = *binding;
    const auto packed = (shortcut.modifiers << 8) | shortcut.key | (preview ? previewShortcutFlag : 0);
    return WriteString(L"SelectionShortcut", std::to_wstring(packed) + L":" + engine);
}

bool SavePinnedEngines(const std::vector<EngineId>& engines)
{
    if (config::Active()) return config::Update([&](auto& settings) { settings.preferences.pinnedEngines = engines; });
    std::wstring pins;
    for (const auto& id : engines) {
        if (id.empty() || id.find(L'\n') != id.npos) return false;
        pins += id;
        pins += L'\n';
    }
    return pins.size() < 32768 && WriteString(L"PinnedEngines", pins);
}

bool SaveHistoryPreference(bool enabled)
{
    if (config::Active()) return config::Update([&](auto& settings) { settings.preferences.recordHistory = enabled; });
    const DWORD value = enabled ? 1 : 0;
    return RegSetKeyValueW(HKEY_CURRENT_USER, settingsKey, L"RecordHistory", REG_DWORD, &value, sizeof(value)) == ERROR_SUCCESS;
}

bool SaveClipboardShortcut(const std::optional<Shortcut>& shortcut)
{
    return config::Active() && (!shortcut || (shortcut->key && IsShortcut(*shortcut)))
        && config::Update([&](auto& settings) { settings.preferences.clipboardShortcut = shortcut; });
}

bool SaveExcludedApps(const std::vector<std::wstring>& apps)
{
    return config::Active() && config::Update([&](auto& settings) { settings.preferences.doubleAltExcludedApps = apps; });
}

void RecordSearch(const SearchRecord& entry, const std::wstring& browserPath, const std::wstring& url)
{
    FILETIME now{};
    GetSystemTimePreciseAsFileTime(&now);
    const auto ticks = (static_cast<ULONGLONG>(now.dwHighDateTime) << 32) | now.dwLowDateTime;
    GUID guid{};
    wchar_t guidText[39]{};
    if (FAILED(CoCreateGuid(&guid)) || !StringFromGUID2(guid, guidText, static_cast<int>(std::size(guidText)))) return;
    wchar_t id[56]{};
    swprintf_s(id, L"%016llX%s", ticks, guidText);

    const bool system = browserPath.empty();
    const auto path = system ? DefaultBrowserPath(url.substr(0, url.find(L':')).c_str()) : browserPath;
    auto browser = path.empty() ? std::wstring{} : DescribeBrowser(path).name;
    if (browser.empty() && system) browser = Tr(Text::windowsDefaultBrowser);
    std::wstring data(1, static_cast<wchar_t>(entry.origin));
    for (const auto* field : {&entry.query, &entry.engineId, &entry.engineName, &entry.source}) AppendField(data, *field);
    AppendField(data, browser);
    RegSetKeyValueW(HKEY_CURRENT_USER, historyKey, id, REG_BINARY, data.data(),
        static_cast<DWORD>(data.size() * sizeof(wchar_t)));
}

std::vector<SearchRecord> LoadHistory(std::wstring_view before, size_t limit,
    std::wstring_view keyword, std::wstring_view source)
{
    YISOU_PERF_MEMORY_SCOPE("load_history");
    const auto matches = [](std::wstring_view value, std::wstring_view filter) {
        return filter.empty() || (!value.empty() && FindStringOrdinal(FIND_FROMSTART,
            value.data(), static_cast<int>(value.size()), filter.data(), static_cast<int>(filter.size()), TRUE) >= 0);
    };
    std::vector<SearchRecord> result;
    HKEY key;
    if (RegOpenKeyExW(HKEY_CURRENT_USER, historyKey, 0, KEY_QUERY_VALUE, &key) != ERROR_SUCCESS) return result;
    std::vector<std::wstring> ids;
    for (DWORD index = 0; ; ++index) {
        wchar_t id[56]{};
        DWORD length = static_cast<DWORD>(std::size(id));
        const auto status = RegEnumValueW(key, index, id, &length, nullptr, nullptr, nullptr, nullptr);
        if (status == ERROR_MORE_DATA) continue;
        if (status != ERROR_SUCCESS) break;
        if (length == 54 && (before.empty() || std::wstring_view(id, length) < before)) ids.emplace_back(id, length);
    }
    std::ranges::sort(ids, std::greater{});
    for (const auto& id : ids) {
        if (result.size() == limit) break;
        DWORD bytes = 0;
        if (RegGetValueW(key, nullptr, id.c_str(), RRF_RT_REG_BINARY, nullptr, nullptr, &bytes) != ERROR_SUCCESS
            || bytes < sizeof(wchar_t) || bytes > 65536 || bytes % sizeof(wchar_t)) continue;
        std::wstring data(bytes / sizeof(wchar_t), L'\0');
        if (RegGetValueW(key, nullptr, id.c_str(), RRF_RT_REG_BINARY, nullptr, data.data(), &bytes) != ERROR_SUCCESS) continue;
        if (data.front() > static_cast<wchar_t>(SearchOrigin::clipboard)) continue;
        SearchRecord entry;
        entry.id = id;
        entry.origin = static_cast<SearchOrigin>(data.front());
        std::span<const wchar_t> record(data.data() + 1, bytes / sizeof(wchar_t) - 1);
        if (ReadField(record, entry.query) && ReadField(record, entry.engineId) && ReadField(record, entry.engineName)
            && ReadField(record, entry.source) && ReadField(record, entry.browser) && record.empty() && !entry.query.empty()
            && matches(entry.query, keyword) && matches(entry.source, source)) {
            result.push_back(std::move(entry));
        }
    }
    RegCloseKey(key);
    return result;
}

bool DeleteHistory(const std::wstring& id)
{
    const auto result = RegDeleteKeyValueW(HKEY_CURRENT_USER, historyKey, id.c_str());
    return result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND;
}

std::wstring HistoryTime(std::wstring_view id)
{
    if (id.size() < 16) return {};
    const std::wstring timestamp(id.substr(0, 16));
    const auto ticks = wcstoull(timestamp.c_str(), nullptr, 16);
    FILETIME time{static_cast<DWORD>(ticks), static_cast<DWORD>(ticks >> 32)};
    SYSTEMTIME utc{}, local{};
    if (!FileTimeToSystemTime(&time, &utc) || !SystemTimeToTzSpecificLocalTime(nullptr, &utc, &local)) return {};
    wchar_t date[64]{}, clock[32]{};
    GetDateFormatEx(i18n::LocaleName(), DATE_SHORTDATE, &local, nullptr, date, static_cast<int>(std::size(date)), nullptr);
    GetTimeFormatEx(i18n::LocaleName(), TIME_NOSECONDS, &local, nullptr, clock, static_cast<int>(std::size(clock)));
    return std::wstring(date) + L" " + clock;
}

std::optional<DisplayPosition> LoadDisplayPosition(const wchar_t* display)
{
    std::array<std::int32_t, 2> offsets{};
    DWORD bytes = sizeof(offsets);
    if (RegGetValueW(HKEY_CURRENT_USER, displayKey, display, RRF_RT_REG_BINARY, nullptr, offsets.data(), &bytes)
        != ERROR_SUCCESS || bytes != sizeof(offsets)) return std::nullopt;
    return DisplayPosition{offsets[0], offsets[1]};
}

void SaveDisplayPosition(const wchar_t* display, DisplayPosition position)
{
    const std::array<std::int32_t, 2> offsets{position.x, position.y};
    RegSetKeyValueW(HKEY_CURRENT_USER, displayKey, display, REG_BINARY, offsets.data(), sizeof(offsets));
}

}
