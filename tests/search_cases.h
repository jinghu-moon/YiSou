#pragma once

#include <array>
#include <string_view>

struct SearchCase {
    size_t engine;
    std::wstring_view query;
    std::wstring_view expected;
};

inline constexpr std::array searchCases{
    SearchCase{0, L" \t\r\n", L""},
    SearchCase{0, L" \talpha beta\r\n", L"https://www.bing.com/search?q=alpha%20beta"},
    SearchCase{1, L"\u4e2d\u6587 + &/%#?", L"https://www.baidu.com/s?wd=%E4%B8%AD%E6%96%87%20%2B%20%26%2F%25%23%3F"},
    SearchCase{2, L"\U0001f50d", L"https://www.google.com/search?q=%F0%9F%94%8D"},
    SearchCase{0, L"AZaz09-._~", L"https://www.bing.com/search?q=AZaz09-._~"}
};
