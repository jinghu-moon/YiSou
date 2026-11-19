cmake_minimum_required(VERSION 3.20)
project(YiSou LANGUAGES CXX RC)

if(NOT DEFINED CMAKE_INTERPROCEDURAL_OPTIMIZATION_RELEASE)
    include(CheckIPOSupported)
    check_ipo_supported(RESULT CMAKE_INTERPROCEDURAL_OPTIMIZATION_RELEASE LANGUAGES CXX)
endif()

function(yisou_target target)
    target_compile_features(${target} PRIVATE cxx_std_20)
    target_compile_options(${target} PRIVATE /W4 /utf-8)
    set_property(TARGET ${target} PROPERTY MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
endfunction()

add_library(yisou_core STATIC src/core/application.cpp)
target_include_directories(yisou_core PUBLIC src/core)
yisou_target(yisou_core)

function(yisou_windows_target target)
    yisou_target(${target})
    target_compile_definitions(${target} PRIVATE UNICODE _UNICODE NOMINMAX)
endfunction()

add_library(yisou_platform STATIC src/platform/windows.cpp src/platform/config.cpp src/platform/browser.cpp src/platform/shortcut.cpp src/platform/selection.cpp src/platform/localization.cpp)
yisou_windows_target(yisou_platform)
target_link_libraries(yisou_platform PUBLIC yisou_core PRIVATE shell32 advapi32 ole32 oleaut32 shlwapi version uiautomationcore user32 gdi32)
option(YISOU_ENABLE_PERF_TRACE "Build opt-in performance diagnostics" OFF)
if(YISOU_ENABLE_PERF_TRACE)
    target_sources(yisou_platform PRIVATE src/platform/performance.cpp)
    target_compile_definitions(yisou_platform PUBLIC YISOU_ENABLE_PERF_TRACE)
endif()

find_program(YISOU_CARGO cargo REQUIRED)
if(CMAKE_VS_PLATFORM_NAME STREQUAL "ARM64" OR CMAKE_CXX_COMPILER_ARCHITECTURE_ID STREQUAL "ARM64")
    set(YISOU_RUST_TARGET aarch64-pc-windows-msvc)
elseif(CMAKE_SIZEOF_VOID_P EQUAL 4)
    set(YISOU_RUST_TARGET i686-pc-windows-msvc)
else()
    set(YISOU_RUST_TARGET x86_64-pc-windows-msvc)
endif()
set(YISOU_SVG_MANIFEST "${CMAKE_CURRENT_SOURCE_DIR}/src/svg/Cargo.toml")
set(YISOU_SVG_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(YISOU_SVG_LIB "${YISOU_SVG_TARGET_DIR}/${YISOU_RUST_TARGET}/release/yisou_svg.lib")
file(GLOB_RECURSE YISOU_SVG_SOURCES CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/src/svg/src/*.rs")
add_custom_command(OUTPUT "${YISOU_SVG_LIB}"
    COMMAND "${CMAKE_COMMAND}" -E env "RUSTFLAGS=-C target-feature=+crt-static"
        "${YISOU_CARGO}" build --release --locked --manifest-path "${YISOU_SVG_MANIFEST}"
        --target "${YISOU_RUST_TARGET}" --target-dir "${YISOU_SVG_TARGET_DIR}"
    DEPENDS ${YISOU_SVG_SOURCES} "${YISOU_SVG_MANIFEST}" src/svg/Cargo.lock
    VERBATIM)
add_custom_target(yisou_svg_build DEPENDS "${YISOU_SVG_LIB}")
add_library(yisou_svg STATIC IMPORTED)
set_target_properties(yisou_svg PROPERTIES IMPORTED_LOCATION "${YISOU_SVG_LIB}")
add_dependencies(yisou_svg yisou_svg_build)
target_link_libraries(yisou_svg INTERFACE advapi32 bcrypt ntdll userenv ws2_32)

set(YISOU_UI_LIBRARIES d2d1 d3d11 dcomp dwrite dwmapi comctl32 comdlg32 imm32 user32 gdi32 msimg32 ole32 shlwapi windowscodecs)
set(YISOU_UI_SOURCES src/ui/engine_flyout.cpp src/ui/engine_icons.cpp src/ui/settings.cpp)
add_library(yisou_ui STATIC src/ui/search_window.cpp ${YISOU_UI_SOURCES})
yisou_windows_target(yisou_ui)
target_link_libraries(yisou_ui PUBLIC yisou_core PRIVATE yisou_platform yisou_svg ${YISOU_UI_LIBRARIES})

file(GLOB YISOU_ICON_FILES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/assets/icons/*.svg"
    "${CMAKE_CURRENT_SOURCE_DIR}/assets/icons/tabler/*.svg")
set(YISOU_ICON_RESOURCES "")
foreach(icon IN LISTS YISOU_ICON_FILES)
    get_filename_component(name "${icon}" NAME_WE)
    string(REPLACE "-" "_" name "${name}")
    string(APPEND YISOU_ICON_RESOURCES "${name} RCDATA \"${icon}\"\n")
endforeach()
set(PREVIEW_THESVG "${CMAKE_CURRENT_SOURCE_DIR}/assets/icon-preview/thesvg")
set(PREVIEW_TABLER "${CMAKE_CURRENT_SOURCE_DIR}/assets/icon-preview/tabler")
string(APPEND YISOU_ICON_RESOURCES
    "preview_search RCDATA \"${PREVIEW_TABLER}/search.svg\"\n"
    "preview_yandex RCDATA \"${PREVIEW_THESVG}/yandex/default.svg\"\n"
    "preview_youtube_mono RCDATA \"${PREVIEW_THESVG}/youtube/mono.svg\"\n"
    "preview_youtube_color RCDATA \"${PREVIEW_THESVG}/youtube/default.svg\"\n"
    "preview_google RCDATA \"${PREVIEW_THESVG}/google/color.svg\"\n"
    "preview_bilibili RCDATA \"${PREVIEW_THESVG}/bilibili/color.svg\"\n"
    "preview_flame RCDATA \"${PREVIEW_TABLER}/flame.svg\"\n")
foreach(size IN ITEMS 16 18 20 24 30 36 42 48 64 72 128 256 512 1024)
    string(APPEND YISOU_ICON_RESOURCES
        "preview_native_${size} RCDATA \"${CMAKE_CURRENT_SOURCE_DIR}/assets/icon-preview/native-${size}.png\"\n"
        "preview_large_${size} RCDATA \"${CMAKE_CURRENT_SOURCE_DIR}/assets/icon-preview/large-${size}.png\"\n")
endforeach()
foreach(size IN ITEMS 16 18 20 24 30 36 42 48 64 72)
    string(APPEND YISOU_ICON_RESOURCES
        "preview_algorithms_${size} RCDATA \"${CMAKE_CURRENT_SOURCE_DIR}/assets/icon-preview/algorithms-${size}.png\"\n"
        "preview_reference_${size} RCDATA \"${CMAKE_CURRENT_SOURCE_DIR}/assets/icon-preview/reference-${size}.png\"\n")
endforeach()
file(GENERATE OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/engine_icons.rc" CONTENT "${YISOU_ICON_RESOURCES}")
set(YISOU_ICON_RC "${CMAKE_CURRENT_BINARY_DIR}/engine_icons.rc")

add_executable(YiSou WIN32 src/main.cpp src/ui/icon_preview.cpp ${YISOU_ICON_RC})
yisou_windows_target(YiSou)
target_link_libraries(YiSou PRIVATE yisou_platform yisou_ui user32 delayimp)
target_link_options(YiSou PRIVATE /DELAYLOAD:d2d1.dll /DELAYLOAD:d3d11.dll
    /DELAYLOAD:dcomp.dll /DELAYLOAD:DWrite.dll /DELAYLOAD:COMDLG32.dll)

include(CTest)
if(BUILD_TESTING)
    add_executable(YiSouCoreTests tests/core_tests.cpp)
    yisou_target(YiSouCoreTests)
    target_link_libraries(YiSouCoreTests PRIVATE yisou_core)
    add_test(NAME YiSou.Core COMMAND YiSouCoreTests)

    add_executable(YiSouPlatformTests tests/platform_tests.cpp)
    yisou_windows_target(YiSouPlatformTests)
    target_link_libraries(YiSouPlatformTests PRIVATE yisou_platform)
    add_test(NAME YiSou.Platform COMMAND YiSouPlatformTests)

    # The window tests compile the search UI with its private window procedure and font state.
    add_executable(YiSouWindowTests tests/window_tests.cpp ${YISOU_UI_SOURCES} ${YISOU_ICON_RC})
    yisou_windows_target(YiSouWindowTests)
    target_link_libraries(YiSouWindowTests PRIVATE yisou_platform yisou_svg ${YISOU_UI_LIBRARIES})
    add_test(NAME YiSou.Windows COMMAND YiSouWindowTests "${CMAKE_CURRENT_SOURCE_DIR}/assets/fonts/HarmonyOS_SansSC_Regular.ttf"
        "${CMAKE_CURRENT_SOURCE_DIR}/assets/icons/youtube.png"
        "${CMAKE_CURRENT_SOURCE_DIR}/assets/icons/youtube.svg")

    add_test(NAME YiSou.SvgFilter COMMAND "${CMAKE_COMMAND}" -E env "RUSTFLAGS=-C target-feature=+crt-static"
        "${YISOU_CARGO}" test --release --locked --manifest-path "${YISOU_SVG_MANIFEST}"
        --target "${YISOU_RUST_TARGET}" --target-dir "${YISOU_SVG_TARGET_DIR}")

    add_executable(YiSouSvgBenchmark tools/icon_algorithms/resvg.cpp)
    yisou_windows_target(YiSouSvgBenchmark)
    target_include_directories(YiSouSvgBenchmark PRIVATE src/ui)
    target_link_libraries(YiSouSvgBenchmark PRIVATE yisou_ui)

    add_executable(YiSouDirect2dBenchmark tools/icon_algorithms/direct2d.cpp)
    yisou_windows_target(YiSouDirect2dBenchmark)
    target_link_libraries(YiSouDirect2dBenchmark PRIVATE yisou_core d3d11 d2d1 shlwapi)
endif()
