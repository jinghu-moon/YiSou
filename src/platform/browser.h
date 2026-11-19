#pragma once

#include <string>
#include <vector>

namespace yisou::platform {

struct Browser {
    std::wstring name;
    std::wstring path;
};

std::wstring BrowserPath(std::wstring path);
Browser DescribeBrowser(const std::wstring& path);
std::wstring DefaultBrowserPath(const wchar_t* protocol = L"https");
std::vector<Browser> DiscoverBrowsers();

}
