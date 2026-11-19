#pragma once

#include "language.h"

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace yisou {

using EngineId = std::wstring;
enum class EngineGroup { web, video, social, development, shopping, custom };

inline constexpr unsigned iconSize = 32;
inline constexpr unsigned svgIconLimit = 64 * 1024;
inline constexpr unsigned engineNameLimit = 48;
inline constexpr unsigned engineUrlLimit = 2048;
inline constexpr unsigned engineAliasLimit = 16;
using IconPixels = std::vector<std::uint32_t>;

struct Engine {
    EngineId id;
    std::wstring name;
    std::wstring url;
    EngineGroup group = EngineGroup::custom;
    std::uint32_t color = 0x005fb8;
    IconPixels icon;
    std::wstring alias;
    std::string svgIcon{};
};

[[nodiscard]] bool IsEngineName(std::wstring_view name);
[[nodiscard]] bool IsSearchTemplate(std::wstring_view url);
[[nodiscard]] bool IsWebUrl(std::wstring_view url);
[[nodiscard]] bool IsExecutableName(std::wstring_view name);

enum class FontSource { defaults, system, file };

struct FontChoice {
    FontSource source = FontSource::defaults;
    std::wstring value;
    bool operator==(const FontChoice&) const = default;
};

inline constexpr unsigned shortcutAlt = 1, shortcutControl = 2, shortcutShift = 4, shortcutWin = 8;

struct Shortcut {
    unsigned modifiers = 0;
    unsigned key = 0; // Zero represents two taps on Left Alt.
    bool operator==(const Shortcut&) const = default;
};

struct SelectionShortcut {
    Shortcut shortcut;
    EngineId engine;
    bool preview = false;
    bool operator==(const SelectionShortcut&) const = default;
};

struct WindowBehavior {
    bool showOnManualLaunch = true;
    bool closeToTray = true;
    bool operator==(const WindowBehavior&) const = default;
};

struct Preferences {
    FontChoice font;
    EngineId engine = L"bing";
    Shortcut shortcut;
    std::wstring browser;
    bool recordHistory = false;
    std::optional<SelectionShortcut> selectionShortcut;
    std::vector<EngineId> pinnedEngines{};
    WindowBehavior windowBehavior{};
    bool startWithWindows = false;
    std::optional<Shortcut> clipboardShortcut;
    std::vector<std::wstring> doubleAltExcludedApps;
    Language language = Language::system;
    bool showEngineTileNames = true;
};

enum class SearchOrigin { typed, selection, history, clipboard };

struct SearchRecord {
    std::wstring id;
    std::wstring query;
    EngineId engineId;
    std::wstring engineName;
    std::wstring browser;
    SearchOrigin origin = SearchOrigin::typed;
    std::wstring source;
};

struct SystemActions {
    bool (*openUrl)(const std::wstring&, const std::wstring& browser);
    bool (*saveFont)(const FontChoice&);
    bool (*saveEngine)(const EngineId&);
    EngineId (*saveCustomEngine)(const Engine&);
    bool (*saveShortcut)(Shortcut) = nullptr;
    bool (*saveBrowser)(const std::wstring&) = nullptr;
    bool (*saveHistoryPreference)(bool) = nullptr;
    void (*recordSearch)(const SearchRecord&, const std::wstring& browserPath, const std::wstring& url) = nullptr;
    bool (*saveEngines)(const std::vector<Engine>&) = nullptr;
    bool (*saveSelectionShortcut)(const std::optional<SelectionShortcut>&) = nullptr;
    bool (*savePinnedEngines)(const std::vector<EngineId>&) = nullptr;
    bool (*saveWindowBehavior)(WindowBehavior) = nullptr;
    bool (*setStartWithWindows)(bool) = nullptr;
    bool (*saveClipboardShortcut)(const std::optional<Shortcut>&) = nullptr;
    bool (*saveExcludedApps)(const std::vector<std::wstring>&) = nullptr;
    bool (*saveEngineTileNames)(bool) = nullptr;
};

enum class SearchResult { opened, emptyQuery, openFailed };
enum class AddEngineResult { added, invalidName, invalidUrl, invalidIcon, invalidAlias, duplicateAlias, saveFailed };

struct SearchInput {
    const Engine* engine;
    std::wstring_view query;
    bool usesAlias = false;
};

class Application {
public:
    Application(Preferences preferences, SystemActions actions, std::vector<Engine> customEngines = {});
    [[nodiscard]] const Preferences& preferences() const;
    [[nodiscard]] const std::vector<Engine>& engines() const;
    [[nodiscard]] const Engine& activeEngine() const;
    [[nodiscard]] const Engine* findEngine(std::wstring_view id) const;
    [[nodiscard]] SearchInput parseInput(std::wstring_view input, std::wstring_view temporary = {}) const;
    [[nodiscard]] std::vector<const Engine*> suggestEngines(std::wstring_view input,
        const wchar_t* (*displayName)(std::wstring_view, const wchar_t*) = nullptr) const;
    [[nodiscard]] bool isPinned(std::wstring_view id) const;
    [[nodiscard]] bool togglePinnedEngine(std::wstring_view id);
    [[nodiscard]] bool changeFont(const FontChoice& font);
    [[nodiscard]] bool changeEngine(const EngineId& engine);
    [[nodiscard]] bool changeShortcut(Shortcut shortcut);
    [[nodiscard]] bool changeSelectionShortcut(const std::optional<SelectionShortcut>& binding);
    [[nodiscard]] bool changeBrowser(const std::wstring& path);
    [[nodiscard]] bool changeHistoryRecording(bool enabled);
    [[nodiscard]] bool changeWindowBehavior(WindowBehavior behavior);
    [[nodiscard]] bool changeEngineTileNames(bool show);
    [[nodiscard]] bool changeStartWithWindows(bool enabled);
    [[nodiscard]] bool changeClipboardShortcut(const std::optional<Shortcut>& shortcut);
    [[nodiscard]] bool changeExcludedApps(std::vector<std::wstring> apps);
    [[nodiscard]] AddEngineResult addEngine(Engine engine);
    [[nodiscard]] AddEngineResult editEngine(Engine engine);
    [[nodiscard]] bool deleteEngine(const EngineId& id);
    [[nodiscard]] bool moveEngine(const EngineId& id, int direction);
    [[nodiscard]] SearchResult testEngine(const Engine& engine, std::wstring_view query = L"易搜") const;
    [[nodiscard]] SearchResult openLink(std::wstring_view url) const;
    [[nodiscard]] SearchResult search(std::wstring_view query, const Engine* engine = nullptr,
        SearchOrigin origin = SearchOrigin::typed, std::wstring_view source = {}) const;

private:
    [[nodiscard]] AddEngineResult validateEngine(Engine& engine) const;
    [[nodiscard]] bool saveEngines(std::vector<Engine> updated);
    Preferences preferences_;
    SystemActions actions_;
    std::vector<Engine> engines_;
};

}
