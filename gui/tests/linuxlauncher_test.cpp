#include "linuxlauncher.h"

#include <cstdlib>
#include <cstring>
#include <iostream>
#include <sys/wait.h>
#include <unistd.h>
#include <string>
#include <vector>

namespace {

void check(bool condition, const char *message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        std::exit(1);
    }
}

bool contains(const std::vector<std::string> &values, const std::string &needle)
{
    for (const std::string &value : values) {
        if (value == needle) {
            return true;
        }
    }
    return false;
}

} // namespace

int main()
{
    ::setenv("DISPLAY", "old", 1);
    ::unsetenv("LD_PRELOAD");
    std::vector<std::string> arguments = {
        "--fm-elevated",
        "--fm-env=DISPLAY=new value",
        "--fm-env=1INVALID=ignored",
        "--fm-env=LD_PRELOAD=/tmp/evil.so",
        "--no-elevate",
        "--print-elevation",
        "--smoke-test",
    };

    bool elevatedChild = false;
    const std::vector<std::string> filtered =
        LinuxLauncher::applyForwardedEnvironment(arguments, elevatedChild);

    check(elevatedChild, "--fm-elevated must be detected");
    check(std::getenv("DISPLAY") != nullptr
              && std::strcmp(std::getenv("DISPLAY"), "new value") == 0,
          "allowlisted environment values must be applied");
    check(std::getenv("LD_PRELOAD") == nullptr,
          "non-allowlisted environment variables must be ignored");
    check(!contains(filtered, "--fm-elevated"), "--fm-elevated must be removed");
    check(!contains(filtered, "--fm-env=DISPLAY=new value"),
          "--fm-env arguments must be removed");
    check(!contains(filtered, "--fm-env=1INVALID=ignored"),
          "invalid environment arguments must be removed");
    check(contains(filtered, "--no-elevate"), "regular arguments must be preserved");
    check(filtered.size() == 3, "unexpected filtered argument count");

    ::setenv("DISPLAY", ":77", 1);
    ::setenv("WAYLAND_DISPLAY", "wayland-9", 1);
    ::setenv("XDG_RUNTIME_DIR", "/run/user/1234", 1);
    ::setenv("QT_PLUGIN_PATH", "/tmp/evil-plugins", 1);

    const std::string self = "/opt/fmaker explore/fmaker-explore-gui";
    const std::vector<std::string> command = LinuxLauncher::buildElevationCommand(
        self, {"--no-elevate", "--print-elevation", "--smoke-test"});

    check(!command.empty()
              && (command[0] == "pkexec" || command[0] == "/usr/bin/pkexec"
                  || command[0] == "/usr/sbin/pkexec" || command[0] == "/bin/pkexec"),
          "pkexec must be the first argument");
    check(command.size() > 2 && command[1] == self, "the executable path must follow pkexec");
    check(command[2] == "--fm-elevated", "the elevation marker must follow the path");
    check(contains(command, "--fm-env=DISPLAY=:77"), "DISPLAY must be forwarded");
    check(contains(command, "--fm-env=WAYLAND_DISPLAY=wayland-9"),
          "WAYLAND_DISPLAY must be forwarded");
    check(contains(command, "--fm-env=XDG_RUNTIME_DIR=/run/user/1234"),
          "XDG_RUNTIME_DIR must be forwarded");
    check(!contains(command, "--fm-env=QT_PLUGIN_PATH=/tmp/evil-plugins"),
          "Qt plugin paths must not be forwarded");
    check(!contains(command, "--no-elevate"), "--no-elevate must not reach the child");
    check(!contains(command, "--print-elevation"),
          "--print-elevation must not reach the child");
    check(contains(command, "--smoke-test"), "unrelated arguments must be preserved");

    const std::string description = LinuxLauncher::describeCommand(command);
    check(description.find("'/opt/fmaker explore/fmaker-explore-gui'")
              != std::string::npos,
          "paths with spaces must be shell quoted");

    check(LinuxLauncher::execCommand({}) == EINVAL,
          "an empty command must be rejected");

    const pid_t child = ::fork();
    check(child >= 0, "fork failed");
    if (child == 0) {
        const int error = LinuxLauncher::execCommand({"/bin/true"});
        ::_exit(error == 0 ? 126 : 127);
    }
    int status = 0;
    check(::waitpid(child, &status, 0) == child, "waitpid failed");
    check(WIFEXITED(status) && WEXITSTATUS(status) == 0,
          "execCommand must replace the process image");

    std::cout << "linuxlauncher tests passed\n";
    return 0;
}
