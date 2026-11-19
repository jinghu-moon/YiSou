#include "platform/localization.h"
#include "platform/windows_services.h"
#include "platform/config.h"
#include "platform/performance.h"
#include "ui/icon_preview.h"
#include "ui/search_window.h"

#include <shellapi.h>

using yisou::i18n::Text;
using yisou::i18n::Tr;

namespace {

int Restart(const std::filesystem::path& path, bool openConfiguration)
{
    std::wstring executable(32768, L'\0');
    const auto length = GetModuleFileNameW(nullptr, executable.data(), static_cast<DWORD>(executable.size()));
    if (!length || length >= executable.size()) return 1;
    executable.resize(length);
    auto command = L"\"" + executable + (openConfiguration ? L"\" --configuration" : L"\" --settings")
        + L" --config \"" + path.native() + L"\"";
    STARTUPINFOW startup{sizeof(startup)};
    PROCESS_INFORMATION child{};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE, 0, nullptr, nullptr,
        &startup, &child)) {
        MessageBoxW(nullptr, Tr(Text::restartFailed), Tr(Text::appName), MB_OK | MB_ICONWARNING);
        return 1;
    }
    AllowSetForegroundWindow(child.dwProcessId);
    CloseHandle(child.hThread);
    CloseHandle(child.hProcess);
    return 0;
}

}

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE, PWSTR, int)
{
#ifdef YISOU_ENABLE_PERF_TRACE
    yisou::perf::Session performance;
#endif
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    using namespace yisou;
    auto mode = ui::LaunchMode::manual;
    std::filesystem::path configPath;
    int count = 0;
    const auto arguments = CommandLineToArgvW(GetCommandLineW(), &count);
    if (!arguments) return 1;
    bool valid = true;
    bool iconPreview = false;
    for (int index = 1; index < count; ++index) {
        const std::wstring_view argument(arguments[index]);
        if (argument == L"--startup") mode = ui::LaunchMode::startup;
        else if (argument == L"--settings") mode = ui::LaunchMode::settings;
        else if (argument == L"--configuration") mode = ui::LaunchMode::configuration;
        else if (argument == L"--icon-preview") iconPreview = true;
        else if (argument == L"--config" && index + 1 < count && *arguments[index + 1]) configPath = arguments[++index];
        else valid = false;
    }
    LocalFree(arguments);
    if (!valid) {
        i18n::SetLanguage(Language::system);
        MessageBoxW(nullptr, Tr(Text::configUsage), Tr(Text::appName), MB_OK | MB_ICONWARNING);
        return 1;
    }
    if (iconPreview) return ui::RunIconPreview(instance);

    const HANDLE singleton = CreateMutexW(nullptr, FALSE, L"Local\\YiSou.Resident");
    if (!singleton) return 1;
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        ui::ActivateSearchWindow(mode);
        CloseHandle(singleton);
        return 0;
    }
    if (const auto result = platform::config::Initialize(configPath); result != platform::config::Result::ok) {
        i18n::SetLanguage(Language::system);
        const auto message = std::wstring(platform::config::Describe(result)) + L"\n\n" + platform::config::ActivePath().native();
        MessageBoxW(nullptr, message.c_str(), Tr(Text::configurationWindowTitle), MB_OK | MB_ICONWARNING);
        CloseHandle(singleton);
        return 1;
    }
    auto preferences = platform::LoadPreferences();
    i18n::SetLanguage(preferences.language);
    Application application(std::move(preferences),
        {platform::OpenBrowser, platform::SaveFont, platform::SaveEngine, platform::SaveCustomEngine,
            platform::SaveShortcut, platform::SaveBrowser, platform::SaveHistoryPreference, platform::RecordSearch,
            platform::SaveEngines, platform::SaveSelectionShortcut, platform::SavePinnedEngines,
            platform::SaveWindowBehavior, platform::SetStartWithWindows, platform::SaveClipboardShortcut, platform::SaveExcludedApps,
            platform::config::SaveEngineTileNames},
        platform::LoadCustomEngines());
    const auto result = ui::RunSearchWindow(instance, application, mode);
    CloseHandle(singleton);
    if (result == ui::restartForConfig || result == ui::restartForLanguage) {
        return Restart(platform::config::ActivePath(), result == ui::restartForConfig);
    }
    return result;
}
