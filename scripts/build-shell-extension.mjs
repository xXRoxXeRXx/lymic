import { execFileSync } from "node:child_process";
import { existsSync, copyFileSync } from "node:fs";
import { join } from "node:path";
import { platform } from "node:process";

// The companion DLL only exists on Windows. Keeping this guard in the bundle
// command preserves the Linux and macOS release jobs.
if (platform === "win32") {
  let cmakeBin = "cmake";
  try {
    execFileSync("cmake", ["--version"], { stdio: "ignore" });
  } catch {
    const candidates = [
      "C:\\Program Files (x86)\\Microsoft Visual Studio\\18\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe",
      "C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe",
      "C:\\Program Files\\Microsoft Visual Studio\\2022\\Professional\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe",
      "C:\\Program Files\\Microsoft Visual Studio\\2022\\Enterprise\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe",
      "C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe",
      "C:\\Program Files\\CMake\\bin\\cmake.exe",
    ];
    for (const candidate of candidates) {
      if (existsSync(candidate)) {
        cmakeBin = candidate;
        break;
      }
    }
  }

  execFileSync(cmakeBin, ["-S", "lymic-shell-ext", "-B", "lymic-shell-ext/build"], {
    stdio: "inherit",
  });
  execFileSync(cmakeBin, ["--build", "lymic-shell-ext/build", "--config", "Release"], {
    stdio: "inherit",
  });

  const builtDll = "lymic-shell-ext/build/Release/lymic-shell-ext.dll";
  if (existsSync(builtDll)) {
    for (const targetDir of ["src-tauri/target/debug", "src-tauri/target/release"]) {
      if (existsSync(targetDir)) {
        copyFileSync(builtDll, join(targetDir, "lymic-shell-ext.dll"));
      }
    }
  }
}
