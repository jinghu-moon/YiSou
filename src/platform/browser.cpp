#include "browser.h"
#include "windows_services.h"

#include <windows.h>
#include <shellapi.h>
#include <shlwapi.h>

#include <algorithm>
#include <filesystem>

namespace yisou::platform {
namespace {

std::wstring QueryString(HKEY root, const wchar_t* subkey)
{
    DWORD bytes = 0;
    constexpr DWORD types = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    if (RegGetValueW(root, subkey, nullptr, types, nullptr, nullptr, &bytes) != ERROR_SUCCESS
        || bytes < sizeof(wchar_t) || bytes > 65536 || bytes % sizeof(wchar_t)) return {};
    std::wstring value(bytes / sizeof(wchar_t), L'\0');
    if (RegGetValueW(root, subkey, nullptr, types, nullptr, value.data(), &bytes) != ERROR_SUCCESS) return {};
    value.resize(wcslen(value.c_str()));
    return value;
}

std::wstring CommandPath(const std::wstring& command)
{
    if (auto path = BrowserPath(command); !path.empty()) return path;
    int count = 0;
    LPWSTR* args = CommandLineToArgvW(command.c_str(), &count);
    if (!args) return {};
    auto path = count ? BrowserPath(args[0]) : std::wstring{};
    LocalFree(args);
    return path;
}

void AppendBrowser(std::vector<Browser>& list, Browser browser)
{
    if (browser.path.empty() || std::ranges::any_of(list, [&](const Browser& known) {
        return CompareStringOrdinal(known.path.c_str(), -1, browser.path.c_str(), -1, TRUE) == CSTR_EQUAL;
    })) return;
    list.push_back(std::move(browser));
}

void ReadRegistered(std::vector<Browser>& list, HKEY root, REGSAM view)
{
    HKEY key;
    if (RegOpenKeyExW(root, LR"(Software\Clients\StartMenuInternet)", 0, KEY_READ | view, &key) != ERROR_SUCCESS) return;
    for (DWORD index = 0; ; ++index) {
        wchar_t name[256]{};
        DWORD size = static_cast<DWORD>(std::size(name));
        const auto result = RegEnumKeyExW(key, index, name, &size, nullptr, nullptr, nullptr, nullptr);
        if (result == ERROR_MORE_DATA) continue;
        if (result != ERROR_SUCCESS) break;
        const auto path = CommandPath(QueryString(key, (std::wstring(name) + LR"(\shell\open\command)").c_str()));
        if (path.empty()) continue;
        auto label = QueryString(key, name);
        if (label.empty() || label.front() == L'@') label = DescribeBrowser(path).name;
        AppendBrowser(list, {std::move(label), path});
    }
    RegCloseKey(key);
}

std::wstring QuoteArgument(std::wstring_view argument)
{
    std::wstring quoted = L"\"";
    size_t slashes = 0;
    for (const wchar_t ch : argument) {
        if (ch == L'\\') {
            ++slashes;
            continue;
        }
        quoted.append(ch == L'"' ? 2 * slashes + 1 : slashes, L'\\');
        slashes = 0;
        quoted.push_back(ch);
    }
    quoted.append(2 * slashes, L'\\');
    quoted.push_back(L'"');
    return quoted;
}

}

std::wstring BrowserPath(std::wstring path)
{
    const auto first = path.find_first_not_of(L" \r\n\t");
    if (first == path.npos) return {};
    path = path.substr(first, path.find_last_not_of(L" \r\n\t") - first + 1);
    if (path.size() > 1 && path.front() == L'"' && path.back() == L'"') path = path.substr(1, path.size() - 2);
    if (path.find(L'"') != path.npos || path.find(L'\0') != path.npos) return {};
    const std::filesystem::path file(path);
    if (!file.is_absolute() || _wcsicmp(file.extension().c_str(), L".exe") != 0) return {};
    const auto attrs = GetFileAttributesW(path.c_str());
    if (attrs == INVALID_FILE_ATTRIBUTES || (attrs & FILE_ATTRIBUTE_DIRECTORY)) return {};
    return file.lexically_normal().wstring();
}

Browser DescribeBrowser(const std::wstring& path)
{
    auto name = std::filesystem::path(path).stem().wstring();
    DWORD ignored = 0;
    const DWORD size = GetFileVersionInfoSizeW(path.c_str(), &ignored);
    if (!size || size > 1024 * 1024) return {std::move(name), path};
    std::vector<BYTE> data(size);
    if (!GetFileVersionInfoW(path.c_str(), 0, size, data.data())) return {std::move(name), path};
    struct Translation { WORD language; WORD codePage; };
    void* value = nullptr;
    UINT length = 0;
    if (VerQueryValueW(data.data(), LR"(\VarFileInfo\Translation)", &value, &length) && length >= sizeof(Translation)) {
        const auto info = *static_cast<const Translation*>(value);
        wchar_t key[64];
        swprintf_s(key, LR"(\StringFileInfo\%04x%04x\FileDescription)", info.language, info.codePage);
        if (VerQueryValueW(data.data(), key, &value, &length) && length > 1) name.assign(static_cast<const wchar_t*>(value), length - 1);
    }
    return {std::move(name), path};
}

std::wstring DefaultBrowserPath(const wchar_t* protocol)
{
    DWORD chars = 0;
    if (AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, protocol, L"open", nullptr, &chars) != S_FALSE
        || !chars || chars > 32768) return {};
    std::wstring path(chars, L'\0');
    if (FAILED(AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, protocol, L"open", path.data(), &chars))) return {};
    path.resize(wcslen(path.c_str()));
    return BrowserPath(std::move(path));
}

std::vector<Browser> DiscoverBrowsers()
{
    std::vector<Browser> browsers;
    for (const HKEY root : {HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE}) {
        ReadRegistered(browsers, root, KEY_WOW64_64KEY);
        ReadRegistered(browsers, root, KEY_WOW64_32KEY);
    }
    if (const auto path = DefaultBrowserPath(); !path.empty()) AppendBrowser(browsers, DescribeBrowser(path));
    std::ranges::sort(browsers, [](const Browser& left, const Browser& right) {
        return CompareStringOrdinal(left.name.c_str(), -1, right.name.c_str(), -1, TRUE) == CSTR_LESS_THAN;
    });
    return browsers;
}

bool OpenBrowser(const std::wstring& url, const std::wstring& browser)
{
    if (browser.empty()) {
        return reinterpret_cast<INT_PTR>(ShellExecuteW(nullptr, L"open", url.c_str(),
            nullptr, nullptr, SW_SHOWNORMAL)) > 32;
    }
    const auto path = BrowserPath(browser);
    if (path.empty()) return false;
    auto command = QuoteArgument(path) + L" " + QuoteArgument(url);
    const auto directory = std::filesystem::path(path).parent_path();
    STARTUPINFOW startup{sizeof(startup)};
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(path.c_str(), command.data(), nullptr, nullptr, FALSE, 0, nullptr,
        directory.c_str(), &startup, &process)) return false;
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    return true;
}

}
