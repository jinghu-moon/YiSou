#pragma once

#include "application.h"
#include <optional>

namespace yisou::platform {

[[nodiscard]] Preferences LoadPreferences();
[[nodiscard]] bool SaveFont(const FontChoice& font);
[[nodiscard]] bool SaveEngine(const EngineId& engine);
[[nodiscard]] std::vector<Engine> LoadCustomEngines();
[[nodiscard]] EngineId SaveCustomEngine(const Engine& engine);
[[nodiscard]] bool SaveEngines(const std::vector<Engine>& engines);
[[nodiscard]] bool SaveShortcut(Shortcut shortcut);
[[nodiscard]] bool SaveSelectionShortcut(const std::optional<SelectionShortcut>& binding);
[[nodiscard]] bool SavePinnedEngines(const std::vector<EngineId>& engines);
[[nodiscard]] bool SaveWindowBehavior(WindowBehavior behavior);
[[nodiscard]] bool SaveClipboardShortcut(const std::optional<Shortcut>& shortcut);
[[nodiscard]] bool SaveExcludedApps(const std::vector<std::wstring>& apps);
[[nodiscard]] bool StartsWithWindows();
[[nodiscard]] bool SetStartWithWindows(bool enabled);
[[nodiscard]] bool SaveBrowser(const std::wstring& path);
[[nodiscard]] bool OpenBrowser(const std::wstring& url, const std::wstring& browser);
[[nodiscard]] bool SaveHistoryPreference(bool enabled);
void RecordSearch(const SearchRecord& record, const std::wstring& browserPath, const std::wstring& url);
[[nodiscard]] std::vector<SearchRecord> LoadHistory(std::wstring_view before = {}, size_t limit = 25,
    std::wstring_view keyword = {}, std::wstring_view source = {});
[[nodiscard]] bool DeleteHistory(const std::wstring& id);
[[nodiscard]] std::wstring HistoryTime(std::wstring_view id);

struct DisplayPosition { int x; int y; };
[[nodiscard]] std::optional<DisplayPosition> LoadDisplayPosition(const wchar_t* display);
void SaveDisplayPosition(const wchar_t* display, DisplayPosition position);

}
