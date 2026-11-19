#pragma once

#include <string_view>

namespace yisou {

enum class Language { system, simplifiedChinese, english };

constexpr std::wstring_view LanguageTag(Language language)
{
    switch (language) {
    case Language::simplifiedChinese: return L"zh-CN";
    case Language::english: return L"en";
    case Language::system: return L"system";
    }
    return L"system";
}

}
