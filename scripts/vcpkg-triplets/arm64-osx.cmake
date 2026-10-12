# vcpkg's stock arm64-osx triplet (vcpkg 9e593bb, triplets/arm64-osx.cmake) plus an explicit
# deployment target. Without it the static libs are built for the host SDK's default (minos 27.0
# with Xcode 27; the macos-14 runner gets 14.x), although the app they are linked into declares
# MIN_MACOS_VERSION 12.3 (flutter-build.yml:914).
set(VCPKG_TARGET_ARCHITECTURE arm64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE static)

set(VCPKG_CMAKE_SYSTEM_NAME Darwin)
set(VCPKG_OSX_ARCHITECTURES arm64)
set(VCPKG_OSX_DEPLOYMENT_TARGET 12.3)
