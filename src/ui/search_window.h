#pragma once

#include "application.h"

#include <windows.h>

namespace yisou::ui {

enum class LaunchMode { manual, settings, configuration, startup };

inline constexpr int restartForConfig = 10;
inline constexpr int restartForLanguage = 11;
void ActivateSearchWindow(LaunchMode mode);
int RunSearchWindow(HINSTANCE instance, Application& application, LaunchMode mode);

}
