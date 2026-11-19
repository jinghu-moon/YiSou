#include "localization.h"

#include <windows.h>

#include <array>
#include <cwchar>
#include <vector>

namespace yisou::i18n {
namespace {

struct Translation {
    std::wstring_view chinese;
    std::wstring_view english;
};

constexpr std::array translations{
#define YISOU_TEXT(id, chinese, english) Translation{{chinese, std::size(chinese) - 1}, {english, std::size(english) - 1}},
#include "strings.inc"
#undef YISOU_TEXT
};
static_assert(translations.size() == static_cast<size_t>(Text::count));
static_assert([] {
    for (const auto& row : translations) {
        if (row.chinese.empty() || row.english.empty()) return false;
        for (const auto token : {L"{0}", L"{1}"}) {
            if ((row.chinese.find(token) == row.chinese.npos) != (row.english.find(token) == row.english.npos)) return false;
        }
    }
    return true;
}());

Language current = Language::english;

Language MatchLanguage(std::wstring_view name)
{
    const auto equal = [](std::wstring_view left, std::wstring_view tag) {
        return CompareStringOrdinal(left.data(), static_cast<int>(left.size()), tag.data(),
            static_cast<int>(tag.size()), TRUE) == CSTR_EQUAL;
    };
    if (name.empty()) return Language::system;
    const auto delimiter = name.find(L'-');
    if (equal(name.substr(0, delimiter), L"en")) return Language::english;
    if (!equal(name.substr(0, delimiter), L"zh")) return Language::system;
    if (delimiter == name.npos) return Language::simplifiedChinese;
    name.remove_prefix(delimiter + 1);
    const auto variant = name.substr(0, name.find(L'-'));
    if (equal(variant, L"CN") || equal(variant, L"SG") || equal(variant, L"Hans")) return Language::simplifiedChinese;
    return Language::system;
}

Language WindowsLanguage()
{
    ULONG count = 0, size = 0;
    if (!GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &count, nullptr, &size) || !size) return Language::english;
    std::wstring names(size, L'\0');
    if (!GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &count, names.data(), &size)) return Language::english;
    std::vector<std::wstring_view> preferred;
    for (const wchar_t* name = names.c_str(); *name; name += std::wcslen(name) + 1) preferred.emplace_back(name);
    return ResolveLanguage(preferred);
}

}

Language ResolveLanguage(std::span<const std::wstring_view> preferred)
{
    for (const auto name : preferred) {
        if (const auto language = MatchLanguage(name); language != Language::system) return language;
    }
    return Language::english;
}

void SetLanguage(Language preference)
{
    current = preference == Language::system ? WindowsLanguage() : preference;
}

const wchar_t* LocaleName()
{
    return current == Language::simplifiedChinese ? L"zh-CN" : L"en-US";
}

const wchar_t* Tr(Text text)
{
    const auto& row = translations[static_cast<size_t>(text)];
    return (current == Language::simplifiedChinese ? row.chinese : row.english).data();
}

std::wstring Format(Text text, std::initializer_list<std::wstring_view> values)
{
    std::wstring result;
    const std::wstring_view pattern(Tr(text));
    for (size_t i = 0; i < pattern.size(); ++i) {
        if (pattern[i] == L'{' && i + 2 < pattern.size() && pattern[i + 2] == L'}'
            && pattern[i + 1] >= L'0' && pattern[i + 1] <= L'9') {
            const auto index = static_cast<size_t>(pattern[i + 1] - L'0');
            if (index < values.size()) {
                result.append(values.begin()[index]);
                i += 2;
                continue;
            }
        }
        result += pattern[i];
    }
    return result;
}

const wchar_t* EngineName(std::wstring_view id, const wchar_t* storedName)
{
    constexpr std::array engineNames{
        std::pair{L"bing", Text::bingName}, std::pair{L"baidu", Text::baiduName},
        std::pair{L"bilibili", Text::bilibiliName}, std::pair{L"zhihu", Text::zhihuName}, std::pair{L"taobao", Text::taobaoName},
    };
    for (const auto& [engine, text] : engineNames) {
        if (id == engine) return Tr(text);
    }
    return storedName;
}

}
