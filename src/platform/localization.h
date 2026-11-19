#pragma once

#include "language.h"

#include <initializer_list>
#include <span>
#include <string>

namespace yisou::i18n {

enum class Text {
#define YISOU_TEXT(id, chinese, english) id,
#include "strings.inc"
#undef YISOU_TEXT
    count
};

Language ResolveLanguage(std::span<const std::wstring_view> preferred);
void SetLanguage(Language preference);
const wchar_t* LocaleName();
const wchar_t* Tr(Text text);
std::wstring Format(Text text, std::initializer_list<std::wstring_view> values);
const wchar_t* EngineName(std::wstring_view id, const wchar_t* storedName);

}
