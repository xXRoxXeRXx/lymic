cmake_minimum_required(VERSION 3.21)
project(lymic_shell_ext LANGUAGES CXX)

add_library(lymic-shell-ext SHARED src/thumbnail_provider.cpp)
target_compile_features(lymic-shell-ext PRIVATE cxx_std_17)
target_compile_definitions(lymic-shell-ext PRIVATE UNICODE _UNICODE NOMINMAX WIN32_LEAN_AND_MEAN)
target_link_libraries(lymic-shell-ext PRIVATE ole32 windowscodecs cldapi)
target_link_options(lymic-shell-ext PRIVATE /EXPORT:DllGetClassObject,PRIVATE /EXPORT:DllCanUnloadNow,PRIVATE)
set_target_properties(lymic-shell-ext PROPERTIES PREFIX "" OUTPUT_NAME "lymic-shell-ext")


# Registration and installation are intentionally owned by ticket #56.
