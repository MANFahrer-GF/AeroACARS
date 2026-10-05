# CMakeLists.txt — AeroACARS X-Plane Native Plugin
#
# Builds a single shared-library plugin (.xpl) for X-Plane 11/12 on
# Windows / macOS / Linux. Linked against the X-Plane Plugin SDK 4.3.0
# (vendored under third_party/XPSDK430/, BSD-licensed).
#
# Plugin output naming (X-Plane convention — fat folder layout):
#   AeroACARS/
#       64/
#           win.xpl          (Windows x86_64)
#           mac.xpl          (macOS universal, x86_64 + arm64)
#           lin.xpl          (Linux x86_64)
#
# Build flow per platform:
#   Windows:   cmake -B build -G "Visual Studio 17 2022" -A x64
#              cmake --build build --config Release --target AeroACARS
#   macOS:     cmake -B build -G "Unix Makefiles" \
#                    -DCMAKE_OSX_ARCHITECTURES="x86_64;arm64"
#              cmake --build build --config Release --target AeroACARS
#   Linux:     cmake -B build -G "Unix Makefiles"
#              cmake --build build --config Release --target AeroACARS
#
# Output ends up in `build/AeroACARS/64/<platform>.xpl` regardless of
# generator — the post-build step renames + relocates as needed.

cmake_minimum_required(VERSION 3.20)

# macOS-Mindestversion fest vorgeben (muss VOR project() stehen). Ohne diese
# Zeile übernimmt der Compiler die SDK-Version des Build-Rechners als
# Mindestversion — ein Release vom CI-Runner "macos-latest" trug dann z. B.
# minos 15.0 und hätte sich auf älteren Macs nicht laden lassen. 11.0 (Big Sur)
# ist das Älteste, was die libc++ aktueller Xcode-Versionen noch unterstützt,
# und liegt unter X-Plane 12s eigener Mindestversion (macOS 12); die
# arm64-Hälfte kann ohnehin nicht tiefer. Überschreibbar per
# -DCMAKE_OSX_DEPLOYMENT_TARGET.
set(CMAKE_OSX_DEPLOYMENT_TARGET "11.0" CACHE STRING "macOS-Mindestversion des Plugins")

project(AeroACARS_XPlane_Plugin
    # v0.7.17: Synced to the actual code state. Plugin had drifted
    # from VERSION 0.5.0 (initial commit) while plugin.cpp got
    # patches across 6 incremental versions (0.5.3 / 0.5.6 / 0.5.8
    # / 0.5.11 / 0.5.13). Every Log.txt line was misreporting v0.5.0.
    # See docs/qs/v0.7.16-fenix-beta-bugs.md N-004.
    # 1.0.0 (AP7): Dataref-Server, Protokoll 2 (ADR-0004). Protokoll 1 bleibt.
    VERSION 1.1.1
    # 1.1.0: HUD-Band (BAND-Befehl, Fenster, Menü), ADR-0005.
    # 1.1.1: Band auch auf einem zweiten Monitor (Fenster vor dem Zeigen auf den Schirm).
    LANGUAGES CXX
    DESCRIPTION "AeroACARS X-Plane Premium Plugin"
)

# ---- C++ standard ---------------------------------------------------------
# C++17 — modern enough for std::optional / structured bindings, conservative
# enough that the X-Plane SDK headers (which are C99) compile cleanly via
# `extern "C"`. Static runtime linking on Windows so the .xpl has no MSVC
# DLL dependencies the pilot has to install.
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(MSVC)
    set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
    add_compile_options(/W4 /permissive-)
    # /MP for parallel build, /Zc:__cplusplus to actually report C++17.
    add_compile_options(/MP /Zc:__cplusplus)
    # Quelltexte sind UTF-8 (deutsche Kommentare); ohne /utf-8 liest MSVC sie
    # je nach Systemsprache als ANSI-Codepage.
    add_compile_options(/utf-8)
    # "Keine Ausnahmen" auch unter Windows: CMake setzt für MSVC ab Werk
    # /EHsc in CMAKE_CXX_FLAGS. Das nehmen wir global heraus; das Plugin
    # bekommt unten /EHs-c- + _HAS_EXCEPTIONS=0 (wie -fno-exceptions), die
    # Tests setzen /EHsc für sich wieder.
    string(REPLACE "/EHsc" "" CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}")
    # Stop windows.h from poisoning min/max macros.
    add_compile_definitions(NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS)
else()
    add_compile_options(-Wall -Wextra -Wpedantic -fvisibility=hidden)
    # No exceptions / RTTI — keeps the binary small and avoids edge cases
    # with X-Plane's C ABI plugin boundary. We program defensively in
    # plain C++ style (return codes, std::optional) instead.
    add_compile_options(-fno-exceptions -fno-rtti)
endif()

# ---- X-Plane SDK ----------------------------------------------------------
set(XPLM_SDK_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/third_party/XPSDK430")
if(NOT EXISTS "${XPLM_SDK_ROOT}/CHeaders/XPLM/XPLMDefs.h")
    message(FATAL_ERROR
        "X-Plane SDK not found at ${XPLM_SDK_ROOT}.\n"
        "Expected to find third_party/XPSDK430/CHeaders/XPLM/*.h.\n"
        "Re-clone the repo (the SDK is vendored) or run scripts/fetch_sdk.sh."
    )
endif()

# XPLM SDK feature defines — minimum versions we target.
# XPLM200 = X-Plane 9.0+ (legacy baseline, all our APIs available)
# XPLM210 = X-Plane 10.0+
# XPLM300 = X-Plane 11.0+
# XPLM301 = X-Plane 11.10+
# XPLM303 = X-Plane 11.50+ (Vulkan/Metal)
# We target XPLM303 = X-Plane 11.50+ minimum, which covers ~99% of pilots
# in 2026 and gives us access to all modern DataRef / flight-loop APIs.
add_compile_definitions(
    XPLM200=1
    XPLM210=1
    XPLM300=1
    XPLM301=1
    XPLM303=1
)

# Platform define expected by the SDK headers.
if(WIN32)
    add_compile_definitions(IBM=1 APL=0 LIN=0)
elseif(APPLE)
    add_compile_definitions(IBM=0 APL=1 LIN=0)
elseif(UNIX)
    add_compile_definitions(IBM=0 APL=0 LIN=1)
else()
    message(FATAL_ERROR "Unsupported platform")
endif()

# ---- Plugin target --------------------------------------------------------
# Der XPLM-freie Kern (Parser, JSON, Pakete, Dienst) steht in einer eigenen
# Liste, weil die Tests dieselben Dateien ohne XPLM übersetzen.
# HUD-Band: Open Sans (SIL OFL 1.1) wird als Byte-Feld eingebettet, erzeugt
# beim Konfigurieren aus third_party/opensans/*.ttf.
include(werkzeuge/schrift_einbetten.cmake)
set(AEROACARS_SCHRIFT_CPP "${CMAKE_BINARY_DIR}/generiert/schrift_daten.cpp")
aeroacars_schrift_einbetten("${AEROACARS_SCHRIFT_CPP}"
    OPENSANS_REGULAR  "${CMAKE_CURRENT_SOURCE_DIR}/third_party/opensans/OpenSans-Regular.ttf"
    OPENSANS_SEMIBOLD "${CMAKE_CURRENT_SOURCE_DIR}/third_party/opensans/OpenSans-SemiBold.ttf"
    OPENSANS_BOLD     "${CMAKE_CURRENT_SOURCE_DIR}/third_party/opensans/OpenSans-Bold.ttf")

set(AEROACARS_KERN_QUELLEN
    src/anfrage.cpp
    src/band.cpp
    src/band_layout.cpp
    src/band_schrift.cpp
    ${AEROACARS_SCHRIFT_CPP}
    src/json_schreiber.cpp
    src/pakete.cpp
    src/dienst.cpp
    src/protokoll1.cpp
    src/netz.cpp
)
add_library(AeroACARS MODULE
    src/plugin.cpp
    src/dienst_xplm.cpp
    src/band_xplm.cpp
    src/netz_os.cpp
    ${AEROACARS_KERN_QUELLEN}
)

if(MSVC)
    # /wd4577: "noexcept ohne Ausnahmemodus" — genau das ist hier gewollt.
    target_compile_options(AeroACARS PRIVATE /EHs-c- /wd4577)
    target_compile_definitions(AeroACARS PRIVATE _HAS_EXCEPTIONS=0)
endif()

target_include_directories(AeroACARS PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/third_party/stb"
    "${XPLM_SDK_ROOT}/CHeaders"
    "${XPLM_SDK_ROOT}/CHeaders/XPLM"
    "${XPLM_SDK_ROOT}/CHeaders/Widgets"
    src
)

# v0.7.17 (N-004 fix): Inject the CMake project version as a macro
# so plugin.cpp logs the truth instead of a stale hardcoded string.
# Format: "0.5.13" — plain quoted version, no SDK number (the SDK
# constant XPLM303 is already a separate define above).
target_compile_definitions(AeroACARS PRIVATE
    AEROACARS_PLUGIN_VERSION="${PROJECT_VERSION}"
)

# ---- Platform-specific linking --------------------------------------------
if(WIN32)
    # Link against X-Plane's import libraries.
    target_link_libraries(AeroACARS PRIVATE
        "${XPLM_SDK_ROOT}/Libraries/Win/XPLM_64.lib"
        ws2_32  # Winsock for our UDP socket
        opengl32  # HUD-Band (band_xplm.cpp)
    )
    # X-Plane expects .xpl files, not .dll. Override suffix.
    set_target_properties(AeroACARS PROPERTIES
        SUFFIX ".xpl"
        OUTPUT_NAME "win"
    )
elseif(APPLE)
    # macOS link strategy — same approach every modern open-source X-Plane
    # plugin uses (xgs, X-RAAS, etc.). XPLM symbols are NOT resolved at
    # link time; the .xpl is shipped with undefined symbols and X-Plane
    # provides its own copy of XPLM at load time. This means:
    #   * No `-framework XPLM` (would force link-time resolution and tie
    #     us to a specific framework path embedded as a load command).
    #   * `-undefined dynamic_lookup` permits undefined symbols at link
    #     time (otherwise the linker rejects every XPLM* call).
    #   * `-flat_namespace` keeps the symbol-resolution model compatible
    #     with X-Plane's own loader.
    # The XPLM headers are still needed at compile time and come via the
    # `target_include_directories` block above.
    set_target_properties(AeroACARS PROPERTIES
        # Universal binary — both Apple Silicon and Intel.
        OSX_ARCHITECTURES "x86_64;arm64"
        # CMake auto-prepends "lib" to MODULE libs on Unix; X-Plane
        # expects exactly `mac.xpl` (no prefix).
        PREFIX ""
        SUFFIX ".xpl"
        OUTPUT_NAME "mac"
        # Plugin doesn't go in an .app bundle, RPATH is irrelevant.
        MACOSX_RPATH FALSE
    )
    target_link_options(AeroACARS PRIVATE
        "-undefined" "dynamic_lookup"
        "-Wl,-flat_namespace"
    )
    # HUD-Band zeichnet mit Fixed-Function-OpenGL (band_xplm.cpp).
    target_link_libraries(AeroACARS PRIVATE "-framework OpenGL")
elseif(UNIX)
    # Linux: no import library, no framework. X-Plane's main binary
    # provides every XPLM* symbol at load time; the .xpl just ships
    # with them undefined.
    #
    # CMake on some distros (notably Ubuntu 22.04+ with newer ld)
    # passes `--as-needed` and would warn about unused libraries —
    # we don't link any, so this is fine. We deliberately do NOT pass
    # `-Wl,--no-undefined` (which would error on every XPLM* call).
    # HUD-Band zeichnet mit Fixed-Function-OpenGL (band_xplm.cpp); braucht die
    # GL-Header (Debian/Ubuntu: libgl-dev).
    target_link_libraries(AeroACARS PRIVATE GL)
    set_target_properties(AeroACARS PROPERTIES
        # CMake auto-prepends "lib" to MODULE libs on Unix; X-Plane
        # expects exactly `lin.xpl` (no prefix).
        PREFIX ""
        SUFFIX ".xpl"
        OUTPUT_NAME "lin"
    )
endif()

# ---- Output layout (X-Plane fat-plugin folder convention) -----------------
# X-Plane expects:
#   <X-Plane>/Resources/plugins/AeroACARS/64/{win,mac,lin}.xpl
# We produce that exact layout in build/AeroACARS/64/ so packaging is a
# straight folder-copy.
set_target_properties(AeroACARS PROPERTIES
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/AeroACARS/64"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/AeroACARS/64"
    LIBRARY_OUTPUT_DIRECTORY_DEBUG "${CMAKE_BINARY_DIR}/AeroACARS/64"
    LIBRARY_OUTPUT_DIRECTORY_RELEASE "${CMAKE_BINARY_DIR}/AeroACARS/64"
    RUNTIME_OUTPUT_DIRECTORY_DEBUG "${CMAKE_BINARY_DIR}/AeroACARS/64"
    RUNTIME_OUTPUT_DIRECTORY_RELEASE "${CMAKE_BINARY_DIR}/AeroACARS/64"
)

# Copy README and license files into the output folder so the packaged
# plugin contains attribution as required by the X-Plane SDK license.
configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/README.md"
    "${CMAKE_BINARY_DIR}/AeroACARS/README.md"
    COPYONLY
)
configure_file(
    "${XPLM_SDK_ROOT}/license.txt"
    "${CMAKE_BINARY_DIR}/AeroACARS/XPLM_SDK_LICENSE.txt"
    COPYONLY
)
# HUD-Band: eingebettete Schrift (OFL verlangt die Lizenz beim Weitergeben)
# und stb_truetype (MIT/Public Domain).
configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/third_party/opensans/OFL.txt"
    "${CMAKE_BINARY_DIR}/AeroACARS/OpenSans_OFL.txt"
    COPYONLY
)
configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/third_party/stb/LICENSE"
    "${CMAKE_BINARY_DIR}/AeroACARS/stb_LICENSE.txt"
    COPYONLY
)

# ---- Tests (ohne XPLM) ----------------------------------------------------
# cmake -B build-tests -DAEROACARS_PLUGIN_TESTS=ON -DCMAKE_BUILD_TYPE=Debug
# cmake --build build-tests --target aeroacars_tests aeroacars_fuzz
# ctest --test-dir build-tests --output-on-failure
#
# Die Tests übersetzen den Kern ein zweites Mal — ohne XPLM, auf macOS und
# Linux mit AddressSanitizer + UndefinedBehaviorSanitizer (abschaltbar über
# AEROACARS_SANITIZER=OFF). Unter MSVC ohne Sanitizer.
option(AEROACARS_PLUGIN_TESTS "Unit- und Fuzz-Tests des Plugin-Kerns bauen" OFF)
option(AEROACARS_SANITIZER "Tests mit ASan/UBSan (nicht MSVC)" ON)

if(AEROACARS_PLUGIN_TESTS)
    enable_testing()

    set(AEROACARS_TEST_FLAGS "")
    if(NOT MSVC AND AEROACARS_SANITIZER)
        set(AEROACARS_TEST_FLAGS
            -fsanitize=address,undefined -fno-sanitize-recover=all
            -fno-omit-frame-pointer)
    endif()

    function(aeroacars_testprogramm name)
        add_executable(${name} ${ARGN} ${AEROACARS_KERN_QUELLEN})
        target_include_directories(${name} PRIVATE src tests
            "${CMAKE_CURRENT_SOURCE_DIR}/third_party/stb")
        target_compile_definitions(${name} PRIVATE AEROACARS_PLUGIN_VERSION="${PROJECT_VERSION}")
        if(MSVC)
            target_compile_options(${name} PRIVATE /EHsc)
        else()
            target_compile_options(${name} PRIVATE ${AEROACARS_TEST_FLAGS})
            target_link_options(${name} PRIVATE ${AEROACARS_TEST_FLAGS})
        endif()
        if(APPLE)
            # Tests nur für die Architektur des Rechners: ASan-Universal-
            # Binaries sind unnötig, und ausführen lässt sich ohnehin nur eine.
            set_target_properties(${name} PROPERTIES
                OSX_ARCHITECTURES "${CMAKE_HOST_SYSTEM_PROCESSOR}")
        endif()
    endfunction()

    # Golden-Datagramme des Rust-Clients (hud_band_golden.json): beide Seiten
    # sollen nachweislich dasselbe Format sprechen.
    set(AEROACARS_BAND_GOLDEN
        "${CMAKE_CURRENT_SOURCE_DIR}/../client/src-tauri/crates/sim-xplane/testdata/hud_band_golden.json")

    aeroacars_testprogramm(aeroacars_tests
        tests/tests_main.cpp
        tests/json_pruefer.cpp
        tests/test_anfrage.cpp
        tests/test_band.cpp
        tests/test_band_layout.cpp
        tests/test_json.cpp
        tests/test_pakete.cpp
        tests/test_dienst.cpp
        tests/test_protokoll1.cpp
        tests/test_netz.cpp
    )
    if(EXISTS "${AEROACARS_BAND_GOLDEN}")
        target_compile_definitions(aeroacars_tests PRIVATE
            AEROACARS_BAND_GOLDEN="${AEROACARS_BAND_GOLDEN}")
    endif()
    aeroacars_testprogramm(aeroacars_fuzz
        tests/fuzz.cpp
        tests/json_pruefer.cpp
    )
    # Die ECHTEN Socket-Aufrufe (netz_os.cpp) — auf allen drei Plattformen,
    # also auch unter Windows, wo es keinen Ende-zu-Ende-Test gibt
    # (Codex-Abnahme H5).
    aeroacars_testprogramm(aeroacars_netz_echt
        tests/test_netz_echt.cpp
        src/netz_os.cpp
    )
    if(WIN32)
        target_link_libraries(aeroacars_netz_echt PRIVATE ws2_32)
    endif()

    add_test(NAME einheitstests COMMAND aeroacars_tests)
    # Fester Startwert, feste Laufzeit (5 s): reproduzierbar und in der CI
    # auf allen drei Plattformen bezahlbar.
    add_test(NAME fuzz COMMAND aeroacars_fuzz 5 20260929)
    add_test(NAME netz_echt COMMAND aeroacars_netz_echt)

    # Sammelziel für die CI: baut alles, was die Tests brauchen.
    add_custom_target(aeroacars_alle_tests)
    add_dependencies(aeroacars_alle_tests aeroacars_tests aeroacars_fuzz aeroacars_netz_echt)

    # Ende-zu-Ende (macOS/Linux): die ECHTE .xpl in einer XPLM-Attrappe,
    # dazu werkzeuge/plugin_sonde.py als Client. Prüft Socket-Schicht,
    # Protokoll-1-Takt, X-Plane-11-Verhalten und belegten Port.
    find_package(Python3 COMPONENTS Interpreter)
    if(NOT WIN32 AND Python3_Interpreter_FOUND)
        add_executable(aeroacars_attrappe tests/attrappe/xplm_attrappe.cpp)
        target_include_directories(aeroacars_attrappe PRIVATE "${XPLM_SDK_ROOT}/CHeaders")
        # XPLM=1: die Attrappe STELLT die XPLM-Funktionen bereit (sichtbar
        # exportiert, damit das per dlopen geladene Plugin sie findet).
        target_compile_definitions(aeroacars_attrappe PRIVATE XPLM=1)
        target_link_libraries(aeroacars_attrappe PRIVATE ${CMAKE_DL_LIBS})
        set_target_properties(aeroacars_attrappe PROPERTIES ENABLE_EXPORTS ON)
        if(APPLE)
            set_target_properties(aeroacars_attrappe PROPERTIES
                OSX_ARCHITECTURES "${CMAKE_HOST_SYSTEM_PROCESSOR}")
        endif()
        add_dependencies(aeroacars_alle_tests aeroacars_attrappe AeroACARS)
        add_test(NAME ende_zu_ende
            COMMAND ${Python3_EXECUTABLE}
                    "${CMAKE_CURRENT_SOURCE_DIR}/tests/attrappe/ende_zu_ende.py"
                    $<TARGET_FILE:aeroacars_attrappe>
                    $<TARGET_FILE:AeroACARS>
                    "${CMAKE_CURRENT_SOURCE_DIR}/werkzeuge/plugin_sonde.py")
        set_tests_properties(ende_zu_ende PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 180)
    endif()
endif()

# ---- Diagnostic summary ---------------------------------------------------
message(STATUS "──────────────────────────────────────────────────────────")
message(STATUS " AeroACARS X-Plane Plugin v${PROJECT_VERSION}")
message(STATUS "  C++ Standard:  ${CMAKE_CXX_STANDARD}")
message(STATUS "  Target system: ${CMAKE_SYSTEM_NAME}")
message(STATUS "  Build type:    ${CMAKE_BUILD_TYPE}")
message(STATUS "  Output:        ${CMAKE_BINARY_DIR}/AeroACARS/64/")
message(STATUS "──────────────────────────────────────────────────────────")
