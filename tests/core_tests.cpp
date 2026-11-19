#include "application.h"
#include "check.h"
#include "search_cases.h"

#include <iostream>

namespace {
using namespace yisou;

std::wstring openedUrl;
bool allowOpen = true;
bool allowSave = true;
int fontWrites = 0;
int engineWrites = 0;
Preferences saved;
std::vector<SearchRecord> history;

bool Open(const std::wstring& url, const std::wstring&)
{
    openedUrl = url;
    return allowOpen;
}

bool SaveFont(const FontChoice& font)
{
    ++fontWrites;
    if (allowSave) saved.font = font;
    return allowSave;
}

bool SaveEngine(const EngineId& engine)
{
    ++engineWrites;
    if (allowSave) saved.engine = engine;
    return allowSave;
}

EngineId SaveCustomEngine(const Engine& engine)
{
    CHECK(engine.group == EngineGroup::custom);
    static unsigned next = 0;
    return allowSave ? L"custom-" + std::to_wstring(++next) : L"";
}

}

int main() try
{
    using namespace yisou;
    const SystemActions actions{Open, SaveFont, SaveEngine, SaveCustomEngine, nullptr, nullptr,
        [](bool enabled) { saved.recordHistory = enabled; return true; },
        [](const SearchRecord& row, const std::wstring&, const std::wstring&) { history.push_back(row); },
        [](const std::vector<Engine>&) { return allowSave; }, nullptr,
        [](const std::vector<EngineId>& engines) {
            if (!allowSave) return false;
            saved.pinnedEngines = engines;
            return true;
        }};
    Application app({}, actions);
    CHECK(app.suggestEngines(L"#").size() == app.engines().size());
    CHECK(app.suggestEngines(L"  #GH").front()->id == L"github");
    CHECK(app.suggestEngines(L"#百度").front()->id == L"baidu");
    CHECK(app.suggestEngines(L"#gh Direct2D").empty());
    CHECK(app.suggestEngines(L"#123").empty());
    CHECK(app.togglePinnedEngine(L"youtube"));
    CHECK(app.suggestEngines(L"#").front()->id == L"youtube");
    CHECK(app.suggestEngines(L"#yt").front()->id == L"youtube");
    CHECK(Application(saved, actions).isPinned(L"youtube"));
    allowSave = false;
    CHECK(!app.togglePinnedEngine(L"youtube") && app.isPinned(L"youtube"));
    allowSave = true;
    CHECK(app.togglePinnedEngine(L"youtube") && !app.isPinned(L"youtube"));
    for (const auto& test : searchCases) {
        CHECK(app.changeEngine(app.engines()[test.engine].id));
        openedUrl.clear();
        CHECK(app.search(test.query) == (test.expected.empty() ? SearchResult::emptyQuery : SearchResult::opened));
        CHECK(openedUrl == test.expected);
    }

    allowOpen = false;
    CHECK(app.search(L"retry") == SearchResult::openFailed);
    CHECK(openedUrl == L"https://www.bing.com/search?q=retry");

    const EngineId invalid = L"unknown-engine";
    const int writes = engineWrites;
    CHECK(!app.changeEngine(invalid));
    CHECK(app.changeEngine(L"bing"));
    CHECK(engineWrites == writes);
    CHECK(Application({{}, invalid}, actions).preferences().engine == L"bing");

    const FontChoice font{FontSource::system, L"Segoe UI"};
    allowSave = false;
    CHECK(!app.changeEngine(L"google"));
    CHECK(!app.changeFont(font));
    CHECK(app.preferences().engine == L"bing");
    CHECK(app.preferences().font == FontChoice{});

    allowSave = true;
    CHECK(app.changeEngine(L"google"));
    CHECK(app.changeFont(font));
    const int savedFontWrites = fontWrites;
    CHECK(app.changeFont(font));
    CHECK(fontWrites == savedFontWrites);
    Application restarted(saved, actions);
    CHECK(restarted.preferences().engine == L"google");
    CHECK(restarted.preferences().font == font);

    const auto defaultCount = app.engines().size();
    CHECK(defaultCount == 11);
    Engine custom;
    CHECK(app.addEngine(custom) == AddEngineResult::invalidName);
    custom.name = L" 自定义搜索 ";
    for (const auto* invalidUrl : {L"file:///C:/%s", L"https://example.org", L"https:///search/%s",
        L"https://%s.example.org/", L"https://example.org\\x?q=%s"}) {
        custom.url = invalidUrl;
        CHECK(app.addEngine(custom) == AddEngineResult::invalidUrl);
    }
    custom.url = L" https://example.org/%s?raw=%25&q=%s ";
    custom.icon = {0};
    CHECK(app.addEngine(custom) == AddEngineResult::invalidIcon);
    custom.icon.clear();
    allowSave = false;
    CHECK(app.addEngine(custom) == AddEngineResult::saveFailed);
    CHECK(app.engines().size() == defaultCount);
    allowSave = allowOpen = true;
    CHECK(app.addEngine(custom) == AddEngineResult::added);
    CHECK(app.engines().back().name == L"自定义搜索");
    CHECK(app.changeEngine(app.engines().back().id));
    CHECK(app.search(L" 中 + ") == SearchResult::opened);
    CHECK(openedUrl == L"https://example.org/%E4%B8%AD%20%2B?raw=%25&q=%E4%B8%AD%20%2B");
    CHECK(history.empty());
    CHECK(app.parseInput(L"#gh").query.empty());
    CHECK(app.parseInput(L"#GH\t Direct2D ").engine->id == L"github");
    CHECK(app.parseInput(L"#GH\t Direct2D ").query == L"Direct2D");
    CHECK(app.parseInput(L"#ghp x").query == L"#ghp x");
    CHECK(app.parseInput(L"C#").query == L"C#");
    CHECK(app.parseInput(L"#123").query == L"#123");
    CHECK(app.changeHistoryRecording(true) && saved.recordHistory);
    CHECK(app.search(L"#gh direct2d", nullptr, SearchOrigin::selection, L"VS Code") == SearchResult::opened);
    CHECK(history.size() == 1 && history[0].query == L"#gh direct2d" && history[0].source == L"VS Code");
    CHECK(history[0].engineId == app.activeEngine().id && history[0].origin == SearchOrigin::selection);
    allowOpen = false;
    CHECK(app.search(L"fail") == SearchResult::openFailed && history.size() == 1);
    allowOpen = true;
    CHECK(app.testEngine(custom) == SearchResult::opened && history.size() == 1);
    CHECK(app.openLink(L" https://example.org/?q=a=b#result ") == SearchResult::opened);
    CHECK(openedUrl == L"https://example.org/?q=a=b#result" && history.size() == 1);
    CHECK(app.openLink(L"https://example.org") == SearchResult::opened);
    CHECK(app.openLink(L"file:///C:/Windows") == SearchResult::emptyQuery);
    CHECK(app.openLink(L"https://trusted.example@other.example") == SearchResult::emptyQuery);
    CHECK(app.openLink(L"https://example.org/a b") == SearchResult::emptyQuery);
    CHECK(IsExecutableName(L"远程桌面.EXE") && !IsExecutableName(L"C:\\Windows\\mstsc.exe"));
    custom.alias = L"GH";
    CHECK(app.addEngine(custom) == AddEngineResult::duplicateAlias);
    custom.alias = L"中文";
    CHECK(app.addEngine(custom) == AddEngineResult::invalidAlias);
    custom.alias = L" #Demo ";
    CHECK(app.addEngine(custom) == AddEngineResult::added);
    const auto second = app.engines().back().id;
    CHECK(app.parseInput(L"#demo query").engine->id == second);
    CHECK(app.moveEngine(second, -1) && app.engines()[defaultCount].id == second);
    Engine patch = *app.findEngine(second);
    patch.alias = L"updated";
    CHECK(app.editEngine(patch) == AddEngineResult::added);
    CHECK(app.parseInput(L"#updated text").engine->id == second);
    allowSave = false;
    CHECK(!app.deleteEngine(second) && app.findEngine(second));
    allowSave = true;
    CHECK(app.changeEngine(second));
    CHECK(app.deleteEngine(second) && !app.findEngine(second) && app.activeEngine().id == L"bing");
    return 0;
} catch (const std::exception& error) {
    std::cerr << error.what() << '\n';
    return 1;
}
