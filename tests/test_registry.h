#pragma once

#include "check.h"

#include <windows.h>
#include <string>

struct TestRegistry {
    const std::wstring path = L"Software\\YiSou-Test-" + std::to_wstring(GetCurrentProcessId())
        + L"-" + std::to_wstring(GetTickCount64());
    HKEY root = nullptr;
    bool overridden = false;

    ~TestRegistry()
    {
        if (overridden) RegOverridePredefKey(HKEY_CURRENT_USER, nullptr);
        if (root) {
            RegDeleteTreeW(root, L"Software");
            RegCloseKey(root);
            RegDeleteKeyW(HKEY_CURRENT_USER, path.c_str());
        }
    }

    void redirect()
    {
        DWORD disposition = 0;
        CHECK(RegCreateKeyExW(HKEY_CURRENT_USER, path.c_str(), 0, nullptr, REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS, nullptr, &root, &disposition) == ERROR_SUCCESS);
        if (disposition != REG_CREATED_NEW_KEY) {
            RegCloseKey(root);
            root = nullptr;
            throw std::runtime_error("Registry test key already exists");
        }
        overridden = RegOverridePredefKey(HKEY_CURRENT_USER, root) == ERROR_SUCCESS;
        CHECK(overridden);
    }
};
