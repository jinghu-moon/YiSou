#include "application.h"

#include <algorithm>
#include <utility>

namespace yisou {
namespace {

std::vector<Engine> BuiltInEngines()
{
    using enum EngineGroup;
    return {
        {L"bing", L"必应", L"https://www.bing.com/search?q=%s", web, 0x005fb8, {}, L"bing"},
        {L"baidu", L"百度", L"https://www.baidu.com/s?wd=%s", web, 0x005fb8, {}, L"bd"},
        {L"google", L"Google", L"https://www.google.com/search?q=%s", web, 0x005fb8, {}, L"g"},
        {L"bilibili", L"哔哩哔哩", L"https://search.bilibili.com/all?keyword=%s", video, 0x005fb8, {}, L"bili"},
        {L"youtube", L"YouTube", L"https://www.youtube.com/results?search_query=%s", video, 0x005fb8, {}, L"yt"},
        {L"zhihu", L"知乎", L"https://www.zhihu.com/search?type=content&q=%s", social, 0x005fb8, {}, L"zh"},
        {L"reddit", L"Reddit", L"https://www.reddit.com/search/?q=%s", social, 0x005fb8, {}, L"rd"},
        {L"github", L"GitHub", L"https://github.com/search?q=%s", development, 0x005fb8, {}, L"gh"},
        {L"stackoverflow", L"Stack Overflow", L"https://stackoverflow.com/search?q=%s", development, 0x005fb8, {}, L"so"},
        {L"taobao", L"淘宝", L"https://s.taobao.com/search?q=%s", shopping, 0x005fb8, {}, L"tb"},
        {L"amazon", L"Amazon", L"https://www.amazon.com/s?k=%s", shopping, 0x005fb8, {}, L"amz"},
    };
}

constexpr std::wstring_view whitespace = L" \t\r\n";

std::wstring_view Trim(std::wstring_view text)
{
    const auto first = text.find_first_not_of(whitespace);
    return first == text.npos ? std::wstring_view{} : text.substr(first, text.find_last_not_of(whitespace) - first + 1);
}

bool IsIcon(const Engine& engine)
{
    return engine.color <= 0xffffff && engine.svgIcon.size() <= svgIconLimit
        && (engine.icon.empty() || (engine.svgIcon.empty() && engine.icon.size() == iconSize * iconSize));
}

wchar_t LowerAscii(wchar_t character)
{
    return character >= L'A' && character <= L'Z' ? character + (L'a' - L'A') : character;
}

bool IsAlias(std::wstring_view alias)
{
    if (alias.empty()) return true;
    if (alias.front() < L'a' || alias.front() > L'z') return false;
    return alias.size() <= engineAliasLimit && std::ranges::all_of(alias, [](wchar_t ch) {
        return (ch >= L'a' && ch <= L'z') || (ch >= L'0' && ch <= L'9') || ch == L'-' || ch == L'_';
    });
}

bool SameAlias(std::wstring_view left, std::wstring_view right)
{
    return std::ranges::equal(left, right, {}, LowerAscii, LowerAscii);
}

void AppendEncodedByte(std::wstring& url, unsigned byte)
{
    constexpr wchar_t hex[] = L"0123456789ABCDEF";
    if ((byte >= 'a' && byte <= 'z') || (byte >= 'A' && byte <= 'Z')
        || (byte >= '0' && byte <= '9') || byte == '-' || byte == '.' || byte == '_' || byte == '~') {
        url.push_back(static_cast<wchar_t>(byte));
        return;
    }
    url.append({L'%', hex[byte >> 4], hex[byte & 15]});
}

void AppendEncodedCharacter(std::wstring& url, std::uint32_t value)
{
    if (value < 0x80) {
        AppendEncodedByte(url, value);
        return;
    }
    if (value < 0x800) {
        AppendEncodedByte(url, 0xc0 | (value >> 6));
    } else if (value < 0x10000) {
        AppendEncodedByte(url, 0xe0 | (value >> 12));
        AppendEncodedByte(url, 0x80 | ((value >> 6) & 0x3f));
    } else {
        AppendEncodedByte(url, 0xf0 | (value >> 18));
        AppendEncodedByte(url, 0x80 | ((value >> 12) & 0x3f));
        AppendEncodedByte(url, 0x80 | ((value >> 6) & 0x3f));
    }
    AppendEncodedByte(url, 0x80 | (value & 0x3f));
}

void AppendQuery(std::wstring& url, std::wstring_view text)
{
    for (std::size_t index = 0; index < text.size(); ++index) {
        auto value = static_cast<std::uint32_t>(text[index]);
        if constexpr (sizeof(wchar_t) == 2) {
            if (value >= 0xd800 && value <= 0xdbff && index + 1 < text.size()) {
                const auto low = static_cast<std::uint32_t>(text[index + 1]);
                if (low >= 0xdc00 && low <= 0xdfff) {
                    value = 0x10000 + ((value - 0xd800) << 10) + low - 0xdc00;
                    ++index;
                }
            }
        }
        if ((value >= 0xd800 && value <= 0xdfff) || value > 0x10ffff) value = 0xfffd;
        AppendEncodedCharacter(url, value);
    }
}

std::wstring SearchUrl(std::wstring_view pattern, std::wstring_view query)
{
    std::wstring encoded;
    AppendQuery(encoded, query);
    std::wstring url;
    for (auto marker = pattern.find(L"%s"); marker != pattern.npos; marker = pattern.find(L"%s")) {
        url.append(pattern.substr(0, marker));
        url.append(encoded);
        pattern.remove_prefix(marker + 2);
    }
    url.append(pattern);
    return url;
}

}

bool IsEngineName(std::wstring_view name)
{
    name = Trim(name);
    return !name.empty() && name.size() <= engineNameLimit
        && std::ranges::none_of(name, [](wchar_t c) { return c < 0x20 || c == 0x7f; });
}

bool IsWebUrl(std::wstring_view url)
{
    using namespace std::literals;
    url = Trim(url);
    const auto scheme = url.find(L"://");
    if (url.size() > engineUrlLimit || scheme == url.npos) return false;
    const auto sameLetter = [](wchar_t a, wchar_t b) { return a == b || a == b - (L'a' - L'A'); };
    if (!std::ranges::equal(url.substr(0, scheme), L"http"sv, sameLetter)
        && !std::ranges::equal(url.substr(0, scheme), L"https"sv, sameLetter)) return false;
    url.remove_prefix(scheme + 3);
    const auto end = url.find_first_of(L"/?#");
    const auto authority = url.substr(0, end);
    return !authority.empty() && authority.front() != L':' && authority.find_first_of(L"@%") == authority.npos
        && std::ranges::none_of(url, [](wchar_t c) { return c <= 0x20 || c == 0x7f || c == L'\\' || c == L'"'; });
}

bool IsSearchTemplate(std::wstring_view url)
{
    url = Trim(url);
    if (!IsWebUrl(url)) return false;
    const auto path = url.find_first_of(L"/?#", url.find(L"://") + 3);
    return path != url.npos && url.find(L"%s", path) != url.npos;
}

bool IsExecutableName(std::wstring_view name)
{
    return name.size() > 4 && name.size() <= 255 && SameAlias(name.substr(name.size() - 4), L".exe")
        && name.find_first_of(L"\\/:*?\"<>|,\t\r\n") == name.npos && Trim(name) == name;
}

Application::Application(Preferences preferences, SystemActions actions, std::vector<Engine> customEngines)
    : preferences_(std::move(preferences)), actions_(actions), engines_(BuiltInEngines())
{
    for (auto& engine : customEngines) {
        if (engine.id.starts_with(L"custom-") && !findEngine(engine.id) && validateEngine(engine) == AddEngineResult::added) {
            engine.group = EngineGroup::custom;
            engines_.push_back(std::move(engine));
        }
    }
    if (std::ranges::find(engines_, preferences_.engine, &Engine::id) == engines_.end()) preferences_.engine = L"bing";
    if (preferences_.selectionShortcut && !findEngine(preferences_.selectionShortcut->engine)) {
        preferences_.selectionShortcut.reset();
    }
    auto savedPins = std::exchange(preferences_.pinnedEngines, {});
    for (auto& id : savedPins) {
        if (findEngine(id) && !isPinned(id)) preferences_.pinnedEngines.push_back(std::move(id));
    }
}

const Preferences& Application::preferences() const
{
    return preferences_;
}

const std::vector<Engine>& Application::engines() const
{
    return engines_;
}

const Engine& Application::activeEngine() const
{
    return *std::ranges::find(engines_, preferences_.engine, &Engine::id);
}

const Engine* Application::findEngine(std::wstring_view id) const
{
    const auto engine = std::ranges::find(engines_, id, &Engine::id);
    return engine == engines_.end() ? nullptr : &*engine;
}

SearchInput Application::parseInput(std::wstring_view input, std::wstring_view temporary) const
{
    input = Trim(input);
    const auto* selected = findEngine(temporary);
    if (!selected) selected = &activeEngine();
    if (!input.starts_with(L'#')) return {selected, input};
    const auto split = input.find_first_of(whitespace);
    const auto alias = input.substr(0, split).substr(1);
    if (alias.empty()) return {selected, input};
    const auto engine = std::ranges::find_if(engines_, [&](const Engine& candidate) { return SameAlias(candidate.alias, alias); });
    if (engine == engines_.end()) return {selected, input};
    return {&*engine, split == input.npos ? std::wstring_view{} : Trim(input.substr(split)), true};
}

bool Application::isPinned(std::wstring_view id) const
{
    return std::ranges::find(preferences_.pinnedEngines, id) != preferences_.pinnedEngines.end();
}

bool Application::togglePinnedEngine(std::wstring_view id)
{
    if (!findEngine(id) || !actions_.savePinnedEngines) return false;
    auto pins = preferences_.pinnedEngines;
    if (const auto item = std::ranges::find(pins, id); item != pins.end()) pins.erase(item);
    else pins.emplace_back(id);
    if (!actions_.savePinnedEngines(pins)) return false;
    preferences_.pinnedEngines = std::move(pins);
    return true;
}

std::vector<const Engine*> Application::suggestEngines(std::wstring_view input,
    const wchar_t* (*displayName)(std::wstring_view, const wchar_t*)) const
{
    const auto first = input.find_first_not_of(whitespace);
    if (first == input.npos) return {};
    input.remove_prefix(first);
    if (!input.starts_with(L'#') || input.find_first_of(whitespace) != input.npos) return {};
    input.remove_prefix(1);

    std::vector<const Engine*> result;
    for (const auto& engine : engines_) {
        const std::wstring_view name = displayName ? displayName(engine.id, engine.name.c_str()) : engine.name.c_str();
        const auto nameMatch = std::ranges::search(name, input, {}, LowerAscii, LowerAscii);
        if (input.empty() || SameAlias(std::wstring_view(engine.alias).substr(0, input.size()), input)
            || !nameMatch.empty()) result.push_back(&engine);
    }
    std::ranges::stable_sort(result, {}, [&](const Engine* engine) {
        if (!input.empty() && SameAlias(engine->alias, input)) return 0;
        return isPinned(engine->id) ? 1 : 2;
    });
    return result;
}

bool Application::changeFont(const FontChoice& font)
{
    if (preferences_.font == font) return true;
    if (!actions_.saveFont(font)) return false;
    preferences_.font = font;
    return true;
}

bool Application::changeEngine(const EngineId& engine)
{
    if (preferences_.engine == engine) return true;
    if (std::ranges::find(engines_, engine, &Engine::id) == engines_.end()) return false;
    if (!actions_.saveEngine(engine)) return false;
    preferences_.engine = engine;
    return true;
}

bool Application::changeShortcut(Shortcut shortcut)
{
    if ((preferences_.selectionShortcut && preferences_.selectionShortcut->shortcut == shortcut)
        || preferences_.clipboardShortcut == shortcut) return false;
    if (preferences_.shortcut == shortcut) return true;
    if (!actions_.saveShortcut || !actions_.saveShortcut(shortcut)) return false;
    preferences_.shortcut = shortcut;
    return true;
}

bool Application::changeBrowser(const std::wstring& path)
{
    if (preferences_.browser == path) return true;
    if (!actions_.saveBrowser || !actions_.saveBrowser(path)) return false;
    preferences_.browser = path;
    return true;
}

bool Application::changeSelectionShortcut(const std::optional<SelectionShortcut>& binding)
{
    if (binding && (!binding->shortcut.key || binding->shortcut == preferences_.shortcut
        || preferences_.clipboardShortcut == binding->shortcut || !findEngine(binding->engine))) return false;
    if (preferences_.selectionShortcut == binding) return true;
    if (!actions_.saveSelectionShortcut || !actions_.saveSelectionShortcut(binding)) return false;
    preferences_.selectionShortcut = binding;
    return true;
}

bool Application::changeHistoryRecording(bool enabled)
{
    if (preferences_.recordHistory == enabled) return true;
    if (!actions_.saveHistoryPreference || !actions_.saveHistoryPreference(enabled)) return false;
    preferences_.recordHistory = enabled;
    return true;
}

bool Application::changeWindowBehavior(WindowBehavior behavior)
{
    if (preferences_.windowBehavior == behavior) return true;
    if (!actions_.saveWindowBehavior || !actions_.saveWindowBehavior(behavior)) return false;
    preferences_.windowBehavior = behavior;
    return true;
}

bool Application::changeEngineTileNames(bool show)
{
    if (preferences_.showEngineTileNames == show) return true;
    if (!actions_.saveEngineTileNames || !actions_.saveEngineTileNames(show)) return false;
    preferences_.showEngineTileNames = show;
    return true;
}

bool Application::changeStartWithWindows(bool enabled)
{
    if (!actions_.setStartWithWindows || !actions_.setStartWithWindows(enabled)) return false;
    preferences_.startWithWindows = enabled;
    return true;
}

bool Application::changeClipboardShortcut(const std::optional<Shortcut>& shortcut)
{
    if (shortcut && (!shortcut->key || *shortcut == preferences_.shortcut
        || (preferences_.selectionShortcut && preferences_.selectionShortcut->shortcut == *shortcut))) return false;
    if (preferences_.clipboardShortcut == shortcut) return true;
    if (!actions_.saveClipboardShortcut || !actions_.saveClipboardShortcut(shortcut)) return false;
    preferences_.clipboardShortcut = shortcut;
    return true;
}

bool Application::changeExcludedApps(std::vector<std::wstring> apps)
{
    if (!std::ranges::all_of(apps, IsExecutableName)) return false;
    if (preferences_.doubleAltExcludedApps == apps) return true;
    if (!actions_.saveExcludedApps || !actions_.saveExcludedApps(apps)) return false;
    preferences_.doubleAltExcludedApps = std::move(apps);
    return true;
}

AddEngineResult Application::validateEngine(Engine& engine) const
{
    engine.name = Trim(engine.name);
    engine.url = Trim(engine.url);
    engine.alias = Trim(engine.alias);
    if (engine.alias.starts_with(L'#')) engine.alias.erase(0, 1);
    std::ranges::transform(engine.alias, engine.alias.begin(), LowerAscii);
    if (!IsEngineName(engine.name)) return AddEngineResult::invalidName;
    if (!IsSearchTemplate(engine.url)) return AddEngineResult::invalidUrl;
    if (!IsIcon(engine)) return AddEngineResult::invalidIcon;
    if (!IsAlias(engine.alias)) return AddEngineResult::invalidAlias;
    if (!engine.alias.empty() && std::ranges::any_of(engines_, [&](const Engine& existing) {
        return existing.id != engine.id && existing.alias == engine.alias;
    })) return AddEngineResult::duplicateAlias;
    return AddEngineResult::added;
}

AddEngineResult Application::addEngine(Engine engine)
{
    engine.id.clear();
    if (const auto result = validateEngine(engine); result != AddEngineResult::added) return result;
    engine.group = EngineGroup::custom;
    engine.id = actions_.saveCustomEngine(engine);
    if (engine.id.empty()) return AddEngineResult::saveFailed;
    engines_.push_back(std::move(engine));
    return AddEngineResult::added;
}

bool Application::saveEngines(std::vector<Engine> updated)
{
    if (!actions_.saveEngines || !actions_.saveEngines(updated)) return false;
    engines_ = std::move(updated);
    return true;
}

AddEngineResult Application::editEngine(Engine engine)
{
    if (const auto result = validateEngine(engine); result != AddEngineResult::added) return result;
    auto updated = engines_;
    const auto existing = std::ranges::find(updated, engine.id, &Engine::id);
    if (existing == updated.end() || existing->group != EngineGroup::custom) return AddEngineResult::saveFailed;
    engine.group = EngineGroup::custom;
    *existing = std::move(engine);
    return saveEngines(std::move(updated)) ? AddEngineResult::added : AddEngineResult::saveFailed;
}

bool Application::deleteEngine(const EngineId& id)
{
    const EngineId removed = id;
    auto updated = engines_;
    const auto existing = std::ranges::find(updated, id, &Engine::id);
    if (existing == updated.end() || existing->group != EngineGroup::custom) return false;
    const bool resetDefault = preferences_.engine == id;
    updated.erase(existing);
    if (!saveEngines(std::move(updated))) return false;
    if (resetDefault) preferences_.engine = L"bing";
    if (preferences_.selectionShortcut && preferences_.selectionShortcut->engine == removed) {
        preferences_.selectionShortcut.reset();
    }
    std::erase(preferences_.pinnedEngines, removed);
    return true;
}

bool Application::moveEngine(const EngineId& id, int direction)
{
    auto updated = engines_;
    const auto engine = std::ranges::find(updated, id, &Engine::id);
    if (engine == updated.end() || engine->group != EngineGroup::custom || (direction != -1 && direction != 1)) return false;
    const auto target = engine - updated.begin() + direction;
    if (target < 0 || target >= static_cast<std::ptrdiff_t>(updated.size()) || updated[target].group != EngineGroup::custom) return false;
    std::iter_swap(engine, updated.begin() + target);
    return saveEngines(std::move(updated));
}

SearchResult Application::testEngine(const Engine& engine, std::wstring_view query) const
{
    if (!IsSearchTemplate(engine.url)) return SearchResult::emptyQuery;
    return actions_.openUrl(SearchUrl(Trim(engine.url), query), preferences_.browser) ? SearchResult::opened : SearchResult::openFailed;
}

SearchResult Application::openLink(std::wstring_view url) const
{
    url = Trim(url);
    if (!IsWebUrl(url)) return SearchResult::emptyQuery;
    return actions_.openUrl(std::wstring(url), preferences_.browser) ? SearchResult::opened : SearchResult::openFailed;
}

SearchResult Application::search(std::wstring_view query, const Engine* engine, SearchOrigin origin, std::wstring_view source) const
{
    query = Trim(query);
    if (query.empty()) return SearchResult::emptyQuery;
    if (!engine) engine = &activeEngine();
    const auto url = SearchUrl(engine->url, query);
    if (!actions_.openUrl(url, preferences_.browser)) return SearchResult::openFailed;
    if (preferences_.recordHistory && actions_.recordSearch) {
        actions_.recordSearch({{}, std::wstring(query), engine->id, engine->name, {}, origin, std::wstring(source)},
            preferences_.browser, url);
    }
    return SearchResult::opened;
}

}
