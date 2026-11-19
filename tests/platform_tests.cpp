#include "../src/platform/windows_services.h"
#include "../src/platform/browser.h"
#include "../src/platform/config.h"
#include "../src/platform/localization.h"
#include "../src/platform/shortcut.h"
#include "../src/platform/selection.h"
#include "check.h"
#include "test_registry.h"

#include <windows.h>
#include <algorithm>
#include <array>
#include <fstream>
#include <iostream>

namespace {

constexpr wchar_t browserProbeUrl[] = L"https://example.org/?q=\"selected%20text\"";
constexpr wchar_t browserProbeVariable[] = L"YISOU_TEST_BROWSER_EVENT";

int ConfirmBrowserArgument(const wchar_t* argument)
{
    if (std::wstring_view(argument) != browserProbeUrl) return 1;
    wchar_t name[128]{};
    const auto length = GetEnvironmentVariableW(browserProbeVariable, name, static_cast<DWORD>(std::size(name)));
    if (!length || length >= std::size(name)) return 1;
    const HANDLE event = OpenEventW(EVENT_MODIFY_STATE, FALSE, name);
    if (!event) return 1;
    const bool signalled = SetEvent(event) != FALSE;
    CloseHandle(event);
    return signalled ? 0 : 1;
}

void CheckBrowserLaunch(const std::wstring& path)
{
    const auto name = L"Local\\YiSou.TestBrowser." + std::to_wstring(GetCurrentProcessId());
    const HANDLE event = CreateEventW(nullptr, FALSE, FALSE, name.c_str());
    CHECK(event);
    const bool ready = SetEnvironmentVariableW(browserProbeVariable, name.c_str()) != FALSE;
    const bool opened = ready && yisou::platform::OpenBrowser(browserProbeUrl, path);
    SetEnvironmentVariableW(browserProbeVariable, nullptr);
    const bool received = opened && WaitForSingleObject(event, 3000) == WAIT_OBJECT_0;
    CloseHandle(event);
    CHECK(received);
}


void CheckShortcuts()
{
    using namespace yisou;
    using namespace yisou::platform;
    AltTaps taps;
    CHECK(!taps.key(VK_LMENU, true, 0, false));
    CHECK(!taps.key(VK_LMENU, false, 50, false));
    CHECK(!taps.key(VK_LMENU, true, 215, false));
    CHECK(taps.key(VK_LMENU, false, 300, false));
    CHECK(!taps.key(VK_LMENU, true, 400, false));
    CHECK(!taps.key(VK_TAB, true, 410, true));
    CHECK(!taps.key(VK_LMENU, false, 450, false));
    CHECK(!taps.key(VK_LMENU, true, 500, false));
    CHECK(!taps.key(VK_LMENU, true, 600, false));
    CHECK(!taps.key(VK_LMENU, false, 900, false));
    CHECK(!taps.key(VK_RMENU, true, 920, false));
    CHECK(!taps.key(VK_RMENU, false, 950, false));
    CHECK(!taps.key(VK_LMENU, true, 1000, false));
    CHECK(!taps.key(VK_LMENU, false, 1040, false));
    CHECK(!taps.key(VK_LMENU, true, 1550, false));
    CHECK(!taps.key(VK_LMENU, false, 1590, false));

    const HWND window = CreateWindowExW(0, L"STATIC", L"", 0, 0, 0, 0, 0, HWND_MESSAGE, nullptr, nullptr, nullptr);
    CHECK(window);
    {
        ShortcutRegistration registration(window, WM_APP);
        const Shortcut original{shortcutControl | shortcutAlt | shortcutShift, VK_F22};
        const Shortcut conflict{shortcutControl | shortcutAlt | shortcutShift, VK_F23};
        CHECK(RegisterHotKey(window, 100, conflict.modifiers, conflict.key));
        CHECK(registration.apply(original) == ShortcutResult::applied);
        CHECK(registration.receives(1));
        CHECK(registration.apply(conflict) == ShortcutResult::unavailable);
        CHECK(registration.receives(1));
        CHECK(UnregisterHotKey(window, 100));
        CHECK(registration.apply(conflict, [](Shortcut) { return false; }) == ShortcutResult::saveFailed);
        CHECK(registration.receives(1));
        CHECK(registration.apply(conflict) == ShortcutResult::applied);
        CHECK(registration.receives(2));
        ShortcutRegistration selection(window, 0, 3);
        CHECK(selection.apply(conflict) == ShortcutResult::unavailable);
        CHECK(selection.apply(original) == ShortcutResult::applied);
        CHECK(selection.receives(3) && registration.receives(2) && !registration.receives(3));
        CHECK(selection.apply(original, [](Shortcut) { return false; }) == ShortcutResult::saveFailed);
        selection.clear();
        CHECK(!selection.receives(3) && registration.receives(2));
        CHECK(RegisterHotKey(window, 100, original.modifiers, original.key));
        CHECK(UnregisterHotKey(window, 100));
    }
    DestroyWindow(window);
}

void CheckSelection()
{
    using yisou::platform::ReadEditSelection;
    const HWND edit = CreateWindowExW(0, L"EDIT", L"前缀 中文测试 尾", ES_AUTOHSCROLL, 0, 0, 200, 30,
        nullptr, nullptr, nullptr, nullptr);
    CHECK(edit);
    SendMessageW(edit, EM_SETSEL, 3, 7);
    CHECK(ReadEditSelection(edit) == L"中文测试");
    SendMessageW(edit, EM_SETSEL, 4, 4);
    CHECK(ReadEditSelection(edit).empty());
    SetWindowLongPtrW(edit, GWL_STYLE, GetWindowLongPtrW(edit, GWL_STYLE) | ES_PASSWORD);
    SendMessageW(edit, EM_SETSEL, 3, 7);
    CHECK(ReadEditSelection(edit).empty());
    DestroyWindow(edit);
}

void CheckConfiguration(const std::wstring& executable)
{
    using namespace yisou;
    using namespace yisou::platform;
    using namespace yisou::platform::config;
    const auto directory = std::filesystem::path(executable).parent_path()
        / (L"ini-test-" + std::to_wstring(GetCurrentProcessId()) + L"-" + std::to_wstring(GetTickCount64()));
    const auto primary = directory / L"易搜.ini";
    const auto exported = directory / L"backup" / L"export.ini";
    const auto moved = directory / L"portable" / L"settings.ini";
    const auto invalid = directory / L"invalid.ini";
    const auto engineBackup = directory / L"engines.ini";
    CHECK(Initialize(primary) == Result::ok && ActivePath() == primary);
    CHECK(Active()->preferences.language == Language::system);
    CHECK(Active()->preferences.showEngineTileNames);
    {
        std::ofstream older(invalid, std::ios::binary);
        older << "[General]\nConfigVersion=1\n";
    }
    Settings oldConfig;
    CHECK(Read(invalid, oldConfig) == Result::ok && oldConfig.preferences.language == Language::system);
    CHECK(oldConfig.preferences.showEngineTileNames);
    {
        std::ofstream future(invalid, std::ios::binary);
        future << "[General]\nConfigVersion=1\nLanguage=ja-JP\n";
    }
    CHECK(Read(invalid, oldConfig) == Result::ok && oldConfig.preferences.language == Language::system);
    CHECK(SaveLanguage(Language::english));
    CHECK(SaveEngineTileNames(false) && !LoadPreferences().showEngineTileNames);
    CHECK(SaveFont({FontSource::file, (directory / L"字体.ttf").native()}));
    CHECK(SaveBrowser(executable));
    Engine wiki{{}, L"中文维基", L"https://example.org/search?q=%s&lang=zh#results", EngineGroup::custom};
    wiki.alias = L"testwiki";
    wiki.svgIcon = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"/>\n";
    const auto id = SaveCustomEngine(wiki);
    CHECK(!id.empty() && SaveEngine(id));
    CHECK(SavePinnedEngines({id, L"github"}));
    CHECK(SaveWindowBehavior({false, true}) && SaveHistoryPreference(true));
    CHECK(SaveSelectionShortcut(SelectionShortcut{{shortcutControl | shortcutShift, VK_F22}, id, true}));
    const Shortcut clipboard{shortcutControl | shortcutShift, VK_F23};
    CHECK(SaveClipboardShortcut(clipboard));
    CHECK(SaveExcludedApps({L"mstsc.exe", L"测试程序.EXE"}));
    CHECK(Export(exported));
    Settings loaded;
    CHECK(Read(exported, loaded) == Result::ok);
    CHECK(loaded.preferences.language == Language::english);
    CHECK(!loaded.preferences.showEngineTileNames);
    CHECK(loaded.preferences.engine == id && loaded.preferences.recordHistory);
    CHECK(loaded.preferences.font.value == (directory / L"字体.ttf").native());
    CHECK(loaded.preferences.browser == executable && loaded.engines.size() == 1);
    CHECK(loaded.engines.front().url == wiki.url && loaded.engines.front().svgIcon == wiki.svgIcon);
    CHECK(loaded.preferences.pinnedEngines == (std::vector<EngineId>{id, L"github"}));
    CHECK(loaded.preferences.selectionShortcut->engine == id && loaded.preferences.selectionShortcut->preview);
    CHECK(loaded.preferences.clipboardShortcut == clipboard);
    CHECK(loaded.preferences.doubleAltExcludedApps == (std::vector<std::wstring>{L"mstsc.exe", L"测试程序.EXE"}));
    CHECK(ExportEngines(engineBackup) && !ExportEngines(primary));
    {
        std::ofstream bad(invalid, std::ios::binary);
        bad << "[General]\nConfigVersion=1\n[Search]\nDefaultEngine=absent\n";
    }
    CHECK(Import(invalid) == Result::invalid && Active()->preferences.engine == id);
    CHECK(ChooseLocation(moved) == Result::ok);
    CHECK(Initialize({}) == Result::ok && ActivePath() == moved);
    CHECK(!LoadPreferences().showEngineTileNames);
    CHECK(SaveEngineTileNames(true) && LoadPreferences().showEngineTileNames);
    CHECK(SaveLanguage(Language::simplifiedChinese));
    CHECK(SaveEngines({}));
    CHECK(Active()->preferences.engine == L"bing" && !Active()->preferences.selectionShortcut);
    CHECK(SavePinnedEngines({L"baidu"}));
    CHECK(ImportEngines(engineBackup) == Result::ok);
    CHECK(Active()->preferences.language == Language::simplifiedChinese);
    CHECK(Active()->engines.size() == 1 && Active()->engines.front().svgIcon == wiki.svgIcon);
    CHECK(Active()->preferences.pinnedEngines == (std::vector<EngineId>{id, L"github", L"baidu"}));
    CHECK(Active()->preferences.engine == L"bing" && Active()->preferences.clipboardShortcut == clipboard);
    CHECK(Import(engineBackup) == Result::invalid);
    CHECK(Import(exported) == Result::ok && Active()->preferences.engine == id);
    CHECK(Active()->preferences.language == Language::english);
    CHECK(!Active()->preferences.showEngineTileNames);
    CHECK(Read(primary, loaded) == Result::ok && loaded.preferences.engine == id);
    CHECK(!Export(directory));
    for (const auto& path : {primary, exported, moved, invalid, engineBackup, exported.parent_path(), moved.parent_path(), directory}) {
        CHECK(std::filesystem::remove(path));
    }
}

void CheckLocalization()
{
    using namespace yisou;
    using namespace yisou::i18n;
    using namespace std::literals;
    CHECK(ResolveLanguage(std::array{L"de-DE"sv, L"zh-Hans-MY"sv, L"en-GB"sv}) == Language::simplifiedChinese);
    CHECK(ResolveLanguage(std::array{L"zh-TW"sv, L"EN-gb"sv, L"zh-CN"sv}) == Language::english);
    CHECK(ResolveLanguage(std::array{L"ja-JP"sv}) == Language::english);
    SetLanguage(Language::english);
    CHECK(Format(Text::engineButton, {L"GitHub"}) == L"Search engine: GitHub");
    CHECK(std::wstring_view(EngineName(L"custom-test", L"中文名称")) == L"中文名称");
    const Application app({}, {});
    const auto suggestions = app.suggestEngines(L"#Bilibili", EngineName);
    CHECK(!suggestions.empty() && suggestions.front()->id == L"bilibili");
    SetLanguage(Language::simplifiedChinese);
    CHECK(Format(Text::engineButton, {L"GitHub"}) == L"搜索引擎：GitHub");
    CHECK(std::wstring_view(EngineName(L"bing", L"unused")) == L"必应");
}

}

int wmain(int argc, wchar_t** argv) try
{
    if (argc == 2) return ConfirmBrowserArgument(argv[1]);
    using namespace yisou;
    using namespace yisou::platform;
    TestRegistry registry;
    registry.redirect();

    CHECK(LoadPreferences().engine == L"bing");
    CHECK(LoadPreferences().font == FontChoice{});
    CHECK(LoadPreferences().shortcut == Shortcut{});
    CHECK(!LoadPreferences().selectionShortcut);
    CHECK(LoadPreferences().browser.empty());
    CHECK(LoadPreferences().windowBehavior == WindowBehavior{});
    CHECK(!LoadPreferences().startWithWindows);

    wchar_t executable[32768]{};
    CHECK(GetModuleFileNameW(nullptr, executable, static_cast<DWORD>(std::size(executable))));
    CHECK(SaveWindowBehavior({false, true}));
    CHECK(LoadPreferences().windowBehavior == (WindowBehavior{false, true}));
    CHECK(SaveWindowBehavior({true, false}));
    CHECK(LoadPreferences().windowBehavior == (WindowBehavior{true, false}));
    CHECK(SaveWindowBehavior({}));
    CHECK(LoadPreferences().windowBehavior == WindowBehavior{});
    CHECK(SetStartWithWindows(false));
    CHECK(SetStartWithWindows(true) && LoadPreferences().startWithWindows);
    constexpr wchar_t startupKey[] = LR"(Software\Microsoft\Windows\CurrentVersion\Run)";
    wchar_t startupCommand[261]{};
    DWORD size = sizeof(startupCommand);
    CHECK(RegGetValueW(HKEY_CURRENT_USER, startupKey, L"YiSou", RRF_RT_REG_SZ,
        nullptr, startupCommand, &size) == ERROR_SUCCESS);
    CHECK(startupCommand == L"\"" + std::wstring(executable) + L"\" --startup");
    CHECK(SetStartWithWindows(false) && !LoadPreferences().startWithWindows);
    CHECK(RegGetValueW(HKEY_CURRENT_USER, startupKey, L"YiSou", RRF_RT_REG_SZ,
        nullptr, nullptr, &size) == ERROR_FILE_NOT_FOUND);
    CHECK(SetStartWithWindows(false));

    const auto path = BrowserPath(L" \"" + std::wstring(executable) + L"\" ");
    CHECK(!path.empty());
    CHECK(BrowserPath(L"missing.exe").empty());
    CHECK(!OpenBrowser(L"https://example.org", path + L".missing.exe"));
    CheckBrowserLaunch(path);
    CHECK(!SaveBrowser(L"missing.exe"));
    CHECK(SaveBrowser(path));
    CHECK(LoadPreferences().browser == path);
    const Shortcut configured{shortcutControl | shortcutShift, VK_SPACE};
    CHECK(SaveShortcut(configured));
    CHECK(!SaveShortcut({0, VK_SPACE}));
    CHECK(LoadPreferences().shortcut == configured);
    const SelectionShortcut selection{{shortcutControl | shortcutShift, VK_F22}, L"github"};
    CHECK(SaveSelectionShortcut(selection));
    CHECK(LoadPreferences().selectionShortcut == selection);
    auto preview = selection;
    preview.preview = true;
    CHECK(SaveSelectionShortcut(preview) && LoadPreferences().selectionShortcut == preview);
    CHECK(!SaveSelectionShortcut(SelectionShortcut{{}, L"github"}));
    CHECK(LoadPreferences().selectionShortcut == preview);
    CHECK(SaveSelectionShortcut(std::nullopt));
    CHECK(!LoadPreferences().selectionShortcut);
    CHECK(SaveShortcut({}));
    CHECK(SaveBrowser({}));
    CHECK(LoadPreferences().shortcut == Shortcut{} && LoadPreferences().browser.empty());
    CHECK(LoadPreferences().pinnedEngines.empty());
    const std::vector<EngineId> pinned{L"github", L"baidu", L"unknown"};
    CHECK(SavePinnedEngines(pinned) && LoadPreferences().pinnedEngines == pinned);
    CHECK(Application(LoadPreferences(), {}).preferences().pinnedEngines == (std::vector<EngineId>{L"github", L"baidu"}));
    CHECK(SavePinnedEngines({}) && LoadPreferences().pinnedEngines.empty());

    const std::wstring command = L"\"" + path + L"\"";
    constexpr wchar_t browserKey[] = LR"(Software\Clients\StartMenuInternet\YiSouTestBrowser)";
    constexpr wchar_t label[] = L"YiSou Test Browser";
    CHECK(RegSetKeyValueW(HKEY_CURRENT_USER, browserKey, nullptr, REG_SZ, label, sizeof(label)) == ERROR_SUCCESS);
    const std::wstring commandKey = std::wstring(browserKey) + LR"(\shell\open\command)";
    CHECK(RegSetKeyValueW(HKEY_CURRENT_USER, commandKey.c_str(), nullptr, REG_SZ, command.c_str(),
        static_cast<DWORD>((command.size() + 1) * sizeof(wchar_t))) == ERROR_SUCCESS);
    CHECK(std::ranges::any_of(DiscoverBrowsers(), [&](const Browser& browser) { return browser.path == path && browser.name == label; }));
    CheckShortcuts();
    CheckSelection();

    const FontChoice font{FontSource::file, LR"(D:\字体\HarmonyOS Sans.ttf)"};
    CHECK(SaveFont(font));
    CHECK(SaveEngine(L"google"));
    const auto loaded = LoadPreferences();
    CHECK(loaded.font == font);
    CHECK(loaded.engine == L"google");

    const FontChoice system{FontSource::system, L"Segoe UI"};
    CHECK(SaveFont(system));
    CHECK(LoadPreferences().font == system);
    CHECK(SaveFont({}));
    CHECK(LoadPreferences().font == FontChoice{});

    const DWORD legacyEngine = 2;
    CHECK(RegSetKeyValueW(HKEY_CURRENT_USER, L"Software\\YiSou", L"Engine", REG_DWORD, &legacyEngine,
        sizeof(legacyEngine)) == ERROR_SUCCESS);
    CHECK(LoadPreferences().engine == L"bing");
    CHECK(SaveEngine(L"baidu"));
    CHECK(LoadPreferences().engine == L"baidu");

    CHECK(LoadCustomEngines().empty());
    const SystemActions actions{OpenBrowser, SaveFont, SaveEngine, SaveCustomEngine};
    Application app(LoadPreferences(), actions);
    Engine custom{L"", L"测试引擎", L"https://example.org/?q=%s", EngineGroup::custom, 0x1090a0,
        IconPixels(iconSize * iconSize, 0xff102030)};
    CHECK(app.addEngine(custom) == AddEngineResult::added);
    const auto id = app.engines().back().id;
    CHECK(app.changeEngine(id));
    Engine solid{L"", L"纯色图标", custom.url, EngineGroup::custom, 0x610020};
    CHECK(app.addEngine(solid) == AddEngineResult::added);
    CHECK(RegSetKeyValueW(HKEY_CURRENT_USER, L"Software\\YiSou\\CustomEngines", L"custom-invalid", REG_BINARY,
        &legacyEngine, sizeof(legacyEngine)) == ERROR_SUCCESS);
    Application restarted(LoadPreferences(), actions, LoadCustomEngines());
    CHECK(restarted.engines().size() == app.engines().size());
    CHECK(restarted.activeEngine().id == id);
    CHECK(restarted.activeEngine().name == custom.name && restarted.activeEngine().url == custom.url);
    CHECK(restarted.activeEngine().color == custom.color && restarted.activeEngine().icon == custom.icon);
    CHECK(restarted.changeEngine(app.engines().back().id));
    CHECK(restarted.activeEngine().color == solid.color && restarted.activeEngine().icon.empty());

    auto current = restarted.engines();
    auto& edited = current.back();
    edited.name = L"新的名称";
    edited.alias = L"quick";
    std::iter_swap(current.end() - 2, current.end() - 1);
    CHECK(SaveEngines(current));
    const auto reloaded = LoadCustomEngines();
    CHECK(reloaded.size() == 2 && reloaded[0].name == L"新的名称" && reloaded[0].alias == L"quick");
    CHECK(reloaded[1].id == id && reloaded[1].icon == custom.icon);

    Engine vectorIcon{L"", L"SVG 图标", custom.url, EngineGroup::custom};
    vectorIcon.svgIcon = R"(<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/></svg>)";
    if (vectorIcon.svgIcon.size() % 2 == 0) vectorIcon.svgIcon.push_back('\n');
    const auto vectorId = SaveCustomEngine(vectorIcon);
    CHECK(!vectorId.empty());
    const auto withVector = LoadCustomEngines();
    CHECK(withVector.size() == 3 && withVector.back().id == vectorId && withVector.back().svgIcon == vectorIcon.svgIcon);
    CHECK(withVector.back().icon.empty() && withVector[1].icon == custom.icon);
    CHECK(SaveEngines({}) && LoadCustomEngines().empty());

    CHECK(!LoadPreferences().recordHistory && LoadHistory().empty());
    CHECK(SaveHistoryPreference(true) && LoadPreferences().recordHistory);
    RecordSearch({{}, L"选中文本", L"bing", L"必应", {}, SearchOrigin::selection, L"记事本"}, path, browserProbeUrl);
    RecordSearch({{}, L"手动搜索", L"github", L"GitHub", {}, SearchOrigin::typed, {}}, path, browserProbeUrl);
    const auto latest = LoadHistory({}, 1);
    CHECK(latest.size() == 1 && latest[0].query == L"手动搜索");
    CHECK(!latest[0].browser.empty() && !HistoryTime(latest[0].id).empty());
    const auto older = LoadHistory(latest[0].id, 1);
    CHECK(older.size() == 1 && older[0].query == L"选中文本" && older[0].origin == SearchOrigin::selection);
    CHECK(older[0].source == L"记事本" && older[0].engineId == L"bing");
    const auto filtered = LoadHistory({}, 1, L"文本", L"事本");
    CHECK(filtered.size() == 1 && filtered[0].id == older[0].id);
    CHECK(LoadHistory(filtered[0].id, 1, L"文本", L"事本").empty());
    CHECK(LoadHistory({}, 1, L"手动", L"记事本").empty());
    CHECK(DeleteHistory(older[0].id) && LoadHistory().size() == 1);
    CHECK(SaveHistoryPreference(false) && !LoadPreferences().recordHistory && LoadHistory().size() == 1);

    CHECK(!LoadDisplayPosition(L"test-display"));
    SaveDisplayPosition(L"test-display", {23, 124});
    const auto position = LoadDisplayPosition(L"test-display");
    CHECK(position && position->x == 23 && position->y == 124);
    CheckConfiguration(executable);
    CheckLocalization();
    return 0;
} catch (const std::exception& error) {
    std::cerr << error.what() << '\n';
    return 1;
}
