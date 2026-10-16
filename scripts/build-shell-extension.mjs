import { execFileSync } from "node:child_process";
import { platform } from "node:process";

// The companion DLL only exists on Windows. Keeping this guard in the bundle
// command preserves the Linux and macOS release jobs.
if (platform === "win32") {
  execFileSync("cmake", ["-S", "lymic-shell-ext", "-B", "lymic-shell-ext/build"], {
    stdio: "inherit",
  });
  execFileSync("cmake", ["--build", "lymic-shell-ext/build", "--config", "Release"], {
    stdio: "inherit",
  });
}
