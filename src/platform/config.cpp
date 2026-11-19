#include "localization.h"
#include "config.h"
#include "shortcut.h"
#include "windows_services.h"

#include <shlobj.h>

#include <algorithm>
#include <cstring>
#include <fstream>
#include <map>
#include <utility>

namespace yisou::platform::config {
using i18n::Text;
using i18n::Tr;

namespace {

using Path = std::filesystem::path;
using Section = std::map<std::wstring, std::wstring, std::less<>>;
using Ini = std::map<std::wstring, Section, std::less<>>;
constexpr size_t maxBytes = 16 * 1024 * 1024;
constexpr wchar_t locatorKey[] = LR"(Software\YiSou\ConfigLocations)";
std::optional<Settings> active;
Path activePath;

std::wstring_view Trim(std::wstring_view value)
{
    const auto first = value.find_first_not_of(L" \t\r");
    if (first == value.npos) return {};
    return value.substr(first, value.find_last_not_of(L" \t\r") - first + 1);
}

std::optional<Ini> Parse(std::wstring_view input)
{
    if (input.find(L'\0') != input.npos) return std::nullopt;
    Ini result;
    Section* section = nullptr;
    while (!input.empty()) {
        const auto end = input.find(L'\n');
        auto line = input.substr(0, end);
        input.remove_prefix(end == input.npos ? input.size() : end + 1);
        if (line.ends_with(L'\r')) line.remove_suffix(1);
        const auto content = Trim(line);
        if (content.empty() || content.starts_with(L';') || content.starts_with(L'#')) continue;
        if (content.starts_with(L'[') && content.ends_with(L']') && content.size() > 2) {
            const auto name = Trim(content.substr(1, content.size() - 2));
            if (name.empty()) return std::nullopt;
            const auto [entry, added] = result.try_emplace(std::wstring(name));
            if (!added) return std::nullopt;
            section = &entry->second;
            continue;
        }
        const auto equals = line.find(L'=');
        if (!section || equals == line.npos || Trim(line.substr(0, equals)).empty()) return std::nullopt;
        if (!section->try_emplace(std::wstring(Trim(line.substr(0, equals))), line.substr(equals + 1)).second) {
            return std::nullopt;
        }
    }
    return result;
}

std::optional<std::wstring> DecodeUtf8(std::string_view bytes)
{
    if (bytes.starts_with("\xef\xbb\xbf")) bytes.remove_prefix(3);
    if (bytes.empty()) return std::wstring{};
    const auto size = static_cast<int>(bytes.size());
    const auto count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, bytes.data(), size, nullptr, 0);
    if (!count) return std::nullopt;
    std::wstring result(count, L'\0');
    if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, bytes.data(), size, result.data(), count)) {
        return std::nullopt;
    }
    return result;
}

std::optional<std::string> EncodeUtf8(std::wstring_view text)
{
    if (text.empty() || text.size() > maxBytes) return std::nullopt;
    const auto size = static_cast<int>(text.size());
    const auto count = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text.data(), size, nullptr, 0, nullptr, nullptr);
    if (!count) return std::nullopt;
    std::string result(count, '\0');
    if (!WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text.data(), size, result.data(), count, nullptr, nullptr)) {
        return std::nullopt;
    }
    return result;
}

std::wstring_view Field(const Ini& ini, std::wstring_view section, std::wstring_view key, std::wstring_view fallback = {})
{
    const auto group = ini.find(section);
    if (group == ini.end()) return fallback;
    const auto value = group->second.find(key);
    return value == group->second.end() ? fallback : value->second;
}

bool ReadBool(std::wstring_view value, bool& target)
{
    value = Trim(value);
    if (value != L"true" && value != L"false") return false;
    target = value == L"true";
    return true;
}

int HexDigit(wchar_t digit)
{
    if (digit >= L'0' && digit <= L'9') return digit - L'0';
    if (digit >= L'a' && digit <= L'f') return digit - L'a' + 10;
    if (digit >= L'A' && digit <= L'F') return digit - L'A' + 10;
    return -1;
}

bool ReadNumber(std::wstring_view text, unsigned& value, unsigned maximum, unsigned base = 10)
{
    text = Trim(text);
    if (text.empty()) return false;
    value = 0;
    for (const auto c : text) {
        const auto digit = static_cast<unsigned>(HexDigit(c));
        if (digit >= base || digit > maximum || value > (maximum - digit) / base) return false;
        value = value * base + digit;
    }
    return true;
}

bool ReadShortcut(std::wstring_view value, Shortcut& shortcut)
{
    value = Trim(value);
    if (value == L"DoubleLeftAlt") {
        shortcut = {};
        return true;
    }
    const auto split = value.find(L'+');
    return split != value.npos && ReadNumber(value.substr(0, split), shortcut.modifiers, 15)
        && ReadNumber(value.substr(split + 1), shortcut.key, 255) && shortcut.key && IsShortcut(shortcut);
}

std::wstring ShortcutText(Shortcut shortcut)
{
    if (!shortcut.key) return L"DoubleLeftAlt";
    return std::to_wstring(shortcut.modifiers) + L"+" + std::to_wstring(shortcut.key);
}

std::vector<std::wstring> Split(std::wstring_view list)
{
    std::vector<std::wstring> result;
    while (!list.empty()) {
        const auto delimiter = list.find(L',');
        result.emplace_back(Trim(list.substr(0, delimiter)));
        if (delimiter == list.npos) break;
        list.remove_prefix(delimiter + 1);
    }
    return result;
}

std::wstring Join(const std::vector<std::wstring>& list)
{
    std::wstring result;
    for (const auto& item : list) {
        if (!result.empty()) result += L',';
        result += item;
    }
    return result;
}

std::wstring Hex(std::string_view data)
{
    constexpr wchar_t digits[] = L"0123456789abcdef";
    std::wstring result;
    result.reserve(data.size() * 2);
    for (const unsigned char byte : data) {
        result += digits[byte >> 4];
        result += digits[byte & 15];
    }
    return result;
}

bool ReadHex(std::wstring_view text, std::string& data, size_t limit)
{
    if (text.size() % 2 || text.size() / 2 > limit) return false;
    data.resize(text.size() / 2);
    for (size_t i = 0; i < data.size(); ++i) {
        const int high = HexDigit(text[i * 2]);
        const int low = HexDigit(text[i * 2 + 1]);
        if (high < 0 || low < 0) return false;
        data[i] = static_cast<char>((high << 4) | low);
    }
    return true;
}

std::wstring ReadPath(std::wstring_view value, const Path& directory)
{
    if (value.empty()) return {};
    const Path path(value);
    return path.is_relative() ? (directory / path).lexically_normal().native() : path.native();
}

std::wstring WritePath(const std::wstring& value, const Path& directory)
{
    if (value.empty()) return {};
    const Path path(value);
    const auto relative = path.lexically_relative(directory);
    if (!relative.empty() && *relative.begin() != L"..") return relative.native();
    return value;
}

void Normalize(Settings& settings)
{
    Application application(settings.preferences, {}, std::move(settings.engines));
    settings.preferences = application.preferences();
    settings.engines.clear();
    for (const auto& engine : application.engines()) {
        if (engine.group == EngineGroup::custom) settings.engines.push_back(engine);
    }
}

bool ReadEngines(const Ini& ini, Settings& settings)
{
    const auto order = Split(Field(ini, L"Search", L"EngineOrder"));
    if (std::ranges::count_if(ini, [](const auto& entry) { return entry.first.starts_with(L"Engine."); })
        != static_cast<std::ptrdiff_t>(order.size())) return false;
    for (const auto& id : order) {
        const auto section = L"Engine." + id;
        if (!id.starts_with(L"custom-") || id.find_first_of(L"[],\t\r\n") != id.npos || !ini.contains(section)) return false;
        Engine engine;
        engine.id = id;
        engine.name = Field(ini, section, L"Name");
        engine.url = Field(ini, section, L"Url");
        engine.alias = Field(ini, section, L"Alias");
        if (!ReadNumber(Field(ini, section, L"Color", L"005fb8"), engine.color, 0xffffff, 16)) return false;
        const auto svg = Field(ini, section, L"SvgHex");
        const auto pixels = Field(ini, section, L"PixelsHex");
        if (!svg.empty() && !pixels.empty()) return false;
        if (!ReadHex(svg, engine.svgIcon, svgIconLimit)) return false;
        if (!pixels.empty()) {
            constexpr size_t pixelBytes = iconSize * iconSize * sizeof(std::uint32_t);
            std::string data;
            if (!ReadHex(pixels, data, pixelBytes) || data.size() != pixelBytes) return false;
            engine.icon.resize(iconSize * iconSize);
            std::memcpy(engine.icon.data(), data.data(), data.size());
        }
        settings.engines.push_back(std::move(engine));
    }
    return true;
}

bool ReadSettings(const Ini& ini, const Path& directory, Settings& settings)
{
    if (Trim(Field(ini, L"General", L"ConfigVersion")) != L"1"
        || Trim(Field(ini, L"General", L"Kind", L"Settings")) != L"Settings") return false;
    auto& prefs = settings.preferences;
    const auto language = Trim(Field(ini, L"General", L"Language", L"system"));
    for (const auto supported : {Language::system, Language::simplifiedChinese, Language::english}) {
        if (language == LanguageTag(supported)) prefs.language = supported;
    }
    if (!ReadBool(Field(ini, L"General", L"ShowOnManualLaunch", L"true"), prefs.windowBehavior.showOnManualLaunch)
        || !ReadBool(Field(ini, L"General", L"CloseToTray", L"true"), prefs.windowBehavior.closeToTray)
        || !ReadBool(Field(ini, L"Appearance", L"ShowEngineTileNames", L"true"), prefs.showEngineTileNames)
        || !ReadBool(Field(ini, L"Search", L"RecordHistory", L"false"), prefs.recordHistory)
        || !ReadShortcut(Field(ini, L"Shortcuts", L"Search", L"DoubleLeftAlt"), prefs.shortcut)) return false;
    prefs.engine = Trim(Field(ini, L"Search", L"DefaultEngine", L"bing"));
    prefs.pinnedEngines = Split(Field(ini, L"Search", L"PinnedEngines"));
    prefs.browser = ReadPath(Field(ini, L"Browser", L"Executable"), directory);
    const auto type = Trim(Field(ini, L"Appearance", L"FontSource", L"Default"));
    const auto font = Field(ini, L"Appearance", L"Font");
    if (type == L"File" && !font.empty()) prefs.font = {FontSource::file, ReadPath(font, directory)};
    else if (type == L"System" && !font.empty()) prefs.font = {FontSource::system, std::wstring(font)};
    else if (type != L"Default" || !font.empty()) return false;

    const auto selection = Field(ini, L"Shortcuts", L"Selection");
    if (!Trim(selection).empty()) {
        SelectionShortcut binding;
        binding.engine = Trim(Field(ini, L"Shortcuts", L"SelectionEngine"));
        if (!ReadShortcut(selection, binding.shortcut) || !binding.shortcut.key || binding.shortcut == prefs.shortcut
            || !ReadBool(Field(ini, L"Shortcuts", L"SelectionPreview", L"false"), binding.preview)) return false;
        prefs.selectionShortcut = std::move(binding);
    }
    if (const auto clipboard = Trim(Field(ini, L"Shortcuts", L"Clipboard")); !clipboard.empty()) {
        Shortcut shortcut;
        if (!ReadShortcut(clipboard, shortcut) || !shortcut.key || shortcut == prefs.shortcut
            || (prefs.selectionShortcut && shortcut == prefs.selectionShortcut->shortcut)) return false;
        prefs.clipboardShortcut = shortcut;
    }
    prefs.doubleAltExcludedApps = Split(Field(ini, L"Shortcuts", L"DoubleAltExcludedApps"));
    if (!std::ranges::all_of(prefs.doubleAltExcludedApps, IsExecutableName)) return false;
    return ReadEngines(ini, settings);
}

bool Validate(Settings& settings)
{
    const auto engine = settings.preferences.engine;
    const auto selection = settings.preferences.selectionShortcut;
    const auto pins = settings.preferences.pinnedEngines;
    const auto count = settings.engines.size();
    Normalize(settings);
    return settings.engines.size() == count && settings.preferences.engine == engine
        && settings.preferences.selectionShortcut == selection && settings.preferences.pinnedEngines == pins;
}

void Append(std::wstring& output, std::wstring_view key, std::wstring_view value)
{
    output.append(key).append(L"=").append(value).append(L"\n");
}

void AppendBool(std::wstring& output, std::wstring_view key, bool value)
{
    Append(output, key, value ? L"true" : L"false");
}

void AppendEngines(std::wstring& output, const std::vector<Engine>& engines)
{
    std::vector<EngineId> order;
    for (const auto& engine : engines) order.push_back(engine.id);
    Append(output, L"EngineOrder", Join(order));
    for (const auto& engine : engines) {
        output.append(L"\n[Engine.").append(engine.id).append(L"]\n");
        Append(output, L"Name", engine.name);
        Append(output, L"Url", engine.url);
        Append(output, L"Alias", engine.alias);
        wchar_t color[7]{};
        swprintf_s(color, L"%06x", engine.color);
        Append(output, L"Color", color);
        if (!engine.svgIcon.empty()) Append(output, L"SvgHex", Hex(engine.svgIcon));
        if (!engine.icon.empty()) {
            Append(output, L"PixelsHex", Hex({reinterpret_cast<const char*>(engine.icon.data()),
                engine.icon.size() * sizeof(std::uint32_t)}));
        }
    }
}

std::wstring FormatSettings(const Settings& settings, const Path& directory)
{
    const auto& prefs = settings.preferences;
    std::wstring output = L"[General]\nConfigVersion=1\nKind=Settings\n";
    Append(output, L"Language", LanguageTag(prefs.language));
    AppendBool(output, L"ShowOnManualLaunch", prefs.windowBehavior.showOnManualLaunch);
    AppendBool(output, L"CloseToTray", prefs.windowBehavior.closeToTray);
    output += L"\n[Appearance]\n";
    AppendBool(output, L"ShowEngineTileNames", prefs.showEngineTileNames);
    const auto& font = prefs.font;
    Append(output, L"FontSource", font.source == FontSource::file ? L"File" : font.source == FontSource::system ? L"System" : L"Default");
    Append(output, L"Font", font.source == FontSource::file ? WritePath(font.value, directory) : font.value);
    output += L"\n[Browser]\n";
    Append(output, L"Executable", WritePath(prefs.browser, directory));
    output += L"\n[Shortcuts]\n";
    Append(output, L"Search", ShortcutText(prefs.shortcut));
    if (const auto& binding = prefs.selectionShortcut) {
        Append(output, L"Selection", ShortcutText(binding->shortcut));
        Append(output, L"SelectionEngine", binding->engine);
        AppendBool(output, L"SelectionPreview", binding->preview);
    }
    if (prefs.clipboardShortcut) Append(output, L"Clipboard", ShortcutText(*prefs.clipboardShortcut));
    if (!prefs.doubleAltExcludedApps.empty()) Append(output, L"DoubleAltExcludedApps", Join(prefs.doubleAltExcludedApps));
    output += L"\n[Search]\n";
    Append(output, L"DefaultEngine", prefs.engine);
    Append(output, L"PinnedEngines", Join(prefs.pinnedEngines));
    AppendBool(output, L"RecordHistory", prefs.recordHistory);
    AppendEngines(output, settings.engines);
    return output;
}

Result ReadFile(const Path& path, Ini& ini)
{
    std::error_code error;
    const auto bytes = std::filesystem::file_size(path, error);
    if (error == std::errc::no_such_file_or_directory) return Result::missing;
    if (error) return Result::ioError;
    if (bytes > maxBytes) return Result::invalid;
    std::ifstream stream(path, std::ios::binary);
    if (!stream) return Result::ioError;
    std::string data(static_cast<size_t>(bytes), '\0');
    if (!stream.read(data.data(), static_cast<std::streamsize>(data.size()))
        || stream.peek() != std::char_traits<char>::eof()) return Result::ioError;
    auto text = DecodeUtf8(data);
    if (!text) return Result::invalid;
    auto parsed = Parse(*text);
    if (!parsed) return Result::invalid;
    ini = std::move(*parsed);
    return Result::ok;
}

bool WriteFile(const Path& path, std::wstring_view text)
{
    const auto data = EncodeUtf8(text);
    if (!data || data->size() > maxBytes || path.filename().empty()) return false;
    std::error_code error;
    if (path.has_parent_path()) std::filesystem::create_directories(path.parent_path(), error);
    if (error) return false;
    GUID id{};
    wchar_t suffix[39]{};
    if (FAILED(CoCreateGuid(&id)) || !StringFromGUID2(id, suffix, static_cast<int>(std::size(suffix)))) return false;
    Path temp = path.parent_path() / (std::wstring(L".YiSou-") + suffix + L".tmp");
    const HANDLE file = CreateFileW(temp.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) return false;
    DWORD written = 0;
    const bool saved = ::WriteFile(file, data->data(), static_cast<DWORD>(data->size()), &written, nullptr)
        && written == data->size() && FlushFileBuffers(file);
    CloseHandle(file);
    if (saved && MoveFileExW(temp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) return true;
    DeleteFileW(temp.c_str());
    return false;
}

Path DefaultPath()
{
    PWSTR directory = nullptr;
    if (FAILED(SHGetKnownFolderPath(FOLDERID_LocalAppData, KF_FLAG_DEFAULT, nullptr, &directory))) return {};
    Path path = Path(directory) / L"YiSou" / L"settings.ini";
    CoTaskMemFree(directory);
    return path;
}

Path ExecutablePath()
{
    std::wstring executable(32768, L'\0');
    const auto length = GetModuleFileNameW(nullptr, executable.data(), static_cast<DWORD>(executable.size()));
    if (!length || length >= executable.size()) return {};
    executable.resize(length);
    return Path(executable);
}

Path RememberedPath()
{
    const auto executable = ExecutablePath();
    if (executable.empty()) return {};
    std::wstring path(32768, L'\0');
    DWORD bytes = static_cast<DWORD>(path.size() * sizeof(wchar_t));
    if (RegGetValueW(HKEY_CURRENT_USER, locatorKey, executable.c_str(), RRF_RT_REG_SZ, nullptr, path.data(), &bytes)
        != ERROR_SUCCESS || bytes < sizeof(wchar_t)) return {};
    path.resize(bytes / sizeof(wchar_t) - 1);
    return Path(path);
}

Path LocateSettings()
{
    if (auto remembered = RememberedPath(); !remembered.empty()) return remembered;
    if (const auto executable = ExecutablePath(); !executable.empty()) {
        const auto portable = executable.parent_path() / L"settings.ini";
        std::error_code error;
        if (std::filesystem::exists(portable, error)) return portable;
    }
    return {};
}

}

Result Read(const Path& path, Settings& settings)
{
    Ini ini;
    if (const auto result = ReadFile(path, ini); result != Result::ok) return result;
    Settings parsed;
    if (!ReadSettings(ini, path.parent_path(), parsed) || !Validate(parsed)) return Result::invalid;
    settings = std::move(parsed);
    return Result::ok;
}

bool Write(const Path& path, const Settings& settings)
{
    const auto text = FormatSettings(settings, path.parent_path());
    auto ini = Parse(text);
    Settings parsed;
    return ini && ReadSettings(*ini, path.parent_path(), parsed) && Validate(parsed) && WriteFile(path, text);
}

Result Initialize(const Path& overridePath)
{
    auto path = overridePath.empty() ? LocateSettings() : overridePath;
    const bool migrate = path.empty();
    if (migrate) path = DefaultPath();
    if (path.empty()) return Result::ioError;
    std::error_code error;
    path = std::filesystem::absolute(path, error).lexically_normal();
    if (error) return Result::ioError;
    activePath = path;
    Settings settings;
    if (const auto result = Read(path, settings); result == Result::missing) {
        if (migrate) {
            settings = {LoadPreferences(), LoadCustomEngines()};
            Normalize(settings);
        }
        if (!Write(path, settings)) return Result::ioError;
    } else if (result != Result::ok) {
        return result;
    }
    activePath = std::move(path);
    active = std::move(settings);
    return Result::ok;
}

const Settings* Active()
{
    return active ? &*active : nullptr;
}

const Path& ActivePath()
{
    return activePath;
}

Path StartupOverride()
{
    if (!active) return {};
    auto regular = LocateSettings();
    if (regular.empty()) regular = DefaultPath();
    if (CompareStringOrdinal(activePath.c_str(), -1, regular.c_str(), -1, TRUE) == CSTR_EQUAL) return {};
    return activePath;
}

bool Save(Settings settings)
{
    if (!active || !Write(activePath, settings)) return false;
    active = std::move(settings);
    return true;
}

bool SaveLanguage(Language language)
{
    return active && Update([&](Settings& settings) { settings.preferences.language = language; });
}

bool SaveEngineTileNames(bool show)
{
    return active && Update([&](Settings& settings) { settings.preferences.showEngineTileNames = show; });
}

Result Import(const Path& path)
{
    Settings settings;
    if (const auto result = Read(path, settings); result != Result::ok) return result;
    return Save(std::move(settings)) ? Result::ok : Result::ioError;
}

bool Export(const Path& path)
{
    return active && Write(path, *active);
}

Result ImportEngines(const Path& path)
{
    if (!active) return Result::ioError;
    Ini ini;
    if (const auto result = ReadFile(path, ini); result != Result::ok) return result;
    Settings incoming;
    if (Trim(Field(ini, L"General", L"ConfigVersion")) != L"1"
        || Trim(Field(ini, L"General", L"Kind")) != L"Engines" || !ReadEngines(ini, incoming)) return Result::invalid;
    incoming.preferences.pinnedEngines = Split(Field(ini, L"Search", L"PinnedEngines"));
    if (!Validate(incoming)) return Result::invalid;
    auto settings = *active;
    std::erase_if(settings.engines, [&](const Engine& engine) {
        return std::ranges::find(incoming.engines, engine.id, &Engine::id) != incoming.engines.end();
    });
    for (auto& engine : settings.engines) incoming.engines.push_back(std::move(engine));
    settings.engines = std::move(incoming.engines);
    auto& pins = settings.preferences.pinnedEngines;
    auto orderedPins = std::move(incoming.preferences.pinnedEngines);
    for (const auto& id : pins) {
        if (std::ranges::find(orderedPins, id) == orderedPins.end()) orderedPins.push_back(id);
    }
    pins = std::move(orderedPins);
    if (!Validate(settings)) return Result::invalid;
    return Save(std::move(settings)) ? Result::ok : Result::ioError;
}

bool ExportEngines(const Path& path)
{
    if (!active || CompareStringOrdinal(path.c_str(), -1, activePath.c_str(), -1, TRUE) == CSTR_EQUAL) return false;
    std::error_code error;
    if (std::filesystem::equivalent(path, activePath, error)) return false;
    std::wstring output = L"[General]\nConfigVersion=1\nKind=Engines\n\n[Search]\n";
    Append(output, L"PinnedEngines", Join(active->preferences.pinnedEngines));
    AppendEngines(output, active->engines);
    return WriteFile(path, output);
}

Result ChooseLocation(const Path& path)
{
    if (!active || !path.is_absolute()) return Result::ioError;
    const auto executable = ExecutablePath();
    if (executable.empty()) return Result::ioError;
    Settings settings;
    if (const auto result = Read(path, settings); result == Result::missing) {
        settings = *active;
        if (!Write(path, settings)) return Result::ioError;
    } else if (result != Result::ok) return result;
    const auto& data = path.native();
    if (RegSetKeyValueW(HKEY_CURRENT_USER, locatorKey, executable.c_str(), REG_SZ, data.c_str(),
        static_cast<DWORD>((data.size() + 1) * sizeof(wchar_t))) != ERROR_SUCCESS) return Result::ioError;
    activePath = path;
    active = std::move(settings);
    return Result::ok;
}

const wchar_t* Describe(Result result)
{
    switch (result) {
    case Result::ok: return Tr(Text::configSaved);
    case Result::missing: return Tr(Text::configMissing);
    case Result::invalid: return Tr(Text::configInvalid);
    case Result::ioError: return Tr(Text::configIoError);
    }
    return Tr(Text::configError);
}

}
