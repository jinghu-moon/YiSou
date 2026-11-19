#pragma once

#include "application.h"

#include <filesystem>
#include <utility>

namespace yisou::platform::config {

struct Settings {
    Preferences preferences;
    std::vector<Engine> engines;
};

enum class Result { ok, missing, invalid, ioError };

Result Read(const std::filesystem::path& path, Settings& settings);
bool Write(const std::filesystem::path& path, const Settings& settings);
Result Initialize(const std::filesystem::path& overridePath);
const Settings* Active();
const std::filesystem::path& ActivePath();
std::filesystem::path StartupOverride();
bool Save(Settings settings);
bool SaveLanguage(Language language);
bool SaveEngineTileNames(bool show);
Result Import(const std::filesystem::path& path);
bool Export(const std::filesystem::path& path);
Result ChooseLocation(const std::filesystem::path& path);
Result ImportEngines(const std::filesystem::path& path);
bool ExportEngines(const std::filesystem::path& path);
const wchar_t* Describe(Result result);

template<class Change>
bool Update(Change change)
{
    auto settings = *Active();
    change(settings);
    return Save(std::move(settings));
}

}
